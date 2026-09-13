package usage

import (
	"database/sql"
	"encoding/base64"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func tempStore(t *testing.T) (*Store, string) {
	t.Helper()
	dir := t.TempDir()
	path := filepath.Join(dir, "test_usage.db")
	s, err := OpenStore(path)
	if err != nil {
		t.Fatalf("OpenStore failed: %v", err)
	}
	return s, path
}

func TestImporterAcceptsEasyCLIProxyAPIV3Database(t *testing.T) {
	destination, _ := tempStore(t)
	defer destination.Close()
	sourcePath := filepath.Join(t.TempDir(), "usage.db")
	db, err := sql.Open("sqlite", sourcePath)
	if err != nil {
		t.Fatal(err)
	}
	_, err = db.Exec(`CREATE TABLE usage_events (
		id INTEGER PRIMARY KEY, event_key TEXT, timestamp TEXT, timestamp_ms INTEGER,
		latency_ms INTEGER, ttft_ms INTEGER, provider TEXT, auth_index TEXT, model TEXT,
		alias TEXT, source TEXT, endpoint TEXT, api_key_hash TEXT, api_key_remark TEXT,
		failed INTEGER, canceled INTEGER, failure_status INTEGER, input_tokens INTEGER,
		output_tokens INTEGER, cache_read_tokens INTEGER, cache_creation_tokens INTEGER,
		reasoning_tokens INTEGER, total_tokens INTEGER, service_tier TEXT, created_at TEXT);
		INSERT INTO usage_events VALUES (1,'event-1','2026-08-31T12:00:00Z',1788177600000,
		120,20,'codex','account-1','gpt-5.6','', 'oauth','responses','hash-1','默认密钥',
		0,0,200,100,20,10,0,5,120,'default','2026-08-31T12:00:00Z')`)
	if err != nil {
		t.Fatal(err)
	}
	_ = db.Close()
	payload, _ := os.ReadFile(sourcePath)
	importer := NewImporter(destination, NewCalculator(destination))
	session, err := importer.BeginSession("usage.db", int64(len(payload)))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = importer.AppendChunk(session.ID, 0, base64.StdEncoding.EncodeToString(payload)); err != nil {
		t.Fatal(err)
	}
	preview, err := importer.PreviewSession(session.ID)
	if err != nil || !preview.Valid || preview.TotalRecords != 1 {
		t.Fatalf("preview = %+v, %v", preview, err)
	}
	if count, err := importer.CommitSession(session.ID); err != nil || count != 1 {
		t.Fatalf("commit = %d, %v", count, err)
	}
	events, _ := destination.GetEvents(Filter{Range: "all", Limit: 10})
	if len(events.Events) != 1 || events.Events[0].AccessKeyID != "hash-1" || events.Events[0].Model != "gpt-5.6" {
		t.Fatalf("imported events = %+v", events.Events)
	}
}

func TestPriceSyncUsesEasyCatalogAndPreservesManualOverride(t *testing.T) {
	store, _ := tempStore(t)
	defer store.Close()
	if err := store.UpsertPrice(&Price{ModelID: "gpt-test", InputPriceMicro: 9, Source: PriceSourceManual}); err != nil {
		t.Fatal(err)
	}
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		_, _ = io.WriteString(writer, `{"schemaVersion":1,"models":{"gpt-test":{"inputPer1M":1.5,"outputPer1M":2}}}`)
	}))
	defer server.Close()
	if count, err := SyncRemotePrices(store, server.URL); err != nil || count != 1 {
		t.Fatalf("sync = %d, %v", count, err)
	}
	price := NewCalculator(store).FindPrice("", "gpt-test")
	if price == nil || price.Source != PriceSourceManual || price.InputPriceMicro != 9 {
		t.Fatalf("manual price was overwritten: %+v", price)
	}
}

