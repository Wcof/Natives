package usage

// T6：受限一次性 tool-event ingest 与提醒入箱（ADR-0030 决策 4、方案 §6.3/§4.4）。
//
// 边界：
//   - 仅白名单事件类型；单条 ≤16 KiB；固定 schema，拒绝额外执行参数；
//   - 幂等：同一事件 ID 重发不重复入箱（方案 §9.2）；
//   - 提醒只入箱记录，不批准、不续跑、不终止原工具任务；
//   - 乱序防护：observed_at 倒退时不倒退覆盖已有状态。

import (
	"database/sql"
	"fmt"
	"time"
)

// maxToolEventBytes 为单条事件输入上限。
const maxToolEventBytes = 16 << 10

// ToolEventSubject 为事件携带的可选归因关系（§4.3/§6）：只接受工具官方
// hook/trace 提供的稳定 ID；没有证据的来源不填，保持 session 级归属。
type ToolEventSubject struct {
	Kind  string `json:"kind"` // skill / plugin / mcp_server / mcp_tool / agent / subagent / project
	ID    string `json:"id"`   // 工具侧稳定 ID
	Label string `json:"label,omitempty"`
	Role  string `json:"role,omitempty"` // 缺省 association；direct_owner 仅限任务事件
}

// ToolEvent 为工具 hook/notify 上报的白名单事件。
type ToolEvent struct {
	EventID    string             `json:"eventId"` // 幂等 ID（工具侧稳定生成）
	ToolID     string             `json:"toolId"`  // claude-code / codex / ...
	SessionID  string             `json:"sessionId"`
	Kind       string             `json:"kind"`       // waiting_permission / waiting_input / ended / error
	OccurredAt string             `json:"occurredAt"` // RFC3339；缺省取服务器时间
	Detail     string             `json:"detail,omitempty"`
	Subjects   []ToolEventSubject `json:"subjects,omitempty"`
}

// Validate 校验白名单与大小限制。
func (e *ToolEvent) Validate() error {
	if e.EventID == "" || e.ToolID == "" || e.SessionID == "" {
		return fmt.Errorf("tool event requires eventId/toolId/sessionId")
	}
	switch e.Kind {
	case "waiting_permission", "waiting_input", "ended", "error":
	default:
		return fmt.Errorf("kind %q not in whitelist", e.Kind)
	}
	if approxEventSize(e) > maxToolEventBytes {
		return fmt.Errorf("tool event exceeds %d bytes limit", maxToolEventBytes)
	}
	if e.OccurredAt != "" {
		if _, err := time.Parse(time.RFC3339, e.OccurredAt); err != nil {
			return fmt.Errorf("occurredAt must be RFC3339: %w", err)
		}
	}
	return nil
}

func approxEventSize(e *ToolEvent) int {
	n := len(e.EventID) + len(e.ToolID) + len(e.SessionID) + len(e.Kind) +
		len(e.OccurredAt) + len(e.Detail)
	for _, sub := range e.Subjects {
		n += len(sub.Kind) + len(sub.ID) + len(sub.Label) + len(sub.Role)
	}
	return n
}

// attentionRank 为提醒排序权重（等待许可 > 等待输入 > 错误 > 本轮结束）。
var attentionRank = map[string]int{
	"waiting_permission": 0,
	"waiting_input":      1,
	"error":              2,
	"ended":              3,
}

