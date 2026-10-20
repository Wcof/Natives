package host

// ADR-0033：Model Host 只读 AI 数据查询 loopback HTTP 服务（127.0.0.1:8766）。
//
// 面向进程外本地消费者（macOS 顶栏等）：AI 用量/额度数据的第三个传输面
// （扩展=Native Messaging、子应用=8765、进程外=8766）。数据权威与聚合口径
// 与 Native Messaging model_usage_* 方法族完全一致——本文件只做传输适配，
// 直接复用 Engine 现有 getUsage*/queryQuota 处理器，不另建数据路径。
//
// 安全边界（R-S5 / ADR-0033 §2.1.3，全部 MUST）：
//   - 仅绑定 127.0.0.1；逐请求校验 Host 头（防 DNS rebinding 与跨源注入）；
//   - 不返回任何 CORS 头（无浏览器消费场景）；
//   - 请求体上限 64KB；并发上限 8（超出 503）；每客户端 10 req/s（超出 429）；
//   - 响应严禁包含 access/refresh token、API key、凭据内容（R-S4：仅允许
//     百分比、时间戳等派生值）——复用现有只读处理器天然满足，额度缓存另经
//     结构体重建兜底，任何未知字段都不会进入响应；
//   - 不暴露 CLIProxyAPI Management API、任意文件、进程、SQL 能力
//     （ADR-0020 边界不变）。
//
// 全服务唯一允许发起上游请求的路由是 POST /ai/quota/refresh（用户显式动作触发）。

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"os"
	"strconv"
	"strings"
	"sync"
	"time"

	log "github.com/sirupsen/logrus"

	"github.com/ldh/natives/model-host/internal/authfiles"
	"github.com/ldh/natives/model-host/internal/quota"
	"github.com/ldh/natives/model-host/internal/usage"
)

const (
	aiQueryPort           = 8766
	aiQueryBodyLimit      = 64 * 1024
	aiQueryConcurrencyMax = 8
	aiQueryRatePerSecond  = 10
)

type aiQueryServer struct {
	engine      *Engine
	listener    net.Listener
	server      *http.Server
	allowedHost map[string]bool
	gate        chan struct{}
	refreshMu   sync.Mutex // 同一时刻仅允许一次全量额度刷新
	mu          sync.Mutex
	buckets     map[string]*aiRateBucket
}

// aiRateBucket 为每客户端令牌桶（容量=速率，突发不超过 10 次/秒）。
type aiRateBucket struct {
	tokens float64
	lastAt time.Time
}

// startAIQueryServer 在 Engine 创建后启动只读查询端口。绑定失败（端口被占用
// 等）仅告警并停用该增强面，不阻断 Native Messaging 主功能；ADR-0033 §2.1.4
// 要求消费方把「8766 不可达」视为常态并自行降级，因此本侧零强依赖。
func startAIQueryServer(engine *Engine) *aiQueryServer {
	listener, err := net.Listen("tcp", fmt.Sprintf("127.0.0.1:%d", aiQueryListenPort()))
	if err != nil {
		log.Warnf("AI 查询端口 8766 绑定失败，只读查询面停用：%v", err)
		return nil
	}
	return newAIQueryServer(engine, listener)
}

// newAIQueryServer 基于已就绪的 listener 构建并启动服务（生产绑定固定端口，
// 测试可传随机端口 listener）。
func newAIQueryServer(engine *Engine, listener net.Listener) *aiQueryServer {
	port := listener.Addr().(*net.TCPAddr).Port
	server := &aiQueryServer{
		engine:   engine,
		listener: listener,
		allowedHost: map[string]bool{
			fmt.Sprintf("127.0.0.1:%d", port): true,
			fmt.Sprintf("localhost:%d", port): true,
		},
		gate:    make(chan struct{}, aiQueryConcurrencyMax),
		buckets: make(map[string]*aiRateBucket),
	}
	server.server = &http.Server{
		Handler:           server.handler(),
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       15 * time.Second,
		WriteTimeout:      120 * time.Second, // 额度刷新可能串行查询多个凭据
		IdleTimeout:       60 * time.Second,
	}
	go func() {
		if serveErr := server.server.Serve(listener); serveErr != nil && !errors.Is(serveErr, http.ErrServerClosed) {
			log.Warnf("AI 查询服务异常退出：%v", serveErr)
		}
	}()
	return server
}

