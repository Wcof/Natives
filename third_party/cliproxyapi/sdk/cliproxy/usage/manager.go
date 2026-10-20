package usage

import (
	"context"
	"fmt"
	"net/http"
	"strings"
	"sync"
	"time"

	"github.com/google/uuid"
	internallogging "github.com/router-for-me/CLIProxyAPI/v8/internal/logging"
	log "github.com/sirupsen/logrus"
)

// DefaultServiceTier is retained for direct SDK and non-OpenAI usage callers.
const DefaultServiceTier = "default"

// AutoServiceTier is the OpenAI request semantics when service_tier is omitted.
// OpenAI HTTP handlers set it explicitly, without changing other providers'
// historical direct-SDK default.
const AutoServiceTier = "auto"

// Record contains the usage statistics captured for a single provider request.
type Record struct {
	// RequestID uniquely identifies this specific model execution instance (UUID v4).
	RequestID string
	// TraceID identifies the parent inbound HTTP request when available (8-character hex).
	TraceID  string
	Provider string
	// BaseURL stores the configured upstream base URL when available.
	BaseURL string
	// ExecutorType stores the concrete executor type that handled the request.
	ExecutorType string
	Model        string
	Alias        string
	APIKey       string
	// IsNativeKey reports whether the client API key was authenticated by CPA's native config provider.
	IsNativeKey bool
	// AccessProvider identifies the client request authentication provider (e.g. "config-inline" or plugin ID).
	AccessProvider  string
	SessionID       string
	ParentSessionID string
	AuthID          string
	AuthIndex       string
	// AccessTokenSHA256 identifies the OAuth token version without exposing the token.
	AccessTokenSHA256 string
	AuthType          string
	Source            string
	// ReasoningEffort stores the translated upstream thinking level for request event logs.
	ReasoningEffort string
	// ServiceTier stores the client-requested service tier.
	ServiceTier string
	// RequestServiceTier is a deprecated input-only alias retained for existing
	// plugin callers. It is normalized into ServiceTier and never emitted.
	RequestServiceTier string
	// ResponseServiceTier stores the final tier reported by the upstream response.
	ResponseServiceTier string
	// ResponseModel stores the model name reported by the upstream response, empty when unknown.
	ResponseModel string
	// Generate reports whether the client requested actual generation.
	// nil or true means generation is enabled; only an explicit false disables generation.
	// Use GenerateFlag to set the value and GenerateEnabled to read it with the default.
	Generate *bool
	// Stream reports whether the request was executed in streaming mode.
	Stream      bool
	RequestedAt time.Time
	Latency     time.Duration
	TTFT        time.Duration
	Failed      bool
	Fail        Failure
	Detail      Detail
	// ResponseHeaders stores a snapshot of upstream response headers for usage sinks.
	ResponseHeaders http.Header
}

// Failure holds HTTP failure metadata for an upstream request attempt.
type Failure struct {
	StatusCode int
	Body       string
}

// Detail holds the token usage breakdown.
type Detail struct {
	InputTokens         int64
	OutputTokens        int64
	ReasoningTokens     int64
	CachedTokens        int64
	CacheReadTokens     int64
	CacheCreationTokens int64
	TotalTokens         int64
	TokenBreakdown      TokenBreakdown
	ResponseServiceTier string
}

type requestedModelAliasContextKey struct{}
type reasoningEffortContextKey struct{}
type serviceTierContextKey struct{}
type generateContextKey struct{}
type streamContextKey struct{}
type executionRequestIDContextKey struct{}
type executionTraceIDContextKey struct{}
type apiKeyContextKey struct{}
type accessProviderContextKey struct{}
type isNativeKeyContextKey struct{}

// WithExecutionRequestID attaches a specific execution instance request ID to the context.
func WithExecutionRequestID(ctx context.Context, requestID string) context.Context {
	if ctx == nil {
		return context.Background()
	}
	return context.WithValue(ctx, executionRequestIDContextKey{}, strings.TrimSpace(requestID))
}

