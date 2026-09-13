package usage

// 来源运行状态（整改 E1，方案 §4.4/§5.1）：每个采集来源返回显式、可恢复
// 的状态，禁止吞错后写"成功"。

import (
	"crypto/sha256"
	"encoding/hex"
	"path/filepath"
)

// SourceAvailability 为来源可用性词表。
type SourceAvailability string

const (
	SrcReady       SourceAvailability = "ready"       // 来源可用；本轮扫描全部成功（0 条记录也是 ready）
	SrcPartial     SourceAvailability = "partial"     // 部分文件/记录成功、部分失败；成功数据已提交
	SrcUnavailable SourceAvailability = "unavailable" // 未安装/未授权
	SrcUnsupported SourceAvailability = "unsupported" // 经审计确认无结构化来源
	SrcError       SourceAvailability = "error"       // 全部失败；不得更新 lastSuccessAt
)

// SourceFileError 为单文件级失败（保留失败列表，不撤销已提交数据）。
type SourceFileError struct {
	Path    string `json:"path"` // 相对授权根目录的路径，不落盘绝对路径
	Code    string `json:"code"` // permission_denied / read_failed / parse_failed / symlink_refused / budget_exceeded / walk_failed
	Message string `json:"message"`
}

// SourceRunStatus 为一次采集后的来源状态（方案 §4.4 字段）。
type SourceRunStatus struct {
	ToolID           string             `json:"toolId"`
	Availability     SourceAvailability `json:"availability"`
	LastAttemptAt    string             `json:"lastAttemptAt"`
	LastSuccessAt    string             `json:"lastSuccessAt,omitempty"`
	LastErrorCode    string             `json:"lastErrorCode,omitempty"`
	LastErrorMessage string             `json:"lastErrorMessage,omitempty"`
	RecordsImported  int64              `json:"recordsImported"`
	BytesRead        int64              `json:"bytesRead"`
	SchemaVersion    string             `json:"schemaVersion,omitempty"`
	SourceVersion    string             `json:"sourceVersion,omitempty"`
	FileErrors       []SourceFileError  `json:"fileErrors,omitempty"`
}

// 采集边界（ADR-0030 决策 6 的显式上限）：
const (
	// maxFilesPerSource 为单来源单次扫描的文件数上限；超限记 budget_exceeded。
	maxFilesPerSource = 5000
	// maxTotalBytesPerSource 为单来源单次扫描的读取总量上限（按实际读取
	// 字节计，不按文件大小预判——否则超大单文件永远无法启动导入）。
	maxTotalBytesPerSource = 512 << 20 // 512 MiB
)

// deriveInstanceID 由「工具 ID + 授权根目录规范路径」派生本机稳定的实例 ID
// （方案 §4.1）。数据库不保存原始路径；同一根目录跨重启稳定，不同
// profile/安装实例互不相同。
func deriveInstanceID(toolID, rootPath string) string {
	cleaned := rootPath
	if resolved, err := filepath.Abs(rootPath); err == nil {
		cleaned = resolved
	}
	sum := sha256.Sum256([]byte("natives-instance:" + toolID + ":" + cleaned))
	return "inst-" + hex.EncodeToString(sum[:])[:16]
}

// mergeCollectOutcome 由收集结果推导 availability（§4.4 状态规则）：
//   - 全部文件成功（含 0 文件）→ ready；
//   - 有成功也有失败 → partial；
//   - 有文件但全部失败，或整来源失败 → error。
func mergeCollectOutcome(imported int64, files int, fileErrs []SourceFileError, wholeErr error) SourceAvailability {
	if wholeErr != nil {
		return SrcError
	}
	switch {
	case len(fileErrs) == 0:
		return SrcReady
	case imported > 0 || files > 0:
		return SrcPartial
	default:
		return SrcError
	}
}
