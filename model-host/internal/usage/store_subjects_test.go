package usage

// 整改 E6 验收测试（方案 §11.3 归因不变量）：
//   - 一次请求关联 Skill/MCP/插件/子代理后总成本不增加；
//   - direct_owner 唯一，父子树按唯一收费原子求和；
//   - 重放相同 trace 不新增 subject/edge/费用；
//   - 没有稳定 relation 时不建关系；无效证据被拒绝。

import (
	"testing"
	"time"
)

func subjectTestEvent(s *Store, t *testing.T, id, toolID, session, atom string, costMicro int64) {
	t.Helper()
	e := &Event{
		ID: id, RequestedAt: time.Date(2026, 9, 11, 10, 0, 0, 0, time.UTC),
		CreatedAt: time.Date(2026, 9, 11, 10, 0, 0, 0, time.UTC),
		Provider:  "anthropic", Model: "claude-3-5-sonnet", Result: ResultSuccess,
		Source: "Claude Code", ToolID: toolID, SourceInstanceID: "inst-x",
		SessionID: session, BillingAtom: atom, CostMicro: costMicro, TotalTokens: 100,
	}
	if err := s.InsertEvent(e); err != nil {
		t.Fatalf("insert %s: %v", id, err)
	}
}

// TestAttributionDoesNotDoubleCount 一次请求关联 skill/mcp/plugin/subagent：
// 事件成本不变，总计只按 billing atom 唯一计一次（§11.3）。
func TestAttributionDoesNotDoubleCount(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	subjectTestEvent(s, t, "evt-1", "claude-code", "sess-1", "claude-code:req-1", 1_000_000)

	for _, sub := range []struct{ kind, id, label string }{
		{"skill", "skill:pdf", "PDF Skill"},
		{"mcp_server", "mcp:github", "GitHub MCP"},
		{"plugin", "plugin:lint", "Lint Plugin"},
		{"subagent", "agent:research", "Research Subagent"},
	} {
		if err := s.RegisterSubject(sub.id, sub.kind, sub.label); err != nil {
			t.Fatalf("register %s: %v", sub.id, err)
		}
		if _, err := s.UpsertEventSubject(UpsertEventSubjectInput{
			EventID: "evt-1", SubjectID: sub.id,
			Role: SubjectRoleAssociation, Evidence: EvidenceStableID,
		}); err != nil {
			t.Fatalf("associate %s: %v", sub.id, err)
		}
	}

	res, err := s.GetSubjectBreakdown(Filter{Range: "all"})
	if err != nil {
		t.Fatal(err)
	}
	if len(res.Subjects) != 4 {
		t.Fatalf("subjects = %d, want 4", len(res.Subjects))
	}
	// association 只证明参与：UniqueCost 为 0（无 direct_owner），不推算。
	for _, sub := range res.Subjects {
		if sub.UniqueCostUSD != 0 {
			t.Errorf("association subject %s must not carry unique cost, got %f", sub.SubjectID, sub.UniqueCostUSD)
		}
		if sub.Associations != 1 {
			t.Errorf("subject %s associations = %d, want 1", sub.SubjectID, sub.Associations)
		}
	}
	// 未归属费用仍为完整 1 USD（关联不增加总成本）。
	if res.UnattributedCostUSD != 1.0 {
		t.Errorf("unattributed cost = %f, want 1.0 (association does not add cost)", res.UnattributedCostUSD)
	}
}

