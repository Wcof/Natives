package usage

// Pi / Hermes parser 的脱敏 fixture 测试（R4，方案 §6.2：先 fixture 后 parser）。
// 格式依据官方文档（earendil-works/pi session-format.md；Hermes session-storage），
// 全部内容为合成数据，不含真实 Prompt/代码。

import (
	"strings"
	"testing"
	"time"
)

func TestParsePiSessionJSONLFixture(t *testing.T) {
	sessionID := "0f1e2d3c-4b5a-6987-8796-a5b4c3d2e1f0"
	content := strings.Join([]string{
		`{"type":"session","version":3,"id":"` + sessionID + `","timestamp":"2026-09-10T10:00:00.000Z","cwd":"/tmp/redacted"}`,
		`{"type":"message","id":"a1b2c3d4","parentId":null,"timestamp":"2026-09-10T10:00:01.000Z","message":{"role":"user","content":"redacted","timestamp":1789098001000}}`,
		`{"type":"message","id":"b2c3d4e5","parentId":"a1b2c3d4","timestamp":"2026-09-10T10:00:02.000Z","message":{"role":"assistant","content":[{"type":"text","text":"redacted"}],"api":"anthropic-messages","provider":"anthropic","model":"claude-sonnet-4-5","responseId":"resp_001","usage":{"input":1000,"output":200,"cacheRead":800,"cacheWrite":100,"totalTokens":2100,"cost":{"input":0.003,"output":0.004,"cacheRead":0.0003,"cacheWrite":0.001,"total":0.0083}},"stopReason":"stop","timestamp":1789098002000}}`,
		`{"type":"message","id":"c3d4e5f6","parentId":"b2c3d4e5","timestamp":"2026-09-10T10:00:03.000Z","message":{"role":"toolResult","toolCallId":"call_1","toolName":"bash","content":[{"type":"text","text":"redacted"}],"isError":false,"timestamp":1789098003000}}`,
		`{not-json`,
		`{"type":"message","id":"d4e5f6a7","parentId":"c3d4e5f6","timestamp":"2026-09-10T10:01:00.000Z","message":{"role":"assistant","content":[],"provider":"anthropic","model":"claude-sonnet-4-5","usage":{"input":50,"output":10,"cacheRead":0,"cacheWrite":0,"totalTokens":60},"stopReason":"error","errorMessage":"redacted error"}}`,
	}, "\n")

	events, parsed, skipped := ParsePiSessionJSONL([]byte(content), sessionID, time.Now().UTC())
	if parsed != 2 || len(events) != 2 {
		t.Fatalf("parsed = %d, events = %d, want 2/2 (skipped=%d)", parsed, len(events), skipped)
	}
	if skipped == 0 {
		t.Errorf("user/toolResult/corrupt lines must be skipped and counted, skipped=0")
	}

	e0 := events[0]
	if e0.Provider != "anthropic" || e0.Model != "claude-sonnet-4-5" {
		t.Errorf("provider/model = %s/%s", e0.Provider, e0.Model)
	}
	if e0.InputTokens != 1000 || e0.OutputTokens != 200 {
		t.Errorf("tokens = %d/%d, want 1000/200 (Pi input 桶为未缓存输入)", e0.InputTokens, e0.OutputTokens)
	}
	if e0.CacheReadTokens != 800 || e0.CacheWriteTokens != 100 {
		t.Errorf("cache = read %d / write %d, want 800/100", e0.CacheReadTokens, e0.CacheWriteTokens)
	}
	if e0.TotalTokens != 2100 {
		t.Errorf("totalTokens = %d, want 2100 (input+output+cacheRead+cacheWrite)", e0.TotalTokens)
	}
	if e0.BillingAtom != "pi:resp_001" {
		t.Errorf("atom = %q, want pi:resp_001 (responseId 优先)", e0.BillingAtom)
	}
	if e0.SessionID != sessionID {
		t.Errorf("sessionId = %q", e0.SessionID)
	}
	if events[1].Result != ResultFailed {
		t.Errorf("stopReason=error must map to failed, got %s", events[1].Result)
	}
}

// TestParsePiSessionJSONLCacheWrite1h 验证 cacheWrite1h 并入 cache_write 桶（§7.2 非重叠桶）。
func TestParsePiSessionJSONLCacheWrite1h(t *testing.T) {
	content := `{"type":"message","id":"aaa11111","parentId":null,"timestamp":"2026-09-10T10:00:02.000Z","message":{"role":"assistant","provider":"openai","model":"gpt-x","usage":{"input":100,"output":10,"cacheRead":0,"cacheWrite":20,"cacheWrite1h":30,"totalTokens":160},"stopReason":"stop"}}`
	events, _, _ := ParsePiSessionJSONL([]byte(content), "sess-x", time.Now().UTC())
	if len(events) != 1 {
		t.Fatalf("events = %d, want 1", len(events))
	}
	if events[0].CacheWriteTokens != 50 {
		t.Errorf("cacheWrite = %d, want 50 (cacheWrite + cacheWrite1h)", events[0].CacheWriteTokens)
	}
}