func TestStoreMigrationsAndInsert(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	now := time.Now().UTC()
	err := s.InsertEvent(&Event{
		ID:               "evt-1",
		RequestedAt:      now,
		LatencyMs:        120,
		TTFTMs:           30,
		Provider:         "openai",
		Model:            "gpt-4o",
		Result:           ResultSuccess,
		HTTPStatus:       200,
		InputTokens:      100,
		OutputTokens:     50,
		CacheReadTokens:  20,
		CacheWriteTokens: 0,
		ReasoningTokens:  0,
		TotalTokens:      150,
		CostMicro:        1500,
		ServiceTier:      "default",
	})
	if err != nil {
		t.Fatalf("InsertEvent failed: %v", err)
	}

	res, err := s.GetOverview(Filter{Range: "4h"})
	if err != nil {
		t.Fatalf("GetOverview failed: %v", err)
	}
	if res.Metrics.TotalRequests != 1 {
		t.Errorf("expected 1 total request, got %d", res.Metrics.TotalRequests)
	}
	if res.Metrics.TotalTokens != 150 {
		t.Errorf("expected 150 total tokens, got %d", res.Metrics.TotalTokens)
	}
	if res.Metrics.SuccessRate != 100.0 {
		t.Errorf("expected 100%% success rate, got %f", res.Metrics.SuccessRate)
	}
}

func TestStoreAnalyticsAndEventsPagination(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	now := time.Now().UTC()
	for i := 0; i < 5; i++ {
		s.InsertEvent(&Event{
			ID:          string(rune('a' + i)),
			RequestedAt: now.Add(time.Duration(i) * time.Minute),
			Provider:    "anthropic",
			Model:       "claude-3-5-sonnet",
			Result:      ResultSuccess,
			HTTPStatus:  200,
			TotalTokens: int64(100 * (i + 1)),
		})
	}

	analytics, err := s.GetAnalytics(Filter{Range: "24h"})
	if err != nil {
		t.Fatalf("GetAnalytics failed: %v", err)
	}
	if len(analytics.ByModel) == 0 || analytics.ByModel[0].Requests != 5 {
		t.Errorf("expected 5 requests in ByModel, got %+v", analytics.ByModel)
	}

	events, err := s.GetEvents(Filter{Range: "24h", Limit: 2, Offset: 0})
	if err != nil {
		t.Fatalf("GetEvents failed: %v", err)
	}
	if len(events.Events) != 2 {
		t.Errorf("expected 2 events, got %d", len(events.Events))
	}
	if events.Total != 5 {
		t.Errorf("expected total 5, got %d", events.Total)
	}
	if !events.HasMore {
		t.Errorf("expected hasMore to be true")
	}
}

func TestPricingAndCostCalculation(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	calc := NewCalculator(s)
	// gpt-4o builtin: 2.50 input / 10.00 output per 1M
	cost := calc.CalculateCost("openai", "gpt-4o", 1000000, 1000000, 0, 0)
	// 2500000 + 10000000 = 12500000 micro-usd ($12.50)
	if cost != 12500000 {
		t.Errorf("expected cost 12500000 micro, got %d", cost)
	}

	// Custom manual price
	err := s.UpsertPrice(&Price{
		ProviderID:       "custom-prov",
		ModelID:          "my-model",
		InputPriceMicro:  1000000,
		OutputPriceMicro: 2000000,
		Source:           PriceSourceManual,
	})
	if err != nil {
		t.Fatalf("UpsertPrice failed: %v", err)
	}

	costCustom := calc.CalculateCost("custom-prov", "my-model", 1000000, 1000000, 0, 0)
	if costCustom != 3000000 {
		t.Errorf("expected custom cost 3000000 micro, got %d", costCustom)
	}
}

