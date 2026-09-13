package usage

// 静态 adapter registry（T2，ADR-0030 决策 3）。
//
// 能力矩阵覆盖“用户明确名单 ∪ agentclients 注册表”的 13 个基线工具。
// 状态是诚实的适配器结论，不是愿望：只有代码中存在已验证实现的能力才标
// implemented；审计后无结构化来源的标 unsupported（仍保留在矩阵中，
// 不从统计范围删除）；依赖外部凭据/权限当前无法核验的标 unavailable。
// “配置已支持”永远不等于“用量支持”。

// CapabilityStatus 为单能力状态。
type CapabilityStatus string

const (
	CapImplemented CapabilityStatus = "implemented"
	CapPartial     CapabilityStatus = "partial"
	CapUnsupported CapabilityStatus = "unsupported"
	CapUnavailable CapabilityStatus = "unavailable"
)

// SourceCapabilities 为一个工具的七项能力状态与审计依据。
type SourceCapabilities struct {
	HistoricalUsage CapabilityStatus `json:"historicalUsage"`
	LiveEvent       CapabilityStatus `json:"liveEvent"`
	Billing         CapabilityStatus `json:"billing"`
	Quota           CapabilityStatus `json:"quota"`
	Attribution     CapabilityStatus `json:"attribution"`
	Notification    CapabilityStatus `json:"notification"`
	Privacy         CapabilityStatus `json:"privacy"` // metadata_only / needs_review
}

// UsageSource 为静态注册的用量来源。
type UsageSource struct {
	ID             string             `json:"id"` // 等于 agentclients 工具 ID；AtomCode/Cursor 为用户指定补充
	Name           string             `json:"name"`
	InAgentClients bool               `json:"inAgentClients"` // 是否在配置注册表中
	Caps           SourceCapabilities `json:"caps"`
	Audit          string             `json:"audit"` // 审计结论/下一步动作；unsupported 必填
}

