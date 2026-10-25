package usage

import (
	"crypto/sha256"
	"encoding/hex"
	"time"
)

// TokenUsage is the normalized token envelope of one completed request.
// InputTokens is uncached input; OutputTokens includes reasoning when the
// provider reports reasoning as part of its completion tokens.
type TokenUsage struct {
	InputTokens      int64
	OutputTokens     int64
	CacheReadTokens  int64
	CacheWriteTokens int64
	ReasoningTokens  int64
	TotalTokens      int64
}

// Record is the provider-neutral usage envelope written by internal direct
// inference. It intentionally has no dependency on a proxy/routing SDK.
type Record struct {
	ID           string
	RequestedAt  time.Time
	LatencyMs    int64
	TTFTMs       int64
	Provider     string
	Model        string
	Source       string
	Endpoint     string
	Result       EventResult
	HTTPStatus   int
	ErrorCode    string
	ErrorSummary string
	Usage        *TokenUsage
	ServiceTier  string
	BillingAtom  string
}

type Recorder struct {
	store      *Store
	calculator *Calculator
}

func NewRecorder(store *Store, calculator *Calculator) *Recorder {
	return &Recorder{store: store, calculator: calculator}
}

// Record writes exactly one event for the given idempotency key. A missing
// provider usage envelope is stored as usage_status=missing with zero token
// counters; consumers must not render that as free consumption.
func (r *Recorder) Record(record Record) (bool, error) {
	if record.ID == "" {
		sum := sha256.Sum256([]byte("natives-direct|" + record.BillingAtom + "|" + record.RequestedAt.UTC().Format(time.RFC3339Nano)))
		record.ID = "evt_" + hex.EncodeToString(sum[:16])
	}
	if record.BillingAtom == "" {
		record.BillingAtom = record.ID
	}
	usageStatus := "missing"
	input, output, cacheRead, cacheWrite, reasoning, total := int64(0), int64(0), int64(0), int64(0), int64(0), int64(0)
	if record.Usage != nil {
		usageStatus = "reported"
		input = nonNegative(record.Usage.InputTokens)
		output = nonNegative(record.Usage.OutputTokens)
		cacheRead = nonNegative(record.Usage.CacheReadTokens)
		cacheWrite = nonNegative(record.Usage.CacheWriteTokens)
		reasoning = nonNegative(record.Usage.ReasoningTokens)
		if reasoning > output {
			reasoning = output
		}
		total = nonNegative(record.Usage.TotalTokens)
		if total == 0 && (input > 0 || output > 0 || cacheRead > 0 || cacheWrite > 0) {
			total = input + output + cacheRead + cacheWrite
		}
	}
	costMicro := int64(0)
	if record.Usage != nil && r.calculator != nil {
		costMicro = r.calculator.CalculateCost(record.Provider, record.Model, input, output, cacheRead, cacheWrite)
	}
	event := &Event{
		ID:               record.ID,
		RequestedAt:      record.RequestedAt,
		LatencyMs:        record.LatencyMs,
		TTFTMs:           record.TTFTMs,
		Provider:         record.Provider,
		Model:            record.Model,
		Source:           record.Source,
		Endpoint:         record.Endpoint,
		Result:           record.Result,
		HTTPStatus:       record.HTTPStatus,
		ErrorCode:        record.ErrorCode,
		ErrorSummary:     record.ErrorSummary,
		InputTokens:      input,
		OutputTokens:     output,
		CacheReadTokens:  cacheRead,
		CacheWriteTokens: cacheWrite,
		ReasoningTokens:  reasoning,
		TotalTokens:      total,
		UsageStatus:      usageStatus,
		CostMicro:        costMicro,
		ServiceTier:      record.ServiceTier,
		BillingAtom:      record.BillingAtom,
		CreatedAt:        time.Now().UTC(),
	}
	return r.store.InsertEventIfAbsent(event)
}

func nonNegative(value int64) int64 {
	if value < 0 {
		return 0
	}
	return value
}
