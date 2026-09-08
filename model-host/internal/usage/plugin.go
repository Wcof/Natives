package usage

import (
	"context"
	"net/http"
	"strings"
	"sync"
	"time"

	cliproxyusage "github.com/router-for-me/CLIProxyAPI/v7/sdk/cliproxy/usage"
)

type KeyResolver func(apiKey string) (id string, name string)
type BroadcastFunc func(eventType string, data interface{})

type NativesUsagePlugin struct {
	store       *Store
	calculator  *Calculator
	keyResolver KeyResolver
	broadcast   BroadcastFunc
	queue       chan cliproxyusage.Record
	stopOnce    sync.Once
	stopCh      chan struct{}
	doneCh      chan struct{}
	lastEmit    time.Time
	emitMu      sync.Mutex
}

func NewNativesUsagePlugin(
	store *Store,
	calculator *Calculator,
	keyResolver KeyResolver,
	broadcast BroadcastFunc,
) *NativesUsagePlugin {
	p := &NativesUsagePlugin{
		store:       store,
		calculator:  calculator,
		keyResolver: keyResolver,
		broadcast:   broadcast,
		queue:       make(chan cliproxyusage.Record, 512),
		stopCh:      make(chan struct{}),
		doneCh:      make(chan struct{}),
	}
	go p.worker()
	return p
}

func (p *NativesUsagePlugin) Close() {
	p.stopOnce.Do(func() {
		close(p.stopCh)
	})
	<-p.doneCh
}

func (p *NativesUsagePlugin) HandleUsage(ctx context.Context, record cliproxyusage.Record) {
	// A usage record represents an actual completed request. Do not drop it
	// if the downstream HTTP context was cancelled on client disconnect.
	select {
	case p.queue <- record:
	case <-p.stopCh:
	}
}

func (p *NativesUsagePlugin) worker() {
	defer close(p.doneCh)
	for {
		select {
		case <-p.stopCh:
			for {
				select {
				case record := <-p.queue:
					p.processRecord(record)
				default:
					return
				}
			}
		case record := <-p.queue:
			p.processRecord(record)
		}
	}
}

func (p *NativesUsagePlugin) processRecord(record cliproxyusage.Record) {
	var keyID, keyName string
	if p.keyResolver != nil && record.APIKey != "" {
		keyID, keyName = p.keyResolver(record.APIKey)
	}
	// Immediately remove plaintext API key from memory scope
	record.APIKey = ""

	result := ResultSuccess
	if record.Failed {
		result = ResultFailed
	}

	detail := cliproxyusage.EnsureTokenBreakdownForProvider(record.Detail, record.Provider, record.ExecutorType)
	bd := detail.TokenBreakdown

	inputToks := bd.Input.TotalTokens
	outputToks := bd.Output.TotalTokens
	cacheRead := bd.Input.CacheReadTokens
	cacheWrite := bd.Input.CacheWriteTokens
	reasoning := bd.Output.ReasoningTokens
	totalToks := bd.TotalTokens

	costMicro := p.calculator.CalculateCost(
		record.Provider,
		record.Model,
		inputToks,
		outputToks,
		cacheRead,
		cacheWrite,
	)

	summary := ""
	if record.Failed {
		summary = strings.TrimSpace(http.StatusText(record.Fail.StatusCode))
		if summary == "" {
			summary = "upstream request failed"
		}
	}

	event := &Event{
		RequestedAt:      record.RequestedAt,
		LatencyMs:        record.Latency.Milliseconds(),
		TTFTMs:           record.TTFT.Milliseconds(),
		Provider:         record.Provider,
		AccountID:        record.AuthID,
		Model:            record.Model,
		ModelAlias:       record.Alias,
		Source:           record.Source,
		Endpoint:         record.ExecutorType,
		AccessKeyID:      keyID,
		AccessKeyName:    keyName,
		Result:           result,
		HTTPStatus:       record.Fail.StatusCode,
		ErrorSummary:     summary,
		InputTokens:      inputToks,
		OutputTokens:     outputToks,
		CacheReadTokens:  cacheRead,
		CacheWriteTokens: cacheWrite,
		ReasoningTokens:  reasoning,
		TotalTokens:      totalToks,
		CostMicro:        costMicro,
		ServiceTier:      record.ServiceTier,
		CreatedAt:        time.Now().UTC(),
	}

	if err := p.store.InsertEvent(event); err == nil {
		p.emitBroadcast()
	}
}

func (p *NativesUsagePlugin) emitBroadcast() {
	if p.broadcast == nil {
		return
	}
	p.emitMu.Lock()
	defer p.emitMu.Unlock()

	now := time.Now()
	if now.Sub(p.lastEmit) >= time.Second {
		p.lastEmit = now
		p.broadcast("model_usage_updated", map[string]interface{}{
			"updatedAt": now.UTC().Format(time.RFC3339),
		})
	}
}