// TestCalculateCostCacheBucketsNonOverlapping 契约黄金用例（T0 缓存计费）：
// 普通输入 $1/百万，缓存读 $0.1/百万；输入总量 100 万，其中缓存读 80 万、无输出。
// 互斥桶正确费用是 $0.28；把 TotalTokens(含缓存) 当普通输入再叠加缓存价会得到 $1.08。
func TestCalculateCostCacheBucketsNonOverlapping(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	calc := NewCalculator(s)
	if err := s.UpsertPrice(&Price{
		ProviderID:          "golden-prov",
		ModelID:             "golden-model",
		InputPriceMicro:     1000000, // $1 / 1M
		OutputPriceMicro:    0,
		CacheReadPriceMicro: 100000, // $0.1 / 1M
		Source:              PriceSourceManual,
	}); err != nil {
		t.Fatalf("UpsertPrice failed: %v", err)
	}

	// 互斥桶：uncached 200k + cache_read 800k + cache_write 0，输出 0。
	const uncached, cacheRead, cacheWrite = 200000, 800000, 0
	cost := calc.CalculateCost("golden-prov", "golden-model", uncached, 0, cacheRead, cacheWrite)
	if cost != 280000 { // $0.28
		t.Errorf("expected golden cost 280000 micro ($0.28), got %d", cost)
	}

	// 回归护栏：TotalTokens(=uncached+cache) 不能作为普通输入计费传入——
	// 该调用形状对应修复前的重复计费路径，必须显著大于黄金值才说明口径被误用。
	total := uncached + cacheRead
	if duplicated := calc.CalculateCost("golden-prov", "golden-model", int64(total), 0, cacheRead, cacheWrite); duplicated == cost {
		t.Errorf("passing total tokens as uncached input must not produce the golden cost")
	}
}

func TestImporterChunkAndCommit(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	// Create dummy staging database
	stagingDir := t.TempDir()
	stagingPath := filepath.Join(stagingDir, "staging.db")
	srcStore, _ := OpenStore(stagingPath)
	srcStore.InsertEvent(&Event{
		ID:          "imp-1",
		RequestedAt: time.Now().UTC(),
		Provider:    "openai",
		Model:       "gpt-4o",
		Result:      ResultSuccess,
		HTTPStatus:  200,
		TotalTokens: 50,
	})
	srcStore.Close()

	imp := NewImporter(s)
	data, _ := os.ReadFile(stagingPath)
	session, err := imp.BeginSession("staging.db", int64(len(data)))
	if err != nil {
		t.Fatalf("Begin failed: %v", err)
	}

	// Copy dummy database file into staging
	session.File.Write(data)
	session.ReceivedSize = int64(len(data))
	session.File.Close()
	session.File = nil

	preview, err := imp.Preview(session.ID)
	if err != nil {
		t.Fatalf("Preview failed: %v", err)
	}
	if !preview.Valid || preview.TotalRecords != 1 {
		t.Errorf("expected valid preview with 1 record, got %+v", preview)
	}

	committed, err := imp.Commit(session.ID)
	if err != nil {
		t.Fatalf("Commit failed: %v", err)
	}
	if committed != 1 {
		t.Errorf("expected 1 committed event, got %d", committed)
	}

	overview, _ := s.GetOverview(Filter{Range: "all"})
	if overview.TotalEventsCount != 1 {
		t.Errorf("expected 1 event in store, got %d", overview.TotalEventsCount)
	}
}

