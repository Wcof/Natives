package usage

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"math/big"
	"net/http"
	"strings"
	"sync"
	"time"
)

const (
	DefaultPriceSyncURL = "https://raw.githubusercontent.com/router-for-me/EasyCLIProxyAPI/main/src-tauri/resources/model_prices.json"
	MaxSyncResponseSize = 5 * 1024 * 1024 // 5 MiB
)

var (
	builtinPricesLock sync.RWMutex
	builtinPrices     = map[string]Price{
		"gpt-4o": {
			ModelID: "gpt-4o", InputPriceMicro: 2500000, OutputPriceMicro: 10000000,
			CacheReadPriceMicro: 1250000, Source: PriceSourceBuiltin,
		},
		"gpt-4o-mini": {
			ModelID: "gpt-4o-mini", InputPriceMicro: 150000, OutputPriceMicro: 600000,
			CacheReadPriceMicro: 75000, Source: PriceSourceBuiltin,
		},
		"claude-3-5-sonnet-20241022": {
			ModelID: "claude-3-5-sonnet-20241022", InputPriceMicro: 3000000, OutputPriceMicro: 15000000,
			CacheReadPriceMicro: 300000, CacheWritePriceMicro: 3750000, Source: PriceSourceBuiltin,
		},
		"claude-3-5-haiku-20241022": {
			ModelID: "claude-3-5-haiku-20241022", InputPriceMicro: 800000, OutputPriceMicro: 4000000,
			CacheReadPriceMicro: 80000, CacheWritePriceMicro: 1000000, Source: PriceSourceBuiltin,
		},
		"gemini-2.0-flash": {
			ModelID: "gemini-2.0-flash", InputPriceMicro: 100000, OutputPriceMicro: 400000,
			CacheReadPriceMicro: 25000, Source: PriceSourceBuiltin,
		},
		"gemini-1.5-pro": {
			ModelID: "gemini-1.5-pro", InputPriceMicro: 1250000, OutputPriceMicro: 5000000,
			CacheReadPriceMicro: 312500, Source: PriceSourceBuiltin,
		},
		"deepseek-chat": {
			ModelID: "deepseek-chat", InputPriceMicro: 140000, OutputPriceMicro: 280000,
			CacheReadPriceMicro: 14000, Source: PriceSourceBuiltin,
		},
		"deepseek-reasoner": {
			ModelID: "deepseek-reasoner", InputPriceMicro: 550000, OutputPriceMicro: 2190000,
			CacheReadPriceMicro: 140000, Source: PriceSourceBuiltin,
		},
	}
)

type Calculator struct {
	store *Store
}

func NewCalculator(store *Store) *Calculator {
	return &Calculator{store: store}
}

func (c *Calculator) CalculateCost(provider, model string, input, output, cacheRead, cacheWrite int64) int64 {
	p := c.FindPrice(provider, model)
	if p == nil {
		return 0
	}

	cost, _ := costWithPrice(p, input, output, cacheRead, cacheWrite)
	return cost
}

// thinkingSuffixes 为 CLIProxyAPI 生态常见的推理档位后缀：档位只影响思考预算
// （如 Gemini 的 -high/-low），不改变 token 单价，价格目录通常只收基础款。
var thinkingSuffixes = []string{"-high", "-low", "-minimal", "-safety-le", "-safety"}

// stripThinkingSuffix 剥离模型名尾部的推理档位后缀（最多两层，
// 如 gemini-3.7-flash-safety-le → gemini-3.7-flash），无后缀时原样返回。
func stripThinkingSuffix(model string) string {
	for attempts := 0; attempts < 2; attempts++ {
		lower := strings.ToLower(model)
		stripped := false
		for _, suffix := range thinkingSuffixes {
			if strings.HasSuffix(lower, suffix) && len(model) > len(suffix) {
				model = model[:len(model)-len(suffix)]
				stripped = true
				break
			}
		}
		if !stripped {
			break
		}
	}
	return model
}

func (c *Calculator) FindPrice(provider, model string) *Price {
	if p := c.findPriceExact(provider, model); p != nil {
		return p
	}
	// 精确匹配失败：按推理档位变体归一化后再匹配一次（gemini-3.8-flash-high
	// → gemini-3.8-flash）。仍无结果返回 nil，调用方按未计价诚实显示 0。
	if normalized := stripThinkingSuffix(model); !strings.EqualFold(normalized, model) {
		return c.findPriceExact(provider, normalized)
	}
	return nil
}

