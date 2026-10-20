package host

// 跨语言契约 fixture（T0）：从真实 Host handler 序列化导出响应 JSON，
// 供前端 ai-performance 契约测试消费，禁止前端单方面手写形状。
//
// 导出内容覆盖 T0 修复的三个契约点：
//  1. Overview.metrics.successRate 为 0–100 协议（GetOverview 计算）；
//  2. Event 时间字段是 requestedAt（usage/types.go JSON tag）；
//  3. Event.costStatus 由 handler 注解（priced/unpriced，缺价≠合法零价）。
//
// 运行 `go test ./internal/host/ -run TestExportContractFixture` 会重新生成
// ../../../../extension/ai-performance/host-fixture.json；前端测试读取该文件。

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/ldh/natives/model-host/internal/usage"
)

func TestExportContractFixture(t *testing.T) {
	s, err := usage.OpenStore(filepath.Join(t.TempDir(), "fixture_usage.db"))
	if err != nil {
		t.Fatalf("OpenStore: %v", err)
	}
	defer s.Close()

	base := time.Date(2026, 9, 10, 9, 0, 0, 0, time.UTC)
	// 事件 1：价格目录内模型（claude-3-5-sonnet builtin 价），priced；含缓存桶。
	priced := &usage.Event{
		ID:               "evt_contract_priced",
		RequestedAt:      base,
		CreatedAt:        base,
		Provider:         "anthropic",
		Model:            "claude-3-5-sonnet-20241022",
		Source:           "Claude Code",
		Result:           usage.ResultSuccess,
		InputTokens:      200000,
		OutputTokens:     0,
		CacheReadTokens:  800000,
		CacheWriteTokens: 0,
		TotalTokens:      1000000,
	}
	// 互斥桶计费（T0 黄金口径）：$1/M 输入 + $0.1/M 缓存读 → 手工断言目录价。
	if err := s.InsertEvent(priced); err != nil {
		t.Fatalf("insert priced event: %v", err)
	}
	// 事件 2：目录外模型，unpriced（cost_micro=0 但 status 必须区分）。
	unpriced := &usage.Event{
		ID:          "evt_contract_unpriced",
		RequestedAt: base.Add(2 * time.Hour * 30),
		CreatedAt:   base.Add(2 * time.Hour * 30),
		Provider:    "openai",
		Model:       "model-without-price",
		Source:      "Codex",
		Result:      usage.ResultFailed,
		HTTPStatus:  500,
		TotalTokens: 5200,
	}
	if err := s.InsertEvent(unpriced); err != nil {
		t.Fatalf("insert unpriced event: %v", err)
	}

	e := &Engine{
		usageStore:      s,
		usageCalculator: usage.NewCalculator(s),
	}

	overview, err := e.getUsageOverview(nil)
	if err != nil {
		t.Fatalf("getUsageOverview: %v", err)
	}
	events, err := e.getUsageEvents(nil)
	if err != nil {
		t.Fatalf("getUsageEvents: %v", err)
	}

	// 契约断言：序列化前先验证语义。
	if overview.Metrics.SuccessRate < 0 || overview.Metrics.SuccessRate > 100 {
		t.Errorf("successRate must be 0-100 protocol, got %v", overview.Metrics.SuccessRate)
	}
	if len(events.Events) != 2 {
		t.Fatalf("expected 2 events, got %d", len(events.Events))
	}
	var pricedStatus, unpricedStatus string
	for _, ev := range events.Events {
		if ev.RequestedAt.IsZero() {
			t.Errorf("event %s must carry requestedAt", ev.ID)
		}
		switch ev.ID {
		case "evt_contract_priced":
			pricedStatus = ev.CostStatus
		case "evt_contract_unpriced":
			unpricedStatus = ev.CostStatus
		}
	}
	if pricedStatus != "priced" {
		t.Errorf("priced event costStatus = %q, want priced", pricedStatus)
	}
	if unpricedStatus != "unpriced" {
		t.Errorf("unpriced event costStatus = %q, want unpriced (zero cost must not be indistinguishable)", unpricedStatus)
	}

	fixture := map[string]any{
		"overview": overview,
		"events":   events,
	}
	payload, err := json.MarshalIndent(fixture, "", "  ")
	if err != nil {
		t.Fatalf("marshal fixture: %v", err)
	}
	payload = append(payload, '\n')

	out := filepath.Join("..", "..", "..", "extension", "ai-performance", "host-fixture.json")
	if err := os.WriteFile(out, payload, 0o644); err != nil {
		t.Fatalf("write fixture: %v", err)
	}
	t.Logf("contract fixture written to %s", out)
}

func TestValidateCanonicalContractsV1(t *testing.T) {
	contractDir := filepath.Join("..", "..", "..", "contracts", "ai-usage-query", "v1")
	if _, err := os.Stat(contractDir); os.IsNotExist(err) {
		t.Fatalf("contracts dir not found at %s", contractDir)
	}

	// 1. Overview
	overviewBytes, err := os.ReadFile(filepath.Join(contractDir, "overview.json"))
	if err != nil {
		t.Fatalf("read overview.json: %v", err)
	}
	var overview usage.OverviewResult
	if err := json.Unmarshal(overviewBytes, &overview); err != nil {
		t.Fatalf("unmarshal overview.json: %v", err)
	}
	if overview.Metrics.TotalTokens <= 0 {
		t.Errorf("overview totalTokens must be > 0, got %d", overview.Metrics.TotalTokens)
	}

	// 2. Analysis
	analysisBytes, err := os.ReadFile(filepath.Join(contractDir, "analysis.json"))
	if err != nil {
		t.Fatalf("read analysis.json: %v", err)
	}
	var analysis usage.AnalyticsResult
	if err := json.Unmarshal(analysisBytes, &analysis); err != nil {
		t.Fatalf("unmarshal analysis.json: %v", err)
	}
	if len(analysis.ByModel) == 0 {
		t.Errorf("analysis byModel must not be empty")
	}

	// 3. Sessions
	sessionsBytes, err := os.ReadFile(filepath.Join(contractDir, "sessions.json"))
	if err != nil {
		t.Fatalf("read sessions.json: %v", err)
	}
	var sessions usage.SessionsResult
	if err := json.Unmarshal(sessionsBytes, &sessions); err != nil {
		t.Fatalf("unmarshal sessions.json: %v", err)
	}
	if len(sessions.Sessions) == 0 {
		t.Errorf("sessions list must not be empty")
	}

	// 4. Events
	eventsBytes, err := os.ReadFile(filepath.Join(contractDir, "events.json"))
	if err != nil {
		t.Fatalf("read events.json: %v", err)
	}
	var events usage.EventsResult
	if err := json.Unmarshal(eventsBytes, &events); err != nil {
		t.Fatalf("unmarshal events.json: %v", err)
	}
	if len(events.Events) == 0 {
		t.Errorf("events list must not be empty")
	}
}
