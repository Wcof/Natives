package usage

// 整改 E5 验收测试（方案 §11.2 成本与账单硬验收场景）：
//   - billing_csv 导入事务：preview 不写账本、commit 原子、重复导入幂等；
//   - 手动确认账务：evidence=user_confirmed，充值不算服务消耗；
//   - 订阅/credits 到期 7/3/1/0 天窗口各提醒一次。

import (
	"fmt"
	"strings"
	"testing"
	"time"
)

func billingCSV(rows ...string) []byte {
	lines := append([]string{"date,kind,amount,currency,period_start,period_end,note"}, rows...)
	return []byte(strings.Join(lines, "\n") + "\n")
}

// TestBillingCSVPreviewDoesNotWrite preview 只读：失败/取消不写入账本（§11.2）。
func TestBillingCSVPreviewDoesNotWrite(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	data := billingCSV(
		"2026-08-01,subscription,20.00,USD,2026-08-01,2026-09-01,Pro plan",
	)
	preview := s.PreviewBillingCSV(data, "cursor", "acct-1")
	if !preview.Valid || preview.TotalRecords != 1 {
		t.Fatalf("preview = %+v, want 1 valid row", preview)
	}
	if len(preview.CurrencySummaries) != 1 || preview.CurrencySummaries[0].RecognizedServiceSpend != 20_000_000 {
		t.Errorf("currency summaries = %+v, want 20 USD service spend", preview.CurrencySummaries)
	}
	var count int64
	if err := s.db.QueryRow("SELECT COUNT(*) FROM usage_billing_entries").Scan(&count); err != nil {
		t.Fatal(err)
	}
	if count != 0 {
		t.Fatalf("preview must not write ledger, got %d entries", count)
	}
}

// TestBillingCSVImportIdempotent 同一 CSV 重复导入不增加金额（§11.2）。
func TestBillingCSVImportIdempotent(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	data := billingCSV(
		"2026-08-01,subscription,20.00,USD,2026-08-01,2026-09-01,Pro plan",
		"2026-08-15,service_usage,3.50,USD,,,API usage",
		"2026-08-16,topup,100.00,USD,,,wallet topup",
	)
	imported, skipped, err := s.ImportBillingCSV(data, "cursor", "acct-1")
	if err != nil || imported != 3 || skipped != 0 {
		t.Fatalf("first import: imported=%d skipped=%d err=%v, want 3/0/nil", imported, skipped, err)
	}
	imported2, skipped2, err := s.ImportBillingCSV(data, "cursor", "acct-1")
	if err != nil || imported2 != 0 || skipped2 != 3 {
		t.Fatalf("re-import: imported=%d skipped=%d err=%v, want 0/3/nil", imported2, skipped2, err)
	}
	summaries, err := s.Reconcile("acct-1", "", "")
	if err != nil {
		t.Fatal(err)
	}
	if len(summaries) != 1 || summaries[0].Currency != "USD" {
		t.Fatalf("summaries = %+v, want single USD summary", summaries)
	}
	sum := summaries[0]
	if sum.RecognizedServiceSpend != 23_500_000 { // 20 订阅 + 3.50 API，充值不算服务消耗
		t.Errorf("service spend = %d, want 23500000", sum.RecognizedServiceSpend)
	}
	if sum.CashOutflow != 100_000_000 { // 充值只入现金流
		t.Errorf("cash outflow = %d, want 100000000", sum.CashOutflow)
	}
	if sum.CreditBalanceDelta != 100_000_000 { // 充值入余额变化
		t.Errorf("credit delta = %d, want 100000000", sum.CreditBalanceDelta)
	}
}

// TestBillingCSVPreviewLineErrors 未知 kind 显式报错且不写入（不静默丢弃）。
func TestBillingCSVPreviewLineErrors(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	data := billingCSV(
		"2026-08-01,subscription,20.00,USD",
		"2026-08-02,galactic_tax,1.00,USD",
	)
	preview := s.PreviewBillingCSV(data, "cursor", "acct-1")
	if preview.Valid {
		t.Fatal("unknown kind must invalidate the preview")
	}
	if !strings.Contains(preview.Error, "galactic_tax") && !strings.Contains(preview.Error, "line 3") {
		t.Errorf("error should identify the bad kind/line, got %q", preview.Error)
	}
}

