package usage

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"strings"
	"testing"
	"time"
)

func TestCodexRealResponseReplay(t *testing.T) {
	path := os.Getenv("CODEX_USAGE_REPLAY")
	if path == "" {
		t.Skip("optional sanitized real usage replay")
	}
	content, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	events, _, skipped := ParseCodexSessions(content, time.Now())
	var total int64
	seen := map[string]bool{}
	for _, e := range events {
		if !seen[e.BillingAtom] {
			total += e.TotalTokens
			seen[e.BillingAtom] = true
		}
	}
	var expected int64
	responseIDs := map[string]bool{}
	for _, line := range strings.Split(string(content), "\n") {
		var rec codexUnifiedLine
		if json.Unmarshal([]byte(line), &rec) == nil && rec.Type == "token_usage_record" && rec.Payload != nil && rec.Payload.Usage != nil && !responseIDs[rec.Payload.ResponseID] {
			expected += rec.Payload.Usage.InputTokens + rec.Payload.Usage.OutputTokens
			responseIDs[rec.Payload.ResponseID] = true
		}
	}
	if expected == 0 || skipped != 0 || total != expected {
		t.Fatalf("real replay: tokens=%d want=%d invalid=%d", total, expected, skipped)
	}
}

func TestCollectCodexResponseModeSurvivesRestart(t *testing.T) {
	s, dbPath := tempStore(t)
	defer func() { s.Close() }()
	path := t.TempDir() + "/rollout.jsonl"
	first := `{"type":"session_meta","payload":{"id":"modern-session"}}` + "\n" +
		`{"type":"turn_context","payload":{"model":"gpt-6.1-sol"}}` + "\n" +
		`{"timestamp":"2026-10-12T10:00:00Z","type":"token_usage_record","payload":{"response_id":"modern-r1","usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":20},"thread_token_usage":{"input_tokens":500000100,"output_tokens":1000020}}}` + "\n"
	if err := os.WriteFile(path, []byte(first), 0600); err != nil {
		t.Fatal(err)
	}
	if n, _, err := s.collectCodexFile(context.Background(), path, time.Now(), "inst-test"); err != nil || n != 1 {
		t.Fatalf("first: %d %v", n, err)
	}
	// 升级前游标已有会话／模型但尚未区分计量器，必须从已提交前缀恢复模式。
	if _, err := s.db.Exec("UPDATE usage_import_cursors SET parser_version='codex-sessions/1', baseline_in=500000100, baseline_out=1000020"); err != nil {
		t.Fatal(err)
	}
	s.Close()
	var err error
	s, err = OpenStore(dbPath)
	if err != nil {
		t.Fatal(err)
	}
	legacy := `{"timestamp":"2026-10-12T10:00:00Z","type":"event_msg","payload":{"info":{"total_token_usage":{"input_tokens":490000100,"output_tokens":900020}}}}` + "\n"
	if err := os.WriteFile(path, []byte(first+legacy), 0600); err != nil {
		t.Fatal(err)
	}
	if n, _, err := s.collectCodexFile(context.Background(), path, time.Now(), "inst-test"); err != nil || n != 0 {
		t.Fatalf("legacy after restart: %d %v", n, err)
	}
	// 归档副本与重试仍按同一响应 ID 去重。
	copyPath := t.TempDir() + "/archive.jsonl"
	if err := os.WriteFile(copyPath, []byte(first+legacy), 0600); err != nil {
		t.Fatal(err)
	}
	if n, _, err := s.collectCodexFile(context.Background(), copyPath, time.Now(), "inst-test"); err != nil || n != 0 {
		t.Fatalf("archive: %d %v", n, err)
	}
	var tokens, count int64
	if err := s.db.QueryRow("SELECT COALESCE(SUM(total_tokens),0),COUNT(*) FROM usage_events").Scan(&tokens, &count); err != nil {
		t.Fatal(err)
	}
	if tokens != 120 || count != 1 {
		t.Fatalf("tokens=%d rows=%d", tokens, count)
	}
}

