// Package engine drives the subscription-pool collection loop and fans
// normalized ticks out to connected clients.
package engine

import (
	"context"
	"log"
	"net/http"
	"strings"
	"sync"
	"time"

	"natives/market-host/internal/model"
	"natives/market-host/internal/quote"
	"natives/market-host/internal/store"
)

const sparklineCap = 60

// normalize canonicalizes client symbols: bare fund codes gain the "F:" tag,
// A-share symbols stay exchange-prefixed; junk returns "".
func normalize(s string) string {
	s = strings.TrimSpace(strings.ToLower(s))
	if s == "" {
		return ""
	}
	if strings.HasPrefix(s, "f:") {
		code := strings.TrimPrefix(s, "f:")
		if isFundCode(code) {
			return "F:" + code
		}
		return ""
	}
	if len(s) == 6 && isFundCode(s) && !strings.HasPrefix(s, "6") && !strings.HasPrefix(s, "0") && !strings.HasPrefix(s, "3") {
		return "F:" + s
	}
	if len(s) == 8 && (strings.HasPrefix(s, "sh") || strings.HasPrefix(s, "sz") || strings.HasPrefix(s, "bj")) {
		return s
	}
	return ""
}

func isFundCode(s string) bool {
	if len(s) != 6 {
		return false
	}
	for _, c := range s {
		if c < '0' || c > '9' {
			return false
		}
	}
	return true
}

// Sink receives merged batches of normalized items.
type Sink func(items []model.NormalizedAssetItem)

// Engine owns subscription state, polling cadence and sparkline buffers.
type Engine struct {
	client *http.Client

	mu         sync.Mutex
	subscribed map[string]bool // canonical symbol: sh600519 / F-code 005827
	sparks     map[string]*store.Sparkline
	last       map[string]model.NormalizedAssetItem
	sink       Sink
}

// New builds an engine; sink is invoked on the loop goroutine per batch.
func New(sink Sink) *Engine {
	return &Engine{
		client:     &http.Client{Timeout: 3 * time.Second},
		subscribed: map[string]bool{},
		sparks:     map[string]*store.Sparkline{},
		last:       map[string]model.NormalizedAssetItem{},
		sink:       sink,
	}
}

// Subscribe registers symbols; returns the current snapshot for them.
func (e *Engine) Subscribe(symbols []string) []model.NormalizedAssetItem {
	ids := make([]string, 0, len(symbols))
	e.mu.Lock()
	for _, s := range symbols {
		s = normalize(s)
		if s == "" {
			continue
		}
		id := toID(s)
		e.subscribed[id] = true
		if _, ok := e.sparks[id]; !ok {
			e.sparks[id] = store.NewSparkline(sparklineCap)
		}
		ids = append(ids, id)
	}
	e.mu.Unlock()
	return e.Snapshot(ids)
}

// toID maps a canonical symbol to the contract item id.
func toID(s string) string {
	if strings.HasPrefix(s, "F:") {
		return s
	}
	return model.ID(s, model.Classify(s))
}

// Unsubscribe drops symbols; empty slice clears all (panel closed).
func (e *Engine) Unsubscribe(symbols []string) {
	e.mu.Lock()
	defer e.mu.Unlock()
	if len(symbols) == 0 {
		e.subscribed = map[string]bool{}
		return
	}
	for _, s := range symbols {
		if s = normalize(s); s != "" {
			delete(e.subscribed, toID(s))
		}
	}
}

// Snapshot returns stored last-known items (reconnect path); ids empty = all.
// Accepts contract ids (A:sh600519 / F:005827) directly.
func (e *Engine) Snapshot(ids []string) []model.NormalizedAssetItem {
	e.mu.Lock()
	defer e.mu.Unlock()
	want := map[string]bool{}
	for _, id := range ids {
		want[id] = true
	}
	out := make([]model.NormalizedAssetItem, 0)
	for sym, item := range e.last {
		if len(want) > 0 && !want[sym] {
			continue
		}
		cp := item
		cp.Sparkline = e.sparks[sym].Snapshot()
		out = append(out, cp)
	}
	return out
}

func (e *Engine) splitSubscribed() (stocks, funds []string) {
	e.mu.Lock()
	defer e.mu.Unlock()
	for id := range e.subscribed {
		// 订阅池存的是契约 ID（A:sh600519 / F:005827）；数据源需要原始代码。
		if strings.HasPrefix(id, "F:") {
			funds = append(funds, id[2:])
		} else if strings.HasPrefix(id, "A:") {
			stocks = append(stocks, id[2:])
		}
	}
	return
}

// Run blocks until ctx is done, polling per the session-state cadence.
func (e *Engine) Run(ctx context.Context) {
	for {
		phase := quote.CurrentPhase(time.Now())
		e.collect(ctx, phase)
		select {
		case <-ctx.Done():
			return
		case <-time.After(phase.PollInterval()):
		}
	}
}

func (e *Engine) collect(ctx context.Context, phase quote.SessionPhase) {
	stocks, funds := e.splitSubscribed()
	var batch []model.NormalizedAssetItem
	if len(stocks) > 0 {
		items, err := e.fetchTencent(ctx, stocks)
		if err != nil {
			log.Printf("tencent fetch: %v", err)
		} else {
			batch = append(batch, items...)
		}
	}
	if len(funds) > 0 {
		items := e.fetchFunds(ctx, funds, phase.Streaming())
		batch = append(batch, items...)
	}
	if len(batch) == 0 {
		return
	}
	e.mu.Lock()
	for _, it := range batch {
		e.sparks[it.ID].Push(it.Price)
		e.last[it.ID] = it
	}
	e.mu.Unlock()
	if e.sink != nil {
		e.sink(batch)
	}
}

func (e *Engine) fetchTencent(ctx context.Context, symbols []string) ([]model.NormalizedAssetItem, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, quote.TencentQuoteURL(symbols), nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Referer", "https://gu.qq.com/")
	resp, err := e.client.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	body := new(strings.Builder)
	if _, err := readAll(resp.Body, body); err != nil {
		return nil, err
	}
	return quote.ParseTencent(body.String(), time.Now())
}

func (e *Engine) fetchFunds(ctx context.Context, codes []string, streaming bool) []model.NormalizedAssetItem {
	out := make([]model.NormalizedAssetItem, 0)
	var wg sync.WaitGroup
	var mu sync.Mutex
	sem := make(chan struct{}, 8)
	for _, code := range codes {
		wg.Add(1)
		go func(code string) {
			defer wg.Done()
			sem <- struct{}{}
			defer func() { <-sem }()
			req, err := http.NewRequestWithContext(ctx, http.MethodGet, quote.FundEstimateURL([]string{code}), nil)
			if err != nil {
				return
			}
			req.Header.Set("Referer", "https://fund.eastmoney.com/")
			resp, err := e.client.Do(req)
			if err != nil {
				return
			}
			defer resp.Body.Close()
			b := new(strings.Builder)
			if _, err := readAll(resp.Body, b); err != nil {
				return
			}
			item, err := quote.ParseFundGz(b.String(), !streaming, time.Now())
			if err != nil {
				return
			}
			mu.Lock()
			out = append(out, item)
			mu.Unlock()
		}(code)
	}
	wg.Wait()
	return out
}
