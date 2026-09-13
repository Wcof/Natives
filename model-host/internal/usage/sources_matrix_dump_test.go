package usage

// 矩阵 dump 测试：把 13 工具逐能力全表以可读文本输出到 t.Log，
// `go test -v` 运行时完整打印，供交付证据落盘（方案 §9.0 逐工具、逐能力、逐证据）。
// 本测试同时断言表完整性：恰好 13 工具、每工具 6 能力 + privacy 齐全，
// 且逐格状态与逐格原因登记表一致（§9.0：unsupported 不能表示"还没来得及做"）。

import (
	"fmt"
	"strings"
	"testing"
)

// cellReasons 为逐格原因冻结登记表（2026-09-12 审计）。
// 一致性由本测试强制：非 IMPL 格必须登记原因；IMPL 格不得遗留原因（状态升格时同步清理）。
// 状态本身仍以 usageSources 为唯一权威，这里只承载"为什么是这个状态"。
var cellReasons = map[string][6]string{ // HIST, LIVE, BILL, QUOTA, ATTR, NOTIF
	"claude-code": {
		"", // IMPL：nativeLogParser 项目 JSONL 增量读取
		"", // IMPL：tool-event ingest（Notification/Stop hooks）
		"Provider 账单需 Console/Admin 导入，本机无接口",
		"OAuth 兼容端点未在本环境核验",
		"JSONL sessionId/stable ID 可用；OTel skill/plugin 维度按 ADR-0030 需显式开启，未启用",
		"", // IMPL：permission_prompt/idle_prompt/Stop 白名单
	},
	"codex": {
		"", // IMPL：sessions/archived_sessions 累计快照求差
		"notify 仅覆盖已验证事件；App Server account/usage 未核验",
		"OpenAI Admin usage 需组织权限",
		"rateLimits 端点未在本环境核验",
		"session/turn 稳定 ID 可用；agent 关系无 trace 证据时不建树",
		"notify 结束事件已接入；事件覆盖按安装版本验证",
	},
	"opencode": {
		"", // IMPL：opencode-store/1 本地 DB 只读投影
		"server/SSE 为文档证据，版本化事件未实现（交付缺口，非产品限制）",
		"Provider 账单无本机接口；本地价格为估算口径",
		"多数账户额度无本地来源，显式 unknown",
		"session_id 稳定归属可用；plugin/tool 关系未采集",
		"plugin permission/session 事件未接线（交付缺口）",
	},
	"pi": {
		"", // IMPL：pi-session/1 官方 session JSONL
		"extension agent_settled 事件未实现（交付缺口）",
		"Provider 账单无本机接口；usage.cost 仅本地参考",
		"无通用额度来源，显式 unknown",
		"session/entry/responseId 稳定 ID 可用；分支不并树",
		"无可靠结束/等待事件来源（按 §6.3 不制造提醒）",
	},
	"kimi-code": {
		"", // IMPL：kimi-session/1 官方 sessions.md wire schema，脱敏 fixture 验证
		"本机未安装；版本化 server/event 未核验（环境缺口）",
		"OAuth usage/wallet 未核验",
		"未核验",
		"wire relation 未采集",
		"schema 变更告警机制未实现（环境缺口）",
	},
	"zcode": {
		"", // IMPL：zcode-rollout/1 model_io JSONL
		"rollout 为 model-io 事件流，无实时事件来源",
		"Provider/账单回退无本机接口",
		"无额度来源，显式 unknown",
		"model_io 行含 requestId/sessionId，归因投影未接线（交付缺口）",
		"无结束/等待事件来源",
	},
	"deepseek-harness": {
		"本机未安装（~/.deepseek 缺失）；待安装版本 session/usage fixture（环境缺口）",
		"同上，事件来源须核验",
		"Provider/账单回退无本机数据",
		"未核验",
		"未采集",
		"无事件来源",
	},
	"claude-desktop": {
		"2026-09-12 独立审计：local-agent-mode-sessions 仅含 remote_cowork_plugins/manifest.json，无结构化用量文件；不得套用 Claude Code 日志（产品限制，有审计证据）",
		"同左，独立审计无实时来源（产品限制）",
		"账户账单回退待手动导入入口",
		"未核验",
		"无来源，不套用 Claude Code 归因（产品限制）",
		"无事件来源（产品限制）",
	},
	"grok-build": {
		"2026-09-12 键名级审计（CLI 0.2.118）：chat_history/events.jsonl/summary.json 均无 usage/token/cost 字段；memtrace 仅心跳。无计量来源（产品限制，有审计证据）",
		"events.jsonl 无结构化用量事件（产品限制）",
		"Provider/账单回退无本机数据",
		"无额度来源",
		"无结构化 session 归因字段（产品限制）",
		"无结束/等待事件（产品限制）",
	},
	"openclaw": {
		"本机未安装；source 待安装版本核验（环境缺口）",
		"同上，事件来源须核验",
		"Provider/账单回退无本机数据",
		"未核验",
		"未采集",
		"无事件来源",
	},
	"hermes": {
		"", // IMPL：hermes-state/1 state.db 白名单投影
		"历史不冒充实时；独立事件来源未核验",
		"本地 cost 字段可核验；Provider 账单独立无本机接口",
		"显式 unknown",
		"session 级归属可用；parent_session_id（压缩续接）不当作子代理",
		"无实时事件来源，不声称即时提醒",
	},
	"cursor": {
		"个人端 dashboard/账单导入，无实时逐请求来源；手动导入入口待实现（环境+产品边界）",
		"个人端不承诺实时逐请求",
		"账单导入回退待 T5 手动入口实现",
		"订阅/credits source 未核验",
		"无逐请求归属数据",
		"不把 IDE 活跃当完成事件",
	},
	"atomcode": {
		"", // IMPL：atomcode-session/1 sessions/<hash>/<uuid>.jsonl 白名单投影（2026-09-12 本机探测）
		"无实时事件来源；CLI 会话为离线 JSONL（产品限制）",
		"账单/导入回退无本机数据",
		"未核验",
		"无结构化归因字段（session/turn 可归属；agent/skill 无来源）",
		"无结束/等待事件（产品限制）",
	},
}

