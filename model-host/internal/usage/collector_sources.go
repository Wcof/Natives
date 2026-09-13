package usage

// 逐来源与单文件采集实现（collector.go 的注册表/状态机拆分；整改 E1/E2）。

import (
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"time"
)

// --- 逐来源实现 -----------------------------------------------------------

func collectClaudeSource(s *Store, root string, now time.Time) collectResult {
	var res collectResult
	info, err := os.Stat(root)
	if err != nil || !info.IsDir() {
		return res
	}
	res.present = true
	instance := deriveInstanceID("claude-code", root)
	projectsDir := filepath.Join(root, "projects")
	files, walkErrs, walkErr := findFilesWithExt(projectsDir, ".jsonl")
	res.fileErrs = append(res.fileErrs, walkErrs...)
	if walkErr != nil {
		res.wholeErr = walkErr
		return res
	}
	// history.jsonl 用 Lstat 拒绝 symlink（不得越过授权根目录）。
	historyFile := filepath.Join(root, "history.jsonl")
	if hInfo, herr := os.Lstat(historyFile); herr == nil && hInfo.Mode().IsRegular() {
		files = append(files, historyFile)
	}
	sortFilesNewestFirst(files)
	totalBytes := int64(0)
	for _, fp := range files {
		if err := checkSourceBudget(&totalBytes, fp); err != nil {
			res.fileErrs = append(res.fileErrs, *err)
			break
		}
		// 单文件内部分块续读：游标未到文件尾且预算未耗尽时继续
		// （10 MiB/批；超大 rollout 文件一次显式采集即可追平）。
		for {
			imported, bytes, err := s.collectClaudeFile(fp, now, instance)
			if err != nil {
				res.fileErrs = append(res.fileErrs, fileReadError(root, fp, err))
				break
			}
			if bytes > 0 {
				res.files++
				res.imported += imported
				res.bytesRead += bytes
				totalBytes += bytes
			}
			if bytes == 0 || totalBytes >= maxTotalBytesPerSource {
				break
			}
		}
	}
	return res
}

func collectCodexSource(s *Store, root string, now time.Time) collectResult {
	var res collectResult
	info, err := os.Stat(root)
	if err != nil || !info.IsDir() {
		return res
	}
	res.present = true
	instance := deriveInstanceID("codex", root)
	// sessions 与 archived_sessions 共用 codex-sessions/1 parser，稳定
	// billingAtom 去重；同一 session 移入 archived 后重复导入不重复计量。
	dirs := []string{filepath.Join(root, "sessions"), filepath.Join(root, "archived_sessions")}
	exts := []string{".jsonl", ".json"}
	var files []string
	for _, dir := range dirs {
		for _, ext := range exts {
			found, walkErrs, walkErr := findFilesWithExt(dir, ext)
			res.fileErrs = append(res.fileErrs, walkErrs...)
			if walkErr != nil {
				res.wholeErr = walkErr
				return res
			}
			files = append(files, found...)
		}
	}
	sortFilesNewestFirst(files)
	totalBytes := int64(0)
	for _, fp := range files {
		if err := checkSourceBudget(&totalBytes, fp); err != nil {
			res.fileErrs = append(res.fileErrs, *err)
			break
		}
		for {
			imported, bytes, err := s.collectCodexFile(fp, now, instance)
			if err != nil {
				res.fileErrs = append(res.fileErrs, fileReadError(root, fp, err))
				break
			}
			if bytes > 0 {
				res.files++
				res.imported += imported
				res.bytesRead += bytes
				totalBytes += bytes
			}
			if bytes == 0 || totalBytes >= maxTotalBytesPerSource {
				break
			}
		}
	}
	return res
}

func collectPiSource(s *Store, root string, now time.Time) collectResult {
	return collectJSONLSessionDir(s, root, "pi", "Pi", s.collectPiFile, now)
}

func collectKimiSource(s *Store, root string, now time.Time) collectResult {
	return collectJSONLSessionDir(s, filepath.Join(root, "sessions"), "kimi-code", "Kimi Code", s.collectKimiFile, now)
}

func collectAtomcodeSource(s *Store, root string, now time.Time) collectResult {
	return collectJSONLSessionDir(s, filepath.Join(root, "sessions"), "atomcode", "AtomCode", s.collectAtomcodeFile, now)
}

