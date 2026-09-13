package usage

// v3 schema（整改 E1/E2/E6）：三元来源实例身份、结构化来源运行状态、
// 事件级归因关联表。全部为幂等增量迁移；不建第二张用量表。

// identityColumns 为 usage_events 补齐的三元身份列（方案 §4.1）：
//
//	tool_id            规范工具 ID（claude-code / codex / pi / ...）
//	source_instance_id 同工具内稳定的账号/profile/安装实例 ID
//	native session id  复用既有 session_id 列
//
// 会话唯一键 = (tool_id, source_instance_id, session_id)。
var identityColumns = []string{
	`tool_id TEXT NOT NULL DEFAULT ''`,
	`source_instance_id TEXT NOT NULL DEFAULT ''`,
}

// sourceStatusColumns 为 usage_sources 补齐的结构化运行状态列（方案 §4.4）。
var sourceStatusColumns = []string{
	`availability TEXT NOT NULL DEFAULT ''`, // ready / partial / unavailable / unsupported / error
	`last_attempt_at TEXT NOT NULL DEFAULT ''`,
	`last_error_code TEXT NOT NULL DEFAULT ''`,
	`records_imported INTEGER NOT NULL DEFAULT 0`,
	`bytes_read INTEGER NOT NULL DEFAULT 0`,
	`schema_version TEXT NOT NULL DEFAULT ''`,
	`source_version TEXT NOT NULL DEFAULT ''`,
}

// migrateV3 执行 v3 迁移。调用方必须已持有 s.mu；本方法不得再自行加锁。
func (s *Store) migrateV3() error {
	for _, col := range identityColumns {
		if _, err := s.db.Exec("ALTER TABLE usage_events ADD COLUMN " + col); err != nil {
			if !isDuplicateColumnErr(err) {
				return err
			}
		}
	}
	for _, col := range sourceStatusColumns {
		if _, err := s.db.Exec("ALTER TABLE usage_sources ADD COLUMN " + col); err != nil {
			if !isDuplicateColumnErr(err) {
				return err
			}
		}
	}
	if _, err := s.db.Exec(`
	CREATE INDEX IF NOT EXISTS idx_usage_events_tool_instance
		ON usage_events(tool_id, source_instance_id, session_id);

	-- 事件级归因关联（方案 §4.3）：同一 (event, subject, role) 唯一；
	-- direct_owner 全库唯一由 UpsertEventSubject 保证。evidence 词表：
	-- stable_id / trace_relation / provider_receipt / user_confirmed。
	CREATE TABLE IF NOT EXISTS usage_event_subjects (
		event_id TEXT NOT NULL,
		subject_id TEXT NOT NULL,
		role TEXT NOT NULL,                        -- direct_owner / association
		evidence TEXT NOT NULL DEFAULT 'stable_id',
		created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
		PRIMARY KEY (event_id, subject_id, role)
	);
	CREATE INDEX IF NOT EXISTS idx_event_subjects_subject ON usage_event_subjects(subject_id);
	`); err != nil {
		return err
	}
	return s.backfillEventIdentity()
}

// backfillEventIdentity 将旧记录迁移为可识别身份（方案 §4.1）：显示名
// source 映射到规范 tool_id；已识别工具但无实例 ID 的行标记 legacy-default，
// 不把旧记录伪装成已识别账号。幂等：只处理 tool_id=”/instance=” 的行。
func (s *Store) backfillEventIdentity() error {
	for display, toolID := range legacySourceDisplayToToolID {
		if _, err := s.db.Exec(
			"UPDATE usage_events SET tool_id = ? WHERE tool_id = '' AND source = ?",
			toolID, display); err != nil {
			return err
		}
	}
	// native_log 域来源（由 CollectSources 写入、带 session）标 legacy-default；
	// proxy 记录（collector_kind 为空且无 session_id）保持空身份，
	// 由 account_id 参与后续查询维度，不冒充工具实例。
	if _, err := s.db.Exec(`
		UPDATE usage_events SET source_instance_id = 'legacy-default'
		WHERE tool_id != '' AND source_instance_id = ''`); err != nil {
		return err
	}
	return nil
}

// legacySourceDisplayToToolID 为旧显示名 → 规范工具 ID 映射（采集器写入的
// 显示名集合；新代码直接写 tool_id，不经过该映射）。
var legacySourceDisplayToToolID = map[string]string{
	"Claude Code":  "claude-code",
	"Codex":        "codex",
	"Pi":           "pi",
	"Hermes Agent": "hermes",
	"OpenCode":     "opencode",
	"ZCode":        "zcode",
	"Kimi Code":    "kimi-code",
	"AtomCode":     "atomcode",
}
