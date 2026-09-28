package host

// ADR-0033 只读查询端口测试：路由白名单、安全边界（Host 校验 / 速率 /
// 并发 / Secret 不泄漏）与额度刷新的优雅空态。

import (
	"encoding/json"
	"io"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/ldh/natives/model-host/internal/authfiles"
	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/secrets"
)

// newAIQueryTestHarness 构建带临时配置目录与随机端口的查询服务测试床。
// authFiles 指向空的临时目录，避免测试读到本机真实 ~/.natives/auth 凭据；
// usage.db 与 quota-cache.json 经 NATIVES_MODEL_HOST_CONFIG_DIR 隔离到临时目录。
func newAIQueryTestHarness(t *testing.T) (*Engine, *aiQueryServer, string) {
	t.Helper()
	t.Setenv("NATIVES_MODEL_HOST_CONFIG_DIR", t.TempDir())
	engine, err := NewEngine(domain.NewRepository(filepath.Join(t.TempDir(), "state.json")), secrets.NewMemoryStore(), nil)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(engine.Close)
	// NewEngine 在生产中绑定固定端口 8766；测试改用随机端口床，先关闭自动监听。
	if engine.aiQuery != nil {
		engine.aiQuery.Close()
		engine.aiQuery = nil
	}
	engine.authFiles, err = authfiles.NewManager(filepath.Join(t.TempDir(), "auth"))
	if err != nil {
		t.Fatal(err)
	}
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	server := newAIQueryServer(engine, listener)
	t.Cleanup(server.Close)
	return engine, server, "http://" + listener.Addr().String()
}

func aiQueryGet(t *testing.T, baseURL, route string) (*http.Response, []byte) {
	t.Helper()
	resp, err := (&http.Client{Timeout: 10 * time.Second}).Get(baseURL + route)
	if err != nil {
		t.Fatalf("%s: %v", route, err)
	}
	body, err := io.ReadAll(resp.Body)
	resp.Body.Close()
	if err != nil {
		t.Fatalf("%s: read body: %v", route, err)
	}
	return resp, body
}

func TestAIQueryGetRoutesServeJSON(t *testing.T) {
	_, _, baseURL := newAIQueryTestHarness(t)
	routes := []string{
		"/ai/healthz",
		"/ai/usage/overview",
		"/ai/usage/analysis",
		"/ai/usage/sessions",
		"/ai/usage/events",
		"/ai/usage/sources",
		"/ai/usage/budgets",
		"/ai/usage/billing",
		"/ai/usage/attention",
		"/ai/usage/insights",
		"/ai/usage/pricing",
		// 分页/范围/筛选项经查询串映射到与 NM 相同的 Filter 结构。
		"/ai/usage/events?range=24h&limit=5&timezone=Asia/Shanghai&result=success&sortBy=cost&sortDir=desc",
		"/ai/usage/billing?includeEntries=true&entryLimit=10",
		"/ai/usage/attention?limit=3",
	}
	for index, route := range routes {
		if index > 0 && index%9 == 0 {
			// 给测试床自己的 10 req/s 令牌桶留出回填时间，避免路由测试触发 429。
			time.Sleep(1100 * time.Millisecond)
		}
		resp, body := aiQueryGet(t, baseURL, route)
		if resp.StatusCode != http.StatusOK {
			t.Fatalf("%s: status=%d body=%s", route, resp.StatusCode, body)
		}
		var payload map[string]any
		if err := json.Unmarshal(body, &payload); err != nil {
			t.Fatalf("%s: invalid JSON: %v", route, err)
		}
		// ADR-0033 §2.1.3：不返回任何 CORS 头。
		for _, header := range []string{"Access-Control-Allow-Origin", "Access-Control-Allow-Methods", "Access-Control-Allow-Headers"} {
			if resp.Header.Get(header) != "" {
				t.Fatalf("%s: unexpected CORS header %s", route, header)
			}
		}
	}
	_, healthBody := aiQueryGet(t, baseURL, "/ai/healthz")
	var health map[string]string
	if err := json.Unmarshal(healthBody, &health); err != nil || health["status"] != "ok" {
		t.Fatalf("healthz shape mismatch: %s", healthBody)
	}
}