// collectJSONLSessionDir 为「目录下每 .jsonl 一个会话」来源的通用扫描。
func collectJSONLSessionDir(s *Store, dir, toolID, displayName string, collectFile func(string, time.Time, string) (int64, int64, error), now time.Time) collectResult {
	var res collectResult
	if info, err := os.Stat(dir); err != nil || !info.IsDir() {
		return res
	}
	res.present = true
	instance := deriveInstanceID(toolID, dir)
	files, walkErrs, walkErr := findFilesWithExt(dir, ".jsonl")
	res.fileErrs = append(res.fileErrs, walkErrs...)
	if walkErr != nil {
		res.wholeErr = walkErr
		return res
	}
	sortFilesNewestFirst(files)
	totalBytes := int64(0)
	for _, fp := range files {
		if err := checkSourceBudget(&totalBytes, fp); err != nil {
			res.fileErrs = append(res.fileErrs, *err)
			break
		}
		for {
			imported, bytes, err := collectFile(fp, now, instance)
			if err != nil {
				res.fileErrs = append(res.fileErrs, fileReadError(dir, fp, err))
				break
			}
			if bytes > 0 {
				res.files++
				res.imported += imported
				res.bytesRead += bytes
				totalBytes += bytes
			}
			if bytes == 0 || totalBytes >= maxTotalBytesPerSource {
				break
			}
		}
	}
	return res
}

func collectHermesSource(s *Store, root string, now time.Time) collectResult {
	var res collectResult
	dbPath := filepath.Join(root, "state.db")
	info, err := os.Stat(dbPath)
	if err != nil || info.IsDir() {
		return res
	}
	res.present = true
	imported, bytes, err := s.collectHermesDB(dbPath)
	if err != nil {
		res.wholeErr = err
		return res
	}
	if bytes > 0 {
		res.files = 1
		res.imported = imported
		res.bytesRead = bytes
	}
	return res
}

func collectOpenCodeSource(s *Store, root string, now time.Time) collectResult {
	var res collectResult
	dbPath := filepath.Join(root, "opencode.db")
	info, err := os.Stat(dbPath)
	if err != nil || info.IsDir() {
		return res
	}
	res.present = true
	imported, bytes, err := s.collectOpenCodeDB(dbPath)
	if err != nil {
		res.wholeErr = err
		return res
	}
	if bytes > 0 {
		res.files = 1
		res.imported = imported
		res.bytesRead = bytes
	}
	return res
}

func collectZCodeSource(s *Store, root string, now time.Time) collectResult {
	var res collectResult
	rolloutDir := filepath.Join(root, "cli", "rollout")
	if info, err := os.Stat(rolloutDir); err != nil || !info.IsDir() {
		return res
	}
	res.present = true
	imported, bytes, err := s.collectZCodeRollout(rolloutDir)
	if err != nil {
		res.wholeErr = err
		return res
	}
	if bytes > 0 {
		res.files = 1
		res.imported = imported
		res.bytesRead = bytes
	}
	return res
}

// checkSourceBudget 校验单来源本次扫描的累计读取上限（按实际已读字节计）；
// 超限返回 budget_exceeded 错误。单个超大文件可以在多次显式采集中分块续读
// （10 MiB/批，游标推进），本检查只保证单次运行有界。
func checkSourceBudget(totalBytes *int64, nextFile string) *SourceFileError {
	if *totalBytes >= maxTotalBytesPerSource {
		return &SourceFileError{
			Path:    filepath.Base(nextFile),
			Code:    "budget_exceeded",
			Message: fmt.Sprintf("source read budget %d bytes exceeded", maxTotalBytesPerSource),
		}
	}
	return nil
}

// fileReadError 把文件级读取/解析错误归一为 SourceFileError；
// 路径只保留授权根下的相对形式，不落盘绝对路径。
func fileReadError(root, filePath string, err error) SourceFileError {
	rel := filePath
	if r, rerr := filepath.Rel(root, filePath); rerr == nil && !strings.HasPrefix(r, "..") {
		rel = r
	}
	code := "read_failed"
	switch {
	case errors.Is(err, fs.ErrPermission):
		code = "permission_denied"
	case errors.Is(err, fs.ErrNotExist):
		code = "vanished"
	}
	return SourceFileError{Path: rel, Code: code, Message: err.Error()}
}

// sortFilesNewestFirst 把文件按路径降序排序（日期嵌套目录的路径字典序
// 降序 ≈ 新到旧），让预算优先服务最近产生的可计量文件，历史无 usage 的
// 大文件不阻塞近期数据导入。
func sortFilesNewestFirst(files []string) {
	for i := 1; i < len(files); i++ {
		for j := i; j > 0 && files[j] > files[j-1]; j-- {
			files[j], files[j-1] = files[j-1], files[j]
		}
	}
}

// --- 单文件增量读取 -------------------------------------------------------