func (c *Calculator) findPriceExact(provider, model string) *Price {
	prices, err := c.store.GetPrices()
	if err == nil {
		// 1. Specific Provider Manual
		for _, p := range prices {
			if p.ProviderID == provider && strings.EqualFold(p.ModelID, model) && p.Source == PriceSourceManual {
				return &p
			}
		}
		// 2. Global Manual
		for _, p := range prices {
			if p.ProviderID == "" && strings.EqualFold(p.ModelID, model) && (p.Source == PriceSourceManual || p.Source == PriceSourceManualGlobal) {
				return &p
			}
		}
		// 3. Synced
		for _, p := range prices {
			if strings.EqualFold(p.ModelID, model) && p.Source == PriceSourceSynced {
				return &p
			}
		}
		// 4. Imported
		for _, p := range prices {
			if strings.EqualFold(p.ModelID, model) && p.Source == PriceSourceImported {
				return &p
			}
		}
	}

	// 5. Built-in
	builtinPricesLock.RLock()
	defer builtinPricesLock.RUnlock()
	for k, p := range builtinPrices {
		if strings.EqualFold(k, model) {
			return &p
		}
	}

	return nil
}

func (c *Calculator) GetPricingCatalog() (*PricingResult, error) {
	customPrices, err := c.store.GetPrices()
	if err != nil {
		return nil, err
	}

	mergedMap := make(map[string]Price)
	// Add builtin prices first
	builtinPricesLock.RLock()
	for k, p := range builtinPrices {
		mergedMap[k] = p
	}
	builtinPricesLock.RUnlock()

	// Override with custom / synced / imported prices
	for _, p := range customPrices {
		key := p.ModelID
		if p.ProviderID != "" {
			key = p.ProviderID + "/" + p.ModelID
		}
		mergedMap[key] = p
	}

	allPrices := make([]Price, 0, len(mergedMap))
	for _, p := range mergedMap {
		allPrices = append(allPrices, p)
	}

	var totalCostMicro, pricedCount, unpricedCount int64
	type modelCount struct {
		provider, model string
		cnt             int64
	}
	var modelCounts []modelCount
	c.store.mu.RLock()
	// 汇总口径（R5，方案 §9.1）：已计价记录数按价格目录判定（含合法零价），
	// 不用 cost_micro>0 区分——合法零价不是未计价；未计价 = 价格目录缺该模型
	// 的记录数。目录判定在锁外做（FindPrice 内部会再取 store.mu，
	// 非重入锁在持锁期间调用会死锁——项目 Rust/Go 锁纪律同样适用于此）。
	row := c.store.db.QueryRow(`SELECT COALESCE(SUM(cost_micro), 0) FROM usage_events`)
	_ = row.Scan(&totalCostMicro)
	modelRows, err := c.store.db.Query(`
		SELECT provider, model, COUNT(*) FROM usage_events
		GROUP BY provider, model`)
	if err == nil {
		for modelRows.Next() {
			var mc modelCount
			if err := modelRows.Scan(&mc.provider, &mc.model, &mc.cnt); err == nil {
				modelCounts = append(modelCounts, mc)
			}
		}
		modelRows.Close()
	}
	c.store.mu.RUnlock()

	for _, mc := range modelCounts {
		if c.FindPrice(mc.provider, mc.model) != nil {
			pricedCount += mc.cnt
		} else {
			unpricedCount += mc.cnt
		}
	}

	return &PricingResult{
		Prices:                allPrices,
		CatalogVersion:        "2026.08",
		TotalEstimatedCostUSD: float64(totalCostMicro) / 1000000.0,
		PricedRequestsCount:   pricedCount,
		UnpricedRequestsCount: unpricedCount,
	}, nil
}

func (c *Calculator) SyncRemoteCatalog(ctx context.Context) error {
	_, err := syncRemotePrices(ctx, c.store, DefaultPriceSyncURL)
	return err
}

// zeroCostRow 聚合了 cost_micro=0 事件的 token 分量（按 provider+model 分组）。
// 本地采集的用量事件不携带计费（cost_micro=0），金额在查询层按价格目录动态估算。
// Key 标识聚合行归属（byModel=模型显示名、sessions=会话三元键、空=全局）。
type zeroCostRow struct {
	Key      string
	Provider string
	Model    string
	In       int64
	Out      int64
	CR       int64
	CW       int64
}

// zeroCostEstimate 为一个聚合组的估算结果：金额与可计价标记。目录缺该模型
// 时 Priced=false，消费方必须显示未知（--）而不是 $0.00（严禁把取不到渲染成 0）。
type zeroCostEstimate struct {
	Micro  int64
	Priced bool
}

