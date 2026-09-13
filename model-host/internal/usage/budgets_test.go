package usage

// R5/R6 缺口验收（§4.3/§7.5/§9.1/§9.2）：预算跨周期与阈值去重、
// evidenceLevel 分离、账单与估算不混算。测试注入 Now，验证周期翻转
// 与 UNIQUE(budget_id, period_key, threshold, kind) 幂等语义。

import (
	"testing"
	"time"
)

func TestBudgetThresholdDedupAcrossPeriods(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	b := &Budget{
		ID: "bud-1", Scope: "api_estimate", Currency: "USD",
		AmountMicro: 1000, Period: "daily", Timezone: "Asia/Shanghai",
		Thresholds: []int{80, 100}, Enabled: true,
	}
	if err := s.UpsertBudget(b); err != nil {
		t.Fatalf("UpsertBudget: %v", err)
	}

	day1 := time.Date(2026, 9, 11, 2, 0, 0, 0, time.UTC)  // 上海 10:00，day1 周期
	day2 := time.Date(2026, 9, 11, 18, 0, 0, 0, time.UTC) // 上海 2026-09-12 02:00，已翻周期

	mk := func(id string, at time.Time, micro int64) *Event {
		return &Event{ID: id, RequestedAt: at, Provider: "openai", Model: "gpt-4o",
			Result: ResultSuccess, HTTPStatus: 200, CostMicro: micro, TotalTokens: 100}
	}
	// day1 花 900 微（90%）：触发 80 阈值一次。
	// 事件必须严格早于评估时刻（periodSpendLocked 用半开区间 [start, now)）。
	for i, m := range []int64{500, 400} {
		if err := s.InsertEvent(mk("e1-"+string(rune('a'+i)), day1.Add(-time.Minute), m)); err != nil {
			t.Fatalf("insert day1: %v", err)
		}
	}
	// day2 花 1000 微（100%）：新周期触发 80、100 各一次。
	if err := s.InsertEvent(mk("e2-a", day2.Add(-time.Minute), 1000)); err != nil {
		t.Fatalf("insert day2: %v", err)
	}

	// 第一次评估（day1）：触发 80。
	eval1, err := s.EvaluateBudgets(EvaluateOpts{Now: day1})
	if err != nil {
		t.Fatalf("evaluate day1: %v", err)
	}
	if len(eval1) != 1 || len(eval1[0].Triggered) != 1 || eval1[0].Triggered[0] != 80 {
		t.Fatalf("day1 eval = %+v, want trigger [80]", eval1)
	}
	// 重复评估同周期：UNIQUE 吞掉，不再产生新触发。
	eval1b, err := s.EvaluateBudgets(EvaluateOpts{Now: day1})
	if err != nil {
		t.Fatalf("re-evaluate day1: %v", err)
	}
	if len(eval1b[0].Triggered) != 0 {
		t.Errorf("same period re-evaluate must not re-trigger: %+v", eval1b[0].Triggered)
	}

	// 周期翻转（day2）：新 period_key，80 阈值允许再次触发，100 也触发。
	eval2, err := s.EvaluateBudgets(EvaluateOpts{Now: day2})
	if err != nil {
		t.Fatalf("evaluate day2: %v", err)
	}
	if len(eval2) != 1 {
		t.Fatalf("day2 eval count = %d", len(eval2))
	}
	got := map[int]bool{}
	for _, th := range eval2[0].Triggered {
		got[th] = true
	}
	if !got[80] || !got[100] {
		t.Errorf("day2 eval triggers = %v, want {80,100} in new period", eval2[0].Triggered)
	}
	if eval2[0].PeriodKey != "2026-09-12" {
		t.Errorf("periodKey must follow user timezone: got %q", eval2[0].PeriodKey)
	}

	// 收件箱应有 3 条预算提醒（80@day1 + 80@day2 + 100@day2），每周期/阈值恰好一条。
	var alertCount int
	if err := s.db.QueryRow(`SELECT COUNT(*) FROM usage_alerts WHERE kind = 'budget_threshold'`).Scan(&alertCount); err != nil {
		t.Fatalf("count budget alerts: %v", err)
	}
	if alertCount != 3 {
		t.Errorf("budget alerts = %d, want 3 (80@day1, 80@day2, 100@day2)", alertCount)
	}
}

func TestBudgetSuppressAlertsForHistoryImport(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	if err := s.UpsertBudget(&Budget{ID: "bud-h", Scope: "api_estimate", Currency: "USD",
		AmountMicro: 100, Period: "monthly", Timezone: "UTC", Thresholds: []int{80, 100}, Enabled: true}); err != nil {
		t.Fatalf("upsert: %v", err)
	}
	now := time.Date(2026, 9, 11, 12, 0, 0, 0, time.UTC)
	if err := s.InsertEvent(&Event{ID: "h-1", RequestedAt: now.Add(-time.Hour), Provider: "openai", Model: "gpt-4o",
		Result: ResultSuccess, HTTPStatus: 200, CostMicro: 500, TotalTokens: 100}); err != nil {
		t.Fatalf("insert: %v", err)
	}

	eval, err := s.EvaluateBudgets(EvaluateOpts{Now: now, SuppressAlerts: true})
	if err != nil {
		t.Fatalf("evaluate: %v", err)
	}
	if len(eval) != 1 || len(eval[0].Triggered) != 2 {
		t.Fatalf("suppressed eval = %+v, want thresholds still reported", eval)
	}
	var alertCount int
	_ = s.db.QueryRow(`SELECT COUNT(*) FROM usage_alerts`).Scan(&alertCount)
	if alertCount != 0 {
		t.Errorf("history import must not batch-notify: %d alerts", alertCount)
	}
}

func TestEvidenceLevelsNeverMergedInReconcile(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	// actual_charge（真账单）与 local_estimate（估算）并存：
	// Reconcile 只汇总账务事实（billing entries），估算永不并入已确认支出。
	entries := []*BillingEntry{
		{ID: "r-1", BillingAccount: "acct-ev", Kind: "service_usage", Currency: "USD",
			ServiceCostImpact: 3000, EvidenceLevel: "actual_charge"},
		{ID: "r-2", BillingAccount: "acct-ev", Kind: "service_usage", Currency: "USD",
			ServiceCostImpact: 700, EvidenceLevel: "provider_reported_usage"},
	}
	for _, e := range entries {
		if err := s.InsertBillingEntry(e); err != nil {
			t.Fatalf("insert %s: %v", e.ID, err)
		}
	}
	// 空账户必须为空（§7.5：无示例假账单）。
	empty, err := s.ListBillingEntries("acct-none", 10)
	if err != nil {
		t.Fatalf("list empty account: %v", err)
	}
	if len(empty) != 0 {
		t.Errorf("empty account must return 0 entries, got %d", len(empty))
	}

	summaries, err := s.Reconcile("acct-ev", "", "")
	if err != nil {
		t.Fatalf("reconcile: %v", err)
	}
	if len(summaries) != 1 || summaries[0].RecognizedServiceSpend != 3700 {
		t.Errorf("billing facts only: %+v, want 3700 (usage estimates excluded)", summaries)
	}

	// 证据等级在读取时保留，不被改写。
	listed, err := s.ListBillingEntries("acct-ev", 10)
	if err != nil {
		t.Fatalf("list: %v", err)
	}
	levels := map[string]string{}
	for _, e := range listed {
		levels[e.ID] = e.EvidenceLevel
	}
	if levels["r-1"] != "actual_charge" || levels["r-2"] != "provider_reported_usage" {
		t.Errorf("evidence levels must round-trip: %+v", levels)
	}
}