// TestTrendBucketingCoversFullRange 历史口径（T0）：趋势按范围选桶且不截断。
// 30d 范围必须用日桶返回全部数据（总量对齐），旧实现固定小时桶 + LIMIT 400
// 会在满负载 720 小时时从第 400 小时处截断；24h 必须保持小时桶粒度。
func TestTrendBucketingCoversFullRange(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	base := time.Now().UTC().AddDate(0, 0, -20)
	// 制造跨 20 天、每天 40 个小时的请求（>400 小时桶总量），模拟满负载 30 天。
	var wantReqs, wantTokens int64
	for day := 0; day < 20; day++ {
		for hour := 0; hour < 40; hour++ {
			at := base.AddDate(0, 0, day).Add(time.Duration(hour) * time.Hour)
			if at.After(time.Now().UTC()) {
				continue
			}
			event := &Event{
				ID:          fmt.Sprintf("evt_trend_%d_%d", day, hour),
				RequestedAt: at,
				CreatedAt:   at,
				Provider:    "openai",
				Model:       "gpt-4o",
				Result:      ResultSuccess,
				TotalTokens: 100,
			}
			if _, err := s.InsertEventIfAbsent(event); err != nil {
				t.Fatalf("insert trend event: %v", err)
			}
			wantReqs++
			wantTokens += 100
		}
	}

	// 30d：日桶序列必须覆盖全部请求且总数对齐，无 400 点截断。
	overview, err := s.GetOverview(Filter{Range: "30d"})
	if err != nil {
		t.Fatalf("GetOverview(30d): %v", err)
	}
	var trendReqs, trendTokens int64
	for _, p := range overview.Trend {
		trendReqs += p.Requests
		trendTokens += p.Tokens
		if len(p.Timestamp) < 10 || p.Timestamp[10:11] != "T" {
			t.Fatalf("trend bucket must be ISO timestamp, got %q", p.Timestamp)
		}
		if !strings.HasSuffix(p.Timestamp, "T00:00:00Z") {
			t.Errorf("30d range must use day buckets (T00:00:00Z), got %q", p.Timestamp)
		}
	}
	if trendReqs != wantReqs || trendTokens != wantTokens {
		t.Errorf("trend must cover full range: got reqs=%d tokens=%d, want reqs=%d tokens=%d",
			trendReqs, trendTokens, wantReqs, wantTokens)
	}
	if wantReqs <= 400 {
		t.Fatalf("test setup must exceed 400 hourly buckets to prove no truncation, got %d", wantReqs)
	}

	// 24h：保持小时桶（T..H:00:00Z）。
	overview24, err := s.GetOverview(Filter{Range: "24h"})
	if err != nil {
		t.Fatalf("GetOverview(24h): %v", err)
	}
	// 24h：保持小时桶。午夜整点桶（T00:00:00Z）与日桶字符串同形，
	// 因此断言序列中至少存在一个非 00 点的小时桶来证明小时粒度。
	hasNonMidnightHour := false
	for _, p := range overview24.Trend {
		if len(p.Timestamp) >= 13 && p.Timestamp[11:13] != "00" {
			hasNonMidnightHour = true
		}
	}
	if !hasNonMidnightHour {
		t.Errorf("24h range must use hour buckets, got buckets: %v", overview24.Trend)
	}
}

// TestSchemaV2SemanticMigration T3：语义列增量迁移幂等，新表存在。
func TestSchemaV2SemanticMigration(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	// migrate() 已在 OpenStore 中执行；重复执行验证幂等（duplicate column 跳过）。
	if err := s.migrate(); err != nil {
		t.Fatalf("migrate must be idempotent: %v", err)
	}

	// 关键新表存在。
	for _, table := range []string{
		"usage_sources", "usage_import_cursors", "usage_sessions",
		"usage_budgets", "usage_alerts", "usage_subjects", "usage_subject_edges",
		"usage_charge_components", "usage_billing_entries", "usage_cost_attributions",
	} {
		var name string
		if err := s.db.QueryRow(
			"SELECT name FROM sqlite_master WHERE type='table' AND name=?", table,
		).Scan(&name); err != nil {
			t.Errorf("table %s must exist after v2 migration: %v", table, err)
		}
	}
}

