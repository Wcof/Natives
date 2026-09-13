package usage

// Pi session JSONL parser（R4，方案 §2.1/§7.3-8）。
//
// source contract（官方文档 earendil-works/pi packages/coding-agent/docs/session-format.md，
// 按安装版本核对）：
//   - 文件位置 ~/.pi/agent/sessions/<path>/<ts>_<session-id>.jsonl；每文件一个会话；
//   - 首行 {"type":"session","version":3,"id":"uuid",...} 为 SessionHeader（无用量）；
//   - 用量在 type=="message" 且 message.role=="assistant" 的 message.usage：
//     {input, output, cacheRead, cacheWrite, cacheWrite1h?, totalTokens, cost:{...}}；
//   - 稳定计费原子：message.responseId 优先，次选 entry id（8 位 hex）；
//   - 分支（parentId 树）不是新的收费证据：abandoned 分支上已发生的请求
//     保留成本；无 usage 的 entry（user/bashExecution/custom 等）跳过计数；
//   - stopReason=="pending" 不应出现在落盘 JSONL（流式中间态），见即跳过。

import (
	"encoding/json"
	"fmt"
	"strings"
	"time"
)

type piSessionLine struct {
	Type      string `json:"type"`
	ID        string `json:"id"`
	Timestamp string `json:"timestamp"`
	Message   *struct {
		Role         string `json:"role"`
		API          string `json:"api"`
		Provider     string `json:"provider"`
		Model        string `json:"model"`
		ResponseID   string `json:"responseId"`
		StopReason   string `json:"stopReason"`
		ErrorMessage string `json:"errorMessage"`
		Usage        *struct {
			Input        int64 `json:"input"`
			Output       int64 `json:"output"`
			CacheRead    int64 `json:"cacheRead"`
			CacheWrite   int64 `json:"cacheWrite"`
			CacheWrite1h int64 `json:"cacheWrite1h"`
			TotalTokens  int64 `json:"totalTokens"`
		} `json:"usage"`
	} `json:"message"`
}

// ParsePiSessionJSONL 解析 Pi 会话 JSONL（单文件=单会话）。
// 返回 (events, parsedLines, skippedLines)。
func ParsePiSessionJSONL(content []byte, sessionID string, now time.Time) ([]NativeEvent, int, int) {
	var events []NativeEvent
	parsed, skipped := 0, 0
	for _, line := range strings.Split(string(content), "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		var rec piSessionLine
		if err := json.Unmarshal([]byte(line), &rec); err != nil {
			skipped++
			continue
		}
		if rec.Type != "message" || rec.Message == nil || rec.Message.Usage == nil {
			skipped++
			continue
		}
		m := rec.Message
		if m.StopReason == "pending" {
			skipped++
			continue
		}
		u := m.Usage
		cacheWrite := u.CacheWrite + u.CacheWrite1h
		total := u.Input + u.Output + u.CacheRead + cacheWrite

		requestedAt := now
		if ts, err := time.Parse(time.RFC3339Nano, rec.Timestamp); err == nil {
			requestedAt = ts
		} else if ts, err := time.Parse(time.RFC3339, rec.Timestamp); err == nil {
			requestedAt = ts
		}

		// 稳定计费原子：responseId 优先（provider response），次选 entry id。
		recordID := m.ResponseID
		if recordID == "" {
			recordID = rec.ID
		}
		atom := ""
		if recordID != "" {
			atom = "pi:" + recordID
		} else {
			recordID = fmt.Sprintf("session:%s@%d", sessionID, requestedAt.UnixMilli())
		}

		result := ResultSuccess
		if m.StopReason == "error" || m.ErrorMessage != "" {
			result = ResultFailed
		}

		events = append(events, NativeEvent{
			BillingAtom:      atom,
			SourceRecordID:   recordID,
			RequestedAt:      requestedAt,
			Provider:         firstNonEmpty(m.Provider, "unknown"),
			Model:            firstNonEmpty(m.Model, "unknown"),
			Source:           "Pi",
			Result:           result,
			InputTokens:      u.Input, // Pi 的 input 为未缓存输入（cacheRead/cacheWrite 独立字段）
			OutputTokens:     u.Output,
			CacheReadTokens:  u.CacheRead,
			CacheWriteTokens: cacheWrite,
			TotalTokens:      total,
			SessionID:        sessionID,
			ParserVersion:    "pi-session/1",
			RecordKind:       "request",
		})
		parsed++
	}
	return events, parsed, skipped
}

func firstNonEmpty(values ...string) string {
	for _, v := range values {
		if v != "" {
			return v
		}
	}
	return ""
}
