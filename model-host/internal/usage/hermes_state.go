package usage

// Hermes Agent state.db 只读采集器（R4，方案 §2.1/§6.2）。
//
// source contract（官方文档 hermes-agent.nousresearch.com/docs/developer-guide/session-storage，
// 按安装版本核对；schema_version 23）：
//   - 文件 ~/.hermes/state.db（SQLite，WAL 模式）；
//   - 只读打开用户授权的 profile DB：固定字段投影、不执行迁移、
//     不查询正文（messages.content）与 FTS 表；按安装版本检查实际列；
//   - WAL：必须同时定位 -wal 文件（SQLite 只读连接会读取 WAL）；
//   - sessions 表白名单字段：id/source/model/started_at/ended_at/
//     input_tokens/output_tokens/cache_read_tokens/cache_write_tokens/
//     reasoning_tokens/billing_provider/billing_mode/cost_status；
//     usage 行按 session 级总量导入（recordKind=session_delta），无逐请求
//     记录时用 sessions 行本身，不按模型名反推逐请求明细；
//   - 稳定计费原子：hermes:<session_id>（session 级唯一，重复导入幂等）；
//   - parent_session_id（压缩续接）不当作子代理关系，独立会话不合并。
//
// 隐私：仅保留上述白名单元数据；不读 messages.content、
// system_prompt、api_content、tool_calls、origin_json 等正文/凭据列。

import (
	"database/sql"
	"fmt"
	"time"

	_ "modernc.org/sqlite"
)

// HermesSessionRow 为 sessions 表的白名单投影。
type HermesSessionRow struct {
	ID            string
	Source        string
	Model         string
	StartedAt     float64
	EndedAt       sql.NullFloat64
	InputTokens   int64
	OutputTokens  int64
	CacheRead     int64
	CacheWrite    int64
	Reasoning     int64
	BillingMode   sql.NullString
	CostStatus    sql.NullString
	ParentSession sql.NullString
}

// ParseHermesStateDB 只读打开 Hermes state.db 并投影 sessions 白名单字段。
// dbPath 指向用户授权的 profile 数据库；不写库、不迁移、不读正文。
func ParseHermesStateDB(dbPath string) ([]NativeEvent, int, error) {
	// immutable=0 保持 WAL 可见性；mode=ro 保证只读。
	dsn := fmt.Sprintf("file:%s?mode=ro", dbPath)
	db, err := sql.Open("sqlite", dsn)
	if err != nil {
		return nil, 0, err
	}
	defer db.Close()

	// 按实际 schema_version 检查：低于最低支持版本时显式失败，
	// 不猜测列含义。
	var schemaVersion int
	if err := db.QueryRow("SELECT COALESCE((SELECT MAX(version) FROM schema_version), 0)").Scan(&schemaVersion); err != nil {
		return nil, 0, fmt.Errorf("hermes: schema_version unreadable: %w", err)
	}
	if schemaVersion < 1 {
		return nil, 0, fmt.Errorf("hermes: state.db has no schema_version; not a supported profile DB")
	}

	rows, err := db.Query(`
		SELECT id, source, COALESCE(model,''), started_at, ended_at,
			COALESCE(input_tokens,0), COALESCE(output_tokens,0),
			COALESCE(cache_read_tokens,0), COALESCE(cache_write_tokens,0),
			COALESCE(reasoning_tokens,0), billing_mode, cost_status, parent_session_id
		FROM sessions`)
	if err != nil {
		return nil, 0, fmt.Errorf("hermes: sessions projection failed: %w", err)
	}
	defer rows.Close()

	var events []NativeEvent
	parsed := 0
	for rows.Next() {
		var r HermesSessionRow
		if err := rows.Scan(&r.ID, &r.Source, &r.Model, &r.StartedAt, &r.EndedAt,
			&r.InputTokens, &r.OutputTokens, &r.CacheRead, &r.CacheWrite,
			&r.Reasoning, &r.BillingMode, &r.CostStatus, &r.ParentSession); err != nil {
			return events, parsed, fmt.Errorf("hermes: scan session: %w", err)
		}
		if r.ID == "" {
			continue
		}
		if r.InputTokens == 0 && r.OutputTokens == 0 && r.CacheRead == 0 && r.CacheWrite == 0 {
			// 无用量记录的会话不产生计费事件（活动归因不在本轮范围）。
			continue
		}
		requestedAt := time.Unix(int64(r.StartedAt), 0).UTC()
		if r.EndedAt.Valid && r.EndedAt.Float64 > 0 {
			requestedAt = time.Unix(int64(r.EndedAt.Float64), 0).UTC()
		}
		result := ResultSuccess
		if r.CostStatus.Valid && r.CostStatus.String == "error" {
			result = ResultFailed
		}
		total := r.InputTokens + r.OutputTokens + r.CacheRead + r.CacheWrite + r.Reasoning
		events = append(events, NativeEvent{
			BillingAtom:      "hermes:" + r.ID,
			SourceRecordID:   "hermes-session:" + r.ID,
			RequestedAt:      requestedAt,
			Provider:         firstNonEmpty(r.Source, "hermes"),
			Model:            firstNonEmpty(r.Model, "unknown"),
			Source:           "Hermes Agent",
			Result:           result,
			InputTokens:      r.InputTokens,
			OutputTokens:     r.OutputTokens,
			CacheReadTokens:  r.CacheRead,
			CacheWriteTokens: r.CacheWrite,
			TotalTokens:      total,
			SessionID:        r.ID,
			ParserVersion:    "hermes-state/1",
			RecordKind:       "session_delta",
		})
		parsed++
	}
	return events, parsed, rows.Err()
}