// usageSources 为冻结的静态矩阵；新增 agentclients 工具必须补录并阻断
// “完整支持”声明（测试强制 13 工具齐全）。
var usageSources = []UsageSource{
	{
		ID: "claude-code", Name: "Claude Code", InAgentClients: true,
		Caps: SourceCapabilities{
			HistoricalUsage: CapImplemented, // nativeLogParser（T4）：项目 JSONL 增量读取
			LiveEvent:       CapImplemented, // tool-event ingest（T6）：Notification/Stop hooks
			Billing:         CapUnavailable, // Provider 账单需 Console/Admin 导入，本机无接口
			Quota:           CapUnavailable, // OAuth 兼容端点未在本环境核验
			Attribution:     CapPartial,     // JSONL sessionId/stable ID；OTel skill/plugin 维度未启用
			Notification:    CapImplemented, // permission_prompt/idle_prompt/Stop（T6 白名单）
			Privacy:         "metadata_only",
		},
		Audit: "2026-09-12 真机联调（~/.claude projects + history.jsonl）：6359 条事件、52 个三元会话入库，二次采集 0 重复。OTel 归因需 ADR-0030 决策 5 的显式开启",
	},
	{
		ID: "codex", Name: "Codex", InAgentClients: true,
		Caps: SourceCapabilities{
			HistoricalUsage: CapImplemented, // sessions/archived_sessions（T4），累计快照求差
			LiveEvent:       CapPartial,     // notify 仅覆盖已验证事件；App Server 未核验
			Billing:         CapUnavailable, // OpenAI Admin usage 需组织权限
			Quota:           CapUnavailable, // rateLimits 端点未在本环境核验
			Attribution:     CapPartial,     // session/turn 稳定 ID；agent 关系无 trace 证据时不建树
			Notification:    CapPartial,     // notify 结束事件已接入；覆盖按版本验证
			Privacy:         "metadata_only",
		},
		Audit: "2026-09-12 真机联调（Codex CLI，sessions/2026/<date>/rollout-*.jsonl）：token_usage_record 行（payload.usage 五桶 + response_id/thread_id/turn_id）解析成功，2470 条事件、18 个三元会话入库；2026-07/08 旧版本 rollout 无 usage 记录（纯对话内容），如实跳过。累计快照 1000→1000→1600 求差已测试。大 rollout（>500 MiB）按 512 MiB/次预算分块续读，partial 为 checkpoint 语义",
	},
	{
		ID: "opencode", Name: "OpenCode", InAgentClients: true,
		Caps: SourceCapabilities{
			HistoricalUsage: CapImplemented, // opencode-store/1（R4）：本地 DB message 白名单只读投影，cache 子集口径已转换
			LiveEvent:       CapUnsupported, // server/SSE 为文档证据，版本化事件未实现
			Billing:         CapUnavailable,
			Quota:           CapUnavailable,
			Attribution:     CapPartial, // session_id 稳定归属；plugin/tool 关系未采集
			Notification:    CapUnsupported,
			Privacy:         "metadata_only", // 只投影 message 白名单元数据，不读 part 正文
		},
		Audit: "2026-09-12 真机联调（本机已安装 opencode；~/.local/share/opencode/opencode.db 只读投影）：360 条事件、15 个三元会话入库，二次采集 0 重复。SSE 实时事件未实现",
	},
	{
		ID: "pi", Name: "Pi", InAgentClients: true,
		Caps: SourceCapabilities{
			HistoricalUsage: CapImplemented, // pi-session/1 parser（R4）：官方 session JSONL 格式
			LiveEvent:       CapUnsupported,
			Billing:         CapUnavailable, // Provider 账单无本机接口；usage.cost 仅本地参考
			Quota:           CapUnavailable, // 无通用额度，显式 unknown
			Attribution:     CapPartial,     // session/entry/responseId 稳定 ID；分支不并树
			Notification:    CapUnsupported,
			Privacy:         "metadata_only",
		},
		Audit: "官方 session-format.md 核验（v3 头/assistant usage）；2026-09-12 真机联调：80 条事件、3 个三元会话入库，二次采集 0 重复",
	},
	{
		ID: "kimi-code", Name: "Kimi Code", InAgentClients: true,
		Caps: SourceCapabilities{
			HistoricalUsage: CapImplemented, LiveEvent: CapUnsupported,
			Billing: CapUnavailable, Quota: CapUnavailable,
			Attribution: CapUnsupported, Notification: CapUnsupported,
			Privacy: "metadata_only",
		},
		Audit: "官方 sessions.md wire schema 冻结（kimi-session/1）；assistant usage 四桶 parser 以脱敏 fixture 验证；本机未安装 Kimi Code（~/.kimi-code 缺失，2026-09-12 复核）——真实 collector 联调待安装版本，当前为交付阻断项",
	},
	{
		ID: "zcode", Name: "ZCode", InAgentClients: true,
		Caps: SourceCapabilities{
			HistoricalUsage: CapImplemented, LiveEvent: CapUnsupported,
			Billing: CapUnavailable, Quota: CapUnavailable,
			Attribution: CapUnsupported, Notification: CapUnsupported,
			Privacy: "metadata_only",
		},
		Audit: "2026-09-12 实测+真机联调 ~/.zcode/cli/rollout/model-io-*.jsonl：269 条事件、3 个三元会话入库（计量原子 zcode:<requestId>，不读 body/text）；二次采集 0 重复；rollout 为 model-io 事件流，无实时事件/账单/额度来源",
	},
	{
		ID: "deepseek-harness", Name: "DeepSeek Harness", InAgentClients: true,
		Caps: SourceCapabilities{
			HistoricalUsage: CapUnsupported, LiveEvent: CapUnsupported,
			Billing: CapUnavailable, Quota: CapUnavailable,
			Attribution: CapUnsupported, Notification: CapUnsupported,
			Privacy: "metadata_only",
		},
		Audit: "本机未安装 Harness（~/.deepseek、~/.dsh 均缺失，2026-09-12 复核）：无安装环境与 fixture，交付阻断；待安装版本 session/usage fixture 后实现 adapter",
	},
	{
		ID: "claude-desktop", Name: "Claude Desktop", InAgentClients: true,
		Caps: SourceCapabilities{
			HistoricalUsage: CapUnsupported, LiveEvent: CapUnsupported,
			Billing: CapUnavailable, Quota: CapUnavailable,
			Attribution: CapUnsupported, Notification: CapUnsupported,
			Privacy: "metadata_only",
		},
		Audit: "2026-09-12 独立 source 审计：local-agent-mode-sessions/<uuid>/<uuid>/ 仅含 remote_cowork_plugins/manifest.json，无结构化用量文件；不得套用 Claude Code 日志（§9.0）。显式 unsupported",
	},
	{
		ID: "grok-build", Name: "Grok Build", InAgentClients: true,
		Caps: SourceCapabilities{
			HistoricalUsage: CapUnsupported, LiveEvent: CapUnsupported,
			Billing: CapUnavailable, Quota: CapUnavailable,
			Attribution: CapUnsupported, Notification: CapUnsupported,
			Privacy: "metadata_only",
		},
		Audit: "2026-09-12 source 审计（~/.grok，CLI 0.2.118）：sessions/<proj>/<uuid>/{chat_history,events}.jsonl 与 summary.json 均无 usage/token/cost 字段（键名级核验）；memtrace 仅 start/sample 心跳。无本机计量来源，显式 unsupported",
	},
	{
		ID: "openclaw", Name: "OpenClaw", InAgentClients: true,
		Caps: SourceCapabilities{
			HistoricalUsage: CapUnsupported, LiveEvent: CapUnsupported,
			Billing: CapUnavailable, Quota: CapUnavailable,
			Attribution: CapUnsupported, Notification: CapUnsupported,
			Privacy: "metadata_only",
		},
		Audit: "配置注入已支持；source 待核验",
	},
	{
		ID: "hermes", Name: "Hermes Agent", InAgentClients: true,
		Caps: SourceCapabilities{
			HistoricalUsage: CapImplemented, // hermes-state/1（R4）：state.db sessions 白名单只读投影
			LiveEvent:       CapUnsupported, // 历史不冒充实时；事件来源独立核验
			Billing:         CapUnavailable, // 本地 cost 字段可核验；Provider 账单独立
			Quota:           CapUnavailable, // 显式 unknown
			Attribution:     CapPartial,     // session 级归属；parent_session_id（压缩续接）不当作子代理
			Notification:    CapUnsupported,
			Privacy:         "metadata_only",
		},
		Audit: "官方 session-storage 文档核验（schema_version 23）；只读白名单投影，不读正文/FTS；WAL 由只读连接处理。2026-09-12 本机状态：~/.hermes/ 仅有 config.yaml 与 skills/，无 state.db——adapter 已实现（fixture 验证），本机来源不可用（unavailable），待真实 state.db 联调",
	},
	{
		// 用户明确指定；不在 agentclients 注册表——账单/脱敏导入路径。
		ID: "cursor", Name: "Cursor", InAgentClients: false,
		Caps: SourceCapabilities{
			HistoricalUsage: CapUnavailable, // 个人端 dashboard/账单导入，无实时逐请求承诺
			LiveEvent:       CapUnavailable,
			Billing:         CapUnavailable, // 账单导入回退待 T5 手动入口
			Quota:           CapUnavailable,
			Attribution:     CapUnavailable,
			Notification:    CapUnavailable,
			Privacy:         "metadata_only",
		},
		Audit: "个人端不承诺实时计量；账单导入回退待手动入口实现；组织 API 仅在权限具备时接入并核验",
	},
	{
		// 用户明确指定；不在 agentclients 注册表。
		ID: "atomcode", Name: "AtomCode", InAgentClients: false,
		Caps: SourceCapabilities{
			HistoricalUsage: CapImplemented, LiveEvent: CapUnavailable,
			Billing: CapUnavailable, Quota: CapUnavailable,
			Attribution: CapUnavailable, Notification: CapUnavailable,
			Privacy: "metadata_only",
		},
		Audit: "2026-09-12 真机联调：~/.atomcode/sessions/<projectHash>/<uuid>.jsonl 实测修正 wire schema（v/turn_id 为数字，兼容字符串；usage={prompt,completion,cached}、iso/ts、undone、session_id）后 946 条事件、166 个三元会话入库，二次采集 0 重复。撤销行跳过、同 turn 流式多行取终态不相加、计费原子 atomcode:<session>:<turn>。model 字段不存在：如实 unknown 不猜；Billing/Quota/LiveEvent 无来源仍为产品限制",
	},
}

