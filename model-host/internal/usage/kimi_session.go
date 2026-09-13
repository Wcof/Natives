package usage

// Kimi Code session JSONL parser（R4，方案 §2.1）。
//
// source contract（官方 MoonshotAI/kimi-code docs/en/guides/sessions.md，
// 按安装版本核对；wire schema 以 fixture 冻结的版本为准）：
//   - 每行一个 JSON 对象；用量在 type=="assistant" 行的 message.usage：
//     {input_tokens, output_tokens, cache_read_input_tokens,
//      cache_creation_input_tokens}（与 Anthropic 上游计费桶同构）；
//   - 稳定计费原子：assistant 行的 messageId（provider response）；
//     缺失时回退 sessionId+timestamp，不与重复行相加（§7.3-2）；
//   - 流式中间态（stop_reason=="pending" 或缺失 usage）跳过计数；
//   - 未知行 type 不计费也不报错（坏行计 skipped，来源状态显式 partial）。

import (
	"encoding/json"
	"fmt"
	"strings"
	"time"
)

type kimiSessionLine struct {
	Type      string `json:"type"`
	MessageID string `json:"messageId"`
	Timestamp string `json:"timestamp"`
	SessionID string `json:"sessionId"`
	Message   *struct {
		Role       string `json:"role"`
		Model      string `json:"model"`
		StopReason string `json:"stopReason"`
		Usage      *struct {
			InputTokens         int64 `json:"input_tokens"`
			CacheReadTokens     int64 `json:"cache_read_input_tokens"`
			CacheCreationTokens int64 `json:"cache_creation_input_tokens"`
			OutputTokens        int64 `json:"output_tokens"`
		} `json:"usage"`
	} `json:"message"`
}

// ParseKimiSessionJSONL 解析 Kimi Code 会话 JSONL。
// 返回 (events, parsedLines, skippedLines)。
func ParseKimiSessionJSONL(content []byte, sessionID string, now time.Time) ([]NativeEvent, int, int) {
	var events []NativeEvent
	parsed, skipped := 0, 0
	for _, line := range strings.Split(string(content), "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		var rec kimiSessionLine
		if err := json.Unmarshal([]byte(line), &rec); err != nil {
			skipped++
			continue
		}
		if rec.Type != "assistant" || rec.Message == nil || rec.Message.Usage == nil {
			skipped++
			continue
		}
		m := rec.Message
		if m.StopReason == "pending" {
			skipped++
			continue
		}
		u := m.Usage
		total := u.InputTokens + u.OutputTokens + u.CacheReadTokens + u.CacheCreationTokens

		requestedAt := now
		if ts, err := time.Parse(time.RFC3339Nano, rec.Timestamp); err == nil {
			requestedAt = ts
		} else if ts, err := time.Parse(time.RFC3339, rec.Timestamp); err == nil {
			requestedAt = ts
		}

		// 稳定计费原子：messageId 优先；无 provider response ID 时
		// 回退 session+毫秒，避免同会话重复行被当作不同收费。
		recordID := rec.MessageID
		atom := ""
		if recordID != "" {
			atom = "kimi-code:" + recordID
		} else {
			recordID = fmt.Sprintf("session:%s@%d", sessionID, requestedAt.UnixMilli())
		}

		result := ResultSuccess
		if m.StopReason == "error" {
			result = ResultFailed
		}

		events = append(events, NativeEvent{
			BillingAtom:      atom,
			SourceRecordID:   recordID,
			RequestedAt:      requestedAt,
			Provider:         "moonshot",
			Model:            firstNonEmpty(m.Model, "unknown"),
			Source:           "Kimi Code",
			Result:           result,
			InputTokens:      u.InputTokens,
			OutputTokens:     u.OutputTokens,
			CacheReadTokens:  u.CacheReadTokens,
			CacheWriteTokens: u.CacheCreationTokens,
			TotalTokens:      total,
			SessionID:        sessionID,
			ParserVersion:    "kimi-session/1",
			RecordKind:       "request",
		})
		parsed++
	}
	return events, parsed, skipped
}
