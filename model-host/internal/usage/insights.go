package usage

// T9：确定性降本建议引擎（方案 §4.5，ADR-0030 §7.4）。
//
// 原则：
//   - 默认使用确定性规则生成，不额外调用模型分析用户对话；
//   - 一次最多三条，按可验证的影响与严重度排序；
//   - 每条建议都有具体证据、建议动作和不足/退出条件；
//   - 仅统计客观用量事实，不妄断“死循环”或承诺虚假省钱额。

import (
	"fmt"
	"sort"
	"time"
)

// Insight 为单条确定性降本建议。
type Insight struct {
	ID          string `json:"id"`
	RuleKey     string `json:"ruleKey"`  // long_context_cost_spike / high_cost_session_concentration / cache_efficiency_drop / failure_rate_spike
	Severity    string `json:"severity"` // warn / info
	Title       string `json:"title"`
	Evidence    string `json:"evidence"`
	Action      string `json:"action"`
	Caveat      string `json:"caveat"`
	GeneratedAt string `json:"generatedAt"`
}

// InsightsResult 为 model_usage_insights 响应。
type InsightsResult struct {
	Status      string    `json:"status"`
	GeneratedAt string    `json:"generatedAt"`
	Insights    []Insight `json:"insights"`
}

// eventSample 用于规则推断的最小事件子集。
type eventSample struct {
	SessionID        string
	Provider         string
	Model            string
	InputTokens      int64
	OutputTokens     int64
	CacheReadTokens  int64
	CacheWriteTokens int64
	CostMicro        int64
	Result           EventResult
	RequestedAt      time.Time
}