// SourceRuntimeState 为本机已启用来源的运行时状态（usage_sources 表）。
// availability/records/bytes 为整改 E1 的结构化状态（§4.4）；旧 lastError
// 文本保留用于兼容展示。
type SourceRuntimeState struct {
	SourceID        string `json:"sourceId"`
	LastSuccessAt   string `json:"lastSuccessAt"`
	LastError       string `json:"lastError,omitempty"`
	Availability    string `json:"availability,omitempty"`
	LastAttemptAt   string `json:"lastAttemptAt,omitempty"`
	LastErrorCode   string `json:"lastErrorCode,omitempty"`
	RecordsImported int64  `json:"recordsImported"`
	BytesRead       int64  `json:"bytesRead"`
	SchemaVersion   string `json:"schemaVersion,omitempty"`
}

// SourceView 为来源目录视图条目：静态能力 + 本机运行时状态（平铺输出）。
type SourceView struct {
	ID              string             `json:"id"`
	Name            string             `json:"name"`
	InAgentClients  bool               `json:"inAgentClients"`
	Caps            SourceCapabilities `json:"caps"`
	Audit           string             `json:"audit"`
	Enabled         bool               `json:"enabled"`
	LastSuccessAt   string             `json:"lastSuccessAt,omitempty"`
	LastError       string             `json:"lastError,omitempty"`
	Availability    string             `json:"availability,omitempty"`
	LastAttemptAt   string             `json:"lastAttemptAt,omitempty"`
	LastErrorCode   string             `json:"lastErrorCode,omitempty"`
	RecordsImported int64              `json:"recordsImported"`
	BytesRead       int64              `json:"bytesRead"`
	SchemaVersion   string             `json:"schemaVersion,omitempty"`
}

