package usage

// Codex 单文件增量采集：日志上下文、累计基线与事件／游标事务。

import (
	"context"
	"database/sql"
	"fmt"
	"io"
	"os"
	"strings"
	"time"
)

func (s *Store) collectCodexFile(ctx context.Context, filePath string, now time.Time, instanceID string) (int64, int64, error) {
	if err := ctx.Err(); err != nil {
		return 0, 0, err
	}
	info, err := os.Stat(filePath)
	if err != nil {
		return 0, 0, err
	}
	if info.Size() == 0 {
		return 0, 0, nil
	}
	fpHash := fileFingerprint(filePath)
	offset, bl, err := s.getCodexBaseline("codex", fpHash)
	if err != nil {
		return 0, 0, err
	}
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
	recoveryBytes := int64(0)
	if offset == 0 {
		bl = codexCursorBaseline{}
	} else if bl.Session == "" || (bl.ParserVersion != "codex-sessions/2" && !bl.Responses) {
		// 首次升级解析已提交前缀，恢复上下文并识别新版计量器，不重入库。
		if offset > maxTotalBytesPerSource {
			return 0, 0, fmt.Errorf("codex baseline recovery exceeds source byte budget")
		}
		var recoveredBL codexCursorBaseline
		for recovered := int64(0); recovered < offset; {
			if err := ctx.Err(); err != nil {
				return 0, recovered, err
			}
			prefix, next, err := readBoundedLines(file, recovered, offset)
			if err != nil {
				return 0, recovered, err
			}
			if next == recovered {
				return 0, recovered, io.ErrUnexpectedEOF
			}
			_, _, _, recoveredBL = ParseCodexSessionsWithBaseline(prefix, now, recoveredBL, fpHash)
			recovered = next
			recoveryBytes = recovered
		}
		if bl.Session == "" || recoveredBL.Responses {
			bl = recoveredBL
		}
	}

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

	bytesRead := recoveryBytes + int64(len(validBuf))
	events, _, skipped, lastBL := ParseCodexSessionsWithBaseline(validBuf, now, bl, fpHash)
	s.mu.Lock()
	defer s.mu.Unlock()
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return 0, bytesRead, err
	}
	defer tx.Rollback()
	var imported int64
	for _, event := range events {
		if err := ctx.Err(); err != nil {
			return 0, bytesRead, err
		}
		// 保留旧快照 atom 的历史记录，升级后归档重读不能重复计量。
		if strings.Contains(event.BillingAtom, ":snap-") {
			var exists bool
			err := tx.QueryRowContext(ctx, `SELECT EXISTS(SELECT 1 FROM usage_events
				WHERE tool_id='codex' AND source_instance_id=? AND session_id=? AND requested_at=?
				AND billing_atom GLOB ?)`, instanceID, event.SessionID, event.RequestedAt.Format(time.RFC3339Nano),
				"codex:*:snap-"+strings.Repeat("?", 64)+"-*").Scan(&exists)
			if err != nil {
				return 0, bytesRead, err
			}
			if exists {
				continue
			}
		}
		inserted, err := insertEventIfAbsent(withIdentity(nativeEventToEvent(event), "codex", instanceID), func(query string, args ...any) (sql.Result, error) { return tx.ExecContext(ctx, query, args...) })
		if err != nil {
			return 0, bytesRead, err
		}
		if inserted {
			imported++
		}
	}
	// 累计基线与文件游标同事务持久化（§3.2）：分批续读对上一批求差。
	if err := saveCodexBaseline(tx, "codex", fpHash, newOffset, lastBL); err != nil {
		return 0, bytesRead, err
	}
	if err := tx.Commit(); err != nil {
		return 0, bytesRead, err
	}
	if skipped > 0 {
		return imported, bytesRead, fmt.Errorf("codex log contains %d invalid records", skipped)
	}
	return imported, bytesRead, nil
}

// getCodexBaseline 读取该文件游标上持久化的 Codex 累计基线。
func (s *Store) getCodexBaseline(sourceID, fp string) (int64, codexCursorBaseline, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	var bl codexCursorBaseline
	var offset int64
	var parserVersion string
	err := s.db.QueryRow(`SELECT committed_offset, baseline_session, baseline_model,
        baseline_in, baseline_cache, baseline_out, baseline_epoch, parser_version
        FROM usage_import_cursors WHERE source_id=? AND file_fingerprint=?`, sourceID, fp).
		Scan(&offset, &bl.Session, &bl.Model, &bl.In, &bl.Cache, &bl.Out, &bl.Epoch, &parserVersion)
	bl.Responses = parserVersion == "codex-responses/1"
	bl.ParserVersion = parserVersion
	if err == sql.ErrNoRows {
		err = nil
	}
	return offset, bl, err
}

// saveCodexBaseline 持久化该文件游标上最新累计基线（与游标写入同表）。
func saveCodexBaseline(tx *sql.Tx, sourceID, fp string, offset int64, bl codexCursorBaseline) error {
	parserVersion := "codex-sessions/2"
	if bl.Responses {
		parserVersion = "codex-responses/1"
	}
	_, err := tx.Exec(
		`INSERT INTO usage_import_cursors
		 (source_id, file_fingerprint, committed_offset, parser_version, baseline_session, baseline_model,
		  baseline_in, baseline_cache, baseline_out, baseline_epoch)
		 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
		 ON CONFLICT(source_id, file_fingerprint) DO UPDATE SET
		 committed_offset = excluded.committed_offset, parser_version = excluded.parser_version, baseline_session = excluded.baseline_session,
		 baseline_model = excluded.baseline_model, baseline_in = excluded.baseline_in,
		 baseline_cache = excluded.baseline_cache, baseline_out = excluded.baseline_out,
		 baseline_epoch = excluded.baseline_epoch, updated_at = datetime('now')`,
		sourceID, fp, offset, parserVersion, bl.Session, bl.Model, bl.In, bl.Cache, bl.Out, bl.Epoch,
	)
	return err
}