func TestAIQueryUnknownPathAndMethodRejected(t *testing.T) {
	_, _, baseURL := newAIQueryTestHarness(t)
	// 未知路径 404；CLIProxyAPI management 面（ADR-0020 禁止暴露）同样 404。
	for _, route := range []string{
		"/ai/unknown",
		"/ai/usage/notaroute",
		"/ai/usage/overview/extra",
		"/management/api/current",
		"/v1/models",
		"/",
	} {
		resp, body := aiQueryGet(t, baseURL, route)
		if resp.StatusCode != http.StatusNotFound {
			t.Fatalf("%s: expected 404, got %d body=%s", route, resp.StatusCode, body)
		}
	}
	// 已知路径上的错误方法 405（含 CORS 预检探测，不得放行）。
	req, err := http.NewRequest(http.MethodPost, baseURL+"/ai/usage/overview", nil)
	if err != nil {
		t.Fatal(err)
	}
	resp, err := (&http.Client{Timeout: 10 * time.Second}).Do(req)
	if err != nil {
		t.Fatal(err)
	}
	body, _ := io.ReadAll(resp.Body)
	resp.Body.Close()
	if resp.StatusCode != http.StatusMethodNotAllowed {
		t.Fatalf("POST overview: expected 405, got %d body=%s", resp.StatusCode, body)
	}
}

func TestAIQueryRejectsForeignHost(t *testing.T) {
	_, server, baseURL := newAIQueryTestHarness(t)
	client := &http.Client{Timeout: 10 * time.Second}
	for _, host := range []string{"evil.example.com", "127.0.0.1:9999", "localhost:1"} {
		req, err := http.NewRequest(http.MethodGet, baseURL+"/ai/healthz", nil)
		if err != nil {
			t.Fatal(err)
		}
		req.Host = host
		resp, err := client.Do(req)
		if err != nil {
			t.Fatalf("host %q: %v", host, err)
		}
		body, _ := io.ReadAll(resp.Body)
		resp.Body.Close()
		if resp.StatusCode != http.StatusForbidden {
			t.Fatalf("host %q: expected 403, got %d body=%s", host, resp.StatusCode, body)
		}
	}
	// 无 Host 头的原始 HTTP/1.0 请求同样必须被拒绝。
	connection, err := net.Dial("tcp", server.listener.Addr().String())
	if err != nil {
		t.Fatal(err)
	}
	defer connection.Close()
	if _, err = connection.Write([]byte("GET /ai/healthz HTTP/1.0\r\n\r\n")); err != nil {
		t.Fatal(err)
	}
	raw, err := io.ReadAll(connection)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(string(raw), "HTTP/1.0 403") && !strings.HasPrefix(string(raw), "HTTP/1.1 403") {
		t.Fatalf("host-less request: expected 403, got %q", raw)
	}
}

func TestAIQueryQuotaRefreshWithoutCredentials(t *testing.T) {
	_, _, baseURL := newAIQueryTestHarness(t)
	resp, err := (&http.Client{Timeout: 30 * time.Second}).Post(baseURL+"/ai/quota/refresh", "application/json", strings.NewReader(`{}`))
	if err != nil {
		t.Fatal(err)
	}
	body, _ := io.ReadAll(resp.Body)
	resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d body=%s", resp.StatusCode, body)
	}
	var payload struct {
		Status    string            `json:"status"`
		Refreshed int               `json:"refreshed"`
		Results   []json.RawMessage `json:"results"`
	}
	if err := json.Unmarshal(body, &payload); err != nil {
		t.Fatalf("invalid JSON: %v", err)
	}
	// 无已启用凭据：优雅返回空结果集，不 panic、不触发上游请求。
	if payload.Status != "triggered" || payload.Refreshed != 0 || len(payload.Results) != 0 {
		t.Fatalf("unexpected refresh result: %s", body)
	}
}

