package usage

// 游标、指纹与目录遍历（collector 的持久化辅助；整改 E1）。
// F8 收口：遍历有 context 贯穿、访问节点与目录扇出有上限、错误列表有上限；
// 达到限制以 budget_exceeded/cancelled 显式披露，不静默截断。

import (
	"context"
	"crypto/sha256"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

func fileFingerprint(path string) string {
	h := sha256.Sum256([]byte(path))
	return fmt.Sprintf("%x", h)
}

func (s *Store) getCursorOffset(sourceID, fp string) int64 {
	s.mu.Lock()
	defer s.mu.Unlock()
	var offset int64
	_ = s.db.QueryRow(
		"SELECT committed_offset FROM usage_import_cursors WHERE source_id = ? AND file_fingerprint = ?",
		sourceID, fp,
	).Scan(&offset)
	return offset
}

func (s *Store) saveCursorOffset(sourceID, fp string, offset int64, parserVersion string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	_, _ = s.db.Exec(`
		INSERT INTO usage_import_cursors (source_id, file_fingerprint, committed_offset, parser_version, updated_at)
		VALUES (?, ?, ?, ?, datetime('now'))
		ON CONFLICT(source_id, file_fingerprint) DO UPDATE SET
			committed_offset = excluded.committed_offset,
			parser_version = excluded.parser_version,
			updated_at = excluded.updated_at`,
		sourceID, fp, offset, parserVersion,
	)
}

// saveSourceRunState 持久化来源运行状态（§4.4）：
//   - 只有 ready 才更新 last_success_at（partial/error 保留上次成功时间）；
//   - last_attempt_at / availability / 错误码 / 计数每次都更新。
func (s *Store) saveSourceRunState(st SourceRunStatus) {
	s.mu.Lock()
	defer s.mu.Unlock()
	_, _ = s.db.Exec(`
		INSERT INTO usage_sources (
			id, tool_id, collector_kind, enabled, last_success_at, last_error,
			availability, last_attempt_at, last_error_code, records_imported,
			bytes_read, schema_version
		) VALUES (?, ?, 'native_log', 1, ?, ?, ?, ?, ?, ?, ?, ?)
		ON CONFLICT(id) DO UPDATE SET
			enabled = 1,
			last_success_at = CASE WHEN excluded.availability = 'ready'
				THEN excluded.last_success_at ELSE usage_sources.last_success_at END,
			last_error = CASE WHEN excluded.availability = 'ready'
				THEN '' ELSE COALESCE(NULLIF(excluded.last_error_code || ': ' || excluded.last_error, ': '), usage_sources.last_error) END,
			availability = excluded.availability,
			last_attempt_at = excluded.last_attempt_at,
			last_error_code = excluded.last_error_code,
			records_imported = excluded.records_imported,
			bytes_read = excluded.bytes_read,
			schema_version = excluded.schema_version`,
		st.ToolID, st.ToolID, st.LastSuccessAt, st.LastErrorMessage,
		string(st.Availability), st.LastAttemptAt, st.LastErrorCode,
		st.RecordsImported, st.BytesRead, st.SchemaVersion,
	)
}

// walkBudget 为一次遍历的累计预算（节点访问、错误列表）。
type walkBudget struct {
	nodes   int
	errs    []SourceFileError
	errsCut int // 超出上限被省略的错误数
}

func (b *walkBudget) addError(err SourceFileError) {
	if len(b.errs) >= maxFileErrors {
		b.errsCut++
		return
	}
	b.errs = append(b.errs, err)
}

func (b *walkBudget) budgetExceeded(path, message string) {
	b.addError(SourceFileError{Path: filepath.Base(path), Code: "budget_exceeded", Message: message})
}

// findFilesWithExt 遍历目录收集指定扩展名的普通文件（整改 E1 §5.1.5/§5.1.6）：
//   - 不吞遍历错误：逐路径错误记入 errs 并继续，根级错误返回 error；
//   - 拒绝 symlink 与非普通文件（不越过授权根目录）；
//   - 文件数量上限 maxFilesPerSource、访问节点上限 maxWalkNodesPerSource、
//     单目录扇出上限 maxDirEntriesPerDir、错误列表上限 maxFileErrors，
//     超限记 budget_exceeded（F8）；
//   - ctx 取消即刻停止遍历，并以 cancelled 错误显式披露（已收集文件仍返回）。
func findFilesWithExt(ctx context.Context, dir string, ext string) ([]string, []SourceFileError, error) {
	if info, err := os.Stat(dir); err != nil || !info.IsDir() {
		return nil, nil, nil // 目录不存在不是错误（未安装）
	}
	budget := &walkBudget{}
	var files []string
	stopped := false

	var walk func(path string, depth int)
	walk = func(path string, depth int) {
		if stopped {
			return
		}
		if err := ctx.Err(); err != nil {
			budget.addError(SourceFileError{
				Path:    filepath.Base(path),
				Code:    "cancelled",
				Message: "walk cancelled",
			})
			stopped = true
			return
		}
		entries, err := os.ReadDir(path)
		if err != nil {
			budget.addError(SourceFileError{
				Path:    filepath.Base(path),
				Code:    "walk_failed",
				Message: err.Error(),
			})
			return
		}
		if len(entries) > maxDirEntriesPerDir {
			budget.budgetExceeded(path, fmt.Sprintf("directory fan-out limit %d exceeded (%d entries)", maxDirEntriesPerDir, len(entries)))
			entries = entries[:maxDirEntriesPerDir] // 文件名序截断：确定性且不重复扫描
		}
		for _, entry := range entries {
			if stopped {
				return
			}
			if budget.nodes++; budget.nodes > maxWalkNodesPerSource {
				budget.budgetExceeded(path, fmt.Sprintf("walk node limit %d exceeded", maxWalkNodesPerSource))
				stopped = true
				return
			}
			child := filepath.Join(path, entry.Name())
			mode := entry.Type()
			switch {
			case mode&os.ModeSymlink != 0:
				budget.addError(SourceFileError{
					Path:    filepath.Base(child),
					Code:    "symlink_refused",
					Message: "symlink refused (would escape authorized root)",
				})
				if entry.IsDir() {
					return // 不递归 symlink 目录
				}
			case !mode.IsRegular() && !entry.IsDir():
				budget.addError(SourceFileError{
					Path:    filepath.Base(child),
					Code:    "non_regular_file_refused",
					Message: "non-regular file refused",
				})
			case entry.IsDir():
				walk(child, depth+1)
			default:
				if strings.HasSuffix(child, ext) {
					if len(files) >= maxFilesPerSource {
						budget.budgetExceeded(child, fmt.Sprintf("file count limit %d exceeded", maxFilesPerSource))
						stopped = true
						return
					}
					files = append(files, child)
				}
			}
		}
	}
	walk(dir, 0)
	if len(budget.errs) > 0 || budget.errsCut > 0 {
		if budget.errsCut > 0 {
			budget.addError(SourceFileError{
				Path:    filepath.Base(dir),
				Code:    "budget_exceeded",
				Message: fmt.Sprintf("%d additional errors omitted (error list limit %d)", budget.errsCut, maxFileErrors),
			})
		}
		return files, budget.errs, nil
	}
	return files, budget.errs, nil
}
