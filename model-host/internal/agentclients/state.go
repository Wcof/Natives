package agentclients

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
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

// ClaudeMappings mirrors the reference GUI's Claude role mapping block.
type ClaudeMappings struct {
	Opus               string `json:"opus"`
	Sonnet             string `json:"sonnet"`
	Haiku              string `json:"haiku"`
	Opus1M             bool   `json:"opus1m"`
	Sonnet1M           bool   `json:"sonnet1m"`
	Haiku1M            bool   `json:"haiku1m"`
	MaxContextTokens   int    `json:"maxContextTokens"`
	AutoCompactPct     int    `json:"autoCompactPct"`
	DisableAutoCompact bool   `json:"disableAutoCompact"`
}

func (m *ClaudeMappings) normalize() {
	if m.Opus == "" {
		m.Opus = "claude-opus-5"
	}
	if m.Sonnet == "" {
		m.Sonnet = "claude-sonnet-4-6"
	}
	if m.Haiku == "" {
		m.Haiku = "claude-haiku-4-5"
	}
	if m.MaxContextTokens <= 0 {
		m.MaxContextTokens = 200000
	}
	if m.AutoCompactPct <= 0 {
		m.AutoCompactPct = 90
	}
	if m.AutoCompactPct > 100 {
		m.AutoCompactPct = 100
	}
}

func (m *ClaudeMappings) with1M(model string, enabled bool) string {
	if enabled && !strings.HasSuffix(model, "[1m]") {
		return model + "[1m]"
	}
	return strings.TrimSuffix(model, "[1m]")
}

// BackupRecord tracks one rolled-aside original file.
type BackupRecord struct {
	Path          string `json:"path"`
	BackupPath    string `json:"backupPath"`
	ExistedBefore bool   `json:"existedBefore"`
}

// ManagedState is the on-disk journal enabling 配置修改/默认配置/关闭配置修改.
type ManagedState struct {
	Version                    int             `json:"version"`
	Client                     string          `json:"client"`
	Model                      string          `json:"model"`
	ConfigurationRevision      int             `json:"configurationRevision"`
	ClaudeDesktopModelMappings *ClaudeMappings `json:"claudeDesktopModelMappings,omitempty"`
	BackupFiles                []BackupRecord  `json:"backupFiles"`
	UpdatedAtUnix              int64           `json:"updatedAtUnix"`
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
func CommitTransaction(clientID, primaryConfig, model string, changes []Change, mappings *ClaudeMappings) error {
	if len(changes) == 0 {
		return fmt.Errorf("没有可写入的配置变更")
	}
	existingState, err := ReadStateFile(primaryConfig)
	if err != nil {
		return err
	}
	journalPath := StatePath(primaryConfig)
	journalSnapshot, _ := os.ReadFile(journalPath)
	snapshots := make(map[string][]byte, len(changes))
	for _, change := range changes {
		if data, err := os.ReadFile(change.Path); err == nil {
			snapshots[change.Path] = data
		}
	}

	records := make([]BackupRecord, 0, len(changes))
	if existingState != nil {
		records = append(records, existingState.BackupFiles...)
	}
	rollback := func(failure error) error {
		for _, change := range changes {
			if data, ok := snapshots[change.Path]; ok {
				_ = os.WriteFile(change.Path, data, 0o644)
			} else {
				_ = os.Remove(change.Path)
			}
		}
		if existingState != nil {
			_ = os.WriteFile(journalPath, journalSnapshot, 0o644)
		} else {
			for _, record := range records {
				if record.BackupPath != "" {
					_ = os.Remove(record.BackupPath)
				}
			}
			_ = os.Remove(journalPath)
		}
		return failure
	}

	for _, change := range changes {
		if err := os.MkdirAll(filepath.Dir(change.Path), 0o755); err != nil {
			return rollback(err)
		}
		if existingState == nil {
			record := BackupRecord{Path: change.Path, ExistedBefore: fileExists(change.Path)}
			if record.ExistedBefore {
				backupPath := datedBackupPath(change.Path)
				if err := copyFile(change.Path, backupPath); err != nil {
					return rollback(err)
				}
				record.BackupPath = backupPath
			}
			records = append(records, record)
		}
		mode := change.Mode
		if mode == 0 {
			mode = 0o644
		}
		if err := os.WriteFile(change.Path, change.Content, mode); err != nil {
			return rollback(err)
		}
	}

	revision := 1
	if existingState != nil {
		revision = existingState.ConfigurationRevision + 1
	}
	state := ManagedState{
		Version:                    4,
		Client:                     clientID,
		Model:                      model,
		ConfigurationRevision:      revision,
		ClaudeDesktopModelMappings: mappings,
		BackupFiles:                records,
		UpdatedAtUnix:              time.Now().Unix(),
	}
	data, err := json.MarshalIndent(state, "", "  ")
	if err != nil {
		return rollback(err)
	}
	if err := os.WriteFile(journalPath, append(data, '\n'), 0o644); err != nil {
		return rollback(err)
	}
	return nil
}

// RefreshManaged rewrites only clients already managed by Natives while
// preserving their original backup journals.
func RefreshManaged(home, baseURL, apiKey string) (int, error) {
	return RefreshManagedWithModels(home, baseURL, apiKey, nil)
}

func RefreshManagedWithModels(home, baseURL, apiKey string, models []ModelOption) (int, error) {
	refreshed := 0
	for _, definition := range Definitions() {
		primary := definition.PrimaryPath(home)
		state, err := ReadStateFile(primary)
		if err != nil {
			return refreshed, err
		}
		if state == nil {
			continue
		}
		changes, _, err := BuildChangesWithModels(definition.ID, home, baseURL, apiKey, state.Model, models, state.ClaudeDesktopModelMappings)
		if err != nil {
			return refreshed, err
		}
		if err := CommitTransaction(definition.ID, primary, state.Model, changes, state.ClaudeDesktopModelMappings); err != nil {
			return refreshed, err
		}
		refreshed++
	}
	return refreshed, nil
}

func copyFile(src, dst string) error {
	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer in.Close()
	out, err := os.Create(dst)
	if err != nil {
		return err
	}
	defer out.Close()
	if _, err = io.Copy(out, in); err != nil {
		return err
	}
	return out.Sync()
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
			if err := copyFile(record.BackupPath, record.Path); err == nil {
				_ = os.Remove(record.BackupPath)
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
