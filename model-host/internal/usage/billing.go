package usage

// T5：billing entries 与对账口径（ADR-0030 决策 9、方案 §4.2.1/§7.2）。
//
// 三口径严格分离，任何一项不重复进入另一项总额：
//   recognized_service_spend = Σ serviceCostImpact（服务消耗，含订阅分摊）
//   cash_outflow             = Σ cashImpact（现金流出，充值计入这里）
//   closing_credit_balance   = opening + Σ creditBalanceImpact（余额变化）
// 充值（topup）只影响 cash_outflow 与 credit balance，不是服务消耗；
// 退款/折扣反向冲减对应口径；多币种分列，不猜汇率。

import (
	"database/sql"
	"fmt"
	"time"
)

// BillingEntry 为一条账务事实。
type BillingEntry struct {
	ID                  string `json:"id"`
	BillingAccount      string `json:"billingAccount"`
	Provider            string `json:"provider,omitempty"`
	Kind                string `json:"kind"` // service_usage / subscription / topup / credit_delta / refund / discount / tax / prepay_expiry / reported_unattributed
	PeriodStart         string `json:"periodStart,omitempty"`
	PeriodEnd           string `json:"periodEnd,omitempty"`
	Currency            string `json:"currency"`
	ServiceCostImpact   int64  `json:"serviceCostImpact"`
	CashImpact          int64  `json:"cashImpact"`
	CreditBalanceImpact int64  `json:"creditBalanceImpact"`
	EvidenceLevel       string `json:"evidenceLevel"` // actual_charge / provider_reported_usage / local_estimate / activity_only
	Note                string `json:"note,omitempty"`
}

// ReconciliationSummary 为单币种对账汇总。
type ReconciliationSummary struct {
	Currency               string `json:"currency"`
	RecognizedServiceSpend int64  `json:"recognizedServiceSpend"`
	CashOutflow            int64  `json:"cashOutflow"`
	CreditBalanceDelta     int64  `json:"creditBalanceDelta"`
}

// kindImpact 决定每类条目允许影响哪些口径。
// 不变量（ADR-0030）：充值不是服务消耗；credits 余额变化不携带现金流；
// 任何一项不得重复进入另一项总额。注意 service_usage 允许同时携带
// serviceCostImpact 与 creditBalanceImpact/cashImpact——同一扣款在
// "服务消耗"与"支付方式"两个账本各记一次，不属于同一总额内重复。
func kindImpact(kind string) (service, cash, credit bool) {
	switch kind {
	case "service_usage", "reported_unattributed":
		return true, true, true
	case "subscription", "tax":
		return true, true, false
	case "topup":
		return false, true, true // 充值：现金流 + 余额，禁止服务消耗
	case "credit_delta", "prepay_expiry":
		return false, false, true
	case "refund":
		return true, true, false // 反向冲减：调用方以负数金额表达
	case "discount":
		return true, false, false // 反向冲减：负数金额
	default:
		return false, false, false
	}
}

// InsertBillingEntry 写入一条账务事实；kind 与三口径字段按 kindImpact 校验，
// 防止充值被误标成服务消耗。
func (s *Store) InsertBillingEntry(e *BillingEntry) error {
	if e.ID == "" || e.BillingAccount == "" || e.Kind == "" || e.Currency == "" {
		return fmt.Errorf("billing entry requires id/account/kind/currency")
	}
	if e.EvidenceLevel == "" {
		e.EvidenceLevel = "actual_charge"
	}
	wantService, wantCash, wantCredit := kindImpact(e.Kind)
	if e.ServiceCostImpact != 0 && !wantService {
		return fmt.Errorf("kind %s must not carry serviceCostImpact (topup/credit is not service spend)", e.Kind)
	}
	if e.CashImpact != 0 && !wantCash {
		return fmt.Errorf("kind %s must not carry cashImpact", e.Kind)
	}
	if e.CreditBalanceImpact != 0 && !wantCredit {
		return fmt.Errorf("kind %s must not carry creditBalanceImpact", e.Kind)
	}

	s.mu.Lock()
	defer s.mu.Unlock()
	_, err := s.db.Exec(`
		INSERT OR REPLACE INTO usage_billing_entries (
			id, billing_account, provider, kind, period_start, period_end,
			currency, service_cost_impact, cash_impact, credit_balance_impact,
			evidence_level, note, created_at
		) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
		e.ID, e.BillingAccount, e.Provider, e.Kind, e.PeriodStart, e.PeriodEnd,
		e.Currency, e.ServiceCostImpact, e.CashImpact, e.CreditBalanceImpact,
		e.EvidenceLevel, e.Note, time.Now().UTC().Format(time.RFC3339Nano),
	)
	return err
}

// Reconcile 按账期与币种汇总三口径。币种分列返回；不合并汇率。
// 同一账期已确认支出（actual_charge）与本地估算的相加由调用方控制——
// 本方法只汇总账务事实本身。
func (s *Store) Reconcile(account, periodStart, periodEnd string) ([]ReconciliationSummary, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	rows, err := s.db.Query(`
			SELECT currency,
			       SUM(service_cost_impact),
			       SUM(cash_impact),
			       SUM(credit_balance_impact)
			FROM usage_billing_entries
			WHERE (? = '' OR billing_account = ?)
			  AND (? = '' OR period_start >= ?)
			  AND (? = '' OR period_end <= ? OR period_end = '')
			GROUP BY currency
			ORDER BY currency`,
		account, account, periodStart, periodStart, periodEnd, periodEnd)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []ReconciliationSummary
	for rows.Next() {
		var r ReconciliationSummary
		if err := rows.Scan(&r.Currency,
			&r.RecognizedServiceSpend, &r.CashOutflow, &r.CreditBalanceDelta); err != nil {
			return nil, err
		}
		out = append(out, r)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	return out, nil
}

// ListBillingEntries 返回某账户的账务条目（分页由 limit 控制，0=默认 100）。
func (s *Store) ListBillingEntries(account string, limit int) ([]BillingEntry, error) {
	if limit <= 0 || limit > 1000 {
		limit = 100
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	rows, err := s.db.Query(`
		SELECT id, billing_account, COALESCE(provider,''), kind,
		       COALESCE(period_start,''), COALESCE(period_end,''), currency,
		       service_cost_impact, cash_impact, credit_balance_impact,
		       evidence_level, COALESCE(note,'')
		FROM usage_billing_entries
		WHERE billing_account = ?
		ORDER BY created_at DESC
		LIMIT ?`, account, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []BillingEntry
	for rows.Next() {
		var e BillingEntry
		if err := rows.Scan(&e.ID, &e.BillingAccount, &e.Provider, &e.Kind,
			&e.PeriodStart, &e.PeriodEnd, &e.Currency,
			&e.ServiceCostImpact, &e.CashImpact, &e.CreditBalanceImpact,
			&e.EvidenceLevel, &e.Note); err != nil && err != sql.ErrNoRows {
			return nil, err
		}
		out = append(out, e)
	}
	return out, rows.Err()
}
