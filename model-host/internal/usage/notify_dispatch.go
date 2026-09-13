package usage

// T6/T8 通知投递与收件箱集成（ADR-0030 §6.5、方案 §6.5）：
//
//   - 一条 alert 的系统投递由 DispatchNotification 统一决策：全局事件 ID
//     （usage_alerts.id）+ delivery_state 防止同一事件重复弹窗/多页重复决策；
//   - queued（入队即返回，等待投递）/ submitted（系统接受请求）/
//     failed（调用失败）三态如实回写 delivery_state；系统接受 ≠ 用户已看到，
//     不存在 "seen" 状态（§6.5）；
//   - 投递失败不阻塞、不循环重试；收件箱记录始终保留；
//   - 系统调用与 SQLite 非原子：崩溃边界不宣称 exactly-once（最多一次弹窗，
//     最坏情况 submitted 未回写、下次重发一次）。

import "fmt"

// ErrAlreadyDispatched 表示该 alert 已投递过（delivery_state 非 inbox_only），
// 全局唯一决策直接跳过——调用方不得再次弹窗。
var ErrAlreadyDispatched = fmt.Errorf("notification already dispatched for this alert")

// DispatchNotification 对一条已入箱的 alert 做一次性系统投递决策：
// 读 alert → 去重检查（delivery_state == inbox_only 才继续）→ 投递 →
// 回写 queued/submitted/failed。返回最终投递状态。
// notifier 参数便于测试注入；生产用 SendNotification。
func (s *Store) DispatchNotification(alertID string, send func(title, body string) NotificationResult) (NotificationResult, error) {
	var kind, title, detail, state string
	err := s.db.QueryRow(
		"SELECT kind, title, COALESCE(detail,''), delivery_state FROM usage_alerts WHERE id = ?",
		alertID,
	).Scan(&kind, &title, &detail, &state)
	if err != nil {
		return NotificationFailed, fmt.Errorf("alert %q: %w", alertID, err)
	}
	if state != "inbox_only" {
		// 已投递（queued/submitted/failed 之一）：同一事件只决策一次。
		// failed 允许显式重试场景由用户动作触发，不在本方法内自动重试。
		return NotificationResult(state), ErrAlreadyDispatched
	}

	// 先占位 queued：并发页面看到非 inbox_only 即不再重复弹窗。
	if _, err := s.db.Exec(
		"UPDATE usage_alerts SET delivery_state = 'queued' WHERE id = ? AND delivery_state = 'inbox_only'",
		alertID,
	); err != nil {
		return NotificationFailed, err
	}

	result := send(title, detail)
	if result != NotificationSubmitted && result != NotificationFailed {
		result = NotificationFailed
	}
	stateStr := string(result)
	if _, err := s.db.Exec(
		"UPDATE usage_alerts SET delivery_state = ? WHERE id = ?", stateStr, alertID,
	); err != nil {
		// 回写失败：投递可能已发生但状态留在 queued；不宣称 exactly-once。
		return result, fmt.Errorf("delivery state writeback: %w", err)
	}
	return result, nil
}

// DispatchPendingAttention 对收件箱中未投递的条目逐条投递（页面可见时的
// 恢复路径）。返回 (submitted, failed) 计数；单条失败不影响后续条目。
func (s *Store) DispatchPendingAttention(send func(title, body string) NotificationResult) (int, int) {
	items, err := s.ListAttention(100)
	if err != nil {
		return 0, 0
	}
	submitted, failed := 0, 0
	for _, it := range items {
		result, err := s.DispatchNotification(it.ID, send)
		if err != nil {
			// 已投递过的条目跳过，不计入 failed。
			continue
		}
		if result == NotificationSubmitted {
			submitted++
		} else {
			failed++
		}
	}
	return submitted, failed
}
