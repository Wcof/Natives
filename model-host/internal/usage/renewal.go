package usage

// R6 缺口收口（方案 §4.3）：订阅续费 / credits（预付余额）到期提醒。
//
// - 日期来源：usage_billing_entries 中 kind=subscription（PeriodEnd=续费日）
//   与 kind=prepay_expiry（PeriodEnd=到期日）的已确认/已报告账目，
//   或用户录入的同结构条目；不从模型名或 OAuth 推断日期。
// - 默认提前 7/3/1 天各提醒一次 + 到期当天一次；每个 (条目, 窗口) 只入箱一次，
//   复用 usage_alerts UNIQUE(budget_id, period_key, threshold, kind) 幂等。
// - 到期已过不提醒；历史导入（SuppressAlerts）只计算不弹窗。
// - 无受支持调度路径时不承诺关机后的定时通知（§6.5）：随页面恢复或
//   预算评估入口（EvaluateBudgets 同一批查询）刷新。

import (
	"fmt"
	"time"
)

// renewalWindows 为默认提醒窗口（提前天数，0=到期当天）。
var renewalWindows = []int{7, 3, 1, 0}

// RenewalOpts 控制续费/到期评估行为。
type RenewalOpts struct {
	SuppressAlerts bool
	Now            time.Time
}

// RenewalEvaluation 为单条到期账目的评估结果。
type RenewalEvaluation struct {
	EntryID     string `json:"entryId"`
	Kind        string `json:"kind"` // subscription / prepay_expiry
	BillingAcct string `json:"billingAccount"`
	Currency    string `json:"currency"`
	DueDate     string `json:"dueDate"`
	DaysUntil   int    `json:"daysUntil"`
	Triggered   []int  `json:"triggered"` // 本次新入箱的窗口（提前天数）
}

// parseDueDay 解析到期日期：支持 YYYY-MM-DD 与 RFC3339 两种格式；
// 解析失败返回 error（不猜测日期）。
func parseDueDay(s string) (time.Time, error) {
	if d, err := time.Parse("2006-01-02", s); err == nil {
		return d, nil
	}
	if d, err := time.Parse(time.RFC3339, s); err == nil {
		y, m, day := d.UTC().Date()
		return time.Date(y, m, day, 0, 0, 0, 0, time.UTC), nil
	}
	return time.Time{}, fmt.Errorf("unparseable due date %q", s)
}

// EvaluateRenewals 评估全部带到期日的订阅/预付条目。
// 幂等：id=renewal-<entry>-<window>，UNIQUE 约束保证每窗口一次。
func (s *Store) EvaluateRenewals(opts RenewalOpts) ([]RenewalEvaluation, error) {
	now := opts.Now
	if now.IsZero() {
		now = time.Now().UTC()
	}
	today := now.UTC()
	y, m, d := today.Date()
	todayDay := time.Date(y, m, d, 0, 0, 0, 0, time.UTC)

	s.mu.Lock()
	defer s.mu.Unlock()

	rows, err := s.db.Query(`
		SELECT id, kind, billing_account, currency, period_end
		FROM usage_billing_entries
		WHERE kind IN ('subscription','prepay_expiry') AND period_end != ''
		ORDER BY id ASC`)
	if err != nil {
		return nil, err
	}
	type due struct {
		id, kind, acct, currency, periodEnd string
	}
	var dues []due
	for rows.Next() {
		var r due
		if err := rows.Scan(&r.id, &r.kind, &r.acct, &r.currency, &r.periodEnd); err != nil {
			rows.Close()
			return nil, err
		}
		dues = append(dues, r)
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return nil, err
	}

	out := []RenewalEvaluation{}
	for _, r := range dues {
		dueDay, err := parseDueDay(r.periodEnd)
		if err != nil {
			// 单条坏日期不阻断其余条目；跳过并如实返回（无 Triggered）。
			out = append(out, RenewalEvaluation{
				EntryID: r.id, Kind: r.kind, BillingAcct: r.acct,
				Currency: r.currency, DueDate: r.periodEnd, DaysUntil: -1,
			})
			continue
		}
		daysUntil := int(dueDay.Sub(todayDay).Hours() / 24)
		eval := RenewalEvaluation{
			EntryID: r.id, Kind: r.kind, BillingAcct: r.acct,
			Currency: r.currency, DueDate: r.periodEnd, DaysUntil: daysUntil,
		}
		if daysUntil < 0 {
			// 已到期：不提醒（UI 由 dueDate+daysUntil 显示过期状态）。
			out = append(out, eval)
			continue
		}
		for _, w := range renewalWindows {
			if daysUntil != w {
				continue
			}
			if opts.SuppressAlerts {
				eval.Triggered = append(eval.Triggered, w)
				continue
			}
			severity := "info"
			title := "订阅/额度即将到期"
			if w == 0 {
				severity = "warn"
				title = "订阅/额度今日到期"
			}
			detail := fmt.Sprintf("%s（%s，账户 %s）%s 到期（%d 天后）",
				kindLabel(r.kind), r.currency, r.acct, r.periodEnd, w)
			res, err := s.db.Exec(`
				INSERT OR IGNORE INTO usage_alerts (
					id, budget_id, kind, severity, title, detail, period_key, threshold, created_at
				) VALUES (?, ?, 'renewal', ?, ?, ?, ?, ?, ?)`,
				fmt.Sprintf("renewal-%s-%d", r.id, w), r.id,
				severity, title, detail, r.periodEnd, w,
				now.UTC().Format(time.RFC3339Nano))
			if err != nil {
				return nil, err
			}
			if n, _ := res.RowsAffected(); n > 0 {
				eval.Triggered = append(eval.Triggered, w)
			}
		}
		out = append(out, eval)
	}
	return out, nil
}

func kindLabel(kind string) string {
	if kind == "subscription" {
		return "订阅"
	}
	return "预付额度"
}
