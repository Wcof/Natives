package usage

// 整改 E1/E2 验收测试（方案 §11.1 数据与会话硬验收场景）：
//   - 三元会话身份：同 session ID 跨 profile/实例不合并；
//   - 用户时区：跨午夜与夏令时的日桶、活跃天数、today 范围；
//   - 采集 partial/error：权限错误、symlink、轮转恢复、结构化状态；
//   - legacy 身份迁移：旧记录迁移为可识别 legacy-default。

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// TestSessionTripleIdentityDistinctProfiles 同一工具的两个 profile 使用相同
// native session ID：统计为两个会话（§11.1）。
func TestSessionTripleIdentityDistinctProfiles(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	now := time.Date(2026, 9, 11, 10, 0, 0, 0, time.UTC)
	mk := func(id, instance, session string) *Event {
		return &Event{
			ID: id, RequestedAt: now, CreatedAt: now,
			Provider: "anthropic", Model: "claude-3-5-sonnet",
			Source: "Claude Code", ToolID: "claude-code",
			SourceInstanceID: instance, SessionID: session,
			Result: ResultSuccess, TotalTokens: 10,
		}
	}
	for _, e := range []*Event{
		mk("evt-p1", "inst-work", "sess-A"),
		mk("evt-p2", "inst-personal", "sess-A"),
	} {
		if err := s.InsertEvent(e); err != nil {
			t.Fatalf("insert %s: %v", e.ID, err)
		}
	}
	res, err := s.GetSessions(Filter{Range: "all"})
	if err != nil {
		t.Fatalf("GetSessions: %v", err)
	}
	if res.TotalSessions != 2 {
		t.Fatalf("two profiles with same session ID must be 2 sessions, got %d", res.TotalSessions)
	}
	if res.Sessions != nil && len(res.Sessions) != 2 {
		t.Errorf("detail rows = %d, want 2", len(res.Sessions))
	}
}

// TestSessionsRangeTotalVsDailyDistinct 同一 session 跨两天各 10/20 次：
// 区间会话 1、每日各 1、活跃天数 2、请求 30（§11.1）。
func TestSessionsRangeTotalVsDailyDistinct(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	day1 := time.Date(2026, 9, 10, 10, 0, 0, 0, time.UTC)
	day2 := time.Date(2026, 9, 11, 9, 0, 0, 0, time.UTC)
	mk := func(id string, at time.Time) *Event {
		return &Event{
			ID: id, RequestedAt: at, CreatedAt: at,
			Provider: "anthropic", Model: "claude-3-5-sonnet",
			Source: "Claude Code", ToolID: "claude-code",
			SourceInstanceID: "inst-x", SessionID: "sess-A",
			Result: ResultSuccess, TotalTokens: 10,
		}
	}
	for i := 0; i < 10; i++ {
		if err := s.InsertEvent(mk(fmt.Sprintf("evt-d1-%d", i), day1.Add(time.Duration(i)*time.Minute))); err != nil {
			t.Fatal(err)
		}
	}
	for i := 0; i < 20; i++ {
		if err := s.InsertEvent(mk(fmt.Sprintf("evt-d2-%d", i), day2.Add(time.Duration(i)*time.Minute))); err != nil {
			t.Fatal(err)
		}
	}
	res, err := s.GetSessions(Filter{Range: "all"})
	if err != nil {
		t.Fatalf("GetSessions: %v", err)
	}
	if res.TotalSessions != 1 {
		t.Errorf("range sessions = %d, want 1 (distinct, not per-day sum)", res.TotalSessions)
	}
	if res.ActiveDays != 2 {
		t.Errorf("activeDays = %d, want 2", res.ActiveDays)
	}
	if len(res.ByDay) != 2 || res.ByDay[0].Sessions != 1 || res.ByDay[1].Sessions != 1 {
		t.Errorf("byDay = %+v, want one distinct session per day", res.ByDay)
	}
}

