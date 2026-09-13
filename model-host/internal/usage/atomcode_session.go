package usage

// AtomCode session JSONL parser（R4，方案 §2.1 用户明确指定工具）。
//
// source contract（wire schema 按本机安装版本真实文件冻结：atomcode-session/1，
// 2026-09-12 真机核对修订；路径 ~/.atomcode/sessions/<projectHash>/<uuid>.jsonl）：
//   - 每行一个 JSON 对象，顶层 v=="1"（真实文件为数字 1，v/turn_id 均兼容
//     数字与字符串两种序列化）；
//   - 用量在 usage 字段：{prompt:int, completion:int, cached:int}（token 数）；
//   - 时间：iso（RFC3339）优先，缺失回退 ts（UnixMilli）；
//   - undone==true 的行是被撤销的操作，不计费（§7.3-8：分支回滚不是新收费证据）；
//   - 稳定计费原子：同一 (session_id, turn_id) 视为一次调用；流式中间态
//     多行更新取 usage 累计值最大的行作为终态（§7.3-2：确定终态记录，
//     不把同调用的中间行相加）。该假设在真实数据上验证（导出器核对）；
//   - 无 model 字段：如实记 unknown，不按上下文猜（§7.5）。

import (
	"encoding/json"
	"fmt"
	"strings"
	"time"
)

// flexiString 兼容 JSON 数字与字符串两种标量（真实安装版本 turn_id/v 为
// 数字；保留字符串兼容以接受旧导出）。
type flexiString string

func (f *flexiString) UnmarshalJSON(data []byte) error {
	s := strings.TrimSpace(string(data))
	if s == "null" {
		*f = ""
		return nil
	}
	if len(s) >= 2 && s[0] == '"' && s[len(s)-1] == '"' {
		var out string
		if err := json.Unmarshal(data, &out); err != nil {
			return err
		}
		*f = flexiString(out)
		return nil
	}
	var num json.Number
	if err := json.Unmarshal(data, &num); err != nil {
		return fmt.Errorf("flexiString: %q is neither number nor string", s)
	}
	*f = flexiString(num.String())
	return nil
}

type atomcodeSessionLine struct {
	V         flexiString `json:"v"`
	SessionID string      `json:"session_id"`
	TurnID    flexiString `json:"turn_id"`
	Iso       string      `json:"iso"`
	Ts        json.Number `json:"ts"`
	Undone    bool        `json:"undone"`
	Usage     *struct {
		Prompt     int64 `json:"prompt"`
		Completion int64 `json:"completion"`
		Cached     int64 `json:"cached"`
	} `json:"usage"`
}

// ParseAtomcodeSessionJSONL 解析 AtomCode 会话 JSONL。
// 返回 (events, parsedLines, skippedLines)。同一 turn 的多行流式更新
// 只产出一条终态事件（usage 累计值最大者）。
func ParseAtomcodeSessionJSONL(content []byte, sessionID string, now time.Time) ([]NativeEvent, int, int) {
	// turn → 终态候选行（usage 最大；同值取最后出现，流式更新单调不减）。
	type candidate struct {
		line      atomcodeSessionLine
		total     int64
		requested time.Time
	}
	turns := map[string]*candidate{}
	turnOrder := []string{}
	parsed, skipped := 0, 0

	for _, line := range strings.Split(string(content), "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		var rec atomcodeSessionLine
		if err := json.Unmarshal([]byte(line), &rec); err != nil {
			skipped++
			continue
		}
		if rec.Undone {
			// 撤销操作：已发生的成本保留原则见 §7.3-8；本来源无法区分
			// "撤销前已计费"与"从未发生"，按 wire schema 语义跳过该行。
			skipped++
			continue
		}
		if rec.Usage == nil || rec.SessionID == "" || rec.TurnID == "" {
			skipped++
			continue
		}

		requested := now
		if rec.Iso != "" {
			if ts, err := time.Parse(time.RFC3339Nano, rec.Iso); err == nil {
				requested = ts
			} else if ts, err := time.Parse(time.RFC3339, rec.Iso); err == nil {
				requested = ts
			}
		} else if rec.Ts != "" {
			if ms, err := rec.Ts.Int64(); err == nil {
				requested = time.UnixMilli(ms)
			}
		}

		u := rec.Usage
		total := u.Prompt + u.Completion
		key := rec.SessionID + "\x00" + string(rec.TurnID)
		cur, seen := turns[key]
		if !seen {
			turns[key] = &candidate{line: rec, total: total, requested: requested}
			turnOrder = append(turnOrder, key)
		} else if total >= cur.total {
			// 流式更新的更晚/更大快照覆盖为终态。
			cur.line, cur.total, cur.requested = rec, total, requested
		}
		parsed++
	}

	var events []NativeEvent
	for _, key := range turnOrder {
		cand := turns[key]
		u := cand.line.Usage
		events = append(events, NativeEvent{
			// 无 provider response ID：session+turn 是该来源最稳定的调用标识
			//（§7.3-1）。
			BillingAtom:     "atomcode:" + cand.line.SessionID + ":" + string(cand.line.TurnID),
			SourceRecordID:  cand.line.SessionID + "/" + string(cand.line.TurnID),
			RequestedAt:     cand.requested,
			Provider:        "unknown",
			Model:           "unknown",
			Source:          "AtomCode",
			Result:          ResultSuccess,
			InputTokens:     u.Prompt,
			OutputTokens:    u.Completion,
			CacheReadTokens: u.Cached,
			TotalTokens:     cand.total,
			SessionID:       cand.line.SessionID,
			ParserVersion:   "atomcode-session/1",
			RecordKind:      "request",
		})
	}
	return events, parsed, skipped
}
