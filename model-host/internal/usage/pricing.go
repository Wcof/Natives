package usage

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
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

	// Cost in millionths of USD ($ / 1M tokens)
	cost := (input*p.InputPriceMicro +
		output*p.OutputPriceMicro +
		cacheRead*p.CacheReadPriceMicro +
		cacheWrite*p.CacheWritePriceMicro) / 1000000

	return cost
}

func (c *Calculator) FindPrice(provider, model string) *Price {
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
	c.store.mu.RLock()
	row := c.store.db.QueryRow(`
		SELECT
			COALESCE(SUM(cost_micro), 0),
			COALESCE(SUM(CASE WHEN cost_micro > 0 THEN 1 ELSE 0 END), 0),
			COALESCE(SUM(CASE WHEN cost_micro = 0 THEN 1 ELSE 0 END), 0)
		FROM usage_events
	`)
	_ = row.Scan(&totalCostMicro, &pricedCount, &unpricedCount)
	c.store.mu.RUnlock()

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
