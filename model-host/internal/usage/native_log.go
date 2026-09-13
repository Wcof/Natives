package usage

// 原生日志适配器（T4，ADR-0030 决策 6）：Claude Code 项目 JSONL 与
// Codex sessions 增量读取的纯函数解析层。只提取白名单用量元数据——
// 不持久化 Prompt、代码、工具参数、全文摘要或凭据。
//
// source contract（先 fixture 后 parser）：
//   - Claude Code：每行一个 JSON 对象；type=="assistant" 的 message.usage
//     提供 input_tokens / cache_read_input_tokens / cache_creation_input_tokens /
//     output_tokens；稳定标识 requestId（无则 sessionId+timestamp+uuid）。
//   - Codex：sessions 快照为累计值；同一计量序列内求差，重复快照记 0，
//     序列重置（当前值 < 上一累计值）开新序列并从 0 重新累计，不静默截负。

import (
	"encoding/json"
	"fmt"
	"strings"
	"time"
)

// NativeEvent 为解析层产出的归一事件（与 Event 的生产字段一一对应）。
type NativeEvent struct {
	BillingAtom      string
	SourceRecordID   string
	RequestedAt      time.Time
	Provider         string
	Model            string
	Source           string
	Result           EventResult
	InputTokens      int64
	OutputTokens     int64
	CacheReadTokens  int64
	CacheWriteTokens int64
	TotalTokens      int64
	SessionID        string
	ParserVersion    string
	RecordKind       string // request / turn_delta / unknown
}

// claudeJSONLLine 为 Claude Code 项目日志中 type=assistant 行的白名单子集。
type claudeJSONLLine struct {
	Type      string `json:"type"`
	Timestamp string `json:"timestamp"`
	SessionID string `json:"sessionId"`
	RequestID string `json:"requestId"`
	UUID      string `json:"uuid"`
	Message   *struct {
		ID    string `json:"id"`
		Model string `json:"model"`
		Usage *struct {
			InputTokens         int64 `json:"input_tokens"`
			CacheReadTokens     int64 `json:"cache_read_input_tokens"`
			CacheCreationTokens int64 `json:"cache_creation_input_tokens"`
			OutputTokens        int64 `json:"output_tokens"`
		} `json:"usage"`
	} `json:"message"`
}

// ParseClaudeJSONL 解析 Claude Code 项目日志（多行 JSONL）。非 assistant 行、
// 缺 usage 的行与损坏行被跳过并计数（调用方据此标记 partial）。
// 返回 (events, parsedLines, skippedLines)。
func ParseClaudeJSONL(content []byte, now time.Time) ([]NativeEvent, int, int) {
	var events []NativeEvent
	parsed, skipped := 0, 0
	for _, line := range strings.Split(string(content), "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		var rec claudeJSONLLine
		if err := json.Unmarshal([]byte(line), &rec); err != nil {
			skipped++
			continue
		}
		if rec.Type != "assistant" || rec.Message == nil || rec.Message.Usage == nil {
			skipped++
			continue
		}
		requestedAt := now
		if ts, err := time.Parse(time.RFC3339Nano, rec.Timestamp); err == nil {
			requestedAt = ts
		} else if ts, err := time.Parse(time.RFC3339, rec.Timestamp); err == nil {
			requestedAt = ts
		}
		u := rec.Message.Usage
		uncached := u.InputTokens
		total := uncached + u.CacheReadTokens + u.CacheCreationTokens + u.OutputTokens
		// 稳定计费原子：requestId 优先；次选 message.id / uuid；缺失退回 session+时间戳
		atom := ""
		reqID := rec.RequestID
		if reqID == "" && rec.Message != nil && rec.Message.ID != "" {
			reqID = rec.Message.ID
		}
		if reqID == "" && rec.UUID != "" {
			reqID = rec.UUID
		}
		if reqID != "" {
			atom = "claude-code:" + reqID
		}
		recordID := reqID
		if recordID == "" {
			recordID = fmt.Sprintf("session:%s@%d", rec.SessionID, requestedAt.UnixMilli())
		}
		model := rec.Message.Model
		if model == "" || model == "<synthetic>" {
			model = "claude-3-5-sonnet"
		}
		events = append(events, NativeEvent{
			BillingAtom:      atom,
			SourceRecordID:   recordID,
			RequestedAt:      requestedAt,
			Provider:         "anthropic",
			Model:            model,
			Source:           "Claude Code",
			Result:           ResultSuccess,
			InputTokens:      uncached, // 互斥桶：uncached 口径（T0 语义）
			OutputTokens:     u.OutputTokens,
			CacheReadTokens:  u.CacheReadTokens,
			CacheWriteTokens: u.CacheCreationTokens,
			TotalTokens:      total,
			SessionID:        rec.SessionID,
			ParserVersion:    "claude-jsonl/1",
			RecordKind:       "request",
		})
		parsed++
	}
	return events, parsed, skipped
}

