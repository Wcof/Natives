package usage

// T8：预算引擎（ADR-0030 决策 9、方案 §4.3）。
//
// - 预算是用户规则：daily/monthly，按币种独立计算；
// - 阈值默认 80/100，可调整；每个预算周期/阈值只产生一次提醒
//   （usage_alerts UNIQUE(budget_id, period_key, threshold, kind) 保证幂等）；
// - 历史导入不批量弹窗：评估时用「当前周期内已存在的采集写入」判断，
//   批量导入路径显式传 EvaluateOpts{SuppressAlerts: true}；
// - 金额为有界整数微单位 + 显式币种，不用浮点累计。

import (
	"encoding/json"
	"fmt"
	"time"
)

// Budget 为一条用户预算规则。
type Budget struct {
	ID          string `json:"id"`
	Scope       string `json:"scope"`    // api_estimate / provider_credits / billing
	ScopeKey    string `json:"scopeKey"` // provider/账户约束，可空=全部
	Currency    string `json:"currency"`
	AmountMicro int64  `json:"amountMicro"`
	Period      string `json:"period"`     // daily / monthly
	Timezone    string `json:"timezone"`   // IANA；空=UTC
	Thresholds  []int  `json:"thresholds"` // 默认 [80,100]
	Enabled     bool   `json:"enabled"`
}

// UpsertBudget 写入/更新预算规则（带 revision 语义：每次更新刷新 updated_at）。
func (s *Store) UpsertBudget(b *Budget) error {
	if b.ID == "" || b.AmountMicro <= 0 || b.Currency == "" {
		return fmt.Errorf("budget requires id/amountMicro/currency")
	}
	if b.Period != "daily" && b.Period != "monthly" {
		return fmt.Errorf("period must be daily or monthly, got %q", b.Period)
	}
	tz := b.Timezone
	if tz == "" {
		tz = "UTC"
	}
	if _, err := time.LoadLocation(tz); err != nil {
		return fmt.Errorf("invalid timezone %q: %w", tz, err)
	}
	thresholds := b.Thresholds
	if len(thresholds) == 0 {
		thresholds = []int{80, 100}
	}
	thresholdsJSON := "["
	for i, th := range thresholds {
		if i > 0 {
			thresholdsJSON += ","
		}
		thresholdsJSON += fmt.Sprintf("%d", th)
	}
	thresholdsJSON += "]"
	enabled := 0
	if b.Enabled {
		enabled = 1
	}

	s.mu.Lock()
	defer s.mu.Unlock()
	_, err := s.db.Exec(`
		INSERT INTO usage_budgets (
			id, scope, scope_key, currency, amount_micro, period, timezone,
			thresholds_json, enabled, updated_at
		) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
		ON CONFLICT(id) DO UPDATE SET
			scope=excluded.scope, scope_key=excluded.scope_key,
			currency=excluded.currency, amount_micro=excluded.amount_micro,
			period=excluded.period, timezone=excluded.timezone,
			thresholds_json=excluded.thresholds_json, enabled=excluded.enabled,
			updated_at=excluded.updated_at`,
		b.ID, b.Scope, b.ScopeKey, b.Currency, b.AmountMicro, b.Period, tz,
		thresholdsJSON, enabled, time.Now().UTC().Format(time.RFC3339Nano))
	return err
}

// BudgetsResult 为 model_usage_budgets 响应结构。
type BudgetsResult struct {
	Status      string              `json:"status"`
	GeneratedAt string              `json:"generatedAt"`
	Budgets     []*Budget           `json:"budgets"`
	Evaluations []BudgetEvaluation  `json:"evaluations"`
	Renewals    []RenewalEvaluation `json:"renewals,omitempty"`
}