// TestBillingAtomDedup T3：同一 billing_atom 多来源只计一次（唯一索引）。
func TestBillingAtomDedup(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	base := time.Now().UTC().Add(-time.Hour)
	first := &Event{
		ID: "evt_atom_a", RequestedAt: base, CreatedAt: base,
		Provider: "openai", Model: "gpt-4o", Source: "Claude Code",
		Result: ResultSuccess, TotalTokens: 100, BillingAtom: "openai:gpt-4o:req-123",
	}
	if err := s.InsertEvent(first); err != nil {
		t.Fatalf("insert first atom record: %v", err)
	}
	// 同一 atom 的第二条记录（如原生日志重复出现的同一请求）：INSERT OR IGNORE
	// 静默去重（幂等入库），结果必须仍是只计一次。
	dup := &Event{
		ID: "evt_atom_b", RequestedAt: base, CreatedAt: base,
		Provider: "openai", Model: "gpt-4o", Source: "native_log",
		Result: ResultSuccess, TotalTokens: 100, BillingAtom: "openai:gpt-4o:req-123",
	}
	if err := s.InsertEvent(dup); err != nil {
		t.Fatalf("dup insert should be silently ignored, not error: %v", err)
	}
	var atomCount int
	if err := s.db.QueryRow(
		"SELECT COUNT(*) FROM usage_events WHERE billing_atom = 'openai:gpt-4o:req-123'",
	).Scan(&atomCount); err != nil {
		t.Fatalf("count atom records: %v", err)
	}
	if atomCount != 1 {
		t.Errorf("duplicate billing_atom must be counted exactly once, got %d records", atomCount)
	}
	// 空 atom 不受唯一索引约束（旧数据/未知来源照常入库）。
	legacy := &Event{
		ID: "evt_atom_c", RequestedAt: base, CreatedAt: base,
		Provider: "openai", Model: "gpt-4o",
		Result: ResultSuccess, TotalTokens: 50,
	}
	if err := s.InsertEvent(legacy); err != nil {
		t.Errorf("empty billing_atom (legacy record) must insert fine: %v", err)
	}
}

// TestGetSessionsDistinctSemantics 会话聚合语义（R3，方案 §9.0）：
//   - 同一 session 在两天分别有 10/20 次调用 → 区间会话 1、每日各 1、活跃天数 2、请求 30；
//   - 不同工具使用相同 session ID → 不合并（distinct 键含 source）；
//   - 无 session_id 的记录只计入未归属，不造会话。
func TestGetSessionsDistinctSemantics(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	day1 := time.Date(2026, 9, 10, 10, 0, 0, 0, time.UTC)
	day2 := time.Date(2026, 9, 11, 9, 0, 0, 0, time.UTC)

	insert := func(id, source, sessionID string, at time.Time) {
		t.Helper()
		e := &Event{
			ID: id, RequestedAt: at, CreatedAt: at,
			Provider: "anthropic", Model: "claude-3-5-sonnet",
			Source: source, SessionID: sessionID,
			ToolID: source, SourceInstanceID: "legacy-default",
			Result: ResultSuccess, TotalTokens: 10,
		}
		if err := s.InsertEvent(e); err != nil {
			t.Fatalf("insert %s: %v", id, err)
		}
	}
	// 同一 (claude-code, sess-A) 跨两天：10 + 20 次。
	for i := 0; i < 10; i++ {
		insert(fmt.Sprintf("evt_sA1_%d", i), "claude-code", "sess-A", day1.Add(time.Duration(i)*time.Minute))
	}
	for i := 0; i < 20; i++ {
		insert(fmt.Sprintf("evt_sA2_%d", i), "claude-code", "sess-A", day2.Add(time.Duration(i)*time.Minute))
	}
	// Pi 用相同 session ID，不与 claude-code 合并。
	insert("evt_pi_1", "pi", "sess-A", day2)
	// 无 session 记录：只入未归属。
	insert("evt_none_1", "claude-code", "", day1)

	res, err := s.GetSessions(Filter{Range: "all"})
	if err != nil {
		t.Fatalf("GetSessions: %v", err)
	}
	if res.TotalSessions != 2 {
		t.Errorf("totalSessions = %d, want 2 (claude-code|sess-A distinct from pi|sess-A)", res.TotalSessions)
	}
	if res.ActiveDays != 2 {
		t.Errorf("activeDays = %d, want 2", res.ActiveDays)
	}
	if res.Unattributed != 1 {
		t.Errorf("unattributed = %d, want 1", res.Unattributed)
	}
	// 每日 distinct 独立：day1 只有 claude-code|sess-A（1）；
	// day2 有 claude-code|sess-A 和 pi|sess-A 两个 distinct 会话（2）。
	if len(res.ByDay) != 2 {
		t.Fatalf("byDay entries = %d, want 2", len(res.ByDay))
	}
	wantPerDay := map[string]int64{
		"2026-09-10": 1,
		"2026-09-11": 2,
	}
	for _, d := range res.ByDay {
		if want, ok := wantPerDay[d.Date]; !ok || d.Sessions != want {
			t.Errorf("day %s sessions = %d, want %d", d.Date, d.Sessions, want)
		}
	}
	// 按来源排名：Requests 承载 distinct 会话数。
	var claudeStat *RankItem
	for i := range res.BySource {
		if res.BySource[i].Key == "claude-code" {
			claudeStat = &res.BySource[i]
		}
	}
	if claudeStat == nil || claudeStat.Requests != 1 {
		t.Errorf("claude-code distinct sessions = %+v, want 1", claudeStat)
	}
}