// codexUnifiedLine 为 Codex sessions 累计快照及 rollout 事件的统一解析结构。
type codexUnifiedLine struct {
	Timestamp string `json:"timestamp"`
	SessionID string `json:"session_id"`
	Type      string `json:"type"`
	Payload   *struct {
		SessionID  string `json:"session_id"`
		ThreadID   string `json:"thread_id"`
		ResponseID string `json:"response_id"`
		Usage      *struct {
			InputTokens  int64 `json:"input_tokens"`
			CachedTokens int64 `json:"cached_input_tokens"`
			OutputTokens int64 `json:"output_tokens"`
		} `json:"usage"`
		Info *struct {
			TotalTokenUsage *struct {
				InputTokens  int64 `json:"input_tokens"`
				CachedTokens int64 `json:"cached_input_tokens"`
				OutputTokens int64 `json:"output_tokens"`
			} `json:"total_token_usage"`
		} `json:"info"`
	} `json:"payload"`
	Info *struct {
		TokenUsage *struct {
			InputTokens  int64 `json:"input_tokens"`
			CachedTokens int64 `json:"cached_input_tokens"`
			OutputTokens int64 `json:"output_tokens"`
		} `json:"token_usage"`
	} `json:"info"`
}

// ParseCodexSessions 将 Codex 累计快照序列归一为本轮增量（无跨批基线，
// 供既有测试与小文件单批解析使用；生产采集路径走 WithBaseline 变体）。
func ParseCodexSessions(content []byte, now time.Time) ([]NativeEvent, int, int) {
	events, parsed, skipped, _ := ParseCodexSessionsWithBaseline(content, now, codexCursorBaseline{}, "")
	return events, parsed, skipped
}