// collectPiFile 增量读取一个 Pi 会话 JSONL；session ID 取自文件名尾段
// （<timestamp>_<session-id>.jsonl，官方格式），游标复用既有 usage_import_cursors。
func (s *Store) collectPiFile(filePath string, now time.Time, instanceID string) (int64, int64, error) {
	info, err := os.Stat(filePath)
	if err != nil || info.Size() == 0 {
		return 0, 0, nil
	}
	fpHash := fileFingerprint(filePath)
	offset := s.getCursorOffset("pi", fpHash)
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

	// 官方文件名格式：<timestamp>_<session-id>.jsonl；session ID 即 .jsonl 前的最后一段。
	base := strings.TrimSuffix(filepath.Base(filePath), ".jsonl")
	sessID := base
	if idx := strings.LastIndex(base, "_"); idx >= 0 && idx+1 < len(base) {
		sessID = base[idx+1:]
	}

	events, _, _ := ParsePiSessionJSONL(validBuf, sessID, now)
	imported := s.commitParsedEvents(events, "pi", "Pi", "pi-session/1", instanceID, fpHash, newOffset)
	return imported, int64(len(validBuf)), nil
}

// collectKimiFile 增量读取一个 Kimi Code 会话 JSONL（kimi-session/1）。
// 游标复用既有 usage_import_cursors；半行不提交 offset（§6.2）。
func (s *Store) collectKimiFile(filePath string, now time.Time, instanceID string) (int64, int64, error) {
	info, err := os.Stat(filePath)
	if err != nil || info.Size() == 0 {
		return 0, 0, nil
	}
	fpHash := fileFingerprint(filePath)
	offset := s.getCursorOffset("kimi-code", fpHash)
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

	// 文件名即会话标识（<session-id>.jsonl）。
	sessID := strings.TrimSuffix(filepath.Base(filePath), ".jsonl")

	events, _, _ := ParseKimiSessionJSONL(validBuf, sessID, now)
	imported := s.commitParsedEvents(events, "kimi-code", "Kimi Code", "kimi-session/1", instanceID, fpHash, newOffset)
	return imported, int64(len(validBuf)), nil
}

// collectAtomcodeFile 增量读取一个 AtomCode 会话 JSONL（atomcode-session/1，
// 路径 ~/.atomcode/sessions/<projectHash>/<uuid>.jsonl）。游标复用既有
// usage_import_cursors；半行不提交 offset（§6.2）；会话 ID 取文件名尾段 uuid，
// 同 turn 流式多行只产出终态事件，计费原子 atomcode:<session>:<turn> 幂等。
func (s *Store) collectAtomcodeFile(filePath string, now time.Time, instanceID string) (int64, int64, error) {
	info, err := os.Stat(filePath)
	if err != nil || info.Size() == 0 {
		return 0, 0, nil
	}
	fpHash := fileFingerprint(filePath)
	offset := s.getCursorOffset("atomcode", fpHash)
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

	// 文件名即会话标识（<uuid>.jsonl）。
	sessID := strings.TrimSuffix(filepath.Base(filePath), ".jsonl")

	events, _, _ := ParseAtomcodeSessionJSONL(validBuf, sessID, now)
	imported := s.commitParsedEvents(events, "atomcode", "AtomCode", "atomcode-session/1", instanceID, fpHash, newOffset)
	return imported, int64(len(validBuf)), nil
}

// collectOpenCodeDB 采集 OpenCode 本地 SQLite（只读白名单投影）。
// 计量原子 opencode:<message.id> 幂等；返回导入事件数与读取字节。
func (s *Store) collectOpenCodeDB(dbPath string) (int64, int64, error) {
	info, err := os.Stat(dbPath)
	if err != nil || info.Size() == 0 {
		return 0, 0, nil
	}
	events, parsed, err := ParseOpenCodeDB(dbPath)
	if err != nil {
		return 0, 0, err
	}
	if parsed == 0 {
		return 0, 0, nil
	}
	var importedCount int64
	for _, ne := range events {
		ev := withIdentity(nativeEventToEvent(ne), "opencode", deriveInstanceID("opencode", dbPath))
		ev.Source = "OpenCode"
		inserted, _ := s.InsertEventIfAbsent(ev)
		if inserted {
			importedCount++
		}
	}
	return importedCount, info.Size(), nil
}

