package usage

// 游标、指纹与目录遍历（collector 的持久化辅助；整改 E1）。

import (
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

// findFilesWithExt 遍历目录收集指定扩展名的普通文件（整改 E1 §5.1.5/§5.1.6）：
//   - 不吞 filepath.Walk 错误：逐路径错误记入 fileErrs 并继续，根级错误返回 error；
//   - 拒绝 symlink 与非普通文件（不越过授权根目录）；
//   - 文件数量上限 maxFilesPerSource，超限记 budget_exceeded。
func findFilesWithExt(dir string, ext string) ([]string, []SourceFileError, error) {
	var files []string
	var errs []SourceFileError
	if info, err := os.Stat(dir); err != nil || !info.IsDir() {
		return nil, nil, nil // 目录不存在不是错误（未安装）
	}
	walkErr := filepath.Walk(dir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			errs = append(errs, SourceFileError{
				Path:    filepath.Base(path),
				Code:    "walk_failed",
				Message: err.Error(),
			})
			return nil // 单路径失败不终止整棵树（partial 语义）
		}
		if info == nil {
			return nil
		}
		mode := info.Mode()
		switch {
		case mode&os.ModeSymlink != 0:
			errs = append(errs, SourceFileError{
				Path:    filepath.Base(path),
				Code:    "symlink_refused",
				Message: "symlink refused (would escape authorized root)",
			})
			if info.IsDir() {
				return filepath.SkipDir
			}
			return nil
		case !mode.IsRegular():
			if !info.IsDir() {
				errs = append(errs, SourceFileError{
					Path:    filepath.Base(path),
					Code:    "non_regular_file_refused",
					Message: "non-regular file refused",
				})
			}
			return nil
		case info.IsDir():
			return nil
		}
		if strings.HasSuffix(path, ext) {
			if len(files) >= maxFilesPerSource {
				errs = append(errs, SourceFileError{
					Path:    filepath.Base(path),
					Code:    "budget_exceeded",
					Message: fmt.Sprintf("file count limit %d exceeded", maxFilesPerSource),
				})
				return filepath.SkipAll
			}
			files = append(files, path)
		}
		return nil
	})
	if walkErr != nil {
		return files, errs, walkErr
	}
	return files, errs, nil
}
