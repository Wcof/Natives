package host

// T7b/T8/T9：六固定视图的 Host 数据方法（方案 §7.4）。
// 命名沿用现有 model_* 协议；只返回状态与汇总，不返回原始日志正文和 Secret。

import (
	"encoding/json"
	"time"

	"github.com/ldh/natives/model-host/internal/usage"
)

// getUsageSources 返回静态能力矩阵 + 本机已授权来源状态。
// §7.5：纯查询，不隐式触发采集；采集由显式 model_usage_collect 执行。
func (e *Engine) getUsageSources() (*usage.SourcesResult, error) {
	sources := usage.UsageSources()
	// 本机实际授权/采集状态（usage_sources 表；无记录=未启用）。
	enabled := map[string]usage.SourceRuntimeState{}
	if e.usageStore != nil {
		states, err := e.usageStore.ListSourceRuntimeStates()
		if err == nil {
			for _, st := range states {
				enabled[st.SourceID] = st
			}
		}
	}
	out := &usage.SourcesResult{
		Status:      "ready",
		GeneratedAt: time.Now().UTC().Format(time.RFC3339),
		Sources:     make([]usage.SourceView, 0, len(sources)),
	}
	for _, s := range sources {
		view := usage.SourceView{
			ID: s.ID, Name: s.Name, InAgentClients: s.InAgentClients,
			Caps: s.Caps, Audit: s.Audit,
		}
		if st, ok := enabled[s.ID]; ok {
			view.Enabled = true
			view.LastSuccessAt = st.LastSuccessAt
			view.LastError = st.LastError
			view.Availability = st.Availability
			view.LastAttemptAt = st.LastAttemptAt
			view.LastErrorCode = st.LastErrorCode
			view.RecordsImported = st.RecordsImported
			view.BytesRead = st.BytesRead
			view.SchemaVersion = st.SchemaVersion
		}
		out.Sources = append(out.Sources, view)
	}
	return out, nil
}

// getUsageAttention 返回提醒收件箱（分页）。
func (e *Engine) getUsageAttention(raw json.RawMessage) (*usage.AttentionResult, error) {
	var params struct {
		Limit int `json:"limit,omitempty"`
	}
	if len(raw) > 0 {
		_ = json.Unmarshal(raw, &params)
	}
	items, err := e.usageStore.ListAttention(params.Limit)
	if err != nil {
		return nil, err
	}
	return &usage.AttentionResult{
		Status:      "ready",
		GeneratedAt: time.Now().UTC().Format(time.RFC3339),
		Items:       items,
	}, nil
}

// ackUsageAttention 标记已查看；不能修改原工具许可状态。
func (e *Engine) ackUsageAttention(raw json.RawMessage) (map[string]bool, error) {
	var params struct {
		ID string `json:"id"`
	}
	if err := json.Unmarshal(raw, &params); err != nil || params.ID == "" {
		return nil, &handlerError{code: "invalid_params", message: "id is required"}
	}
	if err := e.usageStore.AcknowledgeAttention(params.ID); err != nil {
		return nil, err
	}
	return map[string]bool{"ok": true}, nil
}

// getUsageSubjects 返回 subject 归因聚合（skill/MCP/插件/子代理；
// §8.2）。只统计有真实 relation 的事件，无 relation 不推断。
func (e *Engine) getUsageSubjects(raw json.RawMessage) (*usage.SubjectsResult, error) {
	if e.usageStore == nil {
		return nil, invalid("使用记录存储未就绪")
	}
	var filter usage.Filter
	if len(raw) > 0 && string(raw) != "null" {
		if err := json.Unmarshal(raw, &filter); err != nil {
			return nil, invalid("归因查询参数无效")
		}
	}
	if err := validateUsageFilter(filter); err != nil {
		return nil, err
	}
	return e.usageStore.GetSubjectBreakdown(filter)
}