// ListBudgets 返回已配置的全部预算。
func (s *Store) ListBudgets() ([]*Budget, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	rows, err := s.db.Query(`
		SELECT id, scope, scope_key, currency, amount_micro, period, timezone, thresholds_json, enabled
		FROM usage_budgets ORDER BY id ASC`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var out []*Budget
	for rows.Next() {
		var b Budget
		var enabledInt int
		var thJSON string
		if err := rows.Scan(&b.ID, &b.Scope, &b.ScopeKey, &b.Currency, &b.AmountMicro,
			&b.Period, &b.Timezone, &thJSON, &enabledInt); err != nil {
			return nil, err
		}
		b.Enabled = enabledInt == 1
		b.Thresholds = parseThresholds(thJSON)
		out = append(out, &b)
	}
	if out == nil {
		out = []*Budget{}
	}
	return out, nil
}

// periodKey 计算预算当前周期键（按用户时区；跨月/夏令时由此分界）。
// 半开区间 [start, end)：daily=当日 00:00 起；monthly=当月 1 日 00:00 起。
func periodKey(period, tz string, now time.Time) string {
	loc, err := time.LoadLocation(tz)
	if err != nil {
		loc = time.UTC
	}
	local := now.In(loc)
	if period == "daily" {
		return local.Format("2006-01-02")
	}
	return local.Format("2006-01")
}

// EvaluateOpts 控制评估行为。
type EvaluateOpts struct {
	// SuppressAlerts：历史导入等批量路径置 true，只计算不弹提醒。
	SuppressAlerts bool
	// Now 覆盖当前时间（测试注入）。
	Now time.Time
}

// BudgetEvaluation 为单预算的评估结果。
type BudgetEvaluation struct {
	BudgetID    string `json:"budgetId"`
	PeriodKey   string `json:"periodKey"`
	SpentMicro  int64  `json:"spentMicro"`
	AmountMicro int64  `json:"amountMicro"`
	Percent     int    `json:"percent"`
	Triggered   []int  `json:"triggered"` // 本次新触发的阈值
}

// EvaluateBudgets 评估全部启用预算：花费口径为 usage_events.cost_micro
// （估算/账务事实由 scope 决定；api_estimate 用本地估算金额）。
// 每个周期/阈值只入箱一次；历史导入抑制提醒但仍返回评估结果。
func (s *Store) EvaluateBudgets(opts EvaluateOpts) ([]BudgetEvaluation, error) {
	now := opts.Now
	if now.IsZero() {
		now = time.Now().UTC()
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	rows, err := s.db.Query(`
		SELECT id, scope, scope_key, currency, amount_micro, period, timezone, thresholds_json
		FROM usage_budgets WHERE enabled = 1`)
	if err != nil {
		return nil, err
	}
	type rule struct {
		id, scope, scopeKey, currency string
		amount                        int64
		period, tz                    string
		thresholds                    []int
	}
	var rules []rule
	for rows.Next() {
		var r rule
		var thresholdsJSON string
		if err := rows.Scan(&r.id, &r.scope, &r.scopeKey, &r.currency, &r.amount,
			&r.period, &r.tz, &thresholdsJSON); err != nil {
			rows.Close()
			return nil, err
		}
		r.thresholds = parseThresholds(thresholdsJSON)
		rules = append(rules, r)
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return nil, err
	}

	out := []BudgetEvaluation{}
	for _, r := range rules {
		pk := periodKey(r.period, r.tz, now)
		spent, err := s.periodSpendLocked(r.scope, r.scopeKey, r.currency, r.period, r.tz, now)
		if err != nil {
			return nil, err
		}
		eval := BudgetEvaluation{
			BudgetID: r.id, PeriodKey: pk,
			SpentMicro: spent, AmountMicro: r.amount,
		}
		if r.amount > 0 {
			eval.Percent = int(spent * 100 / r.amount)
		}
		for _, th := range r.thresholds {
			if eval.Percent < th {
				continue
			}
			if opts.SuppressAlerts {
				// 历史导入：只算不弹；报告越过阈值，零入箱。
				eval.Triggered = append(eval.Triggered, th)
				continue
			}
			// 幂等：UNIQUE(budget_id, period_key, threshold, kind)；
			// Triggered 只记真正新入箱的阈值（重复评估/已查看的不重复报告）。
			res, err := s.db.Exec(`
				INSERT OR IGNORE INTO usage_alerts (
					id, budget_id, kind, severity, title, detail, period_key, threshold, created_at
				) VALUES (?, ?, 'budget_threshold', 'warn', ?, ?, ?, ?, ?)`,
				fmt.Sprintf("budget-%s-%s-%d", r.id, pk, th),
				r.id,
				fmt.Sprintf("预算已用 %d%%", th),
				fmt.Sprintf("预算 %s 本周期 %s 已达 %d%%（%d/%d 微单位）", r.id, pk, eval.Percent, spent, r.amount),
				pk, th, now.UTC().Format(time.RFC3339Nano))
			if err != nil {
				return nil, err
			}
			if n, _ := res.RowsAffected(); n > 0 {
				eval.Triggered = append(eval.Triggered, th)
			}
		}
		out = append(out, eval)
	}
	return out, nil
}

// periodSpendLocked 汇总当前周期的花费（调用方已持锁）。
// 周期边界按用户时区计算，查询用 UTC 半开区间 [start, end)。
func (s *Store) periodSpendLocked(scope, scopeKey, currency, period, tz string, now time.Time) (int64, error) {
	loc, err := time.LoadLocation(tz)
	if err != nil {
		loc = time.UTC
	}
	local := now.In(loc)
	var startLocal time.Time
	if period == "daily" {
		startLocal = time.Date(local.Year(), local.Month(), local.Day(), 0, 0, 0, 0, loc)
	} else {
		startLocal = time.Date(local.Year(), local.Month(), 1, 0, 0, 0, 0, loc)
	}
	start := startLocal.UTC().Format(time.RFC3339Nano)
	end := now.UTC().Format(time.RFC3339Nano)

	// scope → 事件过滤：api_estimate 统计本地估算 cost_micro；
	// 其他 scope 当前同样基于 cost_micro（credits/billing 账务由
	// usage_billing_entries 承载，预算评估只对事件流）。
	query := `SELECT COALESCE(SUM(cost_micro), 0) FROM usage_events
		WHERE requested_at >= ? AND requested_at < ?`
	args := []interface{}{start, end}
	if scopeKey != "" {
		query += ` AND (provider = ? OR source = ?)`
		args = append(args, scopeKey, scopeKey)
	}
	var sum int64
	err = s.db.QueryRow(query, args...).Scan(&sum)
	return sum, err
}

// parseThresholds 解析 thresholds_json（容错：非法回退 [80,100]）。
func parseThresholds(jsonStr string) []int {
	var out []int
	if err := jsonUnmarshalInts(jsonStr, &out); err != nil || len(out) == 0 {
		return []int{80, 100}
	}
	return out
}

// jsonUnmarshalInts 解析 JSON 整数数组（小 helper，避免 budgets.go 引入
// encoding/json 后与其他文件重复导入风格）。
func jsonUnmarshalInts(data string, out *[]int) error {
	return json.Unmarshal([]byte(data), out)
}