// costWithPrice 按已解析价格计算（micro-USD），避免重复 FindPrice。
func costWithPrice(p *Price, in, out, cr, cw int64) (int64, error) {
	if p == nil {
		return 0, nil
	}
	// 先精确累加再缩放，避免可表示的微美元结果在中间乘法处溢出。
	var total, term big.Int
	for _, part := range [][2]int64{{in, p.InputPriceMicro}, {out, p.OutputPriceMicro}, {cr, p.CacheReadPriceMicro}, {cw, p.CacheWritePriceMicro}} {
		if part[0] < 0 || part[1] < 0 {
			return 0, errors.New("invalid cost input")
		}
		term.Mul(big.NewInt(part[0]), big.NewInt(part[1]))
		total.Add(&total, &term)
	}
	total.Quo(&total, big.NewInt(1000000))
	if !total.IsInt64() {
		return 0, errors.New("estimated cost exceeds micro-USD range")
	}
	return total.Int64(), nil
}

// estimateZeroCostByGroup 在 Store 锁外对零成本聚合行做动态计价，
// 返回 Key → 估算结果。严禁在持有 store.mu 时调用：
// Calculator.FindPrice 内部会再取读锁，非重入（与 GetPricingCatalog 同一纪律）。
func estimateZeroCostByGroup(store *Store, rows []zeroCostRow) (map[string]zeroCostEstimate, error) {
	if len(rows) == 0 {
		return nil, nil
	}
	if _, err := store.GetPrices(); err != nil {
		return nil, err
	}
	calc := &Calculator{store: store}
	out := make(map[string]zeroCostEstimate, len(rows))
	for _, r := range rows {
		e, exists := out[r.Key]
		if !exists {
			e.Priced = true
		}
		price := calc.FindPrice(r.Provider, r.Model)
		if price == nil {
			e.Priced = false
		} else {
			cost, err := costWithPrice(price, r.In, r.Out, r.CR, r.CW)
			if err != nil {
				return nil, err
			}
			if cost > math.MaxInt64-e.Micro {
				return nil, errors.New("estimated cost exceeds micro-USD range")
			}
			e.Micro += cost
		}
		out[r.Key] = e
	}
	return out, nil
}

func SyncRemotePrices(store *Store, syncURL string) (int, error) {
	return syncRemotePrices(context.Background(), store, syncURL)
}

func syncRemotePrices(ctx context.Context, store *Store, syncURL string) (int, error) {
	if syncURL == "" {
		syncURL = DefaultPriceSyncURL
	}

	client := &http.Client{Timeout: 15 * time.Second}
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, syncURL, nil)
	if err != nil {
		return 0, fmt.Errorf("build price catalog request: %w", err)
	}
	resp, err := client.Do(request)
	if err != nil {
		return 0, fmt.Errorf("fetch prices: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return 0, fmt.Errorf("remote price catalog returned status %d", resp.StatusCode)
	}

	body, err := io.ReadAll(io.LimitReader(resp.Body, MaxSyncResponseSize+1))
	if err != nil {
		return 0, fmt.Errorf("read price catalog response: %w", err)
	}

	if len(body) > MaxSyncResponseSize {
		return 0, fmt.Errorf("price catalog exceeds %d bytes", MaxSyncResponseSize)
	}
	var catalog struct {
		Models map[string]struct {
			InputPrice  float64 `json:"inputPer1M"`
			OutputPrice float64 `json:"outputPer1M"`
			CacheRead   float64 `json:"cacheReadPer1M"`
			CacheWrite  float64 `json:"cacheCreationPer1M"`
		} `json:"models"`
	}

	if err := json.Unmarshal(body, &catalog); err != nil {
		return 0, fmt.Errorf("parse price json: %w", err)
	}
	if len(catalog.Models) == 0 {
		return 0, errors.New("price catalog contains no models")
	}

	count := 0
	for modelID, raw := range catalog.Models {
		p := Price{
			ModelID:              modelID,
			InputPriceMicro:      int64(raw.InputPrice * 1000000),
			OutputPriceMicro:     int64(raw.OutputPrice * 1000000),
			CacheReadPriceMicro:  int64(raw.CacheRead * 1000000),
			CacheWritePriceMicro: int64(raw.CacheWrite * 1000000),
			Source:               PriceSourceSynced,
		}
		if err := store.UpsertPrice(&p); err == nil {
			count++
		}
	}

	return count, nil
}