// Close 停止接收新请求并等待在途请求收尾（Engine.Close 调用）。
func (s *aiQueryServer) Close() {
	if s == nil {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	_ = s.server.Shutdown(ctx)
	_ = s.listener.Close()
}

func (s *aiQueryServer) handler() http.Handler {
	return http.HandlerFunc(s.handle)
}

// handle 为逐请求入口：Host 校验 → 速率限制 → 并发闸门 → 请求体上限 →
// 白名单精确路由（未知路径一律 404）。
func (s *aiQueryServer) handle(w http.ResponseWriter, r *http.Request) {
	// 日志脱敏：仅记录方法、路径与状态，不记录查询串与响应内容。
	defer func() {
		log.Debugf("ai-query: %s %s", r.Method, r.URL.Path)
	}()
	if !s.allowedHost[r.Host] {
		writeAIError(w, http.StatusForbidden, "forbidden_host", "仅允许本机回环访问")
		return
	}
	if !s.allowClient(clientKey(r)) {
		writeAIError(w, http.StatusTooManyRequests, "rate_limited", "请求过于频繁，请稍后重试")
		return
	}
	select {
	case s.gate <- struct{}{}:
		defer func() { <-s.gate }()
	default:
		writeAIError(w, http.StatusServiceUnavailable, "busy", "查询并发已达上限，请稍后重试")
		return
	}
	r.Body = http.MaxBytesReader(w, r.Body, aiQueryBodyLimit)

	path := r.URL.Path
	switch {
	case path == "/ai/healthz":
		if !s.methodGuard(w, r, http.MethodGet) {
			return
		}
		writeAIJSON(w, http.StatusOK, map[string]string{"status": "ok"})
	case path == "/ai/quota":
		if !s.methodGuard(w, r, http.MethodGet) {
			return
		}
		s.serveQuotaCache(w)
	case path == "/ai/quota/refresh":
		if !s.methodGuard(w, r, http.MethodPost) {
			return
		}
		s.serveQuotaRefresh(r.Context(), w, r)
	case strings.HasPrefix(path, "/ai/usage/"):
		if !s.methodGuard(w, r, http.MethodGet) {
			return
		}
		s.serveUsage(w, strings.TrimPrefix(path, "/ai/usage/"), r.URL.Query())
	default:
		writeAIError(w, http.StatusNotFound, "not_found", "未知查询路径")
	}
}

// methodGuard 为已知路径做方法白名单；不匹配返回 405 并携带 Allow。
func (s *aiQueryServer) methodGuard(w http.ResponseWriter, r *http.Request, method string) bool {
	if r.Method == method {
		return true
	}
	w.Header().Set("Allow", method)
	writeAIError(w, http.StatusMethodNotAllowed, "method_not_allowed", "不支持的请求方法")
	return false
}

// serveUsage 把 /ai/usage/<name> 分发到与 model_usage_* 方法族相同的
// Engine 处理器；查询串参数先映射为与 NM 参数结构一致的 JSON。
func (s *aiQueryServer) serveUsage(w http.ResponseWriter, name string, query url.Values) {
	e := s.engine
	if e.usageStore == nil {
		writeAIError(w, http.StatusServiceUnavailable, "unavailable", "使用记录存储未就绪")
		return
	}
	var (
		result any
		err    error
	)
	switch name {
	case "overview":
		var raw json.RawMessage
		if raw, err = aiUsageFilterFromQuery(query); err == nil {
			result, err = e.getUsageOverview(raw)
		}
	case "analysis":
		var raw json.RawMessage
		if raw, err = aiUsageFilterFromQuery(query); err == nil {
			result, err = e.getUsageAnalysis(raw)
		}
	case "sessions":
		var raw json.RawMessage
		if raw, err = aiUsageFilterFromQuery(query); err == nil {
			result, err = e.getUsageSessions(raw)
		}
	case "events":
		var raw json.RawMessage
		if raw, err = aiUsageFilterFromQuery(query); err == nil {
			result, err = e.getUsageEvents(raw)
		}
	case "sources":
		result, err = e.getUsageSources()
	case "budgets":
		result, err = e.getUsageBudgets()
	case "billing":
		var raw json.RawMessage
		if raw, err = aiBillingParamsFromQuery(query); err == nil {
			result, err = e.getUsageBilling(raw)
		}
	case "attention":
		var raw json.RawMessage
		if raw, err = aiAttentionParamsFromQuery(query); err == nil {
			result, err = e.getUsageAttention(raw)
		}
	case "insights":
		result, err = e.getUsageInsights()
	case "pricing":
		result, err = e.getUsagePricing()
	default:
		writeAIError(w, http.StatusNotFound, "not_found", "未知用量查询路径")
		return
	}
	s.serveEngineResult(w, result, err)
}

// serveQuotaCache 返回最近一次额度探测缓存（quota-cache.json，无内存缓存，
// 与顶栏退役前读取的是同一份文件）。缓存只由 model_quota_query 成功后写入
// （saveQuotaCacheEntry），本身仅含百分比/时间戳等派生值；此处逐条经
// quota.QuotaResult 反序列化重建，天然丢弃任何未知字段，保证即便文件被
// 第三方写方污染也不会把 Secret 带进响应（ADR-0033 §2.1.3）。
func (s *aiQueryServer) serveQuotaCache(w http.ResponseWriter) {
	writeAIJSON(w, http.StatusOK, cachedQuotaResult())
}

// Both transport faces read the same cached projection, never credentials or upstream.
func cachedQuotaResult() map[string]any {
	entries := readQuotaCacheEntries()
	status := "ready"
	if len(entries) == 0 {
		status = "empty" // 零假数据：从未成功探测过即返回空态
	}
	return map[string]any{
		"status":      status,
		"generatedAt": time.Now().UTC().Format(time.RFC3339),
		"entries":     entries,
	}
}

func readQuotaCacheEntries() map[string]quota.QuotaResult {
	entries := map[string]quota.QuotaResult{}
	path := quotaCachePath()
	if path == "" {
		return entries
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return entries
	}
	var raw map[string]json.RawMessage
	if json.Unmarshal(data, &raw) != nil {
		return entries
	}
	for key, value := range raw {
		var entry quota.QuotaResult
		if json.Unmarshal(value, &entry) != nil {
			continue
		}
		entries[key] = entry
	}
	return entries
}

// serveQuotaRefresh 是全服务唯一允许发起上游请求的路由：与扩展额度卡
// 「刷新全部」同一实现路径（listAuthFiles → 逐已启用项 queryQuota）。
// 成功结果写入 quota-cache.json 的副作用由 queryQuota 内部完成，本函数只汇总；
// 无已启用凭据时返回空结果集，不报错。
func (s *aiQueryServer) serveQuotaRefresh(ctx context.Context, w http.ResponseWriter, r *http.Request) {
	// 请求体当前无参数，仍受 64KB 上限约束（超限 413）。
	if _, err := io.Copy(io.Discard, r.Body); err != nil {
		writeAIError(w, http.StatusRequestEntityTooLarge, "body_too_large", "请求体超过上限")
		return
	}
	if !s.refreshMu.TryLock() {
		writeAIError(w, http.StatusTooManyRequests, "refresh_in_progress", "额度刷新已在进行中")
		return
	}
	defer s.refreshMu.Unlock()

	items, err := s.engine.listAuthFiles()
	if err != nil {
		s.serveEngineResult(w, nil, err)
		return
	}
	files, ok := items.([]authfiles.AuthFileItem)
	if !ok {
		files = nil
	}
	results := make([]map[string]any, 0, len(files))
	for _, item := range files {
		if item.Disabled {
			continue // 仅刷新已启用凭据
		}
		params, marshalErr := json.Marshal(quotaQueryParams{Provider: item.Provider, Name: item.Name, AccountID: item.AccountID})
		if marshalErr != nil {
			continue
		}
		result, queryErr := s.engine.queryQuota(ctx, params)
		outcome := map[string]any{
			"provider": item.Provider,
			"name":     item.Name,
			"account":  item.Account,
		}
		if queryErr != nil {
			code, message := classify(queryErr)
			outcome["status"] = "error"
			outcome["code"] = code
			outcome["error"] = message
		} else if quotaResult, assertOk := result.(*quota.QuotaResult); assertOk {
			outcome["status"] = quotaResult.Status
			outcome["result"] = quotaResult
			if quotaResult.Error != "" {
				outcome["error"] = quotaResult.Error
			}
		}
		results = append(results, outcome)
	}
	writeAIJSON(w, http.StatusOK, map[string]any{
		"status":      "triggered",
		"generatedAt": time.Now().UTC().Format(time.RFC3339),
		"refreshed":   len(results),
		"results":     results,
	})
}

// serveEngineResult 复用 NM 错误分类（classify），把 SafeError 码映射为
// HTTP 状态码；响应体仅含 code/message，不透传内部错误细节。
func (s *aiQueryServer) serveEngineResult(w http.ResponseWriter, result any, err error) {
	if err != nil {
		code, message := classify(err)
		writeAIError(w, aiQueryStatusForCode(code), code, message)
		return
	}
	writeAIJSON(w, http.StatusOK, result)
}

func aiQueryStatusForCode(code string) int {
	switch code {
	case "invalid_request", "invalid_params":
		return http.StatusBadRequest
	case "not_found":
		return http.StatusNotFound
	case "secret_not_configured":
		return http.StatusConflict
	case "keychain_unavailable":
		return http.StatusServiceUnavailable
	default:
		return http.StatusInternalServerError
	}
}

// aiUsageFilterFromQuery 把查询串映射为 usage.Filter（字段语义与 NM 参数
// 结构一致），再序列化为 JSON 交给现有 Engine 处理器，保证校验逻辑与返回
// JSON 形状零分叉。startTime/endTime 仅接受 RFC3339。
func aiUsageFilterFromQuery(query url.Values) (json.RawMessage, error) {
	var filter usage.Filter
	filter.Range = strings.TrimSpace(query.Get("range"))
	filter.SessionKey = query.Get("sessionKey")
	filter.Timezone = strings.TrimSpace(query.Get("timezone"))
	filter.SortBy = strings.TrimSpace(query.Get("sortBy"))
	filter.SortDir = strings.TrimSpace(query.Get("sortDir"))
	filter.Models = aiQueryCSV(query, "models")
	filter.Providers = aiQueryCSV(query, "providers")
	filter.Sources = aiQueryCSV(query, "sources")
	filter.AccessKeyIDs = aiQueryCSV(query, "accessKeyIds")
	var err error
	if filter.StartTime, err = aiQueryTime(query, "startTime"); err != nil {
		return nil, err
	}
	if filter.EndTime, err = aiQueryTime(query, "endTime"); err != nil {
		return nil, err
	}
	if filter.Offset, err = aiQueryInt(query, "offset"); err != nil {
		return nil, err
	}
	if filter.Limit, err = aiQueryInt(query, "limit"); err != nil {
		return nil, err
	}
	if value := query.Get("result"); value != "" {
		switch usage.EventResult(value) {
		case usage.ResultSuccess, usage.ResultFailed, usage.ResultCancelled:
			result := usage.EventResult(value)
			filter.Result = &result
		default:
			return nil, invalid("result 参数无效")
		}
	}
	raw, marshalErr := json.Marshal(filter)
	if marshalErr != nil {
		return nil, marshalErr
	}
	return raw, nil
}

// aiBillingParamsFromQuery 对应 getUsageBilling 参数结构（account、
// includeEntries、entryLimit）。
func aiBillingParamsFromQuery(query url.Values) (json.RawMessage, error) {
	var params struct {
		Account        string `json:"account,omitempty"`
		IncludeEntries bool   `json:"includeEntries,omitempty"`
		EntryLimit     int    `json:"entryLimit,omitempty"`
	}
	params.Account = strings.TrimSpace(query.Get("account"))
	var err error
	if value := query.Get("includeEntries"); value != "" {
		params.IncludeEntries, err = strconv.ParseBool(value)
		if err != nil {
			return nil, invalid("includeEntries 参数无效")
		}
	}
	if params.EntryLimit, err = aiQueryInt(query, "entryLimit"); err != nil {
		return nil, err
	}
	return json.Marshal(params)
}

// aiAttentionParamsFromQuery 对应 getUsageAttention 参数结构（limit）。
func aiAttentionParamsFromQuery(query url.Values) (json.RawMessage, error) {
	limit, err := aiQueryInt(query, "limit")
	if err != nil {
		return nil, err
	}
	return json.Marshal(struct {
		Limit int `json:"limit,omitempty"`
	}{Limit: limit})
}

// aiQueryCSV 读取重复键或逗号分隔的列表参数（models=a&models=b 或 models=a,b）。
func aiQueryCSV(query url.Values, key string) []string {
	var out []string
	for _, value := range query[key] {
		for _, part := range strings.Split(value, ",") {
			if part = strings.TrimSpace(part); part != "" {
				out = append(out, part)
			}
		}
	}
	return out
}

func aiQueryTime(query url.Values, key string) (*time.Time, error) {
	value := strings.TrimSpace(query.Get(key))
	if value == "" {
		return nil, nil
	}
	parsed, err := time.Parse(time.RFC3339, value)
	if err != nil {
		return nil, invalid(key + " 参数无效（需 RFC3339 时间）")
	}
	return &parsed, nil
}

func aiQueryInt(query url.Values, key string) (int, error) {
	value := strings.TrimSpace(query.Get(key))
	if value == "" {
		return 0, nil
	}
	parsed, err := strconv.Atoi(value)
	if err != nil || parsed < 0 {
		return 0, invalid(key + " 参数无效")
	}
	return parsed, nil
}

// allowClient 以客户端 IP 为键做令牌桶限速（每客户端 ≤10 次/秒）。
func (s *aiQueryServer) allowClient(key string) bool {
	now := time.Now()
	s.mu.Lock()
	defer s.mu.Unlock()
	if len(s.buckets) > 128 { // 回环场景客户端极少；防御性清理陈旧桶
		for id, bucket := range s.buckets {
			if now.Sub(bucket.lastAt) > time.Minute {
				delete(s.buckets, id)
			}
		}
	}
	bucket, ok := s.buckets[key]
	if !ok {
		bucket = &aiRateBucket{tokens: aiQueryRatePerSecond, lastAt: now}
		s.buckets[key] = bucket
	}
	bucket.tokens += now.Sub(bucket.lastAt).Seconds() * aiQueryRatePerSecond
	if bucket.tokens > aiQueryRatePerSecond {
		bucket.tokens = aiQueryRatePerSecond
	}
	bucket.lastAt = now
	if bucket.tokens < 1 {
		return false
	}
	bucket.tokens--
	return true
}

// clientKey 取远端地址的 IP 部分（仅回环，实际恒为 127.0.0.1）。
func clientKey(r *http.Request) string {
	if host, _, err := net.SplitHostPort(r.RemoteAddr); err == nil {
		return host
	}
	return r.RemoteAddr
}

// writeAIJSON 输出 JSON 响应。刻意不设置任何 Access-Control-* 头
// （ADR-0033 §2.1.3：无浏览器消费场景，不返回 CORS 头）。
func writeAIJSON(w http.ResponseWriter, status int, payload any) {
	header := w.Header()
	header.Set("Content-Type", "application/json; charset=utf-8")
	header.Set("Cache-Control", "no-store")
	header.Set("X-Content-Type-Options", "nosniff")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(payload)
}

func writeAIError(w http.ResponseWriter, status int, code, message string) {
	writeAIJSON(w, status, map[string]any{
		"error": map[string]string{"code": code, "message": message},
	})
}
