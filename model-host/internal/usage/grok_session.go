package usage

// Grok Build session JSONL parser（收口方案 Phase 0，ADR-0030 决策 6）。
//
// source contract（wire schema 按本机安装版本真实文件冻结：grok-session/1，
// 2026-09-27 真机核对；路径 ~/.grok/sessions/<url-encoded-project>/<uuid>/updates.jsonl）：
//   - 每行一个 JSON 对象；method=="_x.ai/session/update" 且
//     params.update.sessionUpdate=="turn_completed" 的行携带本轮终态用量
//     params.update.usage（inputTokens/outputTokens/totalTokens/cachedReadTokens/
//     reasoningTokens/modelUsage）；其余更新行不含用量，跳过；
//   - _meta.eventId 为稳定事件标识；prompt_id 为一轮（turn）的稳定键；
//   - 同一 prompt_id 多行更新取 usage.totalTokens 最大的行为终态
//     （§7.3-2：确定终态记录，不把同调用的中间行相加）；
//   - modelUsage 按模型拆分归因：每模型一条事件（billingAtom 加模型后缀），
//     顶层 usage 为各模型分量之和；缺失时退回单事件、模型记 unknown；
//   - 时间：timestamp 为 Unix 秒；totalTokens 以上报值为真源，不重算
//     （cachedReadTokens 与 inputTokens 的包含关系版本未承诺）。
//   - 注：2026-09-12 旧审计判 unsupported 针对当时版本的 chat_history/
//     events.jsonl（确无用量字段）；CLI 更新后新增 updates.jsonl 携带
//     结构化 usage，矩阵已随本次核对修订。

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"time"
)

func collectGrokSource(s *Store, root string, now time.Time) collectResult {
	return collectJSONLSessionDir(s, filepath.Join(root, "sessions"), "grok-build", "Grok Build", s.collectGrokFile, now)
}

func (s *Store) collectGrokFile(filePath string, now time.Time, instanceID string) (int64, int64, error) {
	info, err := os.Stat(filePath)
	if err != nil || info.Size() == 0 {
		return 0, 0, nil
	}
	fpHash := fileFingerprint(filePath)
	offset := s.getCursorOffset("grok-build", fpHash)
	if offset > info.Size() {
		// 轮转/截断：文件比游标短——从 0 重读；billingAtom 唯一索引
		// 幂等去重，已导入部分不会重复计量（§5.1.7 恢复语义）。
		offset = 0
	} else if offset == info.Size() {
		return 0, 0, nil
	}

	file, err := os.Open(filePath)
	if err != nil {
		return 0, 0, err
	}
	defer file.Close()

	if offset > 0 {
		if _, err := file.Seek(offset, io.SeekStart); err != nil {
			return 0, 0, err
		}
	}

	validBuf, newOffset, err := readBoundedLines(file, offset, info.Size())
	if err != nil {
		return 0, 0, err
	}
	if len(validBuf) == 0 {
		return 0, 0, nil
	}

	// 会话标识 = updates.jsonl 所在目录名（<uuid>）。
	sessID := filepath.Base(filepath.Dir(filePath))

	events, _, _ := ParseGrokSessionJSONL(validBuf, sessID, now)
	imported := s.commitParsedEvents(events, "grok-build", "Grok Build", "grok-session/1", instanceID, fpHash, newOffset)
	return imported, int64(len(validBuf)), nil
}

type grokUsageBlock struct {
	InputTokens      int64 `json:"inputTokens"`
	OutputTokens     int64 `json:"outputTokens"`
	TotalTokens      int64 `json:"totalTokens"`
	CachedReadTokens int64 `json:"cachedReadTokens"`
	ReasoningTokens  int64 `json:"reasoningTokens"`
	ModelUsage       map[string]struct {
		InputTokens      int64 `json:"inputTokens"`
		OutputTokens     int64 `json:"outputTokens"`
		TotalTokens      int64 `json:"totalTokens"`
		CachedReadTokens int64 `json:"cachedReadTokens"`
		ReasoningTokens  int64 `json:"reasoningTokens"`
	} `json:"modelUsage"`
}