// TestInvalidTimezoneRejected 不识别的时区返回错误，不静默退回 UTC（§4.2）。
func TestInvalidTimezoneRejected(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()
	if _, err := s.GetSessions(Filter{Range: "all", Timezone: "Mars/Olympus"}); err == nil {
		t.Fatal("invalid timezone must return an error")
	}
	if _, err := s.GetOverview(Filter{Range: "all", Timezone: "../etc/passwd"}); err == nil {
		t.Fatal("invalid timezone must return an error")
	}
}

// TestTimezoneDayBucketingCrossMidnight 用户时区跨午夜：23:00Z 与次日 01:00Z
// 在 UTC+8 属同一本地日；UTC 下属两日（§11.1）。
func TestTimezoneDayBucketingCrossMidnight(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	mk := func(id string, at time.Time) *Event {
		return &Event{
			ID: id, RequestedAt: at, CreatedAt: at,
			Provider: "openai", Model: "gpt-4o", Result: ResultSuccess,
			Source: "Codex", ToolID: "codex", SourceInstanceID: "inst-x",
			SessionID: "sess-T", TotalTokens: 10,
		}
	}
	for _, e := range []*Event{
		mk("evt-e1", time.Date(2026, 9, 10, 23, 0, 0, 0, time.UTC)),
		mk("evt-e2", time.Date(2026, 9, 11, 1, 0, 0, 0, time.UTC)),
	} {
		if err := s.InsertEvent(e); err != nil {
			t.Fatal(err)
		}
	}
	// UTC：两日。
	resUTC, err := s.GetSessions(Filter{Range: "all"})
	if err != nil {
		t.Fatal(err)
	}
	if resUTC.ActiveDays != 2 {
		t.Errorf("UTC activeDays = %d, want 2", resUTC.ActiveDays)
	}
	// Asia/Shanghai（+8）：同一本地日 2026-09-11。
	resCN, err := s.GetSessions(Filter{Range: "all", Timezone: "Asia/Shanghai"})
	if err != nil {
		t.Fatal(err)
	}
	if resCN.ActiveDays != 1 {
		t.Errorf("Asia/Shanghai activeDays = %d, want 1", resCN.ActiveDays)
	}
	if len(resCN.ByDay) != 1 || resCN.ByDay[0].Date != "2026-09-11" {
		t.Errorf("Asia/Shanghai byDay = %+v, want [2026-09-11]", resCN.ByDay)
	}
	// 区间会话数与时区无关：同一三元键 = 1。
	if resCN.TotalSessions != 1 {
		t.Errorf("totalSessions must not depend on timezone, got %d", resCN.TotalSessions)
	}
}

// TestTimezoneDayBucketingDST 美国东部 2026-03-08 夏令时切换：本地日界按
// 真实偏移分段，不使用固定偏移近似（§11.1）。
func TestTimezoneDayBucketingDST(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	mk := func(id string, at time.Time) *Event {
		return &Event{
			ID: id, RequestedAt: at, CreatedAt: at,
			Provider: "anthropic", Model: "claude-3-5-sonnet", Result: ResultSuccess,
			Source: "Claude Code", ToolID: "claude-code", SourceInstanceID: "inst-x",
			SessionID: "sess-DST", TotalTokens: 10,
		}
	}
	// 03-08T03:59Z  = EST(UTC-5) 本地 03-07 22:59
	// 03-08T05:59Z  = EST 本地 03-08 00:59（切换前）
	// 03-08T07:00:01Z = EDT(UTC-4) 本地 03-08 03:00（切换后）
	for _, e := range []*Event{
		mk("evt-dst-1", time.Date(2026, 3, 8, 3, 59, 0, 0, time.UTC)),
		mk("evt-dst-2", time.Date(2026, 3, 8, 5, 59, 0, 0, time.UTC)),
		mk("evt-dst-3", time.Date(2026, 3, 8, 7, 0, 1, 0, time.UTC)),
	} {
		if err := s.InsertEvent(e); err != nil {
			t.Fatal(err)
		}
	}
	res, err := s.GetSessions(Filter{Range: "all", Timezone: "America/New_York"})
	if err != nil {
		t.Fatal(err)
	}
	if len(res.ByDay) != 2 {
		t.Fatalf("DST byDay entries = %d (%+v), want 2 (2026-03-07 and 2026-03-08)", len(res.ByDay), res.ByDay)
	}
	if res.ByDay[0].Date != "2026-03-07" || res.ByDay[1].Date != "2026-03-08" {
		t.Errorf("DST byDay = %+v, want [2026-03-07 2026-03-08]", res.ByDay)
	}
	// 全部属于同一三元会话。
	if res.TotalSessions != 1 {
		t.Errorf("DST totalSessions = %d, want 1", res.TotalSessions)
	}
}