// ExecutionRequestIDFromContext retrieves the execution instance request ID from the context.
func ExecutionRequestIDFromContext(ctx context.Context) string {
	if ctx == nil {
		return ""
	}
	if v, ok := ctx.Value(executionRequestIDContextKey{}).(string); ok {
		return v
	}
	return ""
}

// WithTraceID attaches the parent inbound HTTP request ID to the context.
func WithTraceID(ctx context.Context, traceID string) context.Context {
	if ctx == nil {
		return context.Background()
	}
	return context.WithValue(ctx, executionTraceIDContextKey{}, strings.TrimSpace(traceID))
}

// TraceIDFromContext retrieves the parent inbound HTTP request ID from the context.
func TraceIDFromContext(ctx context.Context) string {
	if ctx == nil {
		return ""
	}
	if v, ok := ctx.Value(executionTraceIDContextKey{}).(string); ok {
		return v
	}
	return ""
}

// WithRequestedModelAlias stores the client-requested model name for usage sinks.
func WithRequestedModelAlias(ctx context.Context, alias string) context.Context {
	if ctx == nil {
		ctx = context.Background()
	}
	alias = strings.TrimSpace(alias)
	if alias == "" {
		return ctx
	}
	return context.WithValue(ctx, requestedModelAliasContextKey{}, alias)
}

// RequestedModelAliasFromContext returns the client-requested model name stored in ctx.
func RequestedModelAliasFromContext(ctx context.Context) string {
	if ctx == nil {
		return ""
	}
	raw := ctx.Value(requestedModelAliasContextKey{})
	switch value := raw.(type) {
	case string:
		return strings.TrimSpace(value)
	case []byte:
		return strings.TrimSpace(string(value))
	default:
		return ""
	}
}

// WithReasoningEffort stores the client-requested reasoning effort for usage sinks.
func WithReasoningEffort(ctx context.Context, effort string) context.Context {
	if ctx == nil {
		ctx = context.Background()
	}
	effort = strings.TrimSpace(effort)
	if effort == "" {
		return ctx
	}
	return context.WithValue(ctx, reasoningEffortContextKey{}, effort)
}

// ReasoningEffortFromContext returns the client-requested reasoning effort stored in ctx.
func ReasoningEffortFromContext(ctx context.Context) string {
	if ctx == nil {
		return ""
	}
	raw := ctx.Value(reasoningEffortContextKey{})
	switch value := raw.(type) {
	case string:
		return strings.TrimSpace(value)
	case []byte:
		return strings.TrimSpace(string(value))
	default:
		return ""
	}
}

// WithServiceTier stores the client-requested service tier for usage sinks.
func WithServiceTier(ctx context.Context, tier string) context.Context {
	if ctx == nil {
		ctx = context.Background()
	}
	tier = strings.TrimSpace(tier)
	if tier == "" {
		tier = DefaultServiceTier
	}
	return context.WithValue(ctx, serviceTierContextKey{}, tier)
}

// ServiceTierFromContext returns the client-requested service tier stored in ctx.
func ServiceTierFromContext(ctx context.Context) string {
	if ctx == nil {
		return DefaultServiceTier
	}
	raw := ctx.Value(serviceTierContextKey{})
	switch value := raw.(type) {
	case string:
		tier := strings.TrimSpace(value)
		if tier == "" {
			return DefaultServiceTier
		}
		return tier
	case []byte:
		tier := strings.TrimSpace(string(value))
		if tier == "" {
			return DefaultServiceTier
		}
		return tier
	default:
		return DefaultServiceTier
	}
}

// WithGenerate stores whether the client requested actual generation for usage sinks.
// Missing context values default to true; only an explicit false disables generation.
func WithGenerate(ctx context.Context, generate bool) context.Context {
	if ctx == nil {
		ctx = context.Background()
	}
	return context.WithValue(ctx, generateContextKey{}, generate)
}

// GenerateFromContext returns whether the client requested actual generation.
// Missing values default to true.
func GenerateFromContext(ctx context.Context) bool {
	if ctx == nil {
		return true
	}
	raw := ctx.Value(generateContextKey{})
	switch value := raw.(type) {
	case bool:
		return value
	default:
		return true
	}
}

