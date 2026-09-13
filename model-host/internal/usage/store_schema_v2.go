package usage

// v2 schema（T3，ADR-0030 决策 2/9）：在既有 usage_events 上补语义字段，
// 新增三层账本所需表。全部归同一 Model Host 管理；不建第二张原始用量表。

// semanticColumns 为 usage_events 增量补齐的语义列（ALTER TABLE 幂等迁移）。
var semanticColumns = []string{
	// 来源追溯：与显示用 source 分开；source_record_id 为原始稳定记录标识。
	`source_id TEXT NOT NULL DEFAULT ''`,
	`collector_kind TEXT NOT NULL DEFAULT ''`, // proxy / native_log / sqlite_import / tool_event
	`source_record_id TEXT NOT NULL DEFAULT ''`,
	`parser_version TEXT NOT NULL DEFAULT ''`,
	// recordKind：只有 request 进入请求数；turn_delta/session_delta 汇总可见。
	`record_kind TEXT NOT NULL DEFAULT 'request'`,
	// 计费口径：api / subscription / unknown；证据等级四层见 ADR-0030。
	`billing_mode TEXT NOT NULL DEFAULT 'unknown'`,
	`cost_basis TEXT NOT NULL DEFAULT 'api_estimate'`,
	`evidence_level TEXT NOT NULL DEFAULT 'local_estimate'`,
	// 唯一计费原子：多来源只计一次（唯一索引去重）。
	`billing_atom TEXT NOT NULL DEFAULT ''`,
	// 活动归属：可空；只在来源提供可信元数据时写入。
	`session_id TEXT NOT NULL DEFAULT ''`,
	`project_id TEXT NOT NULL DEFAULT ''`,
}

