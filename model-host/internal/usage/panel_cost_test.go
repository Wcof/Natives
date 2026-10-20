package usage

// 顶栏面板金额与会话标题契约（模型聚合 + 工具分组会话）：
//   - cost_micro=0 的原生活集按价格目录在查询层动态估算（overview/byModel/sessions）；
//   - cost_micro>0 的行保留存储计费，不被估算覆盖；
//   - 会话标题来自 usage_subjects（kind=session，来源适配器显式注册）。

import (
	"testing"
	"time"
)

func insertPanelCostEvent(t *testing.T, s *Store, toolID, sessionID, provider, model string, in, out, cr, cw int64, costMicro int64, at time.Time) {
	t.Helper()
	ev := &Event{
		BillingAtom:      "panel-cost:" + toolID + ":" + sessionID + ":" + at.Format(time.RFC3339Nano),
		RequestedAt:      at,
		Provider:         provider,
		Model:            model,
		Source:           "Test",
		ToolID:           toolID,
		Result:           ResultSuccess,
		InputTokens:      in,
		OutputTokens:     out,
		CacheReadTokens:  cr,
		CacheWriteTokens: cw,
		TotalTokens:      in + out + cr + cw,
		CostMicro:        costMicro,
		SessionID:        sessionID,
	}
	if ok, err := s.InsertEventIfAbsent(ev); err != nil || !ok {
		t.Fatalf("insert event: inserted=%v err=%v", ok, err)
	}
}

func TestPanelSessionTitleAndDynamicCost(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	// 测试价格：$1/百万 input、$2/百万 output、$0.5/百万 cache read（非真实报价）
	if err := s.UpsertPrice(&Price{
		ProviderID:          "gemini",
		ModelID:             "gemini-test",
		InputPriceMicro:     1_000_000,
		OutputPriceMicro:    2_000_000,
		CacheReadPriceMicro: 500_000,
		Source:              "manual",
	}); err != nil {
		t.Fatalf("upsert price: %v", err)
	}

	// 会话 A（zcode）：cost_micro=0 → 查询层动态估算
	// 估算 = (1_000_000*1 + 500_000*2 + 250_000*0.5) / 1e6 = (1+1+0.125) = $2.125 → 2125000 micro
	base := time.Date(2026, 9, 29, 8, 0, 0, 0, time.UTC)
	insertPanelCostEvent(t, s, "zcode", "sess-a", "gemini", "gemini-test", 1_000_000, 500_000, 250_000, 0, 0, base)
	insertPanelCostEvent(t, s, "zcode", "sess-a", "gemini", "gemini-test", 100_000, 50_000, 0, 0, 0, base.Add(time.Minute))

	// 会话 B（codex）：带存储计费 300 micro → 保留存储值，不被估算覆盖
	insertPanelCostEvent(t, s, "codex", "sess-b", "gemini", "gemini-test", 10_000, 10_000, 0, 0, 300, base.Add(2*time.Minute))

	// 会话标题：来源适配器显式注册（ZCode 权威库的 session.title）
	if err := s.RegisterSubjects([][3]string{
		{"sess-a", "session", "修复顶栏弹窗数据显示与刷新问题"},
	}); err != nil {
		t.Fatalf("register subjects: %v", err)
	}

	f := Filter{Range: "all"}
	sessions, err := s.GetSessions(f)
	if err != nil {
		t.Fatalf("GetSessions: %v", err)
	}
	if len(sessions.Sessions) != 2 {
		t.Fatalf("sessions = %d, want 2", len(sessions.Sessions))
	}
	byKey := map[string]SessionStat{}
	for _, st := range sessions.Sessions {
		byKey[st.ToolID] = st
	}
	a := byKey["zcode"]
	if a.Title != "修复顶栏弹窗数据显示与刷新问题" {
		t.Errorf("session title = %q, want 注册标题", a.Title)
	}
	// 两事件合计：input 1_100_000、output 550_000、cache_read 250_000
	// 估算 = 1.1 + 1.1 + 0.125 = $2.325
	if diff := a.CostUSD - 2.325; diff < -0.000001 || diff > 0.000001 {
		t.Errorf("zcode session cost = %f, want 2.325000 (动态估算)", a.CostUSD)
	}
	b := byKey["codex"]
	if b.Title != "" {
		t.Errorf("unregistered session title = %q, want empty", b.Title)
	}
	if b.CostUSD != 0.0003 {
		t.Errorf("codex session cost = %f, want 0.000300 (存储计费优先)", b.CostUSD)
	}

	analysis, err := s.GetAnalytics(f)
	if err != nil {
		t.Fatalf("GetAnalytics: %v", err)
	}
	if len(analysis.ByModel) != 1 {
		t.Fatalf("byModel rows = %d, want 1", len(analysis.ByModel))
	}
	m := analysis.ByModel[0]
	if m.Name != "gemini-test" {
		t.Errorf("byModel name = %q, want gemini-test", m.Name)
	}
	// 模型聚合总估算 = 2.325 + 0.0003 = $2.3253
	if diff := m.CostUSD - 2.3253; diff < -0.000001 || diff > 0.000001 {
		t.Errorf("byModel cost = %f, want 2.325300", m.CostUSD)
	}

	overview, err := s.GetOverview(f)
	if err != nil {
		t.Fatalf("GetOverview: %v", err)
	}
	if diff := overview.Metrics.EstimatedCostUSD - 2.3253; diff < -0.000001 || diff > 0.000001 {
		t.Errorf("overview estimatedCost = %f, want 2.325300", overview.Metrics.EstimatedCostUSD)
	}
}

