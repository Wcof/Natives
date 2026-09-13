package usage

// 事件级归因（整改 E6，方案 §4.3/§8）：
//   - usage_event_subjects 关联事件与 subject（skill / plugin / mcp_server /
//     mcp_tool / agent / subagent / project / session），带 evidence 词表；
//   - direct_owner 全事件唯一；association 可多个但只证明参与；
//   - 同一 (event, subject, role) 重放不新增（主键幂等）；
//   - 只有 stable_id / trace_relation / provider_receipt / user_confirmed
//     证据才允许建关系；缺 relation 不推断（不按时间/模型/Token 相似度建树）；
//   - 不持久化 prompt、tool input 或代码正文。
//
// 会话级归属不在此表：事件已有 session_id 列与 GetSessions 三元键聚合，
// session subject 不逐事件落 usage_event_subjects（避免 1:1 复制膨胀）。

import (
	"fmt"
	"strings"
	"time"
)

// 归因角色与证据词表（方案 §4.3）。
const (
	SubjectRoleDirectOwner = "direct_owner"
	SubjectRoleAssociation = "association"

	EvidenceStableID        = "stable_id"
	EvidenceTraceRelation   = "trace_relation"
	EvidenceProviderReceipt = "provider_receipt"
	EvidenceUserConfirmed   = "user_confirmed"
)

// subjectKinds 为可归因的 subject 类型（§8.2 过滤维度）。
var subjectKinds = map[string]bool{
	"project": true, "session": true, "agent": true, "subagent": true,
	"skill": true, "plugin": true, "mcp_server": true, "mcp_tool": true,
}

// UpsertEventSubjectInput 为一次归因写入。
type UpsertEventSubjectInput struct {
	EventID   string
	SubjectID string
	Role      string
	Evidence  string
}