// toolIds 接线（R3/§7.4）：Filter.Sources 映射 usage_events.source，
// overview/analysis/events/sessions 四条查询共用 buildFilterWhere。
func TestSourcesFilterScopedToAllAggregates(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	now := time.Now().UTC()
	mk := func(id, source, session string, tokens int64) *Event {
		return &Event{
			ID: id, RequestedAt: now, Provider: "openai", Model: "gpt-4o",
			Result: ResultSuccess, HTTPStatus: 200,
			InputTokens: tokens, TotalTokens: tokens,
			Source: source, SessionID: session,
			ToolID: source, SourceInstanceID: "legacy-default",
		}
	}
	if err := s.InsertEvent(mk("evt-a", "claude-code", "sess-1", 100)); err != nil {
		t.Fatalf("insert evt-a: %v", err)
	}
	if err := s.InsertEvent(mk("evt-b", "codex", "sess-1", 50)); err != nil {
		t.Fatalf("insert evt-b: %v", err)
	}

	f := Filter{Range: "all", Sources: []string{"claude-code"}}

	ov, err := s.GetOverview(f)
	if err != nil {
		t.Fatalf("GetOverview: %v", err)
	}
	if ov.Metrics.TotalRequests != 1 || ov.Metrics.TotalTokens != 100 {
		t.Errorf("overview not scoped: req=%d tokens=%d, want 1/100", ov.Metrics.TotalRequests, ov.Metrics.TotalTokens)
	}

	an, err := s.GetAnalytics(f)
	if err != nil {
		t.Fatalf("GetAnalytics: %v", err)
	}
	if len(an.BySource) != 1 || an.BySource[0].Key != "claude-code" {
		t.Errorf("analytics not scoped: %+v", an.BySource)
	}

	ev, err := s.GetEvents(Filter{Range: "all", Sources: []string{"claude-code"}, Limit: 10})
	if err != nil {
		t.Fatalf("GetEvents: %v", err)
	}
	if len(ev.Events) != 1 || ev.Events[0].Source != "claude-code" {
		t.Errorf("events not scoped: %+v", ev.Events)
	}

	sess, err := s.GetSessions(f)
	if err != nil {
		t.Fatalf("GetSessions: %v", err)
	}
	if sess.TotalSessions != 1 {
		t.Errorf("sessions not scoped: total=%d, want 1", sess.TotalSessions)
	}

	// 空 Sources = 全部来源（scopeMode 语义）。
	sessAll, err := s.GetSessions(Filter{Range: "all"})
	if err != nil {
		t.Fatalf("GetSessions all: %v", err)
	}
	// 同一 session ID 跨工具不合并：claude-code 与 codex 的 sess-1 是 2 个 distinct。
	if sessAll.TotalSessions != 2 {
		t.Errorf("cross-tool same sessionId must not merge: total=%d, want 2", sessAll.TotalSessions)
	}
}
