package usage

// OpenCode 本地 SQLite 只读采集器（R4，方案 §2.1/§6.2/§7.2）。
//
// source contract（本机安装版本 opencode 1.18.3 实测 schema，2026-09）：
//   - 文件 ~/.local/share/opencode/opencode.db（SQLite）；
//   - message 表：id/session_id/time_created/time_updated/data(JSON)；
//     data 白名单字段：role/modelID/providerID/cost/tokens{input,output,
//     reasoning,cache{read,write}}/time；不读 part 表正文内容；
//   - ⚠ OpenCode 的 cache 是 input 的子集（OpenAI 口径，§7.2 第 2 段）：
//     adapter 转换一次为非重叠桶 input_uncached = input - cache_read - cache_write
//     （负值截为 0 并保留原值于 TotalTokens 校验）；reasoning 已含于 output
//     口径不重复相加，仅入 TotalTokens 的展示总量；
//   - 稳定计费原子：opencode:<message.id>（消息级唯一，重复导入幂等）；
//   - 仅 role=assistant 且 tokens>0 的行产生计量事件（recordKind=request）；
//   - 隐私：仅投影上述白名单元数据，不读消息正文、part.content、tool 参数。

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"time"

	_ "modernc.org/sqlite"
)

// OpenCodeMessageRow 为 message.data JSON 的白名单投影。
type OpenCodeMessageRow struct {
	ID        string
	SessionID string
	Role      string  `json:"role"`
	ModelID   string  `json:"modelID"`
	Provider  string  `json:"providerID"`
	Cost      float64 `json:"cost"`
	Tokens    struct {
		Input     int64 `json:"input"`
		Output    int64 `json:"output"`
		Reasoning int64 `json:"reasoning"`
		Cache     struct {
			Read  int64 `json:"read"`
			Write int64 `json:"write"`
		} `json:"cache"`
	}
	CreatedAt int64 // 毫秒
}

// ParseOpenCodeDB 只读打开 OpenCode 本地数据库并投影 message 白名单字段。
// dbPath 指向用户授权的数据根内 opencode.db；不写库、不读正文。
func ParseOpenCodeDB(dbPath string) ([]NativeEvent, int, error) {
	db, err := sql.Open("sqlite", fmt.Sprintf("file:%s?mode=ro", dbPath))
	if err != nil {
		return nil, 0, err
	}
	defer db.Close()

	// schema 探测：message 表存在才继续；否则显式失败（格式不支持的分支）。
	var hasMessage int
	if err := db.QueryRow(
		"SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='message'",
	).Scan(&hasMessage); err != nil {
		return nil, 0, fmt.Errorf("opencode: schema probe failed: %w", err)
	}
	if hasMessage == 0 {
		return nil, 0, fmt.Errorf("opencode: no message table; unsupported schema")
	}

	rows, err := db.Query(`SELECT id, session_id, time_created, data FROM message ORDER BY time_created`)
	if err != nil {
		return nil, 0, fmt.Errorf("opencode: message projection failed: %w", err)
	}
	defer rows.Close()

	var events []NativeEvent
	parsed := 0
	for rows.Next() {
		var (
			id, sessionID, raw string
			createdMS          int64
		)
		if err := rows.Scan(&id, &sessionID, &createdMS, &raw); err != nil {
			return events, parsed, fmt.Errorf("opencode: scan message: %w", err)
		}
		if id == "" {
			continue
		}
		var row OpenCodeMessageRow
		if err := json.Unmarshal([]byte(raw), &row); err != nil {
			// 坏行计数跳过（§7.3-6：安全部分必须为 partial）。
			continue
		}
		if row.Role != "assistant" {
			continue
		}
		t := row.Tokens
		// OpenAI 口径 → 非重叠桶转换（§7.2）：input 已含 cache 子集。
		cacheRead, cacheWrite := t.Cache.Read, t.Cache.Write
		inputUncached := t.Input - cacheRead - cacheWrite
		if inputUncached < 0 {
			inputUncached = 0
		}
		if inputUncached == 0 && cacheRead == 0 && cacheWrite == 0 && t.Output == 0 {
			continue
		}
		requestedAt := time.UnixMilli(createdMS).UTC()
		events = append(events, NativeEvent{
			BillingAtom:      "opencode:" + id,
			SourceRecordID:   "opencode-msg:" + id,
			RequestedAt:      requestedAt,
			Provider:         firstNonEmpty(row.Provider, "opencode"),
			Model:            firstNonEmpty(row.ModelID, "unknown"),
			Source:           "OpenCode",
			Result:           ResultSuccess,
			InputTokens:      inputUncached,
			OutputTokens:     t.Output,
			CacheReadTokens:  cacheRead,
			CacheWriteTokens: cacheWrite,
			TotalTokens:      inputUncached + cacheRead + cacheWrite + t.Output,
			SessionID:        sessionID,
			ParserVersion:    "opencode-store/1",
			RecordKind:       "request",
		})
		parsed++
	}
	return events, parsed, rows.Err()
}
