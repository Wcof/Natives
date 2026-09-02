package usage

import (
	"time"
)

type EventResult string

const (
	ResultSuccess   EventResult = "success"
	ResultFailed    EventResult = "failed"
	ResultCancelled EventResult = "cancelled"
)

type PriceSource string

const (
	PriceSourceManual       PriceSource = "manual"
	PriceSourceManualGlobal PriceSource = "manual_global"
	PriceSourceSynced       PriceSource = "synced"
	PriceSourceBuiltin      PriceSource = "builtin"
	PriceSourceImported     PriceSource = "imported"
)

type Event struct {
	ID               string      `json:"id"`
	RequestedAt      time.Time   `json:"requestedAt"`
	LatencyMs        int64       `json:"latencyMs"`
	TTFTMs           int64       `json:"ttftMs"`
	Provider         string      `json:"provider"`
	AccountID        string      `json:"accountId,omitempty"`
	Model            string      `json:"model"`
	ModelAlias       string      `json:"modelAlias,omitempty"`
	Source           string      `json:"source,omitempty"`
	Endpoint         string      `json:"endpoint,omitempty"`
	AccessKeyID      string      `json:"accessKeyId,omitempty"`
	AccessKeyName    string      `json:"accessKeyName,omitempty"`
	Result           EventResult `json:"result"`
	HTTPStatus       int         `json:"httpStatus"`
	ErrorCode        string      `json:"errorCode,omitempty"`
	ErrorSummary     string      `json:"errorSummary,omitempty"`
	InputTokens      int64       `json:"inputTokens"`
	OutputTokens     int64       `json:"outputTokens"`
	CacheReadTokens  int64       `json:"cacheReadTokens"`
	CacheWriteTokens int64       `json:"cacheWriteTokens"`
	ReasoningTokens  int64       `json:"reasoningTokens"`
	TotalTokens      int64       `json:"totalTokens"`
	CostMicro        int64       `json:"costMicro"` // in millionths of USD ($0.000001)
	ServiceTier      string      `json:"serviceTier,omitempty"`
	CreatedAt        time.Time   `json:"createdAt"`
}

type Price struct {
	ID                   string      `json:"id"`
	ProviderID           string      `json:"providerId,omitempty"`
	ModelID              string      `json:"modelId"`
	InputPriceMicro      int64       `json:"inputPriceMicro"`      // $ per 1M tokens in millionths of USD
	OutputPriceMicro     int64       `json:"outputPriceMicro"`     // $ per 1M tokens
	CacheReadPriceMicro  int64       `json:"cacheReadPriceMicro"`  // $ per 1M tokens
	CacheWritePriceMicro int64       `json:"cacheWritePriceMicro"` // $ per 1M tokens
	Source               PriceSource `json:"source"`
	UpdatedAt            string      `json:"updatedAt"`
}

type Filter struct {
	Range        string       `json:"range,omitempty"` // "4h", "24h", "today", "7d", "30d", "all", "custom"
	StartTime    *time.Time   `json:"startTime,omitempty"`
	EndTime      *time.Time   `json:"endTime,omitempty"`
	Models       []string     `json:"models,omitempty"`
	Providers    []string     `json:"providers,omitempty"`
	Sources      []string     `json:"sources,omitempty"`
	AccessKeyIDs []string     `json:"accessKeyIds,omitempty"`
	Result       *EventResult `json:"result,omitempty"`
	Offset       int          `json:"offset,omitempty"`
	Limit        int          `json:"limit,omitempty"`
	SortBy       string       `json:"sortBy,omitempty"`  // "requested_at", "latency", "tokens", "cost"
	SortDir      string       `json:"sortDir,omitempty"` // "asc", "desc"
}

type MetricCards struct {
	TotalRequests    int64   `json:"totalRequests"`
	TotalTokens      int64   `json:"totalTokens"`
	SuccessRate      float64 `json:"successRate"`
	TPS              float64 `json:"tps"`
	CacheHitRate     float64 `json:"cacheHitRate"`
	EstimatedCostUSD float64 `json:"estimatedCostUsd"`
}

type TrendPoint struct {
	Timestamp string  `json:"timestamp"`
	Requests  int64   `json:"requests"`
	Tokens    int64   `json:"tokens"`
	CostUSD   float64 `json:"costUsd"`
}

type TokenComposition struct {
	Input      int64 `json:"input"`
	Output     int64 `json:"output"`
	CacheRead  int64 `json:"cacheRead"`
	CacheWrite int64 `json:"cacheWrite"`
	Reasoning  int64 `json:"reasoning"`
}

type OverviewResult struct {
	Status           string           `json:"status"` // "ready", "empty", "degraded"
	TotalEventsCount int64            `json:"totalEventsCount"`
	Metrics          MetricCards      `json:"metrics"`
	Trend            []TrendPoint     `json:"trend"`
	Tokens           TokenComposition `json:"tokens"`
}

type RankItem struct {
	Key      string  `json:"key"`
	Name     string  `json:"name"`
	Requests int64   `json:"requests"`
	Tokens   int64   `json:"tokens"`
	CostUSD  float64 `json:"costUsd"`
	Percent  float64 `json:"percent"`
}

type AnalyticsResult struct {
	Status      string     `json:"status"`
	ByModel     []RankItem `json:"byModel"`
	ByProvider  []RankItem `json:"byProvider"`
	BySource    []RankItem `json:"bySource"`
	ByAccessKey []RankItem `json:"byAccessKey"`
	ByHour      []RankItem `json:"byHour"`
}

type EventsResult struct {
	Events   []Event `json:"events"`
	Total    int64   `json:"total"`
	Page     int     `json:"page"`
	PageSize int     `json:"pageSize"`
	HasMore  bool    `json:"hasMore"`
}

type PricingResult struct {
	Prices                []Price `json:"prices"`
	CatalogVersion        string  `json:"catalogVersion"`
	TotalEstimatedCostUSD float64 `json:"totalEstimatedCostUsd"`
	PricedRequestsCount   int64   `json:"pricedRequestsCount"`
	UnpricedRequestsCount int64   `json:"unpricedRequestsCount"`
}

type ImportPreviewResult struct {
	Valid          bool   `json:"valid"`
	TotalRecords   int64  `json:"totalRecords"`
	EarliestRecord string `json:"earliestRecord,omitempty"`
	LatestRecord   string `json:"latestRecord,omitempty"`
	PricesCount    int64  `json:"pricesCount"`
	DuplicateCount int64  `json:"duplicateCount"`
	Error          string `json:"error,omitempty"`
}