// TestAIQueryNoSecretLeak 在额度缓存中植入含 Secret 字样的记录（模拟第三方
// 写方污染），并抽查关键路由响应体：不得出现 access_token / refresh_token /
// client_secret 等字符串（ADR-0033 §2.1.3 / R-S4）。
func TestAIQueryNoSecretLeak(t *testing.T) {
	_, _, baseURL := newAIQueryTestHarness(t)
	cachePath := filepath.Join(os.Getenv("NATIVES_MODEL_HOST_CONFIG_DIR"), "quota-cache.json")
	polluted := map[string]any{
		"codex|user@example.com": map[string]any{
			"provider":      "codex",
			"name":          "user@example.com",
			"status":        "success",
			"windows":       []any{},
			"fetchedAt":     1,
			"access_token":  "SHOULD-NOT-LEAK",
			"refresh_token": "SHOULD-NOT-LEAK",
			"client_secret": "SHOULD-NOT-LEAK",
		},
	}
	data, err := json.Marshal(polluted)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(cachePath, data, 0600); err != nil {
		t.Fatal(err)
	}
	client := &http.Client{Timeout: 30 * time.Second}
	routes := []string{
		"/ai/quota",
		"/ai/usage/overview",
		"/ai/usage/events?limit=5",
		"/ai/usage/billing?includeEntries=true",
		"/ai/usage/pricing",
		"/ai/usage/insights",
	}
	for _, route := range routes {
		resp, body := aiQueryGet(t, baseURL, route)
		if resp.StatusCode != http.StatusOK {
			t.Fatalf("%s: status=%d body=%s", route, resp.StatusCode, body)
		}
		for _, marker := range []string{"SHOULD-NOT-LEAK", "access_token", "refresh_token", "client_secret", "accessToken", "refreshToken"} {
			if strings.Contains(string(body), marker) {
				t.Fatalf("%s: secret marker %q leaked in response", route, marker)
			}
		}
	}
	// 刷新路由（唯一上游触发口）同样不得携带 Secret。
	refreshResp, err := client.Post(baseURL+"/ai/quota/refresh", "application/json", nil)
	if err != nil {
		t.Fatal(err)
	}
	refreshBody, _ := io.ReadAll(refreshResp.Body)
	refreshResp.Body.Close()
	for _, marker := range []string{"access_token", "refresh_token", "client_secret"} {
		if strings.Contains(string(refreshBody), marker) {
			t.Fatalf("refresh: secret marker %q leaked in response", marker)
		}
	}
}

func TestAIQueryConcurrencyLimit(t *testing.T) {
	_, server, baseURL := newAIQueryTestHarness(t)
	// 占满并发闸门后新请求必须得到 503（确定性验证闸门本身）。
	for i := 0; i < aiQueryConcurrencyMax; i++ {
		server.gate <- struct{}{}
	}
	resp, body := aiQueryGet(t, baseURL, "/ai/healthz")
	if resp.StatusCode != http.StatusServiceUnavailable {
		t.Fatalf("expected 503 when concurrency is saturated, got %d body=%s", resp.StatusCode, body)
	}
	// 释放闸门后恢复 200。
	for i := 0; i < aiQueryConcurrencyMax; i++ {
		<-server.gate
	}
	resp, body = aiQueryGet(t, baseURL, "/ai/healthz")
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200 after releasing gate, got %d body=%s", resp.StatusCode, body)
	}
}

func TestAIQueryRateLimit(t *testing.T) {
	_, _, baseURL := newAIQueryTestHarness(t)
	client := &http.Client{Timeout: 10 * time.Second}
	limited := false
	for i := 0; i < 20; i++ {
		resp, err := client.Get(baseURL + "/ai/healthz")
		if err != nil {
			t.Fatal(err)
		}
		resp.Body.Close()
		switch resp.StatusCode {
		case http.StatusOK:
		case http.StatusTooManyRequests:
			limited = true
		default:
			t.Fatalf("unexpected status %d at request %d", resp.StatusCode, i)
		}
	}
	if !limited {
		t.Fatal("20 次连续请求未触发每客户端速率限制（10 req/s）")
	}
}
