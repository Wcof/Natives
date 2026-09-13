package usage

// atomcode_session.go 测试：atomcode-session/1（本机探测 wire schema 冻结，
// 2026-09-12）。覆盖：流式多行同 turn 取终态不重复相加（§7.3-2）、
// undone 行跳过、iso/ts 双时间源、坏行与缺 usage 行显式 skipped。

import (
	"testing"
	"time"
)

// fixture 按本机安装版本真实结构脱敏（v/turn_id 为数字；turn_id 字符串
// 形式保留一行以验证兼容旧导出）。
const atomcodeFixtureJSONL = `{"v":1,"session_id":"sess-a","turn_id":1,"iso":"2026-09-12T09:00:00Z","usage":{"prompt":100,"completion":20,"cached":0}}
{"v":1,"session_id":"sess-a","turn_id":1,"iso":"2026-09-12T09:00:02Z","usage":{"prompt":180,"completion":45,"cached":30}}
{"v":1,"session_id":"sess-a","turn_id":2,"iso":"2026-09-12T09:01:00Z","usage":{"prompt":50,"completion":10,"cached":0}}
{"v":1,"session_id":"sess-a","turn_id":"3","ts":1757660400000,"usage":{"prompt":10,"completion":2,"cached":0}}
{"v":1,"session_id":"sess-a","turn_id":4,"iso":"2026-09-12T09:02:00Z","undone":true,"usage":{"prompt":999,"completion":99,"cached":0}}
{"v":"1","session_id":"sess-a","turn_id":"t5","iso":"2026-09-12T09:03:00Z"}
not-json-at-all
`

func TestParseAtomcodeSessionJSONL(t *testing.T) {
	now := time.Date(2026, 9, 12, 12, 0, 0, 0, time.UTC)
	events, parsed, skipped := ParseAtomcodeSessionJSONL([]byte(atomcodeFixtureJSONL), "file-sess-a", now)

	// parsed 计数=有 usage 且未撤销的行（流式中间行计入 parsed）；
	// skipped=撤销 1 + 缺 usage 1 + 坏行 1 = 3。
	if parsed != 4 {
		t.Errorf("parsed = %d, want 4", parsed)
	}
	if skipped != 3 {
		t.Errorf("skipped = %d, want 3 (undone+no-usage+bad)", skipped)
	}

	// 同 turn t1 两条流式行只产出一条终态事件，取 usage 最大的最后快照，
	// 不相加（§7.3-2）：prompt=180 completion=45 cached=30。
	if len(events) != 3 {
		t.Fatalf("events = %d, want 3 (t1 terminal + t2 + t3)", len(events))
	}
	var t1 *NativeEvent
	var t3 *NativeEvent
	for i := range events {
		switch events[i].SourceRecordID {
		case "sess-a/1":
			t1 = &events[i]
		case "sess-a/3":
			t3 = &events[i]
		}
	}
	if t1 == nil {
		t.Fatalf("t1 terminal event missing; got %+v", events)
	}
	if t1.InputTokens != 180 || t1.OutputTokens != 45 || t1.CacheReadTokens != 30 {
		t.Errorf("t1 terminal = in=%d out=%d cached=%d, want 180/45/30 (max snapshot, not sum)",
			t1.InputTokens, t1.OutputTokens, t1.CacheReadTokens)
	}
	if t1.TotalTokens != 225 {
		t.Errorf("t1 total = %d, want 225 (180+45)", t1.TotalTokens)
	}
	// 计费原子稳定：atomcode:<session>:<turn>。
	if t1.BillingAtom != "atomcode:sess-a:1" {
		t.Errorf("t1 atom = %q, want atomcode:sess-a:1", t1.BillingAtom)
	}
	// iso 时间源生效（非 fallback now）。
	if got := t1.RequestedAt.UTC(); got != time.Date(2026, 9, 12, 9, 0, 2, 0, time.UTC) {
		t.Errorf("t1 requestedAt = %v, want 09:00:02Z (iso source)", got)
	}
	// ts (UnixMilli) 时间源：1757660400000 = 2025-09-12T07:00:00Z。
	if t3 == nil {
		t.Fatalf("t3 event missing")
	}
	if got := t3.RequestedAt.UTC(); got != time.Date(2025, 9, 12, 7, 0, 0, 0, time.UTC) {
		t.Errorf("t3 requestedAt = %v, want 2025-09-12T07:00:00Z (ts millis source)", got)
	}
	// 无 model 字段：如实 unknown，不猜（§7.5）。
	if t1.Model != "unknown" || t1.Provider != "unknown" {
		t.Errorf("model/provider = %q/%q, want unknown/unknown", t1.Model, t1.Provider)
	}
	// 撤销行不计费：999 token 的事件不存在。
	for _, e := range events {
		if e.InputTokens == 999 {
			t.Errorf("undone line must not produce a billed event")
		}
	}
	if t1.ParserVersion != "atomcode-session/1" || t1.RecordKind != "request" {
		t.Errorf("parserVersion/recordKind = %q/%q, want atomcode-session/1/request", t1.ParserVersion, t1.RecordKind)
	}
}