type grokSessionLine struct {
	Timestamp int64  `json:"timestamp"`
	Method    string `json:"method"`
	Params    *struct {
		SessionID string `json:"sessionId"`
		Update    *struct {
			SessionUpdate string          `json:"sessionUpdate"`
			PromptID      string          `json:"prompt_id"`
			Usage         *grokUsageBlock `json:"usage"`
		} `json:"update"`
		Meta *struct {
			EventID string `json:"eventId"`
		} `json:"_meta"`
	} `json:"params"`
}

// ParseGrokSessionJSONL 解析 Grok Build updates.jsonl。
// 返回 (events, parsedLines, skippedLines)。同一 prompt_id 的多行更新只产出
// 终态事件（totalTokens 最大者，同值取最后出现）；modelUsage 按模型拆分归因。
func ParseGrokSessionJSONL(content []byte, sessionID string, now time.Time) ([]NativeEvent, int, int) {
	type candidate struct {
		line      grokSessionLine
		total     int64
		requested time.Time
		turnKey   string
	}
	turns := map[string]*candidate{}
	turnOrder := []string{}
	parsed, skipped := 0, 0

	for _, line := range strings.Split(string(content), "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		var rec grokSessionLine
		if err := json.Unmarshal([]byte(line), &rec); err != nil {
			skipped++
			continue
		}
		if rec.Method != "_x.ai/session/update" || rec.Params == nil || rec.Params.Update == nil ||
			rec.Params.Update.SessionUpdate != "turn_completed" || rec.Params.Update.Usage == nil {
			skipped++
			continue
		}

		requested := now
		if rec.Timestamp > 0 {
			requested = time.Unix(rec.Timestamp, 0)
		}

		turnKey := rec.Params.Update.PromptID
		if turnKey == "" {
			turnKey = rec.Params.Meta.EventID
		}
		if turnKey == "" {
			turnKey = fmt.Sprintf("%s@%d", sessionID, rec.Timestamp)
		}

		total := rec.Params.Update.Usage.TotalTokens
		prev := turns[turnKey]
		if prev == nil {
			turns[turnKey] = &candidate{line: rec, total: total, requested: requested, turnKey: turnKey}
			turnOrder = append(turnOrder, turnKey)
		} else if total >= prev.total {
			*prev = candidate{line: rec, total: total, requested: requested, turnKey: turnKey}
		}
		parsed++
	}

	var events []NativeEvent
	for _, turnKey := range turnOrder {
		cand := turns[turnKey]
		u := cand.line.Params.Update.Usage
		eventID := cand.line.Params.Meta.EventID
		if eventID == "" {
			eventID = fmt.Sprintf("session:%s@%d", cand.line.Params.SessionID, cand.requested.UnixMilli())
		}
		sessID := cand.line.Params.SessionID
		if sessID == "" {
			sessID = sessionID
		}

		emit := func(model string, in, out, total, cacheRead, reasoning int64, atomSuffix string) {
			events = append(events, NativeEvent{
				BillingAtom:      "grok-build:" + eventID + atomSuffix,
				SourceRecordID:   eventID + atomSuffix,
				RequestedAt:      cand.requested,
				Provider:         "xai",
				Model:            model,
				Source:           "Grok Build",
				Result:           ResultSuccess,
				InputTokens:      in,
				OutputTokens:     out,
				CacheReadTokens:  cacheRead,
				CacheWriteTokens: 0,
				ReasoningTokens:  reasoning,
				TotalTokens:      total,
				SessionID:        sessID,
				ParserVersion:    "grok-session/1",
				RecordKind:       "request",
			})
		}

		if len(u.ModelUsage) > 0 {
			// 按模型拆分归因：每模型一条事件，顶层 usage 为各模型分量之和。
			for model, mu := range u.ModelUsage {
				emit(model, mu.InputTokens, mu.OutputTokens, mu.TotalTokens, mu.CachedReadTokens, mu.ReasoningTokens, ":"+model)
			}
		} else {
			emit("unknown", u.InputTokens, u.OutputTokens, u.TotalTokens, u.CachedReadTokens, u.ReasoningTokens, "")
		}
	}
	return events, parsed, skipped
}