// TestTodayRangeUsesUserTimezone "today" 按用户本地零点换算：UTC+8 下
// 18:00Z 属今天（本地 02:00），15:00Z 属昨天（本地 23:00）。
func TestTodayRangeUsesUserTimezone(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	mk := func(id string, at time.Time) *Event {
		return &Event{
			ID: id, RequestedAt: at, CreatedAt: at,
			Provider: "openai", Model: "gpt-4o", Result: ResultSuccess,
			TotalTokens: 10,
		}
	}
	now := time.Now().UTC()
	loc, _ := resolveTimezone("Asia/Shanghai")
	midnight := todayStartUTC(now, loc)
	for _, e := range []*Event{
		mk("evt-today-in", midnight.Add(2*time.Hour)),   // 本地今天凌晨 02:00
		mk("evt-today-out", midnight.Add(-1*time.Hour)), // 本地昨天 23:00
	} {
		if err := s.InsertEvent(e); err != nil {
			t.Fatal(err)
		}
	}
	res, err := s.GetOverview(Filter{Range: "today", Timezone: "Asia/Shanghai"})
	if err != nil {
		t.Fatal(err)
	}
	if res.Metrics.TotalRequests != 1 {
		t.Errorf("today (UTC+8) requests = %d, want 1", res.Metrics.TotalRequests)
	}
}