// ParseCodexSessionsWithBaseline 在跨批（分块读取）场景下从上一批持久化的
// 累计基线继续求差（方案 §3.2），并以 atomSalt 保证 atom 跨批不冲突。
// 返回末尾基线，调用方与文件游标同批持久化。
func ParseCodexSessionsWithBaseline(content []byte, now time.Time, bl codexCursorBaseline, atomSalt string) ([]NativeEvent, int, int, codexCursorBaseline) {
	type seq struct {
		in, cacheRead, out int64
		epoch              int
	}
	var events []NativeEvent
	parsed, skipped := 0, 0
	sequences := map[string]*seq{}
	lastBL := bl
	snapAtom := func(sessID string, epoch int) string {
		if atomSalt != "" {
			return fmt.Sprintf("codex:%s:e%d:snap-%s-%d", sessID, epoch, atomSalt, parsed)
		}
		return fmt.Sprintf("codex:%s:e%d:snap-%d", sessID, epoch, parsed)
	}
	for _, line := range strings.Split(string(content), "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		var rec codexUnifiedLine
		if err := json.Unmarshal([]byte(line), &rec); err != nil {
			skipped++
			continue
		}

		var in, cacheRead, out int64
		sessID := rec.SessionID
		respID := ""
		if rec.Payload != nil {
			if sessID == "" {
				sessID = rec.Payload.SessionID
			}
			if sessID == "" {
				sessID = rec.Payload.ThreadID
			}
			respID = rec.Payload.ResponseID
			if rec.Payload.Usage != nil {
				in = rec.Payload.Usage.InputTokens
				cacheRead = rec.Payload.Usage.CachedTokens
				out = rec.Payload.Usage.OutputTokens
			} else if rec.Payload.Info != nil && rec.Payload.Info.TotalTokenUsage != nil {
				in = rec.Payload.Info.TotalTokenUsage.InputTokens
				cacheRead = rec.Payload.Info.TotalTokenUsage.CachedTokens
				out = rec.Payload.Info.TotalTokenUsage.OutputTokens
			}
		}
		if in == 0 && out == 0 && cacheRead == 0 && rec.Info != nil && rec.Info.TokenUsage != nil {
			in = rec.Info.TokenUsage.InputTokens
			cacheRead = rec.Info.TokenUsage.CachedTokens
			out = rec.Info.TokenUsage.OutputTokens
		}
		if (in == 0 && out == 0 && cacheRead == 0) || sessID == "" {
			skipped++
			continue
		}

		requestedAt := now
		if ts, err := time.Parse(time.RFC3339Nano, rec.Timestamp); err == nil {
			requestedAt = ts
		} else if ts, err := time.Parse(time.RFC3339, rec.Timestamp); err == nil {
			requestedAt = ts
		}
		s := sequences[sessID]
		if s == nil {
			s = &seq{}
			// 跨批续接：该会话的累计基线来自上一批持久化值（§7.3-3）。
			if sessID == bl.Session {
				s.in, s.cacheRead, s.out, s.epoch = bl.In, bl.Cache, bl.Out, bl.Epoch
			}
			sequences[sessID] = s
		}
		// 序列重置：当前累计 < 上一累计 → 新 epoch，本轮即新序列起点。
		if in < s.in || out < s.out {
			s.epoch++
			s.in, s.cacheRead, s.out = 0, 0, 0
		}
		deltaIn := in - s.in
		deltaCache := cacheRead - s.cacheRead
		if deltaCache < 0 {
			deltaCache = 0
		}
		deltaOut := out - s.out
		s.in, s.cacheRead, s.out = in, cacheRead, out
		lastBL = codexCursorBaseline{Session: sessID, In: s.in, Cache: s.cacheRead, Out: s.out, Epoch: s.epoch}

		atom := snapAtom(sessID, s.epoch)
		if respID != "" {
			atom = "codex:" + respID
		}

		if deltaIn == 0 && deltaOut == 0 && deltaCache == 0 {
			// 重复快照：归一增量为 0，仍保留元数据记录（recordKind=turn_delta）。
			// atom 唯一（快照序号内建），不会触发去重；计费合计不受影响。
			events = append(events, NativeEvent{
				BillingAtom:    atom + ":dup",
				SourceRecordID: fmt.Sprintf("codex:%s:snap-%d", sessID, parsed),
				RequestedAt:    requestedAt,
				Provider:       "openai",
				Model:          "codex",
				Source:         "Codex",
				Result:         ResultSuccess,
				TotalTokens:    0,
				SessionID:      sessID,
				ParserVersion:  "codex-sessions/1",
				RecordKind:     "turn_delta",
			})
			parsed++
			continue
		}
		uncached := deltaIn - deltaCache
		if uncached < 0 {
			uncached = 0
		}
		total := deltaIn + deltaOut
		events = append(events, NativeEvent{
			BillingAtom:     atom,
			SourceRecordID:  fmt.Sprintf("codex:%s:snap-%d", sessID, parsed),
			RequestedAt:     requestedAt,
			Provider:        "openai",
			Model:           "codex",
			Source:          "Codex",
			Result:          ResultSuccess,
			InputTokens:     uncached,
			OutputTokens:    deltaOut,
			CacheReadTokens: deltaCache,
			TotalTokens:     total,
			SessionID:       sessID,
			ParserVersion:   "codex-sessions/1",
			RecordKind:      "request",
		})
		parsed++
	}
	return events, parsed, skipped, lastBL
}

// codexCursorBaseline 为 Codex 累计快照跨批求差的持久化基线
// （与文件游标同表存储，方案 §7.3-3/§3.2）。
type codexCursorBaseline struct {
	Session string
	In      int64
	Cache   int64
	Out     int64
	Epoch   int
}

// nativeEventToEvent 转换为入库 Event（计费原子参与去重，只计一次）。
// SessionID 必须透传：R3 会话聚合（distinct source+sessionId）依赖它。
func nativeEventToEvent(n NativeEvent) *Event {
	return &Event{
		ID:               "nat-" + n.BillingAtom,
		RequestedAt:      n.RequestedAt,
		CreatedAt:        n.RequestedAt,
		Provider:         n.Provider,
		Model:            n.Model,
		Source:           n.Source,
		Result:           n.Result,
		InputTokens:      n.InputTokens,
		OutputTokens:     n.OutputTokens,
		CacheReadTokens:  n.CacheReadTokens,
		CacheWriteTokens: n.CacheWriteTokens,
		TotalTokens:      n.TotalTokens,
		BillingAtom:      n.BillingAtom,
		SessionID:        n.SessionID,
	}
}