// TestAttributionDirectOwnerUnique direct_owner 唯一；重放幂等。
func TestAttributionDirectOwnerUnique(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	subjectTestEvent(s, t, "evt-1", "claude-code", "sess-1", "claude-code:req-1", 500_000)
	if err := s.RegisterSubject("agent:main", "agent", "Main Agent"); err != nil {
		t.Fatal(err)
	}
	if err := s.RegisterSubject("agent:other", "agent", "Other Agent"); err != nil {
		t.Fatal(err)
	}
	inserted, err := s.UpsertEventSubject(UpsertEventSubjectInput{
		EventID: "evt-1", SubjectID: "agent:main",
		Role: SubjectRoleDirectOwner, Evidence: EvidenceTraceRelation,
	})
	if err != nil || !inserted {
		t.Fatalf("first direct_owner: inserted=%v err=%v", inserted, err)
	}
	// 第二个 direct_owner 被拒绝。
	if _, err := s.UpsertEventSubject(UpsertEventSubjectInput{
		EventID: "evt-1", SubjectID: "agent:other",
		Role: SubjectRoleDirectOwner, Evidence: EvidenceTraceRelation,
	}); err == nil {
		t.Fatal("second direct_owner must be rejected")
	}
	// 重放相同 relation：幂等，不新增。
	inserted2, err := s.UpsertEventSubject(UpsertEventSubjectInput{
		EventID: "evt-1", SubjectID: "agent:main",
		Role: SubjectRoleDirectOwner, Evidence: EvidenceTraceRelation,
	})
	if err != nil || inserted2 {
		t.Fatalf("replay: inserted=%v err=%v, want false/nil", inserted2, err)
	}
	res, err := s.GetSubjectBreakdown(Filter{Range: "all"})
	if err != nil {
		t.Fatal(err)
	}
	if len(res.Subjects) != 1 || res.Subjects[0].UniqueCostUSD != 0.5 {
		t.Fatalf("subjects = %+v, want single direct_owner with 0.5", res.Subjects)
	}
	if res.UnattributedCostUSD != 0 {
		t.Errorf("unattributed = %f, want 0 (direct owner owns the atom)", res.UnattributedCostUSD)
	}
}

// TestAttributionRejectsInference 无证据/未知 kind/幽灵事件被拒绝（不推断）。
func TestAttributionRejectsInference(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	subjectTestEvent(s, t, "evt-1", "claude-code", "sess-1", "claude-code:req-1", 100)
	if err := s.RegisterSubject("skill:x", "skill", "X"); err != nil {
		t.Fatal(err)
	}
	// 无效证据词表。
	if _, err := s.UpsertEventSubject(UpsertEventSubjectInput{
		EventID: "evt-1", SubjectID: "skill:x",
		Role: SubjectRoleAssociation, Evidence: "token_similarity",
	}); err == nil {
		t.Fatal("non-whitelisted evidence must be rejected")
	}
	// 幽灵事件。
	if _, err := s.UpsertEventSubject(UpsertEventSubjectInput{
		EventID: "evt-missing", SubjectID: "skill:x",
		Role: SubjectRoleAssociation, Evidence: EvidenceStableID,
	}); err == nil {
		t.Fatal("attribution to a missing event must be rejected")
	}
	// 未注册 subject。
	if _, err := s.UpsertEventSubject(UpsertEventSubjectInput{
		EventID: "evt-1", SubjectID: "skill:unregistered",
		Role: SubjectRoleAssociation, Evidence: EvidenceStableID,
	}); err == nil {
		t.Fatal("attribution to an unregistered subject must be rejected")
	}
}

// TestToolEventSubjectAssociation 工具 hook 携带 subject 时：会话内事件
// 建立 association（stable_id），重放不新增（§6/§8.1）。
func TestToolEventSubjectAssociation(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	subjectTestEvent(s, t, "evt-1", "claude-code", "sess-1", "claude-code:req-1", 100)
	evt := &ToolEvent{
		EventID:   "hook-1",
		ToolID:    "claude-code",
		SessionID: "sess-1",
		Kind:      "ended",
		Subjects: []ToolEventSubject{
			{Kind: "skill", ID: "skill:code-review", Label: "Code Review"},
		},
	}
	admitted, err := s.IngestToolEvent(evt)
	if err != nil {
		t.Fatalf("ingest: %v", err)
	}
	_ = admitted
	// 重放同一 hook：幂等。
	if _, err := s.IngestToolEvent(evt); err != nil {
		t.Fatalf("replay: %v", err)
	}
	var count int
	if err := s.db.QueryRow("SELECT COUNT(*) FROM usage_event_subjects WHERE subject_id = 'skill:code-review'").Scan(&count); err != nil {
		t.Fatal(err)
	}
	if count != 1 {
		t.Errorf("event-subject rows = %d, want 1 (replay idempotent)", count)
	}
	res, err := s.GetSubjectBreakdown(Filter{Range: "all"})
	if err != nil {
		t.Fatal(err)
	}
	if len(res.Subjects) != 1 || res.Subjects[0].SubjectID != "skill:code-review" || res.Subjects[0].Associations != 1 {
		t.Fatalf("breakdown = %+v, want code-review with 1 association", res.Subjects)
	}
}