// TestCollectorPartialFileError 单文件权限失败：来源 partial、成功文件数据
// 保留、lastSuccessAt 不更新（§4.4/§5.1）。
func TestCollectorPartialFileError(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	s, _ := tempStore(t)
	defer s.Close()

	projDir := filepath.Join(home, ".claude", "projects", "proj-a")
	if err := os.MkdirAll(projDir, 0o755); err != nil {
		t.Fatal(err)
	}
	good := filepath.Join(projDir, "good.jsonl")
	goodLine := `{"type":"assistant","timestamp":"2026-09-11T09:00:00Z","sessionId":"sess-good","requestId":"req-good","message":{"id":"msg_1","model":"claude-3-5-sonnet","usage":{"input_tokens":100,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":20}}}`
	if err := os.WriteFile(good, []byte(goodLine+"\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	bad := filepath.Join(projDir, "bad.jsonl")
	if err := os.WriteFile(bad, []byte(goodLine+"\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(bad, 0o000); err != nil {
		t.Skipf("cannot chmod on this platform: %v", err)
	}
	defer os.Chmod(bad, 0o600)

	summary, err := s.CollectSources()
	if err != nil {
		t.Fatalf("CollectSources: %v", err)
	}
	var claude *SourceRunStatus
	for i := range summary.Sources {
		if summary.Sources[i].ToolID == "claude-code" {
			claude = &summary.Sources[i]
		}
	}
	if claude == nil {
		t.Fatal("claude-code status missing from collect summary")
	}
	if claude.Availability != SrcPartial {
		t.Errorf("availability = %q, want partial", claude.Availability)
	}
	if claude.RecordsImported != 1 {
		t.Errorf("recordsImported = %d, want 1 (good file still committed)", claude.RecordsImported)
	}
	if len(claude.FileErrors) == 0 {
		t.Error("partial source must carry the file error list")
	}
	// partial 不得更新 lastSuccessAt（§5.1.3）。
	if claude.LastSuccessAt != "" {
		t.Errorf("lastSuccessAt must stay empty after first partial run, got %q", claude.LastSuccessAt)
	}
	states, err := s.ListSourceRuntimeStates()
	if err != nil {
		t.Fatal(err)
	}
	var persisted *SourceRuntimeState
	for i := range states {
		if states[i].SourceID == "claude-code" {
			persisted = &states[i]
		}
	}
	if persisted == nil || persisted.Availability != "partial" {
		t.Errorf("persisted state = %+v, want availability partial", persisted)
	}

	// 修复后重扫：ready 且 lastSuccessAt 更新。
	os.Chmod(bad, 0o600)
	summary2, err := s.CollectSources()
	if err != nil {
		t.Fatal(err)
	}
	for _, st := range summary2.Sources {
		if st.ToolID == "claude-code" && st.Availability != SrcReady {
			t.Errorf("after fix availability = %q, want ready", st.Availability)
		}
	}
}

// TestCollectorSymlinkRefused symlink 拒绝：不越过授权根目录，来源 partial（§5.1.6）。
func TestCollectorSymlinkRefused(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	s, _ := tempStore(t)
	defer s.Close()

	projDir := filepath.Join(home, ".claude", "projects", "proj-a")
	if err := os.MkdirAll(projDir, 0o755); err != nil {
		t.Fatal(err)
	}
	outside := filepath.Join(home, "secret.jsonl")
	if err := os.WriteFile(outside, []byte(`{"type":"assistant"}`+"\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, filepath.Join(projDir, "link.jsonl")); err != nil {
		t.Skipf("symlink unavailable: %v", err)
	}
	// 一个正常文件 + 一个 symlink：部分成功部分失败 = partial（§4.4）。
	good := filepath.Join(projDir, "good.jsonl")
	goodLine := `{"type":"assistant","timestamp":"2026-09-11T09:00:00Z","sessionId":"sess-good","requestId":"req-good","message":{"id":"msg_g","model":"claude-3-5-sonnet","usage":{"input_tokens":10,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":2}}}`
	if err := os.WriteFile(good, []byte(goodLine+"\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	summary, err := s.CollectSources()
	if err != nil {
		t.Fatal(err)
	}
	for _, st := range summary.Sources {
		if st.ToolID != "claude-code" {
			continue
		}
		if st.Availability != SrcPartial {
			t.Fatalf("availability = %q, want partial (symlink refused)", st.Availability)
		}
		found := false
		for _, fe := range st.FileErrors {
			if fe.Code == "symlink_refused" {
				found = true
			}
		}
		if !found {
			t.Errorf("fileErrors missing symlink_refused: %+v", st.FileErrors)
		}
	}
	// 越界文件内容不得被读取：只允许 good.jsonl 的一条事件入库。
	var count int64
	if err := s.db.QueryRow(`SELECT COUNT(*) FROM usage_events WHERE billing_atom = 'claude-code:link'`).Scan(&count); err != nil {
		t.Fatal(err)
	}
	if count != 0 {
		t.Errorf("symlinked content must not be imported")
	}
}

// TestCollectorTruncationRereads 轮转/截断：文件短于游标时从 0 重读且
// billingAtom 幂等去重，不重复计量（§5.1.7）。
func TestCollectorTruncationRereads(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	s, _ := tempStore(t)
	defer s.Close()

	projDir := filepath.Join(home, ".claude", "projects", "proj-a")
	if err := os.MkdirAll(projDir, 0o755); err != nil {
		t.Fatal(err)
	}
	fp := filepath.Join(projDir, "sess.jsonl")
	line := func(req string) string {
		return fmt.Sprintf(`{"type":"assistant","timestamp":"2026-09-11T09:00:00Z","sessionId":"sess-r","requestId":"%s","message":{"id":"m_%s","model":"claude-3-5-sonnet","usage":{"input_tokens":100,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":10}}}`, req, req)
	}
	if err := os.WriteFile(fp, []byte(line("r1")+"\n"+line("r2")+"\n"+line("r4")+"\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := s.CollectSources(); err != nil {
		t.Fatal(err)
	}
	// 模拟轮转：同名文件被替换为更短内容（含一条已见 + 一条新请求）。
	if err := os.WriteFile(fp, []byte(line("r1")+"\n"+line("r3")+"\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := s.CollectSources(); err != nil {
		t.Fatal(err)
	}
	// r1/r2/r4 不重复计量；r3 通过重读补齐导入（billing_atom 幂等验证）。
	rows, err := s.db.Query(`SELECT billing_atom FROM usage_events WHERE tool_id = 'claude-code'`)
	if err != nil {
		t.Fatal(err)
	}
	defer rows.Close()
	reqs := map[string]bool{}
	for rows.Next() {
		var atom string
		if err := rows.Scan(&atom); err != nil {
			t.Fatal(err)
		}
		reqs[atom] = true
	}
	if len(reqs) != 4 {
		t.Errorf("after rotation want exactly r1/r2/r3/r4 atoms, got %v", reqs)
	}
	for _, want := range []string{"claude-code:r1", "claude-code:r2", "claude-code:r3", "claude-code:r4"} {
		if !reqs[want] {
			t.Errorf("missing expected atom %q after rotation: %v", want, reqs)
		}
	}
}

// TestLegacyEventIdentityBackfill 旧记录迁移为可识别身份（§4.1）：
// 显示名映射 tool_id；已识别工具标 legacy-default；proxy 行不冒充实例。
func TestLegacyEventIdentityBackfill(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	// 直接插入旧 schema 形状（无 tool_id/instance）。
	rows := []struct {
		id, source, session string
	}{
		{"evt-old-1", "Claude Code", "sess-old"},
		{"evt-old-2", "Pi", ""},
		{"evt-proxy", "", ""},
	}
	for _, r := range rows {
		if _, err := s.db.Exec(`INSERT INTO usage_events (id, requested_at, latency_ms, ttft_ms, provider, model, result, http_status, source, session_id)
			VALUES (?, '2026-09-11T09:00:00Z', 0, 0, 'anthropic', 'm', 'success', 200, ?, ?)`, r.id, r.source, r.session); err != nil {
			t.Fatal(err)
		}
	}
	if err := s.backfillEventIdentity(); err != nil {
		t.Fatal(err)
	}
	var toolID, inst string
	if err := s.db.QueryRow(`SELECT tool_id, source_instance_id FROM usage_events WHERE id='evt-old-1'`).Scan(&toolID, &inst); err != nil {
		t.Fatal(err)
	}
	if toolID != "claude-code" || inst != "legacy-default" {
		t.Errorf("legacy row = (%q,%q), want (claude-code,legacy-default)", toolID, inst)
	}
	if err := s.db.QueryRow(`SELECT tool_id, source_instance_id FROM usage_events WHERE id='evt-proxy'`).Scan(&toolID, &inst); err != nil {
		t.Fatal(err)
	}
	if toolID != "" || inst != "" {
		t.Errorf("proxy row must keep empty identity, got (%q,%q)", toolID, inst)
	}
	// 迁移后旧记录参与真实会话统计。
	res, err := s.GetSessions(Filter{Range: "all"})
	if err != nil {
		t.Fatal(err)
	}
	if res.TotalSessions != 1 {
		t.Errorf("legacy session count = %d, want 1", res.TotalSessions)
	}
}

// TestDeriveInstanceID 实例 ID 稳定且不含原始路径（§4.1）。
func TestDeriveInstanceID(t *testing.T) {
	a1 := deriveInstanceID("pi", "/home/u/.pi/agent")
	a2 := deriveInstanceID("pi", "/home/u/.pi/agent/../agent")
	if a1 != a2 {
		t.Errorf("same root via different spelling must yield same instance id: %q vs %q", a1, a2)
	}
	b := deriveInstanceID("pi", "/home/u2/.pi/agent")
	if a1 == b {
		t.Error("different roots must yield different instance ids")
	}
	c := deriveInstanceID("codex", "/home/u/.pi/agent")
	if a1 == c {
		t.Error("different tools with same root must differ")
	}
	if strings.Contains(a1, "/") {
		t.Errorf("instance id must not embed raw path: %q", a1)
	}
	if !strings.HasPrefix(a1, "inst-") {
		t.Errorf("instance id must be namespaced, got %q", a1)
	}
}
