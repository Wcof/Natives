package usage

// 13 工具逐能力矩阵证据测试（方案 §9.0 完成门槛；整改 E0）。
//
// 强制不变量：
//   1. 矩阵 = agentclients 生产注册表 ∪ 用户明确名单，缺一即失败（直接读
//      agentclients.Definitions()，注册表新增工具后本测试立即暴露缺口）；
//   2. 每个工具的七项能力字段全部显式声明，取值必须在已知状态词表内；
//   3. unsupported/unavailable 必须填写审计结论（"还没来得及做"不许冒充
//      产品限制，§9.0：unsupported 不能表示没来得及做）；
//   4. HistoricalUsage 标 implemented 的工具必须有真实采集器路径——
//      矩阵声明与 CollectSources 实际遍历（nativeSources 注册表）一致，
//      "列了工具"不算统计。

import (
	"strings"
	"testing"

	"github.com/ldh/natives/model-host/internal/agentclients"
)

// userNamedToolIDs 为用户明确名单（产品冻结输入；不在 agentclients 注册表）。
var userNamedToolIDs = []string{"cursor", "atomcode"}

func TestCapabilityMatrixCoversRegistryAndUserList(t *testing.T) {
	have := map[string]bool{}
	for _, s := range usageSources {
		if have[s.ID] {
			t.Errorf("duplicate source id %q", s.ID)
		}
		have[s.ID] = true
	}
	// E0：注册表 ID 直接来自生产 agentclients.Definitions()，禁止手写副本。
	for _, def := range agentclients.Definitions() {
		if !have[def.ID] {
			t.Errorf("registry tool %q missing from capability matrix (add it to usageSources with an audit conclusion)", def.ID)
		}
	}
	for _, id := range userNamedToolIDs {
		if !have[id] {
			t.Errorf("user-specified tool %q missing from capability matrix", id)
		}
	}
}

func TestCapabilityMatrixFieldsCompleteAndValid(t *testing.T) {
	valid := map[CapabilityStatus]bool{
		CapImplemented: true, CapPartial: true,
		CapUnsupported: true, CapUnavailable: true,
	}
	for _, s := range usageSources {
		caps := s.Caps
		for name, status := range map[string]CapabilityStatus{
			"historicalUsage": caps.HistoricalUsage,
			"liveEvent":       caps.LiveEvent,
			"billing":         caps.Billing,
			"quota":           caps.Quota,
			"attribution":     caps.Attribution,
			"notification":    caps.Notification,
		} {
			if !valid[status] {
				t.Errorf("%s: %s status %q not in implemented|partial|unsupported|unavailable", s.ID, name, status)
			}
		}
		// unsupported/unavailable 必须有审计结论，否则就是"没来得及做"。
		if (caps.HistoricalUsage == CapUnsupported || caps.HistoricalUsage == CapUnavailable) &&
			!strings.Contains(s.Audit, "待") && !strings.Contains(s.Audit, "核验") &&
			!strings.Contains(s.Audit, "审计") && !strings.Contains(s.Audit, "导入") &&
			!strings.Contains(s.Audit, "安装") {
			t.Errorf("%s: unsupported/unavailable historicalUsage lacks actionable audit: %q", s.ID, s.Audit)
		}
		switch caps.Privacy {
		case "metadata_only", "needs_review":
		default:
			t.Errorf("%s: privacy %q must be metadata_only|needs_review", s.ID, caps.Privacy)
		}
	}
}

// TestImplementedSourcesHaveCollectorPath 矩阵声明与采集器实际覆盖一致：
// HistoricalUsage=implemented 的集合必须与 nativeSources 采集注册表完全相等
// （"目录列出工具"不算完成统计，§9.0；nativeSources 是唯一真源）。
func TestImplementedSourcesHaveCollectorPath(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	implemented := map[string]bool{}
	for _, src := range usageSources {
		if src.Caps.HistoricalUsage == CapImplemented {
			implemented[src.ID] = true
		}
	}
	collectorCovered := map[string]bool{}
	for _, id := range NativeCollectorToolIDs() {
		collectorCovered[id] = true
	}
	// 用一次空 home 的 Collect 调用验证不 panic 且返回结构化状态。
	if _, err := s.CollectSources(); err != nil {
		t.Fatalf("CollectSources: %v", err)
	}
	for id := range implemented {
		if !collectorCovered[id] {
			t.Errorf("source %q claims historicalUsage=implemented but has no collector path", id)
		}
	}
	for id := range collectorCovered {
		if !implemented[id] {
			t.Errorf("collector covers %q but matrix does not claim implemented (understated)", id)
		}
	}
}
