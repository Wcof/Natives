package usage

// billing_import.go 测试：cursor-billing-csv/1 导入回退（§9.0 Cursor 行
// "个人账单导入"、§4.2.1 三口径不变量、§7.2 定点金额）。

import (
	"strings"
	"testing"
)

const billingCSVFixture = `date,kind,amount,currency,period_start,period_end,note
2026-08-01,subscription,20.00,USD,2026-08-01,2026-08-31,Pro plan monthly
2026-08-05,topup,50.00,USD,,,credits topup
2026-08-10,usage,1.25,USD,,,API usage
2026-08-15,discount,-0.50,USD,,,promo
`

func TestParseBillingCSVAndImport(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	entries, err := ParseBillingCSV([]byte(billingCSVFixture), "cursor", "acct-cursor")
	if err != nil {
		t.Fatalf("ParseBillingCSV: %v", err)
	}
	if len(entries) != 4 {
		t.Fatalf("entries = %d, want 4", len(entries))
	}
	// 定点金额：20.00 USD → 20_000_000 微单位；-0.50 → -500_000。
	if entries[0].ServiceCostImpact != 20_000_000 {
		t.Errorf("subscription amount = %d, want 20000000", entries[0].ServiceCostImpact)
	}
	if entries[3].ServiceCostImpact != -500_000 {
		t.Errorf("discount amount = %d, want -500000", entries[3].ServiceCostImpact)
	}
	// evidence_level：账单文件一律 actual_charge（§4.2 已确认支出）。
	for i, e := range entries {
		if e.EvidenceLevel != "actual_charge" {
			t.Errorf("entry %d evidenceLevel = %q, want actual_charge", i, e.EvidenceLevel)
		}
	}
	// 口径分配：topup 只影响现金+余额，不携带服务消耗（kindImpact 不变量）。
	if entries[1].ServiceCostImpact != 0 {
		t.Errorf("topup serviceCostImpact = %d, want 0 (topup is not service spend)", entries[1].ServiceCostImpact)
	}
	if entries[1].CashImpact != 50_000_000 {
		t.Errorf("topup cashImpact = %d, want 50000000", entries[1].CashImpact)
	}
	// 内容指纹 ID 稳定。
	if !strings.HasPrefix(entries[0].ID, "billcsv-") {
		t.Errorf("entry id = %q, want billcsv-<fingerprint>", entries[0].ID)
	}

	imported, skipped, err := s.ImportBillingCSV([]byte(billingCSVFixture), "cursor", "acct-cursor")
	if err != nil {
		t.Fatalf("ImportBillingCSV: %v", err)
	}
	if imported != 4 || skipped != 0 {
		t.Fatalf("first import: imported=%d skipped=%d, want 4/0", imported, skipped)
	}

	// 重导同一文件：内容指纹幂等，不重复计量（§9.1）。
	imported2, skipped2, err := s.ImportBillingCSV([]byte(billingCSVFixture), "cursor", "acct-cursor")
	if err != nil {
		t.Fatalf("re-import: %v", err)
	}
	if imported2 != 0 || skipped2 != 4 {
		t.Fatalf("re-import: imported=%d skipped=%d, want 0/4 (fingerprint idempotent)", imported2, skipped2)
	}

	// 三口径对账：订阅 20 + usage 1.25 − 折扣 0.50 = 20.75 服务消耗；
	// 现金 50（topup）；充值不重复进入服务消耗（§4.2.1）。
	sums, err := s.Reconcile("acct-cursor", "", "")
	if err != nil {
		t.Fatalf("Reconcile: %v", err)
	}
	if len(sums) != 1 || sums[0].Currency != "USD" {
		t.Fatalf("reconcile currencies = %+v, want single USD", sums)
	}
	if sums[0].RecognizedServiceSpend != 20_750_000 {
		t.Errorf("serviceSpend = %d, want 20750000 (20+1.25-0.50)", sums[0].RecognizedServiceSpend)
	}
	if sums[0].CashOutflow != 50_000_000 {
		t.Errorf("cashOutflow = %d, want 50000000 (topup only)", sums[0].CashOutflow)
	}
}

func TestParseBillingCSVRejectsUnknownKindAndOverflow(t *testing.T) {
	if _, err := ParseBillingCSV([]byte("date,kind,amount,currency\n2026-08-01,alien_money,1.00,USD\n"), "cursor", "a"); err == nil {
		t.Errorf("unknown kind must error explicitly, got nil")
	} else if !strings.Contains(err.Error(), "unknown billing kind") {
		t.Errorf("error should name unknown kind, got %v", err)
	}
	if _, err := ParseBillingCSV([]byte("date,kind,amount,currency\n2026-08-01,subscription,123456789012345.00,USD\n"), "cursor", "a"); err == nil {
		t.Errorf("overflow amount must be rejected, got nil")
	}
	if _, err := ParseBillingCSV([]byte("date,kind,amount,currency\n2026-08-01,subscription,abc,USD\n"), "cursor", "a"); err == nil {
		t.Errorf("non-numeric amount must be rejected, got nil")
	}
	if _, err := ParseBillingCSV([]byte("day,kind,amount,currency\n2026-08-01,subscription,1.00,USD\n"), "cursor", "a"); err == nil {
		t.Errorf("missing 'date' header must be rejected, got nil")
	}
}