// WithStream stores whether the request was executed in streaming mode for usage sinks.
func WithStream(ctx context.Context, stream bool) context.Context {
	if ctx == nil {
		ctx = context.Background()
	}
	return context.WithValue(ctx, streamContextKey{}, stream)
}

// StreamFromContext returns whether the request was executed in streaming mode.
// Missing values default to false.
func StreamFromContext(ctx context.Context) bool {
	if ctx == nil {
		return false
	}
	raw := ctx.Value(streamContextKey{})
	switch value := raw.(type) {
	case bool:
		return value
	default:
		return false
	}
}

// WithAccessProvider stores the client authentication provider identifier for usage sinks.
func WithAccessProvider(ctx context.Context, provider string) context.Context {
	if ctx == nil {
		ctx = context.Background()
	}
	provider = strings.TrimSpace(provider)
	if provider == "" {
		return ctx
	}
	return context.WithValue(ctx, accessProviderContextKey{}, provider)
}

// WithAPIKey stores the client API key for usage sinks.
func WithAPIKey(ctx context.Context, apiKey string) context.Context {
	if ctx == nil {
		ctx = context.Background()
	}
	apiKey = strings.TrimSpace(apiKey)
	if apiKey == "" {
		return ctx
	}
	return context.WithValue(ctx, apiKeyContextKey{}, apiKey)
}

// APIKeyFromContext returns the client API key stored in ctx or gin.Context.
func APIKeyFromContext(ctx context.Context) string {
	if ctx == nil {
		return ""
	}
	raw := ctx.Value(apiKeyContextKey{})
	switch value := raw.(type) {
	case string:
		return strings.TrimSpace(value)
	case []byte:
		return strings.TrimSpace(string(value))
	}
	if ginCtx, ok := ctx.Value("gin").(interface{ Get(string) (any, bool) }); ok && ginCtx != nil {
		if v, exists := ginCtx.Get("userApiKey"); exists {
			switch value := v.(type) {
			case string:
				return strings.TrimSpace(value)
			case fmt.Stringer:
				return strings.TrimSpace(value.String())
			default:
				return strings.TrimSpace(fmt.Sprintf("%v", value))
			}
		}
	}
	return ""
}

// AccessProviderFromContext returns the client authentication provider stored in ctx.
func AccessProviderFromContext(ctx context.Context) string {
	if ctx == nil {
		return ""
	}
	raw := ctx.Value(accessProviderContextKey{})
	switch value := raw.(type) {
	case string:
		return strings.TrimSpace(value)
	case []byte:
		return strings.TrimSpace(string(value))
	}
	if ginCtx, ok := ctx.Value("gin").(interface{ Get(string) (any, bool) }); ok && ginCtx != nil {
		if v, exists := ginCtx.Get("accessProvider"); exists {
			switch value := v.(type) {
			case string:
				return strings.TrimSpace(value)
			case []byte:
				return strings.TrimSpace(string(value))
			}
		}
	}
	return ""
}

// WithIsNativeKey stores whether the client API key is native to CPA for usage sinks.
func WithIsNativeKey(ctx context.Context, isNative bool) context.Context {
	if ctx == nil {
		ctx = context.Background()
	}
	return context.WithValue(ctx, isNativeKeyContextKey{}, isNative)
}

// IsNativeKeyFromContext returns whether the client API key is native to CPA if explicitly set in ctx.
func IsNativeKeyFromContext(ctx context.Context) (bool, bool) {
	if ctx == nil {
		return false, false
	}
	raw := ctx.Value(isNativeKeyContextKey{})
	if value, ok := raw.(bool); ok {
		return value, true
	}
	return false, false
}

// IsNativeAccessProvider reports whether provider represents CPA's built-in inline key provider.
func IsNativeAccessProvider(provider string) bool {
	p := strings.TrimSpace(provider)
	return strings.EqualFold(p, "config-inline") || strings.EqualFold(p, "config-api-key")
}

// GenerateFlag returns a pointer suitable for Record.Generate.
func GenerateFlag(generate bool) *bool {
	return &generate
}