// getUsageBilling 返回按账户/币种的对账汇总（三口径分离），可选返回
// 条目列表（数据与用量详情页展示/对账用）。
func (e *Engine) getUsageBilling(raw json.RawMessage) (*usage.BillingResult, error) {
	if e.usageStore == nil {
		return nil, invalid("使用记录存储未就绪")
	}
	// §7.5：纯查询，不隐式触发采集。
	var params struct {
		Account        string `json:"account,omitempty"`
		IncludeEntries bool   `json:"includeEntries,omitempty"`
		EntryLimit     int    `json:"entryLimit,omitempty"`
	}
	if len(raw) > 0 {
		_ = json.Unmarshal(raw, &params)
	}
	summaries, err := e.usageStore.Reconcile(params.Account, "", "")
	if err != nil {
		return nil, err
	}
	if summaries == nil {
		summaries = []usage.ReconciliationSummary{}
	}
	result := &usage.BillingResult{
		Status:      "ready",
		GeneratedAt: time.Now().UTC().Format(time.RFC3339),
		Summaries:   summaries,
	}
	if params.IncludeEntries {
		entries, err := e.usageStore.ListBillingEntries(params.Account, params.EntryLimit)
		if err != nil {
			return nil, err
		}
		if entries == nil {
			entries = []usage.BillingEntry{}
		}
		result.Entries = entries
	}
	return result, nil
}

// collectUsage 执行全量增量日志扫描采集。
func (e *Engine) collectUsage() (*usage.CollectSummary, error) {
	if e.usageStore == nil {
		return nil, invalid("使用记录存储未就绪")
	}
	return e.usageStore.CollectSources()
}

// dismissUsageInsight 忽略一条降本建议（按 ruleKey，7 天后到期自动恢复）。
func (e *Engine) dismissUsageInsight(raw json.RawMessage) (map[string]bool, error) {
	if e.usageStore == nil {
		return nil, invalid("使用记录存储未就绪")
	}
	var params struct {
		RuleKey string `json:"ruleKey"`
	}
	if err := json.Unmarshal(raw, &params); err != nil || params.RuleKey == "" {
		return nil, invalid("ruleKey is required")
	}
	if err := e.usageStore.DismissInsight(params.RuleKey); err != nil {
		return nil, invalid("unknown insight ruleKey")
	}
	return map[string]bool{"ok": true}, nil
}

// getUsageBudgets 返回全部已配置预算及当前周期的评估结果。
// 额度卡打开即评估续费窗口（§7.3 入口之一：用户打开额度卡）。
func (e *Engine) getUsageBudgets() (*usage.BudgetsResult, error) {
	if e.usageStore == nil {
		return nil, invalid("使用记录存储未就绪")
	}
	budgets, err := e.usageStore.ListBudgets()
	if err != nil {
		return nil, err
	}
	evals, err := e.usageStore.EvaluateBudgets(usage.EvaluateOpts{SuppressAlerts: false})
	if err != nil {
		return nil, err
	}
	renewals, err := e.usageStore.EvaluateRenewals(usage.RenewalOpts{SuppressAlerts: false})
	if err != nil {
		return nil, err
	}
	return &usage.BudgetsResult{
		Status:      "ready",
		GeneratedAt: time.Now().UTC().Format(time.RFC3339),
		Budgets:     budgets,
		Evaluations: evals,
		Renewals:    renewals,
	}, nil
}

// upsertUsageBudget 新增或修改一条预算规则。
func (e *Engine) upsertUsageBudget(raw json.RawMessage) (map[string]bool, error) {
	if e.usageStore == nil {
		return nil, invalid("使用记录存储未就绪")
	}
	var b usage.Budget
	if err := json.Unmarshal(raw, &b); err != nil {
		return nil, invalid("预算配置参数无效")
	}
	if err := e.usageStore.UpsertBudget(&b); err != nil {
		return nil, err
	}
	return map[string]bool{"ok": true}, nil
}

// getUsageInsights 返回确定性降本规则建议（最多 3 条，按证据和优先级排序）。
func (e *Engine) getUsageInsights() (*usage.InsightsResult, error) {
	if e.usageStore == nil {
		return nil, invalid("使用记录存储未就绪")
	}
	return e.usageStore.GetInsights()
}

// ingestToolEvent 接收受限一次性工具 hook/notify 事件。
func (e *Engine) ingestToolEvent(raw json.RawMessage) (map[string]any, error) {
	if e.usageStore == nil {
		return nil, invalid("使用记录存储未就绪")
	}
	var evt usage.ToolEvent
	if err := json.Unmarshal(raw, &evt); err != nil {
		return nil, invalid("工具事件参数无效")
	}
	admitted, err := e.usageStore.IngestToolEvent(&evt)
	if err != nil {
		return nil, err
	}
	return map[string]any{"ok": true, "admitted": admitted}, nil
}

// handlerError 为本文件最小错误类型（与 engine 现有错误路径一致时可直接复用）。
type handlerError struct {
	code    string
	message string
}

func (e *handlerError) Error() string { return e.message }