// IngestToolEvent 幂等写入事件并更新提醒收件箱与会话状态。
// 返回 (是否新入箱, 错误)。重复 EventID 静默接受但不重复入箱。
func (s *Store) IngestToolEvent(e *ToolEvent) (bool, error) {
	if err := e.Validate(); err != nil {
		return false, err
	}
	observed := time.Now().UTC()
	if e.OccurredAt != "" {
		if ts, err := time.Parse(time.RFC3339, e.OccurredAt); err == nil {
			observed = ts
		}
	}
	observedStr := observed.Format(time.RFC3339Nano)

	// 可选归因：subjects 只在有稳定 ID 时由工具提供；hook 是该会话的
	// 官方来源（evidence=stable_id），把该会话已入库的事件关联为
	// association 参与；不按时间/模型/Token 相似度推断（§8.1）。
	// direct_owner 不从 hook 推断：总成本不因关联而增加（§8.3）。
	// 必须在持有 s.mu 之前执行——associateSessionEvents 会自行加锁
	// （Mutex 非重入，持锁调用会死锁）。
	if len(e.Subjects) > 0 {
		if err := s.associateSessionEvents(e.ToolID, e.SessionID, e.Subjects); err != nil {
			return false, err
		}
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	// 幂等：同 ID 已存在则直接返回（不重复入箱，不倒退状态）。
	var exists int
	if err := s.db.QueryRow(
		"SELECT COUNT(*) FROM usage_alerts WHERE id = ?", "evt-"+e.EventID,
	).Scan(&exists); err != nil {
		return false, err
	}
	if exists > 0 {
		return false, nil
	}

	// 会话状态：不倒退（新观测的 rank 不小于已存 rank 才覆盖状态语义；
	// 时间倒退一律保留旧状态与旧时间）。
	var curState string
	var curObserved string
	err := s.db.QueryRow(
		"SELECT last_state, last_observed_at FROM usage_sessions WHERE id = ?",
		e.SessionID,
	).Scan(&curState, &curObserved)
	switch {
	case err == sql.ErrNoRows:
		_, err = s.db.Exec(
			"INSERT INTO usage_sessions (id, tool_id, last_state, last_observed_at) VALUES (?, ?, ?, ?)",
			e.SessionID, e.ToolID, e.Kind, observedStr)
		if err != nil {
			return false, err
		}
	case err != nil:
		return false, err
	default:
		if curObserved <= observedStr {
			_, err = s.db.Exec(
				"UPDATE usage_sessions SET last_state = ?, last_observed_at = ?, updated_at = ? WHERE id = ?",
				e.Kind, observedStr, observedStr, e.SessionID)
			if err != nil {
				return false, err
			}
		}
	}

	// ended 事件默认只更新状态，不入收件箱（用户显式开启才提醒，T8 处理）。
	if e.Kind == "ended" {
		return false, nil
	}

	title := map[string]string{
		"waiting_permission": "等待许可",
		"waiting_input":      "等待输入",
		"error":              "发生错误",
	}[e.Kind]
	// period_key=e.EventID：attention 条目借 UNIQUE(budget_id, period_key,
	// threshold, kind) 实现按事件去重；否则同 kind 的不同工具/会话事件
	// 会在空元组上冲突被 OR IGNORE 静默丢弃（§9.2：两个工具各等待许可
	// 必须各自出现提醒）。同事件重放仍由主键 id 双重幂等。
	_, err = s.db.Exec(`
		INSERT OR IGNORE INTO usage_alerts (
			id, kind, severity, title, detail, tool_id, session_id, period_key, delivery_state, created_at
		) VALUES (?, ?, 'info', ?, ?, ?, ?, ?, 'inbox_only', ?)`,
		"evt-"+e.EventID, e.Kind, title, e.Detail, e.ToolID, e.SessionID, e.EventID, observedStr)
	if err != nil {
		return false, err
	}
	return true, nil
}

// AttentionItem 为收件箱条目。
type AttentionItem struct {
	ID           string `json:"id"`
	Kind         string `json:"kind"`
	Title        string `json:"title"`
	Detail       string `json:"detail,omitempty"`
	ToolID       string `json:"toolId"`
	SessionID    string `json:"sessionId"`
	CreatedAt    string `json:"createdAt"`
	Acknowledged bool   `json:"acknowledged"`
}

// ListAttention 返回未查看条目（按提醒顺序排序，最多 limit 条）。
func (s *Store) ListAttention(limit int) ([]AttentionItem, error) {
	if limit <= 0 || limit > 100 {
		limit = 5 // 方案 §4.4：默认最多五项
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	rows, err := s.db.Query(`
		SELECT id, kind, title, COALESCE(detail,''), tool_id, session_id, created_at, acknowledged
		FROM usage_alerts
		WHERE acknowledged = 0 AND kind IN ('waiting_permission','waiting_input','error')
		ORDER BY created_at DESC
		LIMIT ?`, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	items := []AttentionItem{}
	for rows.Next() {
		var it AttentionItem
		var ack int
		if err := rows.Scan(&it.ID, &it.Kind, &it.Title, &it.Detail,
			&it.ToolID, &it.SessionID, &it.CreatedAt, &ack); err != nil {
			return nil, err
		}
		it.Acknowledged = ack != 0
		items = append(items, it)
	}
	// 按提醒顺序稳定排序（同 created_at 时 rank 决定先后）。
	for i := 1; i < len(items); i++ {
		for j := i; j > 0 && attentionRank[items[j].Kind] < attentionRank[items[j-1].Kind]; j-- {
			items[j], items[j-1] = items[j-1], items[j]
		}
	}
	return items, nil
}

// AcknowledgeAttention 标记已查看；不修改原工具许可状态。
func (s *Store) AcknowledgeAttention(id string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	res, err := s.db.Exec("UPDATE usage_alerts SET acknowledged = 1 WHERE id = ?", id)
	if err != nil {
		return err
	}
	if n, _ := res.RowsAffected(); n == 0 {
		return fmt.Errorf("attention %q not found", id)
	}
	return nil
}