// migrateV2 执行 v2 语义迁移。调用方必须已持有 s.mu（migrate 持锁调用）；
// 本方法不得再自行加锁——Mutex 非重入，重复 Lock 会死锁。
func (s *Store) migrateV2() error {
	for _, col := range semanticColumns {
		// 无 IF NOT EXISTS 语法，逐列探测；已存在则跳过。
		if _, err := s.db.Exec("ALTER TABLE usage_events ADD COLUMN " + col); err != nil {
			if !isDuplicateColumnErr(err) {
				return err
			}
		}
	}
	if _, err := s.db.Exec(`
	-- 同一 billing_atom 只允许一条记录：多来源（Proxy/原生日志/OTel/账单）只计一次。
	CREATE UNIQUE INDEX IF NOT EXISTS idx_usage_events_billing_atom
		ON usage_events(billing_atom) WHERE billing_atom != '';
	CREATE INDEX IF NOT EXISTS idx_usage_events_session ON usage_events(session_id);
	CREATE INDEX IF NOT EXISTS idx_usage_events_project ON usage_events(project_id);

	CREATE TABLE IF NOT EXISTS usage_sources (
		id TEXT PRIMARY KEY,                       -- sourceId（来源实例）
		tool_id TEXT NOT NULL,
		surface TEXT NOT NULL DEFAULT '',          -- cli / ide / desktop / web
		collector_kind TEXT NOT NULL,
		root_ref TEXT NOT NULL DEFAULT '',         -- 授权根引用（Host 私有，不传页面）
		capabilities_json TEXT NOT NULL DEFAULT '{}',
		enabled INTEGER NOT NULL DEFAULT 1,
		last_success_at TEXT NOT NULL DEFAULT '',
		last_error TEXT NOT NULL DEFAULT '',
		coverage_note TEXT NOT NULL DEFAULT '',
		created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
	);

	CREATE TABLE IF NOT EXISTS usage_import_cursors (
		source_id TEXT NOT NULL,
		file_fingerprint TEXT NOT NULL,
		committed_offset INTEGER NOT NULL DEFAULT 0,
		parser_version TEXT NOT NULL DEFAULT '',
		updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
		PRIMARY KEY (source_id, file_fingerprint)
	);
	`); err != nil {
		return err
	}

	// Codex 累计快照基线跨 checkpoint 持久化（方案 §3.2：分批读取时
	// delta 必须对上一批的累计值求差，不能每批从 0 重新累计）。
	for _, col := range []string{
		`baseline_in INTEGER NOT NULL DEFAULT 0`,
		`baseline_cache INTEGER NOT NULL DEFAULT 0`,
		`baseline_out INTEGER NOT NULL DEFAULT 0`,
		`baseline_epoch INTEGER NOT NULL DEFAULT 0`,
	} {
		if _, err := s.db.Exec("ALTER TABLE usage_import_cursors ADD COLUMN " + col); err != nil {
			if !isDuplicateColumnErr(err) {
				return err
			}
		}
	}

	if _, err := s.db.Exec(`
	CREATE TABLE IF NOT EXISTS usage_sessions (
		id TEXT PRIMARY KEY,                       -- 不透明 session 标识
		tool_id TEXT NOT NULL DEFAULT '',
		project_id TEXT NOT NULL DEFAULT '',
		last_state TEXT NOT NULL DEFAULT 'unknown', -- running / waiting_input / waiting_permission / ended / unknown
		last_observed_at TEXT NOT NULL,
		updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
	);

	CREATE TABLE IF NOT EXISTS usage_budgets (
		id TEXT PRIMARY KEY,
		scope TEXT NOT NULL,                       -- api_estimate / provider_credits / billing
		scope_key TEXT NOT NULL DEFAULT '',        -- provider 或账户约束
		currency TEXT NOT NULL DEFAULT 'USD',
		amount_micro INTEGER NOT NULL,
		period TEXT NOT NULL,                      -- daily / monthly
		timezone TEXT NOT NULL DEFAULT 'UTC',
		thresholds_json TEXT NOT NULL DEFAULT '[80,100]',
		enabled INTEGER NOT NULL DEFAULT 1,
		updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
	);

	CREATE TABLE IF NOT EXISTS usage_alerts (
		id TEXT PRIMARY KEY,
		budget_id TEXT NOT NULL DEFAULT '',
		kind TEXT NOT NULL,                        -- budget_threshold / attention / anomaly
		severity TEXT NOT NULL DEFAULT 'info',
		title TEXT NOT NULL,
		detail TEXT NOT NULL DEFAULT '',
		tool_id TEXT NOT NULL DEFAULT '',
		session_id TEXT NOT NULL DEFAULT '',
		period_key TEXT NOT NULL DEFAULT '',       -- 预算周期去重键
		threshold INTEGER NOT NULL DEFAULT 0,      -- 阈值去重（80/100 各一次）
		acknowledged INTEGER NOT NULL DEFAULT 0,
		delivery_state TEXT NOT NULL DEFAULT 'inbox_only', -- queued / submitted / failed / inbox_only
		created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
		UNIQUE(budget_id, period_key, threshold, kind)
	);

	CREATE TABLE IF NOT EXISTS usage_subjects (
		id TEXT PRIMARY KEY,
		kind TEXT NOT NULL,                        -- session / agent / subagent / skill / plugin / mcp_server / mcp_tool / tool_call / provider_request / project
		label TEXT NOT NULL DEFAULT ''
	);

	CREATE TABLE IF NOT EXISTS usage_subject_edges (
		parent_id TEXT NOT NULL,
		child_id TEXT NOT NULL,
		relation TEXT NOT NULL,                    -- parent_of / participates_in
		evidence TEXT NOT NULL DEFAULT 'stable_id',-- stable_id / trace_relation / none
		PRIMARY KEY (parent_id, child_id, relation)
	);

	CREATE TABLE IF NOT EXISTS usage_charge_components (
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		event_id TEXT NOT NULL,
		billing_atom TEXT NOT NULL,
		component TEXT NOT NULL,                   -- input_text_tokens / cache_read_tokens / image_units / web_search_requests / ...
		unit_type TEXT NOT NULL DEFAULT 'token',
		quantity INTEGER NOT NULL DEFAULT 0,
		rate_snapshot TEXT NOT NULL DEFAULT '',    -- 命中的价格规则快照（JSON）
		amount_micro INTEGER,                      -- NULL=无价（partial/unpriced），合法零价可为 0
		currency TEXT NOT NULL DEFAULT 'USD',
		status TEXT NOT NULL DEFAULT 'priced',     -- priced / partial / unpriced
		created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
	);
	CREATE INDEX IF NOT EXISTS idx_charge_components_atom ON usage_charge_components(billing_atom);

	CREATE TABLE IF NOT EXISTS usage_billing_entries (
		id TEXT PRIMARY KEY,
		billing_account TEXT NOT NULL,
		provider TEXT NOT NULL DEFAULT '',
		kind TEXT NOT NULL,                        -- service_usage / subscription / topup / credit_delta / refund / discount / tax / prepay_expiry / reported_unattributed
		period_start TEXT NOT NULL DEFAULT '',
		period_end TEXT NOT NULL DEFAULT '',
		currency TEXT NOT NULL DEFAULT 'USD',
		service_cost_impact INTEGER NOT NULL DEFAULT 0,
		cash_impact INTEGER NOT NULL DEFAULT 0,
		credit_balance_impact INTEGER NOT NULL DEFAULT 0,
		evidence_level TEXT NOT NULL DEFAULT 'actual_charge',
		note TEXT NOT NULL DEFAULT '',
		created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
	);
	CREATE INDEX IF NOT EXISTS idx_billing_entries_account ON usage_billing_entries(billing_account, period_start);

	CREATE TABLE IF NOT EXISTS usage_cost_attributions (
		billing_atom TEXT NOT NULL,
		subject_id TEXT NOT NULL,
		relation TEXT NOT NULL,                    -- direct_owner / association / allocation
		weight_ppm INTEGER NOT NULL DEFAULT 0,     -- allocation 时权重和必须为 1000000
		PRIMARY KEY (billing_atom, subject_id, relation)
	);

	-- 降本建议忽略记录（R7）：Insight ID 每次重新生成（含时间戳），
	-- 忽略按 ruleKey + 有效期持久化；到期后规则可再次出现。
	CREATE TABLE IF NOT EXISTS usage_dismissed_insights (
		rule_key TEXT PRIMARY KEY,
		dismissed_until TEXT NOT NULL,
		created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
	);
	`); err != nil {
		return err
	}
	return nil
}

func isDuplicateColumnErr(err error) bool {
	if err == nil {
		return false
	}
	// SQLite: "duplicate column name: xxx"
	const marker = "duplicate column name"
	return len(err.Error()) > len(marker) && contains(err.Error(), marker)
}

func contains(haystack, needle string) bool {
	for i := 0; i+len(needle) <= len(haystack); i++ {
		if haystack[i:i+len(needle)] == needle {
			return true
		}
	}
	return false
}
