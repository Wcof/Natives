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
	// CostStatus 区分"未计价"与合法零价：priced=价格命中（金额可为 0）；
	// unpriced=当前目录无该 (provider, model) 价格。读时判定，由 handler 注解。
	// BillingAtom 是唯一计费原子（ADR-0030 决策 2）：同一请求跨来源
	//（Proxy/原生日志/OTel）只计一次；空串表示旧数据/未知来源不参与去重。
	BillingAtom string `json:"billingAtom,omitempty"`
	SessionID   string `json:"sessionId,omitempty"`
	// 三元来源实例身份（方案 §4.1）：会话唯一键 =
	// (tool_id, source_instance_id, session_id)。实例 ID 由授权根目录摘要
	// 派生或为 legacy-default；不得以空字符串冒充实例语义。
	ToolID           string    `json:"toolId,omitempty"`
	SourceInstanceID string    `json:"sourceInstanceId,omitempty"`
	CostStatus       string    `json:"costStatus,omitempty"`
	ServiceTier      string    `json:"serviceTier,omitempty"`
	CreatedAt        time.Time `json:"createdAt"`
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
	// Timezone 为用户 IANA 时区（方案 §4.2）：活跃天数/日桶/today 范围按它
	// 计算；不识别的时区在 handler 校验时报参数错误，不静默退回 UTC。
	Timezone string `json:"timezone,omitempty"`
	Offset   int    `json:"offset,omitempty"`
	Limit    int    `json:"limit,omitempty"`
	SortBy   string `json:"sortBy,omitempty"`  // "requested_at", "latency", "tokens", "cost"
	SortDir  string `json:"sortDir,omitempty"` // "asc", "desc"
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

// SessionStat 为一次真实会话聚合（R3，方案 §4.1.1）。
// Key 是 (toolId, sourceInstanceId, sessionId) 的稳定三元组合键；
// 同 session ID 跨工具、跨 profile/安装实例不合并。
type SessionStat struct {
	Key              string  `json:"key"`
	ToolID           string  `json:"toolId,omitempty"`
	SourceInstanceID string  `json:"sourceInstanceId,omitempty"`
	Source           string  `json:"source,omitempty"`
	Requests         int64   `json:"requests"`
	Tokens           int64   `json:"tokens"`
	CostUSD          float64 `json:"costUsd"`
	FirstAt          string  `json:"firstAt,omitempty"`
	LastAt           string  `json:"lastAt,omitempty"`
}

// DayCount 为单日 distinct 会话数；每日独立去重，不得跨日相加当区间总数。
type DayCount struct {
	Date     string `json:"date"`
	Sessions int64  `json:"sessions"`
}

// SessionsResult 为 model_usage_sessions 的返回：真实 distinct 会话统计，
// 与请求数严格分开；无 session_id 的记录只计入 Unattributed，不造会话。
type SessionsResult struct {
	Status        string        `json:"status"`
	TotalSessions int64         `json:"totalSessions"`
	ActiveDays    int64         `json:"activeDays"`
	Unattributed  int64         `json:"unattributedRequests"`
	ByDay         []DayCount    `json:"byDay,omitempty"`
	BySource      []RankItem    `json:"bySource,omitempty"`
	Sessions      []SessionStat `json:"sessions,omitempty"`
	Total         int64         `json:"total"`
	Page          int           `json:"page"`
	PageSize      int           `json:"pageSize"`
	HasMore       bool          `json:"hasMore"`
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
	// billing_csv 预览附加字段（§7.1）：币种三口径与逐行错误。
	CurrencySummaries []ReconciliationSummary `json:"currencySummaries,omitempty"`
	Kinds             map[string]int64        `json:"kinds,omitempty"`
	LineErrors        []BillingPreviewLine    `json:"lineErrors,omitempty"`
}
