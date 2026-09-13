package usage

// R6 缺口验收（§4.3）：订阅续费 / credits 到期提醒。
// 验证点：7/3/1/0 窗口各入箱一次（UNIQUE 幂等）、已到期不提醒、
// 历史导入抑制（SuppressAlerts 只报不入箱）、坏日期不阻断其余条目。

import (
	"testing"
	"time"
)

func TestRenewalWindowsFireOnceEach(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	// 到期日 = today+7（窗口 7 命中；3/1/0 不命中）。
	due := time.Now().UTC().AddDate(0, 0, 7).Format("2006-01-02")
	if err := s.InsertBillingEntry(&BillingEntry{
		ID: "sub-1", BillingAccount: "acct-a", Kind: "subscription",
		Currency: "USD", PeriodEnd: due, EvidenceLevel: "actual_charge",
	}); err != nil {
		t.Fatalf("insert subscription: %v", err)
	}

	// 第一次评估：恰好触发窗口 7 一次。
	ev1, err := s.EvaluateRenewals(RenewalOpts{})
	if err != nil {
		t.Fatalf("evaluate: %v", err)
	}
	if len(ev1) != 1 || len(ev1[0].Triggered) != 1 || ev1[0].Triggered[0] != 7 {
		t.Fatalf("first eval = %+v, want trigger [7]", ev1)
	}
	// 重复评估：UNIQUE 吞掉，不再产生新触发。
	ev2, err := s.EvaluateRenewals(RenewalOpts{})
	if err != nil {
		t.Fatalf("re-evaluate: %v", err)
	}
	if len(ev2[0].Triggered) != 0 {
		t.Errorf("re-evaluate must not re-trigger: %+v", ev2[0].Triggered)
	}
	// 收件箱恰好一条。
	var n int
	if err := s.db.QueryRow(`SELECT COUNT(*) FROM usage_alerts WHERE kind='renewal'`).Scan(&n); err != nil {
		t.Fatalf("count: %v", err)
	}
	if n != 1 {
		t.Errorf("renewal alerts = %d, want 1", n)
	}
}

func TestRenewalDueTodayAndExpired(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	today := time.Now().UTC().Format("2006-01-02")
	past := time.Now().UTC().AddDate(0, 0, -2).Format("2006-01-02")
	entries := []*BillingEntry{
		{ID: "pre-1", BillingAccount: "acct-b", Kind: "prepay_expiry",
			Currency: "EUR", PeriodEnd: today, EvidenceLevel: "actual_charge"},
		{ID: "sub-old", BillingAccount: "acct-b", Kind: "subscription",
			Currency: "USD", PeriodEnd: past, EvidenceLevel: "actual_charge"},
	}
	for _, e := range entries {
		if err := s.InsertBillingEntry(e); err != nil {
			t.Fatalf("insert %s: %v", e.ID, err)
		}
	}
	ev, err := s.EvaluateRenewals(RenewalOpts{})
	if err != nil {
		t.Fatalf("evaluate: %v", err)
	}
	byID := map[string]RenewalEvaluation{}
	for _, r := range ev {
		byID[r.EntryID] = r
	}
	// prepay 今日到期：窗口 0 触发，severity=warn。
	if got := byID["pre-1"]; len(got.Triggered) != 1 || got.Triggered[0] != 0 {
		t.Errorf("prepay due today = %+v, want trigger [0]", got)
	}
	// 已过期：不触发任何提醒。
	if got := byID["sub-old"]; got.DaysUntil < 0 && len(got.Triggered) != 0 {
		t.Errorf("expired entry must not alert: %+v", got)
	}
	var n int
	_ = s.db.QueryRow(`SELECT COUNT(*) FROM usage_alerts WHERE kind='renewal'`).Scan(&n)
	if n != 1 {
		t.Errorf("renewal alerts = %d, want 1 (today only)", n)
	}
}

func TestRenewalSuppressAlertsAndBadDate(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	due := time.Now().UTC().AddDate(0, 0, 3).Format("2006-01-02")
	entries := []*BillingEntry{
		{ID: "sub-s", BillingAccount: "acct-c", Kind: "subscription",
			Currency: "USD", PeriodEnd: due, EvidenceLevel: "provider_reported_usage"},
		{ID: "sub-bad", BillingAccount: "acct-c", Kind: "subscription",
			Currency: "USD", PeriodEnd: "not-a-date", EvidenceLevel: "actual_charge"},
	}
	for _, e := range entries {
		if err := s.InsertBillingEntry(e); err != nil {
			t.Fatalf("insert %s: %v", e.ID, err)
		}
	}
	// 抑制模式（历史导入）：报告越过窗口但零入箱。
	ev, err := s.EvaluateRenewals(RenewalOpts{SuppressAlerts: true})
	if err != nil {
		t.Fatalf("evaluate suppressed: %v", err)
	}
	byID := map[string]RenewalEvaluation{}
	for _, r := range ev {
		byID[r.EntryID] = r
	}
	if got := byID["sub-s"]; len(got.Triggered) != 1 || got.Triggered[0] != 3 {
		t.Errorf("suppressed eval = %+v, want reported [3]", got)
	}
	if got := byID["sub-bad"]; got.DaysUntil != -1 || len(got.Triggered) != 0 {
		t.Errorf("bad date entry = %+v, want DaysUntil=-1 and no trigger", got)
	}
	var n int
	_ = s.db.QueryRow(`SELECT COUNT(*) FROM usage_alerts`).Scan(&n)
	if n != 0 {
		t.Errorf("history import must not batch-notify: %d alerts", n)
	}
	// 正常模式：坏日期条目被跳过但不阻断好条目入箱。
	ev2, err := s.EvaluateRenewals(RenewalOpts{})
	if err != nil {
		t.Fatalf("evaluate normal: %v", err)
	}
	for _, r := range ev2 {
		if r.EntryID == "sub-s" && len(r.Triggered) != 1 {
			t.Errorf("good entry must still alert after bad-date skip: %+v", r)
		}
	}
}