// collectHermesDB 只读导入 Hermes state.db 的 session 级用量（幂等：
// billingAtom=hermes:<session>，重复导入被唯一索引去重）。
func (s *Store) collectHermesDB(dbPath string) (int64, int64, error) {
	info, err := os.Stat(dbPath)
	if err != nil || info.Size() == 0 {
		return 0, 0, nil
	}
	events, parsed, err := ParseHermesStateDB(dbPath)
	if err != nil {
		return 0, 0, err
	}
	if parsed == 0 {
		return 0, 0, nil
	}
	var importedCount int64
	for _, ne := range events {
		ev := withIdentity(nativeEventToEvent(ne), "hermes", deriveInstanceID("hermes", dbPath))
		ev.Source = "Hermes Agent"
		inserted, _ := s.InsertEventIfAbsent(ev)
		if inserted {
			importedCount++
		}
	}
	return importedCount, info.Size(), nil
}

func (s *Store) collectClaudeFile(filePath string, now time.Time, instanceID string) (int64, int64, error) {
	info, err := os.Stat(filePath)
	if err != nil || info.Size() == 0 {
		return 0, 0, nil
	}
	fpHash := fileFingerprint(filePath)
	offset := s.getCursorOffset("claude-code", fpHash)
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

	events, _, _ := ParseClaudeJSONL(validBuf, now)
	imported := s.commitParsedEvents(events, "claude-code", "Claude Code", "claude-jsonl/1", instanceID, fpHash, newOffset)
	return imported, int64(len(validBuf)), nil
}

func (s *Store) collectCodexFile(filePath string, now time.Time, instanceID string) (int64, int64, error) {
	info, err := os.Stat(filePath)
	if err != nil || info.Size() == 0 {
		return 0, 0, nil
	}
	fpHash := fileFingerprint(filePath)
	offset := s.getCursorOffset("codex", fpHash)
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

	events, _, _, lastBL := ParseCodexSessionsWithBaseline(validBuf, now, s.getCodexBaseline("codex", fpHash), fpHash)
	imported := s.insertNativeEvents(events, "codex", "Codex", instanceID)
	// 累计基线与文件游标同事务持久化（§3.2）：分批续读对上一批求差。
	s.saveCodexBaseline("codex", fpHash, lastBL)
	s.saveCursorOffset("codex", fpHash, newOffset, "codex-sessions/1")
	return imported, int64(len(validBuf)), nil
}

// readBoundedLines 有界读取（单批至多 10 MiB）并在最后一个换行处截断，
// 半行不提交（游标不得越过未完成记录，§6.2/§11.1）。
func readBoundedLines(file *os.File, offset, size int64) ([]byte, int64, error) {
	limit := int64(10 * 1024 * 1024)
	toRead := size - offset
	if toRead > limit {
		toRead = limit
	}
	buf := make([]byte, toRead)
	n, err := io.ReadFull(file, buf)
	if err != nil && err != io.EOF && err != io.ErrUnexpectedEOF {
		return nil, 0, err
	}
	if n == 0 {
		return nil, 0, nil
	}
	buf = buf[:n]

	lastNL := -1
	for i := len(buf) - 1; i >= 0; i-- {
		if buf[i] == '\n' {
			lastNL = i
			break
		}
	}
	if lastNL < 0 {
		return nil, 0, nil
	}
	return buf[:lastNL+1], offset + int64(lastNL+1), nil
}

// commitParsedEvents 入库解析事件并提交游标（游标只在成功提交后推进）。
func (s *Store) commitParsedEvents(events []NativeEvent, toolID, displayName, parserVersion, instanceID, fpHash string, newOffset int64) int64 {
	imported := s.insertNativeEvents(events, toolID, displayName, instanceID)
	s.saveCursorOffset(toolID, fpHash, newOffset, parserVersion)
	return imported
}

// getCodexBaseline 读取该文件游标上持久化的 Codex 累计基线。
func (s *Store) getCodexBaseline(sourceID, fp string) codexCursorBaseline {
	s.mu.Lock()
	defer s.mu.Unlock()
	var bl codexCursorBaseline
	_ = s.db.QueryRow(
		`SELECT baseline_in, baseline_cache, baseline_out, baseline_epoch
		 FROM usage_import_cursors WHERE source_id = ? AND file_fingerprint = ?`,
		sourceID, fp,
	).Scan(&bl.In, &bl.Cache, &bl.Out, &bl.Epoch)
	return bl
}

// saveCodexBaseline 持久化该文件游标上最新累计基线（与游标写入同表）。
func (s *Store) saveCodexBaseline(sourceID, fp string, bl codexCursorBaseline) {
	s.mu.Lock()
	defer s.mu.Unlock()
	_, _ = s.db.Exec(
		`UPDATE usage_import_cursors
		 SET baseline_in = ?, baseline_cache = ?, baseline_out = ?, baseline_epoch = ?
		 WHERE source_id = ? AND file_fingerprint = ?`,
		bl.In, bl.Cache, bl.Out, bl.Epoch, sourceID, fp,
	)
}