// GenerateEnabled reports whether generation is enabled for the record field.
// A nil value defaults to true so legacy callers that omit Generate keep the historical behavior.
func GenerateEnabled(generate *bool) bool {
	if generate == nil {
		return true
	}
	return *generate
}

// Plugin consumes usage records emitted by the proxy runtime.
type Plugin interface {
	HandleUsage(ctx context.Context, record Record)
}

type queueItem struct {
	record  Record
	payload int // approximate queued byte weight of the record
}

// Queue budget constants. The constructor capacity must be real: Publish never
// grows the queue beyond `buffer` items or `maxQueueBytes` total bytes. Records
// waiting in the queue must not pin request contexts, oversized response bodies
// or credential-bearing header values (Natives lifecycle plan L3/F1).
const (
	defaultQueueBuffer    = 512
	maxQueueBytes         = 8 << 20 // 8 MiB total queued payload
	maxFailBodyBytes      = 4 << 10 // 4 KiB diagnostic failure body prefix
	maxResponseHeaderData = 8 << 10 // 8 KiB total response-header snapshot bytes
)

// Manager maintains a bounded queue of usage records and delivers them to
// registered plugins. The queue applies backpressure when full (Publish waits
// until space frees or the manager is stopped) instead of growing without
// bound; Stop drains pending records, joins the dispatcher and never allows
// Publish to silently resurrect it.
type Manager struct {
	cancel context.CancelFunc
	buffer int

	mu      sync.Mutex
	cond    *sync.Cond
	queue   []queueItem
	bytes   int
	running bool
	closed  bool
	exited  chan struct{} // closed when the dispatcher goroutine fully returns

	pluginsMu sync.RWMutex
	plugins   []Plugin
	named     map[string]int
}

// NewManager constructs a manager with a bounded queue. Non-positive buffers
// fall back to the 512-slot default.
func NewManager(buffer int) *Manager {
	if buffer <= 0 {
		buffer = defaultQueueBuffer
	}
	m := &Manager{buffer: buffer}
	m.cond = sync.NewCond(&m.mu)
	return m
}

// Start launches the background dispatcher. Calling Start multiple times is
// safe. Start after Stop re-opens the manager for gateway restarts; only
// Publish may not resurrect a stopped dispatcher.
func (m *Manager) Start(ctx context.Context) {
	if m == nil {
		return
	}
	m.mu.Lock()
	if m.running {
		m.mu.Unlock()
		return
	}
	if ctx == nil {
		ctx = context.Background()
	}
	var workerCtx context.Context
	workerCtx, m.cancel = context.WithCancel(ctx)
	m.running = true
	m.closed = false
	exited := make(chan struct{})
	m.exited = exited
	m.mu.Unlock()

	go m.run(workerCtx, exited)
}

// Stop stops the dispatcher, drains every queued record into the registered
// plugins and joins the dispatcher goroutine before returning. Repeated or
// concurrent Stop calls are safe and idempotent.
func (m *Manager) Stop() {
	if m == nil {
		return
	}
	m.mu.Lock()
	if !m.running && m.closed {
		m.mu.Unlock()
		return
	}
	if m.cancel != nil {
		m.cancel()
		m.cancel = nil
	}
	m.closed = true
	exited := m.exited
	// Wake Publish backpressure waiters: they fall through to synchronous
	// delivery once closed, and the dispatcher drains the remaining queue.
	m.cond.Broadcast()
	m.running = false
	m.mu.Unlock()
	m.cond.Broadcast()
	if exited != nil {
		// Bounded join: the dispatcher only drains its bounded queue into
		// plugin enqueue paths, so this normally returns immediately.
		select {
		case <-exited:
		case <-time.After(5 * time.Second):
			log.Warnf("usage: dispatcher drain did not finish within 5s; %d record(s) remain queued", m.queueLength())
		}
	}
}

// Register appends a plugin to the delivery list.
func (m *Manager) Register(plugin Plugin) {
	if m == nil || plugin == nil {
		return
	}
	m.pluginsMu.Lock()
	m.plugins = append(m.plugins, plugin)
	m.pluginsMu.Unlock()
}

