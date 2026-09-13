package usage

// OpenCode 采集器 fixture 测试：按本机实测 schema（opencode 1.18.3）冻结契约。
// 期望值均为测试数据（§9.1）。

import (
	"database/sql"
	"path/filepath"
	"testing"

	_ "modernc.org/sqlite"
)

func openTestSQLite(dbPath string) (*sql.DB, error) {
	return sql.Open("sqlite", dbPath)
}

func fixtureOpenCodeDB(t *testing.T) string {
	t.Helper()
	dbPath := filepath.Join(t.TempDir(), "opencode.db")
	db, err := openTestSQLite(dbPath)
	if err != nil {
		t.Fatalf("open: %v", err)
	}
	defer db.Close()

	schema := `
CREATE TABLE message (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  time_created INTEGER NOT NULL,
  time_updated INTEGER NOT NULL,
  data TEXT NOT NULL
);`
	if _, err := db.Exec(schema); err != nil {
		t.Fatalf("schema: %v", err)
	}
	rows := []struct {
		id, session string
		created     int64
		data        string
	}{
		{
			id: "msg-1", session: "sess-A", created: 1760000000000,
			// input 已含 cache 子集（OpenAI 口径）：input_uncached=10639-800=9839。
			data: `{"role":"assistant","modelID":"kimi-k2","providerID":"moonshot","cost":0.012,
				"tokens":{"input":10639,"output":420,"reasoning":30,"cache":{"write":120,"read":800}},"finish":"stop"}`,
		},
		{
			id: "msg-2", session: "sess-A", created: 1760000600000,
			data: `{"role":"assistant","modelID":"kimi-k2","providerID":"moonshot","cost":0,
				"tokens":{"input":0,"output":0,"reasoning":0,"cache":{"read":0,"write":0}},"finish":"stop"}`,
		},
		{id: "msg-user", session: "sess-A", created: 1760000100000, data: `{"role":"user","tokens":{}}`},
		{id: "msg-bad", session: "sess-A", created: 1760000200000, data: `{not-json`},
		{
			id: "msg-nostr", session: "", created: 1760000300000,
			data: `{"role":"assistant","modelID":"","providerID":"","tokens":{"input":10,"output":5}}`,
		},
	}
	for _, r := range rows {
		if _, err := db.Exec(
			"INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES (?,?,?,?,?)",
			r.id, r.session, r.created, r.created, r.data,
		); err != nil {
			t.Fatalf("seed %s: %v", r.id, err)
		}
	}
	return dbPath
}

func TestParseOpenCodeDB(t *testing.T) {
	dbPath := fixtureOpenCodeDB(t)
	events, parsed, err := ParseOpenCodeDB(dbPath)
	if err != nil {
		t.Fatalf("ParseOpenCodeDB: %v", err)
	}
	// msg-1 计量；msg-2 全零跳过；msg-user 非 assistant 跳过；msg-bad 坏行跳过；
	// msg-nostr 无 session 仍计量（§4.1.1 未归属由 GetSessions 单独计数）。
	if parsed != 2 || len(events) != 2 {
		t.Fatalf("parsed=%d events=%d, want 2/2", parsed, len(events))
	}

	first := events[0]
	// OpenAI 口径 → 非重叠桶：input_uncached = 10639 - 800 - 120 = 9719。
	if first.InputTokens != 9719 {
		t.Errorf("input_uncached=%d, want 9719 (no double-count of cache subset)", first.InputTokens)
	}
	if first.CacheReadTokens != 800 || first.CacheWriteTokens != 120 {
		t.Errorf("cache buckets=%d/%d, want 800/120", first.CacheReadTokens, first.CacheWriteTokens)
	}
	if first.OutputTokens != 420 {
		t.Errorf("output=%d, want 420", first.OutputTokens)
	}
	// TotalTokens = 9719+800+120+420 = 11059（reasoning 已含于 output 口径，不重复加）。
	if first.TotalTokens != 11059 {
		t.Errorf("total=%d, want 11059", first.TotalTokens)
	}
	// 稳定计费原子与 session 归属。
	if first.BillingAtom != "opencode:msg-1" {
		t.Errorf("billingAtom=%q", first.BillingAtom)
	}
	if first.SessionID != "sess-A" {
		t.Errorf("sessionId=%q", first.SessionID)
	}
	if first.Model != "kimi-k2" || first.Provider != "moonshot" {
		t.Errorf("model/provider=%q/%q", first.Model, first.Provider)
	}
	if first.RecordKind != "request" || first.ParserVersion != "opencode-store/1" {
		t.Errorf("recordKind/parserVersion=%q/%q", first.RecordKind, first.ParserVersion)
	}
	// 幂等原子：同 id 重复解析产生相同 atom（重复导入由唯一索引去重）。
	again, _, err := ParseOpenCodeDB(dbPath)
	if err != nil || len(again) == 0 || again[0].BillingAtom != first.BillingAtom {
		t.Fatalf("re-parse not idempotent: %v", err)
	}

	second := events[1]
	if second.SessionID != "" {
		t.Errorf("sessionless event must keep empty sessionId, got %q", second.SessionID)
	}
}

func TestParseOpenCodeDBRejectsForeignSchema(t *testing.T) {
	dbPath := filepath.Join(t.TempDir(), "other.db")
	db, err := openTestSQLite(dbPath)
	if err != nil {
		t.Fatalf("open: %v", err)
	}
	defer db.Close()
	if _, err := db.Exec(`CREATE TABLE unrelated (x INTEGER)`); err != nil {
		t.Fatalf("schema: %v", err)
	}
	if _, _, err := ParseOpenCodeDB(dbPath); err == nil {
		t.Fatal("expected explicit failure for schema without message table")
	}
}
