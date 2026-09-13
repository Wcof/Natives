package usage

import "testing"

// T5 测试：三口径分离与 kind 校验（验收矩阵 §9.1：
// “充值、credits 消耗、订阅、退款、税费和折扣——分别影响现金流、
// 余额和服务消耗，任何一项不重复进入另一项总额”）。

func TestBillingEntryKindValidation(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	// 充值不得携带服务消耗：拒绝写入，防止重复计消耗。
	topup := &BillingEntry{
		ID: "bill-topup-1", BillingAccount: "acct-openai", Kind: "topup",
		Currency: "USD", ServiceCostImpact: 2000, CashImpact: 2000,
	}
	if err := s.InsertBillingEntry(topup); err == nil {
		t.Errorf("topup with serviceCostImpact must be rejected (not service spend)")
	}
	// 正确的充值：只影响 cash + credit balance。
	correctTopup := &BillingEntry{
		ID: "bill-topup-2", BillingAccount: "acct-openai", Kind: "topup",
		Currency: "USD", CashImpact: 2000, CreditBalanceImpact: 2000,
	}
	if err := s.InsertBillingEntry(correctTopup); err != nil {
		t.Fatalf("valid topup rejected: %v", err)
	}
	// credits 变化不得携带现金或服务消耗。
	badCredit := &BillingEntry{
		ID: "bill-credit-1", BillingAccount: "acct-openai", Kind: "credit_delta",
		Currency: "USD", CashImpact: -500, CreditBalanceImpact: -500,
	}
	if err := s.InsertBillingEntry(badCredit); err == nil {
		t.Errorf("credit_delta with cashImpact must be rejected")
	}
}

func TestReconcileSeparatesThreeImpacts(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	entries := []*BillingEntry{
		// 服务消耗：API 用量扣款（credits 抵扣 → 只算服务消耗，不算现金流）。
		{ID: "b1", BillingAccount: "acct-a", Kind: "service_usage", Currency: "USD",
			CreditBalanceImpact: -1200},
		// 订阅：服务消耗 + 现金。
		{ID: "b2", BillingAccount: "acct-a", Kind: "subscription", Currency: "USD",
			ServiceCostImpact: 2000, CashImpact: 2000},
		// 充值：现金 + 余额，不是服务消耗。
		{ID: "b3", BillingAccount: "acct-a", Kind: "topup", Currency: "USD",
			CashImpact: 5000, CreditBalanceImpact: 5000},
		// 税费：服务消耗 + 现金。
		{ID: "b4", BillingAccount: "acct-a", Kind: "tax", Currency: "USD",
			ServiceCostImpact: 200, CashImpact: 200},
		// 折扣：负数冲减服务消耗。
		{ID: "b5", BillingAccount: "acct-a", Kind: "discount", Currency: "USD",
			ServiceCostImpact: -300},
		// 另一币种：分列，不合并。
		{ID: "b6", BillingAccount: "acct-a", Kind: "service_usage", Currency: "EUR",
			ServiceCostImpact: 800},
	}
	for _, e := range entries {
		if err := s.InsertBillingEntry(e); err != nil {
			t.Fatalf("insert %s: %v", e.ID, err)
		}
	}

	summaries, err := s.Reconcile("acct-a", "", "")
	if err != nil {
		t.Fatalf("Reconcile: %v", err)
	}
	byCur := map[string]ReconciliationSummary{}
	for _, r := range summaries {
		byCur[r.Currency] = r
	}
	usd, ok := byCur["USD"]
	if !ok {
		t.Fatalf("USD summary missing: %+v", summaries)
	}
	// 服务消耗 = 2000（订阅）+ 200（税）− 300（折扣）= 1900；充值 5000 不进入。
	if usd.RecognizedServiceSpend != 1900 {
		t.Errorf("service spend = %d, want 1900 (topup excluded)", usd.RecognizedServiceSpend)
	}
	// 现金 = 2000 + 5000 + 200 = 7200；credits 抵扣的 1200 不是现金流。
	if usd.CashOutflow != 7200 {
		t.Errorf("cash outflow = %d, want 7200 (credit-funded usage excluded)", usd.CashOutflow)
	}
	// 余额 = −1200（抵扣）+ 5000（充值）= 3800。
	if usd.CreditBalanceDelta != 3800 {
		t.Errorf("credit balance delta = %d, want 3800", usd.CreditBalanceDelta)
	}
	// EUR 分列。
	eur, ok := byCur["EUR"]
	if !ok || eur.RecognizedServiceSpend != 800 {
		t.Errorf("EUR must be listed separately: %+v", byCur)
	}
}

func TestBillingEntryUpsertByIdempotent(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	e := &BillingEntry{
		ID: "bill-same", BillingAccount: "acct-b", Kind: "service_usage",
		Currency: "USD", CreditBalanceImpact: -100,
	}
	if err := s.InsertBillingEntry(e); err != nil {
		t.Fatalf("first insert: %v", err)
	}
	if err := s.InsertBillingEntry(e); err != nil {
		t.Fatalf("re-import (upsert by id): %v", err)
	}
	list, err := s.ListBillingEntries("acct-b", 0)
	if err != nil {
		t.Fatalf("list: %v", err)
	}
	if len(list) != 1 {
		t.Errorf("same billing entry id must not duplicate, got %d entries", len(list))
	}
}