// AttentionResult 为 model_usage_attention 响应。
type AttentionResult struct {
	Status      string          `json:"status"`
	GeneratedAt string          `json:"generatedAt"`
	Items       []AttentionItem `json:"items"`
}

// BillingResult 为 model_usage_billing 响应（三口径按币种分列）。
// includeEntries 时附带条目列表（§7.2 手动确认账务的对账入口）。
type BillingResult struct {
	Status      string                  `json:"status"`
	GeneratedAt string                  `json:"generatedAt"`
	Summaries   []ReconciliationSummary `json:"summaries"`
	Entries     []BillingEntry          `json:"entries,omitempty"`
}

// SourcesResult 为 model_usage_sources 响应。
type SourcesResult struct {
	Status      string       `json:"status"`
	GeneratedAt string       `json:"generatedAt"`
	Sources     []SourceView `json:"sources"`
}

// ListSourceRuntimeStates 读取本机来源运行时状态（无记录=未启用）。
func (s *Store) ListSourceRuntimeStates() ([]SourceRuntimeState, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	rows, err := s.db.Query(`
		SELECT id, COALESCE(last_success_at,''), COALESCE(last_error,''),
			COALESCE(availability,''), COALESCE(last_attempt_at,''), COALESCE(last_error_code,''),
			COALESCE(records_imported,0), COALESCE(bytes_read,0), COALESCE(schema_version,'')
		FROM usage_sources WHERE enabled = 1`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []SourceRuntimeState{}
	for rows.Next() {
		var st SourceRuntimeState
		if err := rows.Scan(&st.SourceID, &st.LastSuccessAt, &st.LastError,
			&st.Availability, &st.LastAttemptAt, &st.LastErrorCode,
			&st.RecordsImported, &st.BytesRead, &st.SchemaVersion); err != nil {
			return nil, err
		}
		out = append(out, st)
	}
	return out, rows.Err()
}

// UsageSources 返回静态矩阵快照（按 ID 排序保证输出稳定）。
func UsageSources() []UsageSource {
	out := make([]UsageSource, len(usageSources))
	copy(out, usageSources)
	for i := 1; i < len(out); i++ {
		for j := i; j > 0 && out[j].ID < out[j-1].ID; j-- {
			out[j], out[j-1] = out[j-1], out[j]
		}
	}
	return out
}
