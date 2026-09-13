package usage

// Hermes state.db 只读投影测试（R4）：用临时 SQLite 构造最小 state.db
// （schema_version + sessions 白名单列），验证投影、幂等原子与无用量跳过。
// 不使用真实用户 DB；fixture 仅含元数据列。

import (
	"path/filepath"
	"testing"

	"database/sql"
	_ "modernc.org/sqlite"
)

func TestParseHermesStateDBProjection(t *testing.T) {
	dbPath := filepath.Join(t.TempDir(), "state.db")
	db, err := sql.Open("sqlite", dbPath)
	if err != nil {
		t.Fatal(err)
	}
	setup := []string{
		`CREATE TABLE schema_version (version INTEGER NOT NULL)`,
		`INSERT INTO schema_version VALUES (23)`,
		`CREATE TABLE sessions (
			id TEXT PRIMARY KEY, source TEXT NOT NULL, model TEXT,
			started_at REAL NOT NULL, ended_at REAL,
			input_tokens INTEGER DEFAULT 0, output_tokens INTEGER DEFAULT 0,
			cache_read_tokens INTEGER DEFAULT 0, cache_write_tokens INTEGER DEFAULT 0,
			reasoning_tokens INTEGER DEFAULT 0, billing_mode TEXT, cost_status TEXT,
			parent_session_id TEXT)`,
		// 有用量的会话（压缩续接 parent 不影响投影）。
		`INSERT INTO sessions (id, source, model, started_at, ended_at, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens, billing_mode, cost_status, parent_session_id)
		 VALUES ('sess-H1', 'cli', 'glm-5.3-flash', 1789098000.0, 1789099200.0, 1000, 200, 800, 100, 50, 'api', 'ok', 'sess-H0')`,
		// 无用量会话：不产生计费事件。
		`INSERT INTO sessions (id, source, model, started_at, input_tokens)
		 VALUES ('sess-H2', 'cli', 'glm-5.3-flash', 1789100000.0, 0)`,
	}
	for _, q := range setup {
		if _, err := db.Exec(q); err != nil {
			t.Fatalf("setup: %v", err)
		}
	}
	_ = db.Close()

	events, parsed, err := ParseHermesStateDB(dbPath)
	if err != nil {
		t.Fatalf("ParseHermesStateDB: %v", err)
	}
	if parsed != 1 || len(events) != 1 {
		t.Fatalf("parsed = %d, events = %d, want 1/1（无用量会话必须跳过）", parsed, len(events))
	}
	e := events[0]
	if e.BillingAtom != "hermes:sess-H1" {
		t.Errorf("atom = %q, want hermes:sess-H1（session 级幂等原子）", e.BillingAtom)
	}
	if e.InputTokens != 1000 || e.OutputTokens != 200 || e.CacheReadTokens != 800 || e.CacheWriteTokens != 100 {
		t.Errorf("tokens = %d/%d/%d/%d", e.InputTokens, e.OutputTokens, e.CacheReadTokens, e.CacheWriteTokens)
	}
	if e.TotalTokens != 2150 {
		t.Errorf("totalTokens = %d, want 2150（含 reasoning）", e.TotalTokens)
	}
	if e.SessionID != "sess-H1" {
		t.Errorf("sessionId = %q", e.SessionID)
	}
	if e.RecordKind != "session_delta" {
		t.Errorf("recordKind = %q, want session_delta（session 级总量，不冒充逐请求）", e.RecordKind)
	}
}

func TestParseHermesStateDBRejectsNonProfileDB(t *testing.T) {
	dbPath := filepath.Join(t.TempDir(), "not-hermes.db")
	db, err := sql.Open("sqlite", dbPath)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := db.Exec(`CREATE TABLE other (x INTEGER)`); err != nil {
		t.Fatal(err)
	}
	_ = db.Close()

	if _, _, err := ParseHermesStateDB(dbPath); err == nil {
		t.Errorf("non-profile DB must fail explicitly, got nil error")
	}
}
