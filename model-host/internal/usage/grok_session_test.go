package usage

import (
	"strconv"
	"strings"
	"testing"
	"time"
)

// fixture 行取自 2026-09-27 真机 updates.jsonl（结构冻结为 grok-session/1），
// 值按测试需要缩放，字段名与嵌套形状保持一致。
const grokTurnLine = `{"timestamp":1784855596,"method":"_x.ai/session/update","params":{"sessionId":"019f91ab-09d3-7780-b150-d9d87dcb2cde","update":{"sessionUpdate":"turn_completed","prompt_id":"115e65c6-c008-4d87-b64d-08cb2bb7a070","stop_reason":"end_turn","usage":{"inputTokens":728750,"outputTokens":13577,"totalTokens":742327,"cachedReadTokens":0,"reasoningTokens":2051,"modelCalls":18,"apiDurationMs":193810,"modelUsage":{"grok-4.5-build-free":{"inputTokens":728750,"outputTokens":13577,"totalTokens":742327,"cachedReadTokens":0,"reasoningTokens":2051,"modelCalls":18,"apiDurationMs":193810}}}},"_meta":{"eventId":"019f91ab-09d3-7780-b150-d9d87dcb2cde-1079","agentTimestampMs":1784855596046}}}`

func TestParseGrokSessionJSONLTurnUsage(t *testing.T) {
	content := []byte(strings.Join([]string{
		`{"timestamp":1784855500,"method":"_x.ai/session/update","params":{"update":{"sessionUpdate":"agent_message_start"}}}`,
		grokTurnLine,
	}, "\n"))

	events, parsed, skipped := ParseGrokSessionJSONL(content, "019f91ab-09d3-7780-b150-d9d87dcb2cde", time.Now())
	if parsed != 1 || skipped != 1 {
		t.Fatalf("parsed=%d skipped=%d, want 1/1（无用量更新行跳过）", parsed, skipped)
	}
	if len(events) != 1 {
		t.Fatalf("events=%d, want 1（单模型 turn 拆一条）", len(events))
	}
	ev := events[0]
	if ev.Model != "grok-4.5-build-free" {
		t.Fatalf("model=%q, want grok-4.5-build-free（modelUsage 归因）", ev.Model)
	}
	if ev.InputTokens != 728750 || ev.OutputTokens != 13577 || ev.TotalTokens != 742327 {
		t.Fatalf("tokens=%d/%d/%d, want 728750/13577/742327", ev.InputTokens, ev.OutputTokens, ev.TotalTokens)
	}
	if ev.ReasoningTokens != 2051 {
		t.Fatalf("reasoning=%d, want 2051", ev.ReasoningTokens)
	}
	if ev.SessionID != "019f91ab-09d3-7780-b150-d9d87dcb2cde" {
		t.Fatalf("session=%q", ev.SessionID)
	}
	if !strings.HasPrefix(ev.BillingAtom, "grok-build:019f91ab-09d3-7780-b150-d9d87dcb2cde-1079:grok-4.5-build-free") {
		t.Fatalf("billingAtom=%q 缺少稳定 eventId+模型后缀", ev.BillingAtom)
	}
	if ev.RequestedAt.Unix() != 1784855596 {
		t.Fatalf("requestedAt=%v, want unix 1784855596", ev.RequestedAt)
	}
}

func TestParseGrokSessionJSONLTerminalStateWins(t *testing.T) {
	// 同一 prompt_id 两行：流式更新中间态 totalTokens 较小，终态取最大。
	// 注意：_meta 与 update 同级（真实文件形状）。
	turn := func(total int64, eventID string) string {
		return `{"timestamp":1784855596,"method":"_x.ai/session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"turn_completed","prompt_id":"p1","usage":{"inputTokens":100,"outputTokens":10,"totalTokens":` +
			strconv.FormatInt(total, 10) + `}},"_meta":{"eventId":"` + eventID + `"}}}`
	}
	content := []byte(strings.Join([]string{turn(500, "e-1"), turn(900, "e-2")}, "\n"))

	events, parsed, _ := ParseGrokSessionJSONL(content, "s1", time.Now())
	if parsed != 2 {
		t.Fatalf("parsed=%d, want 2", parsed)
	}
	if len(events) != 1 {
		t.Fatalf("events=%d, want 1（同 prompt_id 只留终态）", len(events))
	}
	if events[0].TotalTokens != 900 || events[0].BillingAtom != "grok-build:e-2" {
		t.Fatalf("终态选择错误: total=%d atom=%q", events[0].TotalTokens, events[0].BillingAtom)
	}
}

func TestParseGrokSessionJSONLMultiModelSplit(t *testing.T) {
	line := `{"timestamp":1784855596,"method":"_x.ai/session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"turn_completed","prompt_id":"p1","usage":{"inputTokens":300,"outputTokens":30,"totalTokens":330,"modelUsage":{"grok-a":{"inputTokens":200,"outputTokens":20,"totalTokens":220},"grok-b":{"inputTokens":100,"outputTokens":10,"totalTokens":110}}}},"_meta":{"eventId":"e-1"}}}`
	events, _, _ := ParseGrokSessionJSONL([]byte(line), "s1", time.Now())
	if len(events) != 2 {
		t.Fatalf("events=%d, want 2（modelUsage 按模型拆分）", len(events))
	}
	byModel := map[string]NativeEvent{}
	for _, ev := range events {
		byModel[ev.Model] = ev
	}
	if byModel["grok-a"].TotalTokens != 220 || byModel["grok-b"].TotalTokens != 110 {
		t.Fatalf("模型分量错误: %+v", byModel)
	}
	if byModel["grok-a"].BillingAtom == byModel["grok-b"].BillingAtom {
		t.Fatal("不同模型的 billingAtom 必须不同")
	}
}