// RegisterNamed registers or replaces a plugin by name.
func (m *Manager) RegisterNamed(name string, plugin Plugin) {
	if m == nil || plugin == nil {
		return
	}
	name = strings.TrimSpace(name)
	if name == "" {
		return
	}

	m.pluginsMu.Lock()
	if m.named == nil {
		m.named = make(map[string]int)
	}
	if index, exists := m.named[name]; exists && index >= 0 && index < len(m.plugins) {
		m.plugins[index] = plugin
		m.pluginsMu.Unlock()
		return
	}
	m.named[name] = len(m.plugins)
	m.plugins = append(m.plugins, plugin)
	m.pluginsMu.Unlock()
}

// Publish enqueues a usage record for processing. If no plugin is registered
// the record will be discarded downstream. The queue is bounded: when full,
// Publish applies cancellable backpressure (waiting on manager Stop or queue
// space, never on the caller's request context, so a completed upstream
// request's usage is not dropped by a downstream disconnect). A record
// published after Stop is delivered synchronously to the still-registered
// plugins instead of resurrecting the dispatcher.
func (m *Manager) Publish(ctx context.Context, record Record) {
	if m == nil {
		return
	}
	if strings.TrimSpace(record.RequestID) == "" {
		if reqID := ExecutionRequestIDFromContext(ctx); reqID != "" {
			record.RequestID = reqID
		} else {
			record.RequestID = uuid.NewString()
		}
	}
	if strings.TrimSpace(record.TraceID) == "" {
		if trID := TraceIDFromContext(ctx); trID != "" {
			record.TraceID = trID
		} else if trID := internallogging.GetRequestID(ctx); trID != "" {
			record.TraceID = trID
		}
	}
	if strings.TrimSpace(record.APIKey) == "" {
		if key := APIKeyFromContext(ctx); key != "" {
			record.APIKey = key
		}
	}
	if strings.TrimSpace(record.AccessProvider) == "" {
		if provider := AccessProviderFromContext(ctx); provider != "" {
			record.AccessProvider = provider
		}
	}
	if !record.IsNativeKey {
		if nativeExplicit, ok := IsNativeKeyFromContext(ctx); ok {
			record.IsNativeKey = nativeExplicit
		} else if record.AccessProvider != "" {
			record.IsNativeKey = IsNativeAccessProvider(record.AccessProvider)
		}
	}
	// Bound record weight before queueing: diagnostic bodies and response
	// header snapshots are trimmed; billing atoms (tokens, cost inputs,
	// identity) are never altered.
	trimRecordForQueue(&record)
	payload := recordWeight(record)

	m.mu.Lock()
	// ensure worker is running even if Start was not called explicitly,
	// but never resurrect a manager that Stop already drained and closed.
	if !m.running && !m.closed {
		m.mu.Unlock()
		m.Start(context.Background())
		m.mu.Lock()
	}
	if m.closed {
		// Stopped manager: synchronous best-effort delivery to still-open
		// plugins keeps late records persistable without a new dispatcher.
		m.mu.Unlock()
		m.dispatch(queueItem{record: record, payload: payload})
		return
	}
	for len(m.queue) >= m.buffer || m.bytes+payload > maxQueueBytes {
		m.cond.Wait()
		if m.closed {
			m.mu.Unlock()
			m.dispatch(queueItem{record: record, payload: payload})
			return
		}
	}
	m.queue = append(m.queue, queueItem{record: record, payload: payload})
	m.bytes += payload
	m.mu.Unlock()
	m.cond.Signal()
}

func (m *Manager) queueLength() int {
	m.mu.Lock()
	defer m.mu.Unlock()
	return len(m.queue)
}

func (m *Manager) run(ctx context.Context, exited chan struct{}) {
	defer func() {
		m.mu.Lock()
		remaining := m.queue
		m.queue = nil
		m.bytes = 0
		m.mu.Unlock()
		for _, item := range remaining {
			m.dispatch(item)
		}
		// Only the dispatcher created with this exited channel closes it;
		// a restarted manager installs a fresh channel, so a late return of
		// a previous dispatcher can never double-close the new one.
		close(exited)
	}()
	for {
		m.mu.Lock()
		for !m.closed && len(m.queue) == 0 {
			m.cond.Wait()
		}
		if len(m.queue) == 0 && m.closed {
			m.mu.Unlock()
			return
		}
		item := m.queue[0]
		m.queue[0] = queueItem{} // release the consumed slot reference
		m.queue = m.queue[1:]
		m.bytes -= item.payload
		m.mu.Unlock()
		m.cond.Broadcast() // wake backpressure waiters
		m.dispatch(item)
	}
}

