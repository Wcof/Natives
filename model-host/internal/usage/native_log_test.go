package usage

import (
	"os"
	"strings"
	"testing"
	"time"
)

// 脱敏 fixture（T4 source contract）：合成、非敏感的最小行集，
// 冻结期望值；真实工具 fixture 后续按同 schema 冻结替换。

func TestParseClaudeJSONLFixture(t *testing.T) {
	// 脱敏 fixture：2 条 assistant 行（1 条带 requestId，1 条缺）+ 1 条非用量行 + 1 条损坏行。
	content := strings.Join([]string{
		`{"type":"user","timestamp":"2026-09-11T09:00:00Z","sessionId":"sess-A","message":{"role":"user","content":"redacted"}}`,
		`{"type":"assistant","timestamp":"2026-09-11T09:00:05Z","sessionId":"sess-A","requestId":"req_001","message":{"model":"claude-3-5-sonnet-20241022","usage":{"input_tokens":1000,"cache_read_input_tokens":8000,"cache_creation_input_tokens":200,"output_tokens":500}}}`,
		`{"type":"assistant","timestamp":"2026-09-11T09:01:00Z","sessionId":"sess-A","message":{"model":"claude-3-5-sonnet-20241022","usage":{"input_tokens":300,"output_tokens":100}}}`,
		`{not-json`,
		`{"type":"assistant","timestamp":"2026-09-11T09:02:00Z","sessionId":"sess-A"}`,
	}, "\n")

	events, parsed, skipped := ParseClaudeJSONL([]byte(content), time.Now().UTC())
	if parsed != 2 || skipped != 3 {
		t.Fatalf("parsed=%d skipped=%d, want 2/3", parsed, skipped)
	}
	if len(events) != 2 {
		t.Fatalf("expected 2 events, got %d", len(events))
	}
	e0 := events[0]
	// 互斥桶：uncached=1000；缓存读/写独立；total=1000+8000+200+500=9700。
	if e0.InputTokens != 1000 || e0.CacheReadTokens != 8000 || e0.CacheWriteTokens != 200 {
		t.Errorf("mutually exclusive buckets wrong: %+v", e0)
	}
	if e0.TotalTokens != 9700 {
		t.Errorf("total = %d, want 9700", e0.TotalTokens)
	}
	// 稳定计费原子来自 requestId。
	if e0.BillingAtom != "claude-code:req_001" {
		t.Errorf("billing atom = %q, want claude-code:req_001", e0.BillingAtom)
	}
	// 缺 requestId：不伪造 atom（legacy_unknown 语义），仍有稳定 record ID。
	if events[1].BillingAtom != "" {
		t.Errorf("missing requestId must not fabricate an atom, got %q", events[1].BillingAtom)
	}
	if events[1].SourceRecordID == "" {
		t.Errorf("missing requestId must still produce a stable record id")
	}
	if e0.SessionID != "sess-A" {
		t.Errorf("sessionId = %q, want sess-A", e0.SessionID)
	}
}

