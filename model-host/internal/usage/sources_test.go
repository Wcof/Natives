package usage

import "testing"

// TestUsageSourcesBaseline13 T2：矩阵必须覆盖 13 个基线工具
// （用户明确名单 ∪ agentclients 注册表并集），缺一即阻断“完整支持”声明。
func TestUsageSourcesBaseline13(t *testing.T) {
	want := map[string]bool{
		"claude-code": false, "codex": false, "opencode": false, "pi": false,
		"kimi-code": false, "zcode": false, "deepseek-harness": false,
		"claude-desktop": false, "grok-build": false, "openclaw": false,
		"hermes": false, "cursor": false, "atomcode": false,
	}
	sources := UsageSources()
	if len(sources) != len(want) {
		t.Fatalf("expected %d sources, got %d", len(want), len(sources))
	}
	for _, s := range sources {
		if _, ok := want[s.ID]; !ok {
			t.Errorf("unexpected source %q", s.ID)
			continue
		}
		want[s.ID] = true
	}
	for id, seen := range want {
		if !seen {
			t.Errorf("baseline tool %q missing from capability matrix", id)
		}
	}
}

// TestUsageSourcesHonestStatuses T2：状态必须来自词表；unsupported 的
// 工具必须带审计结论（诚实可见，不是从统计范围删除）。
func TestUsageSourcesHonestStatuses(t *testing.T) {
	valid := map[CapabilityStatus]bool{
		CapImplemented: true, CapPartial: true,
		CapUnsupported: true, CapUnavailable: true,
	}
	for _, s := range UsageSources() {
		caps := []CapabilityStatus{
			s.Caps.HistoricalUsage, s.Caps.LiveEvent, s.Caps.Billing,
			s.Caps.Quota, s.Caps.Attribution, s.Caps.Notification,
		}
		for _, c := range caps {
			if !valid[c] {
				t.Errorf("%s: invalid capability status %q", s.ID, c)
			}
		}
		if s.Caps.Privacy != "metadata_only" && s.Caps.Privacy != "needs_review" {
			t.Errorf("%s: privacy must be explicit, got %q", s.ID, s.Caps.Privacy)
		}
		hasImplemented := false
		for _, c := range caps {
			if c == CapImplemented || c == CapPartial {
				hasImplemented = true
			}
		}
		if !hasImplemented && s.Audit == "" {
			t.Errorf("%s: unsupported/unavailable source must carry an audit conclusion", s.ID)
		}
	}
	// 关键诚实性：未实现采集的工具不得标 implemented。
	for _, s := range UsageSources() {
		if s.ID == "zcode" && s.Caps.HistoricalUsage != CapImplemented {
			t.Errorf("zcode historicalUsage adapter exists (zcode-rollout/1) and must be implemented")
		}
		if s.ID == "claude-code" && s.Caps.HistoricalUsage != CapImplemented {
			t.Errorf("claude-code historicalUsage adapter exists (T4) and must be implemented")
		}
	}
}
