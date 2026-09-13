package usage

// 自动扫描与增量采集已授权工具日志（方案 §6.2，ADR-0030 决策 6；整改 E1/E2）。
//
// 原则：
//   - 仅扫描已知的标准授权工具目录（~/.claude, ~/.codex 等）；
//   - 仅提取白名单用量元数据（Token/Model/Time/Session/ResponseID），不落盘 Prompt/代码/凭据；
//   - 增量游标（usage_import_cursors）记录文件指纹与读取位移，不重复读取；
//   - 单次扫描有界（每文件至多 10 MiB，单来源至多 maxFilesPerSource 个文件 /
//     maxTotalBytesPerSource），防止无界阻塞；
//   - 计费原子（billingAtom）唯一去重，Proxy 与原生日志多来源只计一次；
//   - 文件级错误显式聚合为 partial/error 状态（§4.4），不再吞错写"成功"；
//   - 拒绝越过授权根目录的 symlink 与非普通文件；
//   - 每个事件携带三元身份 (tool_id, source_instance_id, session_id)（§4.1），
//     实例 ID 由「工具 ID + 授权根目录规范路径」派生，数据库不保存原始路径。

import (
	"fmt"
	"os"
	"path/filepath"
	"time"
)

// CollectSummary 为一次采集汇总（含逐来源结构化状态）。
type CollectSummary struct {
	SourcesScanned int               `json:"sourcesScanned"`
	FilesProcessed int               `json:"filesProcessed"`
	EventsImported int64             `json:"eventsImported"`
	BytesRead      int64             `json:"bytesRead"`
	Sources        []SourceRunStatus `json:"sources,omitempty"`
}

// collectResult 为单来源一次采集的结果（错误显式聚合，不吞）。
type collectResult struct {
	present   bool // 授权根存在（未安装的来源不产生状态行）
	imported  int64
	bytesRead int64
	files     int // 实际读到内容的文件数
	fileErrs  []SourceFileError
	wholeErr  error // 整来源失败（目录/DB 不可读等）
}

// nativeSource 为一个真实采集来源：toolID 即矩阵 ID，parserVersion 是
// wire schema 冻结版本，rootParts 是授权根目录（相对 HOME）。
type nativeSource struct {
	toolID        string
	displayName   string
	parserVersion string
	rootParts     []string
	collect       func(s *Store, root string, now time.Time) collectResult
}

// nativeSources 为采集来源注册表（E0 真源）。HistoricalUsage=implemented
// 的矩阵声明必须与这里一致（sources_matrix_test 强制）。
var nativeSources = []nativeSource{
	{toolID: "claude-code", displayName: "Claude Code", parserVersion: "claude-jsonl/1", rootParts: []string{".claude"},
		collect: collectClaudeSource},
	{toolID: "codex", displayName: "Codex", parserVersion: "codex-sessions/1", rootParts: []string{".codex"},
		collect: collectCodexSource},
	{toolID: "pi", displayName: "Pi", parserVersion: "pi-session/1", rootParts: []string{".pi", "agent", "sessions"},
		collect: collectPiSource},
	{toolID: "hermes", displayName: "Hermes Agent", parserVersion: "hermes-state/1", rootParts: []string{".hermes"},
		collect: collectHermesSource},
	{toolID: "opencode", displayName: "OpenCode", parserVersion: "opencode-store/1", rootParts: []string{".local", "share", "opencode"},
		collect: collectOpenCodeSource},
	{toolID: "zcode", displayName: "ZCode", parserVersion: "zcode-rollout/1", rootParts: []string{".zcode"},
		collect: collectZCodeSource},
	{toolID: "kimi-code", displayName: "Kimi Code", parserVersion: "kimi-session/1", rootParts: []string{".kimi-code"},
		collect: collectKimiSource},
	{toolID: "atomcode", displayName: "AtomCode", parserVersion: "atomcode-session/1", rootParts: []string{".atomcode"},
		collect: collectAtomcodeSource},
}