func TestParseCodexCumulativeDiff(t *testing.T) {
	// 验收矩阵 §9.1：累计快照 1000 → 1000 → 1600 → 归一增量 1000、0、600。
	content := strings.Join([]string{
		`{"timestamp":"2026-09-11T09:00:00Z","session_id":"cx-1","info":{"token_usage":{"input_tokens":1000,"cached_input_tokens":400,"output_tokens":200}}}`,
		`{"timestamp":"2026-09-11T09:01:00Z","session_id":"cx-1","info":{"token_usage":{"input_tokens":1000,"cached_input_tokens":400,"output_tokens":200}}}`,
		`{"timestamp":"2026-09-11T09:02:00Z","session_id":"cx-1","info":{"token_usage":{"input_tokens":1600,"cached_input_tokens":600,"output_tokens":350}}}`,
	}, "\n")

	events, parsed, skipped := ParseCodexSessions([]byte(content), time.Now().UTC())
	if parsed != 3 || skipped != 0 {
		t.Fatalf("parsed=%d skipped=%d, want 3/0", parsed, skipped)
	}
	deltas := []struct{ in, cache, out int64 }{
		{events[0].InputTokens, events[0].CacheReadTokens, events[0].OutputTokens},
		{events[1].InputTokens, events[1].CacheReadTokens, events[1].OutputTokens},
		{events[2].InputTokens, events[2].CacheReadTokens, events[2].OutputTokens},
	}
	// 增量 1：总输入 1000（其中缓存 400 → uncached 600）、out 200。互斥桶口径。
	if deltas[0].in != 600 || deltas[0].cache != 400 || deltas[0].out != 200 {
		t.Errorf("snapshot1 delta wrong (uncached/cache/out): %+v", deltas[0])
	}
	// 重复快照：全 0，且 recordKind=turn_delta。
	if deltas[1].in != 0 || deltas[1].cache != 0 || deltas[1].out != 0 {
		t.Errorf("duplicate snapshot must normalize to zero delta: %+v", deltas[1])
	}
	if events[1].RecordKind != "turn_delta" {
		t.Errorf("duplicate snapshot recordKind = %q, want turn_delta", events[1].RecordKind)
	}
	// 增量 3：总输入 600（其中缓存 200 → uncached 400）、out 150。
	if deltas[2].in != 400 || deltas[2].cache != 200 || deltas[2].out != 150 {
		t.Errorf("snapshot3 delta wrong (uncached/cache/out): %+v", deltas[2])
	}
}

func TestParseCodexSequenceReset(t *testing.T) {
	// 序列重置：累计 1600 → 300（回退/重启）→ 新 epoch，delta=300，不静默截负续旧账。
	content := strings.Join([]string{
		`{"timestamp":"2026-09-11T09:00:00Z","session_id":"cx-2","info":{"token_usage":{"input_tokens":1600,"cached_input_tokens":0,"output_tokens":0}}}`,
		`{"timestamp":"2026-09-11T09:05:00Z","session_id":"cx-2","info":{"token_usage":{"input_tokens":300,"cached_input_tokens":0,"output_tokens":50}}}`,
	}, "\n")

	events, _, _ := ParseCodexSessions([]byte(content), time.Now().UTC())
	if len(events) != 2 {
		t.Fatalf("expected 2 events, got %d", len(events))
	}
	if events[1].InputTokens != 300 || events[1].OutputTokens != 50 {
		t.Errorf("reset must start a new sequence with delta=current: %+v", events[1])
	}
	// 两个 epoch 的 atom 必须不同（避免去重误伤新序列）。
	if events[0].BillingAtom == events[1].BillingAtom {
		t.Errorf("reset must produce distinct billing atoms, both %q", events[0].BillingAtom)
	}
	if !strings.Contains(events[1].BillingAtom, "e1") {
		t.Errorf("second sequence should carry epoch suffix e1, got %q", events[1].BillingAtom)
	}
}

func TestNativeEventInsertDedup(t *testing.T) {
	// 端到端：同一 fixture 导入两次，billingAtom 去重保证统计只计一次。
	s, _ := tempStore(t)
	defer s.Close()

	content := `{"type":"assistant","timestamp":"2026-09-11T09:00:05Z","sessionId":"sess-A","requestId":"req_dup","message":{"model":"claude-3-5-sonnet-20241022","usage":{"input_tokens":100,"output_tokens":10}}}`
	events, _, _ := ParseClaudeJSONL([]byte(content), time.Now().UTC())
	if len(events) != 1 {
		t.Fatalf("expected 1 event, got %d", len(events))
	}
	if err := s.InsertEvent(nativeEventToEvent(events[0])); err != nil {
		t.Fatalf("first insert: %v", err)
	}
	if err := s.InsertEvent(nativeEventToEvent(events[0])); err != nil {
		t.Fatalf("re-import must be silently ignored: %v", err)
	}
	overview, err := s.GetOverview(Filter{Range: "all"})
	if err != nil {
		t.Fatalf("GetOverview: %v", err)
	}
	if overview.Metrics.TotalRequests != 1 {
		t.Errorf("re-imported event must be counted once, got %d requests", overview.Metrics.TotalRequests)
	}
}