func TestDumpCapabilityMatrixForEvidence(t *testing.T) {
	if len(usageSources) != 13 {
		t.Fatalf("usageSources = %d tools, want 13", len(usageSources))
	}
	capsOf := func(src UsageSource) [6]CapabilityStatus {
		return [6]CapabilityStatus{
			src.Caps.HistoricalUsage, src.Caps.LiveEvent, src.Caps.Billing,
			src.Caps.Quota, src.Caps.Attribution, src.Caps.Notification,
		}
	}
	names := [6]string{"HIST", "LIVE", "BILL", "QUOTA", "ATTR", "NOTIF"}

	// 一致性断言：每个工具的逐格原因登记必须存在且与状态同步。
	for _, src := range usageSources {
		reasons, ok := cellReasons[src.ID]
		if !ok {
			t.Fatalf("cellReasons missing tool %q — every tool must register per-cell reasons", src.ID)
		}
		caps := capsOf(src)
		for i, c := range caps {
			hasReason := reasons[i] != ""
			isImpl := c == CapImplemented
			if !isImpl && !hasReason {
				t.Errorf("%s.%s = %s but no per-cell reason registered (§9.0: 状态必须有原因)", src.ID, names[i], short(c))
			}
			if isImpl && hasReason {
				t.Errorf("%s.%s = IMPL but stale reason registered: %q (状态升格时同步清理)", src.ID, names[i], reasons[i])
			}
		}
	}

	var b strings.Builder
	b.WriteString("TOOL             | HIST | LIVE | BILL | QUOTA | ATTR | NOTIF | PRIVACY\n")
	b.WriteString("-----------------+------+------+------+-------+------+------+-----------\n")
	for _, src := range usageSources {
		b.WriteString(fmt.Sprintf("%-16s | %-4s | %-4s | %-4s | %-5s | %-4s | %-5s | %s\n",
			src.ID,
			short(src.Caps.HistoricalUsage),
			short(src.Caps.LiveEvent),
			short(src.Caps.Billing),
			short(src.Caps.Quota),
			short(src.Caps.Attribution),
			short(src.Caps.Notification),
			src.Caps.Privacy,
		))
	}
	b.WriteString("\n-- 逐格原因（非 IMPL 格）--\n")
	for _, src := range usageSources {
		reasons := cellReasons[src.ID]
		caps := capsOf(src)
		for i, c := range caps {
			if c != CapImplemented && reasons[i] != "" {
				b.WriteString(fmt.Sprintf("%s.%s [%s]: %s\n", src.ID, names[i], short(c), reasons[i]))
			}
		}
	}
	for _, src := range usageSources {
		b.WriteString(fmt.Sprintf("AUDIT[%s]: %s\n", src.ID, src.Audit))
	}
	for _, line := range strings.Split(strings.TrimRight(b.String(), "\n"), "\n") {
		t.Log(line)
	}
}

func short(c CapabilityStatus) string {
	switch c {
	case CapImplemented:
		return "IMPL"
	case CapPartial:
		return "PART"
	case CapUnsupported:
		return "UNSUP"
	case CapUnavailable:
		return "UNAVL"
	default:
		return string(c)
	}
}