// GetInsights 执行确定性规则分析并返回最多 3 条建议。
// 已被用户忽略且未过期的规则不再出现（忽略按 ruleKey 持久化，
// 默认 7 天后到期，规则可再次出现）。
func (s *Store) GetInsights() (*InsightsResult, error) {
	now := time.Now().UTC()
	nowStr := now.Format(time.RFC3339)

	s.mu.Lock()
	defer s.mu.Unlock()

	// 读取忽略记录：未过期的 ruleKey 不再生成。
	dismissed := map[string]bool{}
	disRows, err := s.db.Query(`
		SELECT rule_key FROM usage_dismissed_insights
		WHERE dismissed_until > ?`, nowStr)
	if err == nil {
		for disRows.Next() {
			var rk string
			if disRows.Scan(&rk) == nil {
				dismissed[rk] = true
			}
		}
		disRows.Close()
	}

	// 读取最近样本（最多 500 条）。
	rows, err := s.db.Query(`
		SELECT session_id, provider, model, input_tokens, output_tokens,
		       cache_read_tokens, cache_write_tokens, cost_micro, result, requested_at
		FROM usage_events
		ORDER BY requested_at DESC
		LIMIT 500`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var events []eventSample
	for rows.Next() {
		var e eventSample
		var resStr, reqAtStr string
		if err := rows.Scan(&e.SessionID, &e.Provider, &e.Model, &e.InputTokens, &e.OutputTokens,
			&e.CacheReadTokens, &e.CacheWriteTokens, &e.CostMicro, &resStr, &reqAtStr); err != nil {
			return nil, err
		}
		e.Result = EventResult(resStr)
		if ts, err := time.Parse(time.RFC3339Nano, reqAtStr); err == nil {
			e.RequestedAt = ts
		} else if ts, err := time.Parse(time.RFC3339, reqAtStr); err == nil {
			e.RequestedAt = ts
		}
		events = append(events, e)
	}

	var candidates []Insight

	// 忽略中的规则不再生成（保留证据能力，到期自动恢复）。
	if !dismissed["failure_rate_spike"] {
		if ins := checkFailureRateSpike(events, now); ins != nil {
			candidates = append(candidates, *ins)
		}
	}
	if !dismissed["high_cost_session_concentration"] {
		if ins := checkHighCostSession(events, now); ins != nil {
			candidates = append(candidates, *ins)
		}
	}
	if !dismissed["long_context_cost_spike"] {
		if ins := checkLongContextSpike(events, now); ins != nil {
			candidates = append(candidates, *ins)
		}
	}
	if !dismissed["cache_efficiency_drop"] {
		if ins := checkCacheDrop(events, now); ins != nil {
			candidates = append(candidates, *ins)
		}
	}

	// 排序：warn 优先，最多截取 3 条。
	sort.SliceStable(candidates, func(i, j int) bool {
		if candidates[i].Severity != candidates[j].Severity {
			return candidates[i].Severity == "warn"
		}
		return candidates[i].RuleKey < candidates[j].RuleKey
	})
	if len(candidates) > 3 {
		candidates = candidates[:3]
	}
	if candidates == nil {
		candidates = []Insight{}
	}

	return &InsightsResult{
		Status:      "ready",
		GeneratedAt: nowStr,
		Insights:    candidates,
	}, nil
}

// checkFailureRateSpike 检查失败率激增。
func checkFailureRateSpike(events []eventSample, now time.Time) *Insight {
	windowStart := now.Add(-15 * time.Minute)
	total, failed := 0, 0
	for _, e := range events {
		if e.RequestedAt.Before(windowStart) {
			continue
		}
		total++
		if e.Result == ResultFailed {
			failed++
		}
	}
	if total >= 10 && float64(failed)/float64(total) >= 0.20 {
		pct := float64(failed) / float64(total) * 100
		return &Insight{
			ID:          fmt.Sprintf("insight-failure-%d", now.Unix()),
			RuleKey:     "failure_rate_spike",
			Severity:    "warn",
			Title:       "近期调用失败率偏高",
			Evidence:    fmt.Sprintf("15 分钟窗口内共 %d 次请求，其中失败 %d 次 (%.1f%%)", total, failed, pct),
			Action:      "检查网络连接、模型配额或 API Key 凭据有效性，避免无意义的无效重试",
			Caveat:      "仅统计已记录的错误请求；已重试成功的请求不额外扣费",
			GeneratedAt: now.Format(time.RFC3339),
		}
	}
	return nil
}

// checkHighCostSession 检查高费用会话集中。
func checkHighCostSession(events []eventSample, now time.Time) *Insight {
	sessionCost := map[string]int64{}
	var totalCost int64
	for _, e := range events {
		if e.SessionID == "" || e.CostMicro <= 0 {
			continue
		}
		sessionCost[e.SessionID] += e.CostMicro
		totalCost += e.CostMicro
	}
	if len(sessionCost) < 5 || totalCost <= 0 {
		return nil
	}
	var maxSession string
	var maxCost int64
	for sID, c := range sessionCost {
		if c > maxCost {
			maxCost = c
			maxSession = sID
		}
	}
	pct := float64(maxCost) / float64(totalCost)
	if pct > 0.30 {
		return &Insight{
			ID:          fmt.Sprintf("insight-session-conc-%s", maxSession),
			RuleKey:     "high_cost_session_concentration",
			Severity:    "warn",
			Title:       "单会话费用集中度过高",
			Evidence:    fmt.Sprintf("会话 %s 消耗已知费用的 %.1f%% (约 $%.2f / 总额 $%.2f)", maxSession, pct*100, float64(maxCost)/1e6, float64(totalCost)/1e6),
			Action:      "检查该会话是否存在反复返工或试错；建议将复杂目标拆分为独立子任务新起会话",
			Caveat:      "仅基于费用占比统计，不代表会话逻辑存在死循环或无产出",
			GeneratedAt: now.Format(time.RFC3339),
		}
	}
	return nil
}

// checkLongContextSpike 检查长上下文成本膨胀。
func checkLongContextSpike(events []eventSample, now time.Time) *Insight {
	// 按 (session_id, model) 分组
	type key struct{ s, m string }
	groups := map[key][]int64{}

	// events 是 DESC 排序的，先倒序成正序（时间先后）
	for i := len(events) - 1; i >= 0; i-- {
		e := events[i]
		if e.SessionID == "" || e.InputTokens <= 0 {
			continue
		}
		k := key{s: e.SessionID, m: e.Model}
		groups[k] = append(groups[k], e.InputTokens)
	}

	for k, tokens := range groups {
		if len(tokens) < 10 {
			continue
		}
		first5 := tokens[:5]
		last5 := tokens[len(tokens)-5:]
		m1 := medianTokens(first5)
		m2 := medianTokens(last5)
		if m1 > 0 && m2 > 2.0*m1 {
			return &Insight{
				ID:          fmt.Sprintf("insight-ctx-spike-%s", k.s),
				RuleKey:     "long_context_cost_spike",
				Severity:    "info",
				Title:       "长会话输入 Context 显著膨胀",
				Evidence:    fmt.Sprintf("会话 %s (%s) 前 5 次输入中位数 %.0f tokens，后 5 次升至 %.0f tokens (> 2 倍)", k.s, k.m, m1, m2),
				Action:      "检查是否跨任务复用对话历史；在新任务时开启新会话或适时精简上下文",
				Caveat:      "由用户自主决定是否另起会话，系统不主动清空会话上下文",
				GeneratedAt: now.Format(time.RFC3339),
			}
		}
	}
	return nil
}

// checkCacheDrop 检查缓存读取比例下跌。
func checkCacheDrop(events []eventSample, now time.Time) *Insight {
	groups := map[string][]eventSample{}
	for i := len(events) - 1; i >= 0; i-- {
		e := events[i]
		if e.Model == "" {
			continue
		}
		groups[e.Model] = append(groups[e.Model], e)
	}

	for model, evs := range groups {
		if len(evs) < 20 {
			continue
		}
		half := len(evs) / 2
		firstHalf := evs[:half]
		secondHalf := evs[half:]

		ratio1 := cacheReadRatio(firstHalf)
		ratio2 := cacheReadRatio(secondHalf)

		if ratio1 > 0 && (ratio1-ratio2) >= 0.20 {
			return &Insight{
				ID:          fmt.Sprintf("insight-cache-drop-%s", model),
				RuleKey:     "cache_efficiency_drop",
				Severity:    "info",
				Title:       "模型缓存读取占比下降",
				Evidence:    fmt.Sprintf("模型 %s 前期缓存读取占比 %.1f%%，近期降至 %.1f%% (下降 ≥ 20 个百分点)", model, ratio1*100, ratio2*100),
				Action:      "检查 Prompt 前缀或系统指令是否有频繁微小变动导致破坏缓存命中",
				Caveat:      "缓存匹配取决于服务商策略，低缓存率本身并非系统故障",
				GeneratedAt: now.Format(time.RFC3339),
			}
		}
	}
	return nil
}

// DismissInsight 将一条降本建议按规则键忽略。有效期 7 天，到期后规则
// 可再次出现（Insight ID 含时间戳、每次重生成，不能按 ID 忽略）。
// 返回错误当 ruleKey 不在已知规则集合内（防拼写错误静默生效）。
func (s *Store) DismissInsight(ruleKey string) error {
	valid := map[string]bool{
		"failure_rate_spike":              true,
		"high_cost_session_concentration": true,
		"long_context_cost_spike":         true,
		"cache_efficiency_drop":           true,
	}
	if !valid[ruleKey] {
		return fmt.Errorf("unknown insight ruleKey %q", ruleKey)
	}
	until := time.Now().UTC().Add(7 * 24 * time.Hour).Format(time.RFC3339)

	s.mu.Lock()
	defer s.mu.Unlock()
	_, err := s.db.Exec(`
		INSERT INTO usage_dismissed_insights (rule_key, dismissed_until)
		VALUES (?, ?)
		ON CONFLICT(rule_key) DO UPDATE SET dismissed_until = excluded.dismissed_until`,
		ruleKey, until)
	return err
}

func medianTokens(nums []int64) float64 {
	if len(nums) == 0 {
		return 0
	}
	sorted := make([]int64, len(nums))
	copy(sorted, nums)
	sort.Slice(sorted, func(i, j int) bool { return sorted[i] < sorted[j] })
	mid := len(sorted) / 2
	if len(sorted)%2 == 0 {
		return float64(sorted[mid-1]+sorted[mid]) / 2.0
	}
	return float64(sorted[mid])
}

func cacheReadRatio(evs []eventSample) float64 {
	var cacheRead, totalInput int64
	for _, e := range evs {
		inp := e.InputTokens + e.CacheReadTokens + e.CacheWriteTokens
		if inp <= 0 {
			inp = e.InputTokens
		}
		totalInput += inp
		cacheRead += e.CacheReadTokens
	}
	if totalInput <= 0 {
		return 0
	}
	return float64(cacheRead) / float64(totalInput)
}