func TestParseCodexCrossBatchBaseline(t *testing.T) {
	// §3.2：分批读取时 delta 必须对上一批持久化基线求差，
	// 且 atomSalt 保证跨批 atom 不冲突（重复导入不丢新数据）。
	now := time.Now().UTC()
	batch1 := strings.Join([]string{
		`{"timestamp":"2026-09-11T09:00:00Z","session_id":"cx-9","info":{"token_usage":{"input_tokens":1000,"cached_input_tokens":400,"output_tokens":200}}}`,
		`{"timestamp":"2026-09-11T09:01:00Z","session_id":"cx-9","info":{"token_usage":{"input_tokens":1200,"cached_input_tokens":500,"output_tokens":250}}}`,
	}, "\n")
	batch2 := `{"timestamp":"2026-09-11T09:02:00Z","session_id":"cx-9","info":{"token_usage":{"input_tokens":1600,"cached_input_tokens":600,"output_tokens":350}}}`

	ev1, _, _, bl := ParseCodexSessionsWithBaseline([]byte(batch1), now, codexCursorBaseline{}, "fp-a")
	if len(ev1) != 2 {
		t.Fatalf("batch1 events = %d, want 2", len(ev1))
	}
	if bl.Session != "cx-9" || bl.In != 1200 || bl.Cache != 500 || bl.Out != 250 {
		t.Fatalf("baseline after batch1 wrong: %+v", bl)
	}
	ev2, _, _, bl2 := ParseCodexSessionsWithBaseline([]byte(batch2), now, bl, "fp-a")
	if len(ev2) != 1 {
		t.Fatalf("batch2 events = %d, want 1", len(ev2))
	}
	// 1600-1200=400 总输入（缓存 600-500=100 → uncached 300）；无基线会错算 1600。
	if ev2[0].InputTokens != 300 || ev2[0].CacheReadTokens != 100 || ev2[0].OutputTokens != 100 {
		t.Fatalf("batch2 delta must diff against persisted baseline: %+v", ev2[0])
	}
	if bl2.In != 1600 {
		t.Fatalf("baseline after batch2 wrong: %+v", bl2)
	}
	// 跨批 atom 必须不同：盐内嵌 fp，第二批不会与第一批 atom 冲突而被去重丢弃。
	if ev1[1].BillingAtom == ev2[0].BillingAtom {
		t.Fatalf("cross-batch atoms must differ via salt: %q", ev2[0].BillingAtom)
	}
}

func TestCollectCodexArchivedFileNoDoubleCount(t *testing.T) {
	// §9.1：同一文件重复导入（归档路径）统计不增加；游标与基线持久化。
	s, _ := tempStore(t)
	defer s.Close()

	dir := t.TempDir()
	path := dir + "/rollout-archived.jsonl"
	content := strings.Join([]string{
		`{"timestamp":"2026-09-11T09:00:00Z","session_id":"cx-a","info":{"token_usage":{"input_tokens":1000,"cached_input_tokens":0,"output_tokens":200}}}`,
		`{"timestamp":"2026-09-11T09:01:00Z","session_id":"cx-a","info":{"token_usage":{"input_tokens":1600,"cached_input_tokens":0,"output_tokens":350}}}`,
	}, "\n") + "\n"
	if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
		t.Fatal(err)
	}
	imported1, bytes1, err := s.collectCodexFile(path, time.Now().UTC(), "inst-test")
	if err != nil || imported1 != 2 || bytes1 == 0 {
		t.Fatalf("first import: imported=%d bytes=%d err=%v, want 2/>0/nil", imported1, bytes1, err)
	}
	imported2, _, err := s.collectCodexFile(path, time.Now().UTC(), "inst-test")
	if err != nil {
		t.Fatal(err)
	}
	// 游标已到文件尾：重复导入不再产生事件（不重复计量）。
	if imported2 != 0 {
		t.Fatalf("re-import of archived file must import 0, got %d", imported2)
	}
}