// NativeCollectorToolIDs 返回有真实采集路径的工具 ID（矩阵测试对照用）。
func NativeCollectorToolIDs() []string {
	out := make([]string, 0, len(nativeSources))
	for _, src := range nativeSources {
		out = append(out, src.toolID)
	}
	return out
}

// CollectSources 扫描本机所有已授权已知工具的日志并增量入库。
// 采集只由显式 model_usage_collect 触发；来源注册表 nativeSources 是
// HistoricalUsage=implemented 声明的唯一真源。
func (s *Store) CollectSources() (*CollectSummary, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return nil, err
	}

	summary := &CollectSummary{}
	now := time.Now().UTC()

	for _, src := range nativeSources {
		root := filepath.Join(home, filepath.Join(src.rootParts...))
		res := src.collect(s, root, now)
		if !res.present {
			continue // 未安装：不产生运行状态行（矩阵声明为 unavailable）
		}
		s.recordSourceRun(summary, src, root, now, res)
	}

	// 不注入任何示例账单：空账户必须为空（方案 §7.5），
	// 示例账单只存在于测试 fixture 中。
	return summary, nil
}

// recordSourceRun 汇总单条来源：推导 availability、更新总计、持久化状态。
func (s *Store) recordSourceRun(summary *CollectSummary, src nativeSource, root string, now time.Time, res collectResult) {
	availability := mergeCollectOutcome(res.imported, res.files, res.fileErrs, res.wholeErr)
	status := SourceRunStatus{
		ToolID:          src.toolID,
		Availability:    availability,
		LastAttemptAt:   now.Format(time.RFC3339),
		RecordsImported: res.imported,
		BytesRead:       res.bytesRead,
		SchemaVersion:   src.parserVersion,
	}
	switch availability {
	case SrcReady:
		status.LastSuccessAt = now.Format(time.RFC3339)
	case SrcPartial:
		status.LastErrorCode = firstFileErrorCode(res.fileErrs)
		status.LastErrorMessage = firstFileErrorMessage(res.fileErrs)
		status.FileErrors = res.fileErrs
	case SrcError:
		if res.wholeErr != nil {
			status.LastErrorCode = "source_failed"
			status.LastErrorMessage = res.wholeErr.Error()
		} else {
			status.LastErrorCode = firstFileErrorCode(res.fileErrs)
			status.LastErrorMessage = firstFileErrorMessage(res.fileErrs)
			status.FileErrors = res.fileErrs
		}
	}
	summary.SourcesScanned++
	summary.FilesProcessed += res.files
	summary.EventsImported += res.imported
	summary.BytesRead += res.bytesRead
	summary.Sources = append(summary.Sources, status)
	s.saveSourceRunState(status)
}

func firstFileErrorCode(errs []SourceFileError) string {
	if len(errs) == 0 {
		return ""
	}
	return errs[0].Code
}

func firstFileErrorMessage(errs []SourceFileError) string {
	if len(errs) == 0 {
		return ""
	}
	if len(errs) == 1 {
		return errs[0].Message
	}
	return fmt.Sprintf("%s (+%d more)", errs[0].Message, len(errs)-1)
}

// withIdentity 为解析事件补三元身份（工具 ID 与本机实例 ID）。
func withIdentity(ev *Event, toolID, instanceID string) *Event {
	ev.ToolID = toolID
	ev.SourceInstanceID = instanceID
	return ev
}

// insertNativeEvents 批量入库解析事件（计费原子去重）。
func (s *Store) insertNativeEvents(events []NativeEvent, toolID, displayName, instanceID string) int64 {
	var imported int64
	for _, ne := range events {
		ev := withIdentity(nativeEventToEvent(ne), toolID, instanceID)
		ev.Source = displayName
		inserted, _ := s.InsertEventIfAbsent(ev)
		if inserted {
			imported++
		}
	}
	return imported
}