// 无价格目录覆盖的模型：金额保持 0（诚实空态），不得把未计价渲染成免费。
func TestPanelDynamicCostMissingPriceStaysZero(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	base := time.Date(2026, 9, 29, 8, 0, 0, 0, time.UTC)
	insertPanelCostEvent(t, s, "zcode", "sess-x", "unknown-provider", "unknown-model", 1_000_000, 1_000_000, 0, 0, 0, base)

	sessions, err := s.GetSessions(Filter{Range: "all"})
	if err != nil {
		t.Fatalf("GetSessions: %v", err)
	}
	for _, st := range sessions.Sessions {
		if st.CostUSD != 0 {
			t.Errorf("unpriced session cost = %f, want 0（缺价不得估算）", st.CostUSD)
		}
	}
}

func TestPanelPartialCostIsNotComplete(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()
	now := time.Now().UTC()
	insertPanelCostEvent(t, s, "codex", "mixed", "provider", "known", 100, 0, 0, 0, 250000, now)
	insertPanelCostEvent(t, s, "codex", "mixed", "provider", "unknown", 100, 0, 0, 0, 0, now.Add(time.Second))
	overview, err := s.GetOverview(Filter{Range: "all"})
	if err != nil {
		t.Fatal(err)
	}
	if overview.Metrics.Priced {
		t.Fatal("partially priced total cannot claim a complete amount")
	}
	sessions, err := s.GetSessions(Filter{Range: "all"})
	if err != nil {
		t.Fatal(err)
	}
	if len(sessions.Sessions) != 1 || sessions.Sessions[0].Priced {
		t.Fatalf("partial session: %+v", sessions)
	}
	if sessions.Sessions[0].CostUSD != 0.25 {
		t.Fatal("known subtotal must be preserved")
	}
}

func TestPanelSessionQueryFailureIsNotEmpty(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()
	insertPanelCostEvent(t, s, "codex", "s", "provider", "known", 10, 0, 0, 0, 1, time.Now())
	if _, err := s.db.Exec("DROP TABLE usage_subjects"); err != nil {
		t.Fatal(err)
	}
	if result, err := s.GetSessions(Filter{Range: "all"}); err == nil {
		t.Fatalf("broken session query returned success: %+v", result)
	}
}

func TestPanelLargeTokenCostDoesNotOverflowBeforeScaling(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()
	price := &Price{ProviderID: "fixture", ModelID: "large", InputPriceMicro: 75000000, OutputPriceMicro: 150000000, Source: PriceSourceManual}
	if err := s.UpsertPrice(price); err != nil {
		t.Fatal(err)
	}
	const wantMicro int64 = 60000000000000
	calc := NewCalculator(s)
	if got := calc.CalculateCost("fixture", "large", 200000000000, 300000000000, 0, 0); got != wantMicro {
		t.Fatalf("cost=%d want=%d", got, wantMicro)
	}
	insertPanelCostEvent(t, s, "codex", "large", "fixture", "large", 200000000000, 300000000000, 0, 0, 0, time.Now())
	result, err := s.GetOverview(Filter{Range: "all"})
	if err != nil || result.Metrics.EstimatedCostUSD != float64(wantMicro)/1e6 {
		t.Fatalf("large aggregate=%+v err=%v", result, err)
	}
}