// UpsertEventSubject 建立事件-subject 归因。约束：
//   - event 必须已存在（不给幽灵收费原子建关系）；
//   - subject 必须已存在（subjects 由显式注册产生，不隐式创建）；
//   - direct_owner 每事件最多一个；
//   - 重放（同 event+subject+role）幂等。
func (s *Store) UpsertEventSubject(in UpsertEventSubjectInput) (inserted bool, err error) {
	if in.EventID == "" || in.SubjectID == "" {
		return false, fmt.Errorf("attribution requires eventId/subjectId")
	}
	switch in.Role {
	case SubjectRoleDirectOwner, SubjectRoleAssociation:
	default:
		return false, fmt.Errorf("role %q must be direct_owner|association", in.Role)
	}
	switch in.Evidence {
	case EvidenceStableID, EvidenceTraceRelation, EvidenceProviderReceipt, EvidenceUserConfirmed:
	default:
		return false, fmt.Errorf("evidence %q not in stable_id|trace_relation|provider_receipt|user_confirmed", in.Evidence)
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	var eventCount, subjectCount int
	if err := s.db.QueryRow("SELECT COUNT(*) FROM usage_events WHERE id = ?", in.EventID).Scan(&eventCount); err != nil {
		return false, err
	}
	if eventCount == 0 {
		return false, fmt.Errorf("event %q not found", in.EventID)
	}
	if err := s.db.QueryRow("SELECT COUNT(*) FROM usage_subjects WHERE id = ?", in.SubjectID).Scan(&subjectCount); err != nil {
		return false, err
	}
	if subjectCount == 0 {
		return false, fmt.Errorf("subject %q not found (register it explicitly first)", in.SubjectID)
	}

	// 重放优先：同 (event, subject, role) 已存在则幂等返回，不再触发
	// direct_owner 唯一性检查。
	var replayCount int
	if err := s.db.QueryRow(
		"SELECT COUNT(*) FROM usage_event_subjects WHERE event_id = ? AND subject_id = ? AND role = ?",
		in.EventID, in.SubjectID, in.Role).Scan(&replayCount); err != nil {
		return false, err
	}
	if replayCount > 0 {
		return false, nil
	}

	if in.Role == SubjectRoleDirectOwner {
		var ownerCount int
		if err := s.db.QueryRow(
			"SELECT COUNT(*) FROM usage_event_subjects WHERE event_id = ? AND role = ?",
			in.EventID, SubjectRoleDirectOwner).Scan(&ownerCount); err != nil {
			return false, err
		}
		if ownerCount > 0 {
			return false, fmt.Errorf("event %q already has a direct_owner subject", in.EventID)
		}
	}

	res, err := s.db.Exec(`
		INSERT OR IGNORE INTO usage_event_subjects (event_id, subject_id, role, evidence)
		VALUES (?, ?, ?, ?)`,
		in.EventID, in.SubjectID, in.Role, in.Evidence)
	if err != nil {
		return false, err
	}
	n, err := res.RowsAffected()
	return n > 0, err
}

// RegisterSubject 显式注册一个 subject（稳定 ID + 类型 + 展示名）。
// 由来源适配器在有官方稳定 ID/relation 时调用；没有证据的来源不注册。
func (s *Store) RegisterSubject(id, kind, label string) error {
	if id == "" {
		return fmt.Errorf("subject id is required")
	}
	if !subjectKinds[kind] {
		return fmt.Errorf("subject kind %q not in known kinds", kind)
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	_, err := s.db.Exec(`
		INSERT INTO usage_subjects (id, kind, label) VALUES (?, ?, ?)
		ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, label = excluded.label`,
		id, kind, label)
	return err
}

// SubjectBreakdown 为单个 subject 的归因视图（§8.2）。
type SubjectBreakdown struct {
	SubjectID     string  `json:"subjectId"`
	Kind          string  `json:"kind,omitempty"`
	Label         string  `json:"label,omitempty"`
	Requests      int64   `json:"requests"`
	Tokens        int64   `json:"tokens"`
	UniqueCostUSD float64 `json:"uniqueCostUsd"` // 仅 direct_owner 口径
	Associations  int64   `json:"associations"`  // 参与的 association 事件数
	FirstAt       string  `json:"firstAt,omitempty"`
	LastAt        string  `json:"lastAt,omitempty"`
}

// SubjectsResult 为 model_usage_subjects 响应。
type SubjectsResult struct {
	Status      string             `json:"status"`
	GeneratedAt string             `json:"generatedAt"`
	Subjects    []SubjectBreakdown `json:"subjects"`
	// UnattributedCostUSD 为无任何 subject 关联的收费原子成本合计。
	UnattributedCostUSD float64 `json:"unattributedCostUsd"`
	// InvariantNote 口径说明：总计只计 direct_owner，association 只证明参与。
	InvariantNote string `json:"invariantNote"`
}

// GetSubjectBreakdown 按 filter 汇总 subject 归因（只统计有 relation 的事件；
// 无 relation 的数据保持未归属，不推断）。
func (s *Store) GetSubjectBreakdown(f Filter) (*SubjectsResult, error) {
	loc, err := resolveTimezone(f.Timezone)
	if err != nil {
		return nil, err
	}
	s.mu.RLock()
	defer s.mu.RUnlock()

	where, args := s.buildFilterWhereIn(f, loc)
	// direct_owner / association 分别聚合；role 条件并入 WHERE。
	ownerWhere := where
	assocWhere := where
	if ownerWhere == "" {
		ownerWhere = "WHERE es.role = 'direct_owner'"
		assocWhere = "WHERE es.role = 'association'"
	} else {
		ownerWhere += " AND es.role = 'direct_owner'"
		assocWhere += " AND es.role = 'association'"
	}

	// direct_owner：事件聚合（每个 subject 的唯一承担成本）。
	ownerRows, err := s.db.Query(fmt.Sprintf(`
		SELECT sub.id, sub.kind, COALESCE(sub.label,''),
			COUNT(DISTINCT e.id),
			COALESCE(SUM(e.total_tokens),0),
			COALESCE(SUM(e.cost_micro),0),
			MIN(e.requested_at), MAX(e.requested_at)
		FROM usage_event_subjects es
		JOIN usage_subjects sub ON sub.id = es.subject_id
		JOIN usage_events e ON e.id = es.event_id
		%s
		GROUP BY sub.id
	`, ownerWhere), args...)
	if err != nil {
		return nil, err
	}
	out := []SubjectBreakdown{}
	seen := map[string]int{}
	for ownerRows.Next() {
		var b SubjectBreakdown
		var costMicro int64
		if err := ownerRows.Scan(&b.SubjectID, &b.Kind, &b.Label, &b.Requests, &b.Tokens, &costMicro, &b.FirstAt, &b.LastAt); err != nil {
			ownerRows.Close()
			return nil, err
		}
		b.UniqueCostUSD = float64(costMicro) / 1000000.0
		seen[b.SubjectID] = len(out)
		out = append(out, b)
	}
	ownerRows.Close()
	if err := ownerRows.Err(); err != nil {
		return nil, err
	}

	// association 计数（参与证据；费用不进总计）。
	assocRows, err := s.db.Query(fmt.Sprintf(`
		SELECT sub.id, COALESCE(sub.kind,''), COALESCE(sub.label,''), COUNT(DISTINCT e.id)
		FROM usage_event_subjects es
		JOIN usage_subjects sub ON sub.id = es.subject_id
		JOIN usage_events e ON e.id = es.event_id
		%s
		GROUP BY sub.id
	`, assocWhere), args...)
	if err != nil {
		return nil, err
	}
	for assocRows.Next() {
		var sid, kind, label string
		var cnt int64
		if err := assocRows.Scan(&sid, &kind, &label, &cnt); err != nil {
			assocRows.Close()
			return nil, err
		}
		if idx, ok := seen[sid]; ok {
			out[idx].Associations = cnt
		} else {
			// 只有 association（无 direct_owner）的 subject：不推算成本（§8.3）。
			out = append(out, SubjectBreakdown{SubjectID: sid, Kind: kind, Label: label, Associations: cnt})
		}
	}
	assocRows.Close()
	if err := assocRows.Err(); err != nil {
		return nil, err
	}

	// 未归属费用：没有 direct_owner 承担的收费原子成本合计
	// （association 只证明参与，不改变归属，§8.3）。
	unattributedWhere := strings.TrimSpace(where)
	if unattributedWhere == "" {
		unattributedWhere = "WHERE 1=1"
	}
	var unattributedMicro int64
	if err := s.db.QueryRow(fmt.Sprintf(`
		SELECT COALESCE(SUM(e.cost_micro),0) FROM usage_events e
		%s AND e.billing_atom != '' AND NOT EXISTS (
			SELECT 1 FROM usage_event_subjects es
			WHERE es.event_id = e.id AND es.role = 'direct_owner')
	`, unattributedWhere), args...).Scan(&unattributedMicro); err != nil {
		return nil, err
	}

	return &SubjectsResult{
		Status:              "ready",
		GeneratedAt:         time.Now().UTC().Format(time.RFC3339),
		Subjects:            out,
		UnattributedCostUSD: float64(unattributedMicro) / 1000000.0,
		InvariantNote:       "总计只计 direct_owner；association 只证明参与，不计入总计",
	}, nil
}

// associateSessionEvents 把工具 hook 声明的 subject 与该会话已入库的事件
// 建立 association 关联（stable_id 证据；hook 是该会话的官方来源）。
// 幂等：同 (event, subject, role) 重放不新增；不建 direct_owner。
func (s *Store) associateSessionEvents(toolID, sessionID string, subjects []ToolEventSubject) error {
	if sessionID == "" {
		return fmt.Errorf("attribution requires sessionId")
	}
	s.mu.Lock()
	var eventIDs []string
	rows, err := s.db.Query(
		"SELECT id FROM usage_events WHERE tool_id = ? AND session_id = ?",
		toolID, sessionID)
	if err != nil {
		s.mu.Unlock()
		return err
	}
	for rows.Next() {
		var id string
		if err := rows.Scan(&id); err == nil {
			eventIDs = append(eventIDs, id)
		}
	}
	rows.Close()
	err = rows.Err()
	s.mu.Unlock()
	if err != nil {
		return err
	}
	for _, sub := range subjects {
		if err := s.RegisterSubject(sub.ID, sub.Kind, sub.Label); err != nil {
			return err
		}
		for _, eventID := range eventIDs {
			if _, err := s.UpsertEventSubject(UpsertEventSubjectInput{
				EventID:   eventID,
				SubjectID: sub.ID,
				Role:      SubjectRoleAssociation,
				Evidence:  EvidenceStableID,
			}); err != nil {
				return err
			}
		}
	}
	return nil
}
