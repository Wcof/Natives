package agentclients

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"
)

// ProviderID is the managed provider key written into every client config.
const ProviderID = "natives-gateway"

// ProviderName is the display name shown inside each client's provider list.
const ProviderName = "Natives Gateway"

// StateSuffix marks files managed by this tool.
const StateSuffix = ".natives.state.json"

// BackupRecord tracks one rolled-aside original file.
type BackupRecord struct {
	Path          string `json:"path"`
	BackupPath    string `json:"backupPath"`
	ExistedBefore bool   `json:"existedBefore"`
}

// ManagedState is the on-disk journal enabling 配置修改/默认配置/关闭配置修改.
type ManagedState struct {
	Version                int            `json:"version"`
	Client                 string         `json:"client"`
	Model                  string         `json:"model"`
	ConfigurationRevision  int            `json:"configurationRevision"`
	BackupFiles            []BackupRecord `json:"backupFiles"`
	UpdatedAtUnix          int64          `json:"updatedAtUnix"`
}

// StatePath returns the journal path for a client's primary config file.
func StatePath(primaryConfig string) string {
	return primaryConfig + StateSuffix
}

// ReadStateFile loads the journal, returning nil when the client is unmanaged.
func ReadStateFile(primaryConfig string) (*ManagedState, error) {
	data, err := os.ReadFile(StatePath(primaryConfig))
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return nil, nil
		}
		return nil, err
	}
	var state ManagedState
	if err := json.Unmarshal(data, &state); err != nil {
		return nil, fmt.Errorf("状态文件损坏: %w", err)
	}
	return &state, nil
}

// Change is one file write planned by a config builder.
type Change struct {
	Path    string
	Content []byte
	Mode    os.FileMode
}

// CommitTransaction backs up existing targets, writes the new contents, and
// records the journal. Any failure rolls every write back from memory.
func CommitTransaction(clientID, primaryConfig, model string, changes []Change) error {
	if len(changes) == 0 {
		return fmt.Errorf("没有可写入的配置变更")
	}
	snapshots := make(map[string][]byte, len(changes))
	for _, change := range changes {
		if data, err := os.ReadFile(change.Path); err == nil {
			snapshots[change.Path] = data
		}
	}

	records := make([]BackupRecord, 0, len(changes))
	rollback := func(failure error) error {
		for _, record := range records {
			if record.ExistedBefore {
				if data, ok := snapshots[record.Path]; ok {
					_ = os.WriteFile(record.Path, data, 0o644)
				}
			} else {
				_ = os.Remove(record.Path)
			}
		}
		_ = os.Remove(StatePath(primaryConfig))
		return failure
	}

	for _, change := range changes {
		if err := os.MkdirAll(filepath.Dir(change.Path), 0o755); err != nil {
			return rollback(err)
		}
		record := BackupRecord{Path: change.Path, ExistedBefore: fileExists(change.Path)}
		if record.ExistedBefore {
			backupPath := datedBackupPath(change.Path)
			if err := os.Rename(change.Path, backupPath); err != nil {
				return rollback(err)
			}
			record.BackupPath = backupPath
		}
		mode := change.Mode
		if mode == 0 {
			mode = 0o644
		}
		if err := os.WriteFile(change.Path, change.Content, mode); err != nil {
			return rollback(err)
		}
		records = append(records, record)
	}

	state := ManagedState{
		Version:               4,
		Client:                clientID,
		Model:                 model,
		ConfigurationRevision: 1,
		BackupFiles:           records,
		UpdatedAtUnix:         time.Now().Unix(),
	}
	data, err := json.MarshalIndent(state, "", "  ")
	if err != nil {
		return rollback(err)
	}
	if err := os.WriteFile(StatePath(primaryConfig), append(data, '\n'), 0o644); err != nil {
		return rollback(err)
	}
	return nil
}

// CloseModification restores the dated backups and clears the journal.
func CloseModification(primaryConfig string) (string, error) {
	state, err := ReadStateFile(primaryConfig)
	if err != nil {
		return "", err
	}
	if state == nil {
		return "", fmt.Errorf("该客户端当前没有已应用的配置修改")
	}
	var restored []string
	for _, record := range state.BackupFiles {
		if record.ExistedBefore && record.BackupPath != "" {
			if err := os.Rename(record.BackupPath, record.Path); err == nil {
				restored = append(restored, record.Path)
			}
			continue
		}
		if err := os.Remove(record.Path); err == nil {
			restored = append(restored, record.Path)
		}
	}
	if err := os.Remove(StatePath(primaryConfig)); err != nil && !errors.Is(err, os.ErrNotExist) {
		return "", err
	}
	if len(restored) == 0 {
		return "", fmt.Errorf("没有可恢复的备份")
	}
	return "已关闭配置修改并还原原文件", nil
}

func datedBackupPath(path string) string {
	stamp := time.Now().Format("2006-01-02")
	candidate := fmt.Sprintf("%s.%s.bak", path, stamp)
	for index := 2; fileExists(candidate); index++ {
		candidate = fmt.Sprintf("%s.%s.%d.bak", path, stamp, index)
	}
	return candidate
}

func marshalJSONIndent(value any) ([]byte, error) {
	data, err := json.MarshalIndent(value, "", "  ")
	if err != nil {
		return nil, err
	}
	return append(data, '\n'), nil
}

// decodeJSONFile reads a JSON object file, returning an empty map when absent.
func decodeJSONFile(path string) (map[string]any, error) {
	document := map[string]any{}
	data, err := os.ReadFile(path)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return document, nil
		}
		return nil, err
	}
	if len(strings.TrimSpace(string(data))) == 0 {
		return document, nil
	}
	if err := json.Unmarshal(data, &document); err != nil {
		return nil, fmt.Errorf("%s 解析失败: %w", filepath.Base(path), err)
	}
	return document, nil
}

func nestedMap(document map[string]any, keys ...string) map[string]any {
	current := document
	for _, key := range keys {
		next, ok := current[key].(map[string]any)
		if !ok {
			next = map[string]any{}
			current[key] = next
		}
		current = next
	}
	return current
}
