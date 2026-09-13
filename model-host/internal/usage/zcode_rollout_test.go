package usage

import (
	"testing"
)

// TestParseZCodeRolloutJSONL 基于 source contract 冻结的脱敏 fixture
// （§7.2 非重叠桶 + request 级计量原子）。
func TestParseZCodeRolloutJSONL(t *testing.T) {
	content := []byte(
		`{"type":"model_io","requestId":"r-1","sessionId":"sess_a29f6c46","startedAt":"2026-09-11T14:13:40.844Z",` +
			`"model":{"modelId":"gemini-3.8-flash-high","providerId":"gemini"},` +
			`"response":{"usage":{"inputTokens":100,"cacheReadTokens":40,"cacheWriteTokens":10,"outputTokens":50,"totalTokens":200}}}` + "\n" +
			`{"type":"model_io","requestId":"r-2","sessionId":"sess_a29f6c46","startedAt":"2026-09-11T14:13:47.935Z",` +
			`"model":{"modelId":"gemini-3.8-flash-high","providerId":"gemini"},` +
			`"response":{"usage":{"inputTokens":0,"cacheReadTokens":0,"cacheWriteTokens":0,"outputTokens":0,"totalTokens":0}}}` + "\n" +
			`{"type":"model_io","sessionId":"sess_x","startedAt":"2026-09-11T14:14:00Z",` +
			`"response":{"usage":{"inputTokens":5,"outputTokens":5}}}` + "\n" + // 缺 requestId：跳过
			`{broken` + "\n")

	events, parsed, skipped := ParseZCodeRolloutJSONL(content)
	if parsed != 1 {
		t.Fatalf("parsed = %d, want 1 (全零桶与缺 requestId 行不计)", parsed)
	}
	if skipped != 1 {
		t.Fatalf("skipped = %d, want 1（坏行计数）", skipped)
	}
	ev := events[0]
	if ev.BillingAtom != "zcode:r-1" {
		t.Fatalf("BillingAtom = %q, want zcode:r-1", ev.BillingAtom)
	}
	if ev.SessionID != "sess_a29f6c46" || ev.Model != "gemini-3.8-flash-high" || ev.Provider != "gemini" {
		t.Fatalf("归属字段不符: %+v", ev)
	}
	// Claude 口径独立桶：Total=100+40+10+50=200，与来源 totalTokens 一致。
	if ev.TotalTokens != 200 {
		t.Fatalf("TotalTokens = %d, want 200", ev.TotalTokens)
	}
	if ev.CacheReadTokens != 40 || ev.CacheWriteTokens != 10 || ev.InputTokens != 100 || ev.OutputTokens != 50 {
		t.Fatalf("桶字段不符: %+v", ev)
	}
	if ev.RecordKind != "request" || ev.ParserVersion != "zcode-rollout/1" {
		t.Fatalf("recordKind/parserVersion 不符: %+v", ev)
	}
	want := "2026-09-11T14:13:40.844Z"
	if got := ev.RequestedAt.UTC().Format("2006-01-02T15:04:05.000Z"); got != want {
		t.Fatalf("RequestedAt = %s, want %s（startedAt 优先于当前时间）", got, want)
	}
}

// TestParseZCodeRolloutTotalOverride：来源 totalTokens 与桶和不一致时
// 以来源 totalTokens 为展示总量（口径未声明的保护分支）。
func TestParseZCodeRolloutTotalOverride(t *testing.T) {
	content := []byte(
		`{"type":"model_io","requestId":"r-9","sessionId":"s","startedAt":"2026-09-11T10:00:00Z",` +
			`"model":{"modelId":"m","providerId":"p"},` +
			`"response":{"usage":{"inputTokens":120,"cacheReadTokens":20,"cacheWriteTokens":0,"outputTokens":0,"totalTokens":120}}}` + "\n")
	events, parsed, _ := ParseZCodeRolloutJSONL(content)
	if parsed != 1 {
		t.Fatalf("parsed = %d, want 1", parsed)
	}
	// 桶和=140，来源 total=120（OpenAI 口径，input 含 cache 子集）→ 取 120。
	if events[0].TotalTokens != 120 {
		t.Fatalf("TotalTokens = %d, want 120（来源 totalTokens 覆盖）", events[0].TotalTokens)
	}
	if events[0].InputTokens != 120 || events[0].CacheReadTokens != 20 {
		t.Fatalf("桶原值必须保留: %+v", events[0])
	}
}