// TestManualBillingEntry 手动条目：evidence=user_confirmed，充值禁带服务消耗。
func TestManualBillingEntry(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	sub := &BillingEntry{
		BillingAccount: "acct-1", Provider: "cursor", Kind: "subscription",
		PeriodStart: "2026-08-01", PeriodEnd: "2026-09-01",
		Currency: "USD", ServiceCostImpact: 20_000_000, Note: "手动确认 Pro 订阅",
	}
	if err := s.InsertManualBillingEntry(sub); err != nil {
		t.Fatalf("manual subscription: %v", err)
	}
	if sub.EvidenceLevel != "user_confirmed" {
		t.Errorf("evidence = %q, want user_confirmed", sub.EvidenceLevel)
	}
	// 内容指纹幂等：重复提交同一事实不再新增。
	var count int64
	s.db.QueryRow("SELECT COUNT(*) FROM usage_billing_entries").Scan(&count)
	if err := s.InsertManualBillingEntry(&BillingEntry{
		BillingAccount: "acct-1", Provider: "cursor", Kind: "subscription",
		PeriodStart: "2026-08-01", PeriodEnd: "2026-09-01",
		Currency: "USD", ServiceCostImpact: 20_000_000, Note: "手动确认 Pro 订阅",
	}); err != nil {
		t.Fatal(err)
	}
	s.db.QueryRow("SELECT COUNT(*) FROM usage_billing_entries").Scan(&count)
	if count != 1 {
		t.Errorf("duplicate manual entry must be idempotent, got %d rows", count)
	}
	// 充值禁止携带服务消耗（kindImpact 校验在 InsertBillingEntry）。
	err := s.InsertManualBillingEntry(&BillingEntry{
		BillingAccount: "acct-1", Kind: "topup", Currency: "USD",
		ServiceCostImpact: 50_000_000,
	})
	if err == nil || !strings.Contains(err.Error(), "must not carry serviceCostImpact") {
		t.Fatalf("topup with service impact must be rejected, got %v", err)
	}
}

// TestRenewalWindowsOnce 订阅与 credits 到期在 7/3/1/0 天窗口各提醒一次，
// 同窗口重复评估不重复入箱（§11.2）。
func TestRenewalWindowsOnce(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	now := time.Date(2026, 9, 12, 8, 0, 0, 0, time.UTC)
	due := now.AddDate(0, 0, 7).Format("2006-01-02")
	if err := s.InsertBillingEntry(&BillingEntry{
		ID: "sub-1", BillingAccount: "acct-1", Kind: "subscription",
		PeriodEnd: due, Currency: "USD", ServiceCostImpact: 20_000_000,
	}); err != nil {
		t.Fatal(err)
	}
	if err := s.InsertBillingEntry(&BillingEntry{
		ID: "credit-1", BillingAccount: "acct-1", Kind: "prepay_expiry",
		PeriodEnd: now.AddDate(0, 0, 1).Format("2006-01-02"), Currency: "USD",
		CreditBalanceImpact: 5_000_000,
	}); err != nil {
		t.Fatal(err)
	}

	// 第一次评估：订阅触发 7 天窗口，credits 触发 1 天窗口。
	evals, err := s.EvaluateRenewals(RenewalOpts{Now: now})
	if err != nil {
		t.Fatal(err)
	}
	triggered := map[string][]int{}
	for _, ev := range evals {
		triggered[ev.EntryID] = ev.Triggered
	}
	if fmt.Sprint(triggered["sub-1"]) != "[7]" {
		t.Errorf("sub-1 triggered = %v, want [7]", triggered["sub-1"])
	}
	if fmt.Sprint(triggered["credit-1"]) != "[1]" {
		t.Errorf("credit-1 triggered = %v, want [1]", triggered["credit-1"])
	}
	// 重复评估：同窗口不重复入箱。
	evals2, err := s.EvaluateRenewals(RenewalOpts{Now: now})
	if err != nil {
		t.Fatal(err)
	}
	for _, ev := range evals2 {
		if len(ev.Triggered) != 0 {
			t.Errorf("re-evaluation must not re-trigger %s: %v", ev.EntryID, ev.Triggered)
		}
	}
	// 窗口推进（+6 天 → 订阅剩 1 天）：新窗口各一次。
	evals3, err := s.EvaluateRenewals(RenewalOpts{Now: now.AddDate(0, 0, 6)})
	if err != nil {
		t.Fatal(err)
	}
	for _, ev := range evals3 {
		if ev.EntryID == "sub-1" && fmt.Sprint(ev.Triggered) != "[1]" {
			t.Errorf("sub-1 at +6d triggered = %v, want [1]", ev.Triggered)
		}
	}
	// 到期日已过：不提醒。
	evals4, err := s.EvaluateRenewals(RenewalOpts{Now: now.AddDate(0, 0, 8)})
	if err != nil {
		t.Fatal(err)
	}
	for _, ev := range evals4 {
		if ev.EntryID == "sub-1" && len(ev.Triggered) != 0 {
			t.Errorf("expired entry must not trigger, got %v", ev.Triggered)
		}
	}
}
