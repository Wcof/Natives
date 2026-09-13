package usage

// R4 kimi-code parser 验收（§2.1/§7.3/§9.1）：版本化脱敏 fixture。
// 验证点：assistant 行 usage 四桶归一、缓存桶不重复计输入、
// messageId 计费原子幂等、pending/非 assistant 行跳过、
// 无 messageId 回退 session+时间戳、坏行计 skipped 不阻断。

import (
	"testing"
	"time"
)

const kimiFixtureV1 = `{"type":"session","version":1,"id":"kimi-sess-1"}
{"type":"user","message":{"role":"user"},"timestamp":"2026-09-11T10:00:00Z"}
{"type":"assistant","messageId":"msg-aaa","timestamp":"2026-09-11T10:00:05Z","message":{"role":"assistant","model":"kimi-k2","stopReason":"end_turn","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":800,"cache_creation_input_tokens":200}}}
{"type":"assistant","messageId":"msg-aaa","timestamp":"2026-09-11T10:00:06Z","message":{"role":"assistant","model":"kimi-k2","stopReason":"end_turn","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":800,"cache_creation_input_tokens":200}}}
{"type":"assistant","messageId":"msg-pending","timestamp":"2026-09-11T10:01:00Z","message":{"role":"assistant","model":"kimi-k2","stopReason":"pending","usage":{"input_tokens":10,"output_tokens":1}}}
{"type":"tool_use","toolName":"read_file","timestamp":"2026-09-11T10:01:10Z"}
not-json-garbage-line
{"type":"assistant","timestamp":"2026-09-11T10:02:00Z","message":{"role":"assistant","model":"kimi-k2","stopReason":"end_turn","usage":{"input_tokens":30,"output_tokens":20}}}
`

func TestParseKimiSessionJSONLV1(t *testing.T) {
	now := time.Date(2026, 9, 11, 12, 0, 0, 0, time.UTC)
	events, parsed, skipped := ParseKimiSessionJSONL([]byte(kimiFixtureV1), "kimi-sess-1", now)

	// 计费行：msg-aaa（重复行同原子）、无 messageId 回退行 = 3 个事件；
	// pending、user、session 头、tool_use、坏行均跳过。
	if parsed != 3 || skipped != 5 {
		t.Fatalf("parsed=%d skipped=%d, want 3/5", parsed, skipped)
	}
	if len(events) != 3 {
		t.Fatalf("events = %d, want 3", len(events))
	}

	e := events[0]
	// 计费原子：messageId 优先，重复行幂等（§7.3-2 同 message ID 不相加）。
	if e.BillingAtom != "kimi-code:msg-aaa" {
		t.Errorf("atom = %q, want kimi-code:msg-aaa", e.BillingAtom)
	}
	// 非重叠桶：input 为未缓存输入，缓存读/写独立（§7.2）。
	if e.InputTokens != 100 || e.OutputTokens != 50 ||
		e.CacheReadTokens != 800 || e.CacheWriteTokens != 200 {
		t.Errorf("buckets = in %d / out %d / read %d / write %d",
			e.InputTokens, e.OutputTokens, e.CacheReadTokens, e.CacheWriteTokens)
	}
	if e.TotalTokens != 100+50+800+200 {
		t.Errorf("total = %d, want 1150", e.TotalTokens)
	}
	if e.SessionID != "kimi-sess-1" || e.ParserVersion != "kimi-session/1" {
		t.Errorf("session/parser = %s / %s", e.SessionID, e.ParserVersion)
	}
	if e.RequestedAt != time.Date(2026, 9, 11, 10, 0, 5, 0, time.UTC) {
		t.Errorf("requestedAt = %v, want source timestamp", e.RequestedAt)
	}
	if e.RecordKind != "request" || e.Result != ResultSuccess {
		t.Errorf("kind/result = %s / %v", e.RecordKind, e.Result)
	}

	// 同 messageId 重复行：解析为独立事件但同一计费原子（§7.3-2 不相加）。
	e2 := events[1]
	if e2.BillingAtom != "kimi-code:msg-aaa" {
		t.Errorf("duplicate line atom = %q, want kimi-code:msg-aaa", e2.BillingAtom)
	}

	// 无 messageId：回退 session@毫秒，仍产生事件（成本可归属到会话）。
	e3 := events[2]
	if e3.SourceRecordID == "" || e3.BillingAtom != "" {
		t.Errorf("fallback record: %+v", e3)
	}

	// 同 fixture 重复导入（幂等验收 §9.1）：原子相同，入库层按 atom 去重。
	events2, parsed2, _ := ParseKimiSessionJSONL([]byte(kimiFixtureV1), "kimi-sess-1", now)
	if parsed2 != 3 || len(events2) != 3 {
		t.Fatalf("re-parse = %d/%d, want 3/3", parsed2, len(events2))
	}
	if events2[0].BillingAtom != events[0].BillingAtom {
		t.Errorf("re-parse atom changed: %q vs %q", events2[0].BillingAtom, events[0].BillingAtom)
	}
}