func TestCodexInvalidUsageIsNotToday(t *testing.T) {
	for _, record := range []string{
		`{"timestamp":"2026-10-12T10:00:00Z","type":"token_usage_record","payload":{"session_id":"s","usage":{"input_tokens":100}}}`,
		`{"session_id":"s","info":{"token_usage":{"input_tokens":100}}}`,
		`{"timestamp":"invalid","session_id":"s","info":{"token_usage":{"input_tokens":100}}}`,
		`{"timestamp":"2026-10-12T10:00:00Z","session_id":"s","info":{"token_usage":{"input_tokens":-1}}}`,
		`{"timestamp":"2026-10-12T10:00:00Z","session_id":"s","info":{"token_usage":{"input_tokens":1,"cached_input_tokens":2}}}`,
		`{"timestamp":"2026-10-12T10:00:00Z","session_id":"s","info":{"token_usage":{"input_tokens":9223372036854775807,"output_tokens":1}}}`,
	} {
		events, _, skipped := ParseCodexSessions([]byte(record), time.Now())
		if len(events) != 0 || skipped != 1 {
			t.Fatalf("invalid usage accepted: events=%+v skipped=%d", events, skipped)
		}
	}
	s, _ := tempStore(t)
	defer s.Close()
	path := t.TempDir() + "/rollout.jsonl"
	if err := os.WriteFile(path, []byte(`{"session_id":"s","info":{"token_usage":{"input_tokens":100}}}`+"\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if imported, _, err := s.collectCodexFile(context.Background(), path, time.Now(), "inst-test"); imported != 0 || err == nil {
		t.Fatalf("invalid source must report an error: imported=%d err=%v", imported, err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, _, err := s.collectCodexFile(ctx, path, time.Now(), "inst-test"); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled collection: %v", err)
	}
}

func TestCodexRolloutModelAndResponseUsage(t *testing.T) {
	content := strings.Join([]string{
		`{"type":"session_meta","payload":{"id":"real-session"}}`,
		`{"type":"turn_context","payload":{"model":"gpt-6.1-sol"}}`,
		`{"timestamp":"2026-10-12T10:00:00Z","type":"token_usage_record","payload":{"session_id":"real-session","response_id":"r1","usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":20},"thread_token_usage":{"input_tokens":500000100,"cached_input_tokens":400000040,"output_tokens":1000020}}}`,
		`{"timestamp":"2026-10-12T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":20}}}}`,
		`{"type":"turn_context","payload":{"model":"gpt-6-astra"}}`,
		`{"timestamp":"2026-10-12T10:00:00Z","type":"token_usage_record","payload":{"session_id":"real-session","response_id":"r2","usage":{"input_tokens":60,"cached_input_tokens":20,"output_tokens":10},"thread_token_usage":{"input_tokens":160,"cached_input_tokens":60,"output_tokens":30}}}`,
	}, "\n")
	events, _, _, bl := ParseCodexSessionsWithBaseline([]byte(content), time.Now(), codexCursorBaseline{}, "fixture")
	var total int64
	for _, event := range events {
		total += event.TotalTokens
	}
	if len(events) == 0 || events[0].Model != "gpt-6.1-sol" || total != 190 {
		t.Fatalf("model and distinct response usage: events=%+v total=%d", events, total)
	}
	next := `{"timestamp":"2026-10-12T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":200,"cached_input_tokens":70,"output_tokens":40}}}}`
	events, _, _, _ = ParseCodexSessionsWithBaseline([]byte(next), time.Now(), bl, "fixture")
	if len(events) != 0 || !bl.Responses {
		t.Fatalf("legacy meter must remain ignored across batches: %+v baseline=%+v", events, bl)
	}
	next = `{"timestamp":"2026-10-12T10:00:01Z","type":"token_usage_record","payload":{"response_id":"r3","usage":{"input_tokens":30,"output_tokens":5}}}`
	events, _, _, _ = ParseCodexSessionsWithBaseline([]byte(next), time.Now(), bl, "fixture")
	if len(events) != 1 || events[0].TotalTokens != 35 || events[0].Model != "gpt-6-astra" || events[0].SessionID != "real-session" {
		t.Fatalf("response context: %+v", events)
	}
}

func TestCollectCodexAppendPreservesBaseline(t *testing.T) {
	s, dbPath := tempStore(t)
	defer func() { s.Close() }()
	path := t.TempDir() + "/rollout.jsonl"
	first := `{"type":"session_meta","payload":{"id":"append-session"}}` + "\n" +
		`{"type":"turn_context","payload":{"model":"gpt-6.1-sol"}}` + "\n" +
		`{"timestamp":"2026-10-12T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"output_tokens":20}}}}` + "\n"
	if err := os.WriteFile(path, []byte(first), 0600); err != nil {
		t.Fatal(err)
	}
	if _, _, err := s.collectCodexFile(context.Background(), path, time.Now(), "inst-test"); err != nil {
		t.Fatal(err)
	}
	_, bl, baselineErr := s.getCodexBaseline("codex", fileFingerprint(path))
	if baselineErr != nil {
		t.Fatal(baselineErr)
	}
	if bl.Session != "append-session" || bl.Model != "gpt-6.1-sol" || bl.In != 100 {
		t.Fatalf("saved baseline: %+v", bl)
	}
	// 模拟旧库：两列不存在，首次采集的累计基线尚未持久化。
	legacyAtom := "codex:append-session:e0:snap-" + fileFingerprint(path) + "-0"
	if _, err := s.db.Exec("UPDATE usage_events SET billing_atom=?, id=?", legacyAtom, "nat-"+legacyAtom); err != nil {
		t.Fatal(err)
	}
	if _, err := s.db.Exec(`UPDATE usage_import_cursors SET baseline_in=0, baseline_out=0;
		ALTER TABLE usage_import_cursors DROP COLUMN baseline_session;
		ALTER TABLE usage_import_cursors DROP COLUMN baseline_model;`); err != nil {
		t.Fatal(err)
	}
	s.Close()
	var err error
	s, err = OpenStore(dbPath)
	if err != nil {
		t.Fatal(err)
	}
	next := `{"timestamp":"2026-10-12T10:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":160,"output_tokens":30}}}}` + "\n"
	if err := os.WriteFile(path, []byte(first+next), 0600); err != nil {
		t.Fatal(err)
	}
	if _, _, err := s.collectCodexFile(context.Background(), path, time.Now(), "inst-test"); err != nil {
		t.Fatal(err)
	}
	var total int64
	if err := s.db.QueryRow("SELECT SUM(total_tokens) FROM usage_events").Scan(&total); err != nil {
		t.Fatal(err)
	}
	if total != 190 {
		t.Fatalf("appended total=%d, want 190", total)
	}
	archive := path + ".archived"
	if err := os.WriteFile(archive, []byte(first+next), 0600); err != nil {
		t.Fatal(err)
	}
	if _, _, err := s.collectCodexFile(context.Background(), archive, time.Now(), "inst-test"); err != nil {
		t.Fatal(err)
	}
	overview, err := s.GetOverview(Filter{Range: "all"})
	if err != nil || overview.Metrics.TotalTokens != 190 {
		t.Fatalf("archive overview=%+v err=%v", overview, err)
	}
	analysis, err := s.GetAnalytics(Filter{Range: "all"})
	if err != nil || len(analysis.ByModel) != 1 || analysis.ByModel[0].Name != "gpt-6.1-sol" {
		t.Fatalf("model projection=%+v err=%v", analysis, err)
	}
	sessions, err := s.GetSessions(Filter{Range: "all"})
	if err != nil || sessions.TotalSessions != 1 || sessions.Sessions[0].Tokens != 190 {
		t.Fatalf("session projection=%+v err=%v", sessions, err)
	}
}

func TestCollectCodexCursorFailureRollsBackEvents(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()
	if _, err := s.db.Exec(`CREATE TRIGGER reject_cursor BEFORE INSERT ON usage_import_cursors BEGIN SELECT RAISE(ABORT, 'fixture failure'); END`); err != nil {
		t.Fatal(err)
	}
	path := t.TempDir() + "/rollout.jsonl"
	content := `{"timestamp":"2026-10-12T10:00:00Z","session_id":"rollback","info":{"token_usage":{"input_tokens":100,"output_tokens":20}}}` + "\n"
	if err := os.WriteFile(path, []byte(content), 0600); err != nil {
		t.Fatal(err)
	}
	imported, _, err := s.collectCodexFile(context.Background(), path, time.Now(), "inst-test")
	if err == nil || imported != 0 {
		t.Fatalf("failed cursor imported=%d err=%v", imported, err)
	}
	var count int
	if err := s.db.QueryRow("SELECT COUNT(*) FROM usage_events").Scan(&count); err != nil {
		t.Fatal(err)
	}
	if count != 0 || s.getCursorOffset("codex", fileFingerprint(path)) != 0 {
		t.Fatal("events and cursor must roll back together")
	}
}

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
	imported1, bytes1, err := s.collectCodexFile(context.Background(), path, time.Now().UTC(), "inst-test")
	if err != nil || imported1 != 2 || bytes1 == 0 {
		t.Fatalf("first import: imported=%d bytes=%d err=%v, want 2/>0/nil", imported1, bytes1, err)
	}
	imported2, _, err := s.collectCodexFile(context.Background(), path, time.Now().UTC(), "inst-test")
	if err != nil {
		t.Fatal(err)
	}
	// 游标已到文件尾：重复导入不再产生事件（不重复计量）。
	if imported2 != 0 {
		t.Fatalf("re-import of archived file must import 0, got %d", imported2)
	}
}