func (m *Manager) dispatch(item queueItem) {
	m.pluginsMu.RLock()
	plugins := make([]Plugin, len(m.plugins))
	copy(plugins, m.plugins)
	m.pluginsMu.RUnlock()
	if len(plugins) == 0 {
		return
	}
	// The request context is intentionally not retained in the queue; plugins
	// receive a detached context because usage persistence must not be
	// cancelled by downstream HTTP disconnects.
	for _, plugin := range plugins {
		if plugin == nil {
			continue
		}
		safeInvoke(plugin, context.Background(), item.record)
	}
}

// recordWeight approximates the queued byte weight of a record.
func recordWeight(record Record) int {
	weight := 128 // struct baseline
	for _, value := range []string{
		record.RequestID, record.TraceID, record.Provider, record.BaseURL,
		record.ExecutorType, record.Model, record.Alias, record.APIKey,
		record.SessionID, record.ParentSessionID, record.AuthID, record.AuthIndex,
		record.AccessTokenSHA256, record.AuthType, record.Source,
		record.ReasoningEffort, record.ServiceTier, record.RequestServiceTier,
		record.ResponseServiceTier, record.ResponseModel, record.Fail.Body,
	} {
		weight += len(value)
	}
	for key, values := range record.ResponseHeaders {
		weight += len(key)
		for _, value := range values {
			weight += len(value)
		}
	}
	return weight
}

// trimRecordForQueue bounds diagnostic payloads retained while queued:
// failure bodies are cut to a fixed prefix and response-header snapshots are
// trimmed to a total byte budget. Tokens, costs, identity and model fields
// are preserved exactly so billing and dedup stay correct.
func trimRecordForQueue(record *Record) {
	if len(record.Fail.Body) > maxFailBodyBytes {
		record.Fail.Body = record.Fail.Body[:maxFailBodyBytes] + "...[truncated]"
	}
	if record.ResponseHeaders == nil {
		return
	}
	total := 0
	for key, values := range record.ResponseHeaders {
		total += len(key)
		for _, value := range values {
			total += len(value)
		}
	}
	if total <= maxResponseHeaderData {
		return
	}
	trimmed := make(http.Header, len(record.ResponseHeaders))
	used := 0
	for key, values := range record.ResponseHeaders {
		if used+len(key) > maxResponseHeaderData {
			break
		}
		used += len(key)
		for _, value := range values {
			if used+len(value) > maxResponseHeaderData {
				break
			}
			used += len(value)
			trimmed[key] = append(trimmed[key], value)
		}
	}
	record.ResponseHeaders = trimmed
}

func safeInvoke(plugin Plugin, ctx context.Context, record Record) {
	defer func() {
		if r := recover(); r != nil {
			log.Errorf("usage: plugin panic recovered: %v", r)
		}
	}()
	plugin.HandleUsage(ctx, record)
}

var defaultManager = NewManager(512)

// DefaultManager returns the global usage manager instance.
func DefaultManager() *Manager { return defaultManager }

// RegisterPlugin registers a plugin on the default manager.
func RegisterPlugin(plugin Plugin) { DefaultManager().Register(plugin) }

// RegisterNamedPlugin registers or replaces a named plugin on the default manager.
func RegisterNamedPlugin(name string, plugin Plugin) { DefaultManager().RegisterNamed(name, plugin) }

// PublishRecord publishes a record using the default manager.
func PublishRecord(ctx context.Context, record Record) { DefaultManager().Publish(ctx, record) }

// StartDefault starts the default manager's dispatcher.
func StartDefault(ctx context.Context) { DefaultManager().Start(ctx) }

// StopDefault stops the default manager's dispatcher.
func StopDefault() { DefaultManager().Stop() }
