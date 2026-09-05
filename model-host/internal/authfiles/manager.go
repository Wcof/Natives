package authfiles

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"time"
)

type AuthFileItem struct {
	AccountID      string   `json:"accountId,omitempty"`
	Source         string   `json:"source"`
	Name           string   `json:"name"`
	Provider       string   `json:"provider"`
	Account        string   `json:"account"`
	Status         string   `json:"status"`
	Disabled       bool     `json:"disabled"`
	Size           int64    `json:"size"`
	UpdatedAt      string   `json:"updatedAt"`
	Priority       int      `json:"priority"`
	ExcludedModels []string `json:"excludedModels,omitempty"`
}

type Manager struct {
	baseDir string
	mu      sync.Mutex
}

func DefaultAuthDir() (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	dir := filepath.Join(home, ".natives", "auth")
	if err := os.MkdirAll(dir, 0755); err != nil {
		return "", err
	}
	return dir, nil
}

func NewManager(baseDir string) (*Manager, error) {
	if baseDir == "" {
		var err error
		baseDir, err = DefaultAuthDir()
		if err != nil {
			return nil, err
		}
	}
	if err := os.MkdirAll(baseDir, 0755); err != nil {
		return nil, err
	}
	return &Manager{baseDir: baseDir}, nil
}

func (m *Manager) BaseDir() string {
	return m.baseDir
}

func (m *Manager) List() ([]AuthFileItem, error) {
	m.mu.Lock()
	defer m.mu.Unlock()

	entries, err := os.ReadDir(m.baseDir)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return []AuthFileItem{}, nil
		}
		return nil, err
	}

	var items []AuthFileItem
	for _, entry := range entries {
		if entry.IsDir() || !strings.HasSuffix(strings.ToLower(entry.Name()), ".json") {
			continue
		}
		info, err := entry.Info()
		if err != nil {
			continue
		}
		filePath := filepath.Join(m.baseDir, entry.Name())
		data, err := os.ReadFile(filePath)
		if err != nil {
			continue
		}

		item := parseAuthFile(entry.Name(), info.Size(), info.ModTime(), data)
		items = append(items, item)
	}
	return items, nil
}

func (m *Manager) Import(name string, content []byte) (*AuthFileItem, error) {
	m.mu.Lock()
	defer m.mu.Unlock()

	cleanName := filepath.Base(name)
	if !strings.HasSuffix(strings.ToLower(cleanName), ".json") {
		cleanName += ".json"
	}
	var testJSON map[string]any
	if err := json.Unmarshal(content, &testJSON); err != nil {
		return nil, fmt.Errorf("invalid json content: %w", err)
	}

	targetPath := filepath.Join(m.baseDir, cleanName)
	if err := os.WriteFile(targetPath, content, 0644); err != nil {
		return nil, err
	}

	info, err := os.Stat(targetPath)
	if err != nil {
		return nil, err
	}

	item := parseAuthFile(cleanName, info.Size(), info.ModTime(), content)
	return &item, nil
}

func (m *Manager) Update(name string, disabled *bool, priority *int, excludedModels []string) (*AuthFileItem, error) {
	m.mu.Lock()
	defer m.mu.Unlock()

	cleanName := filepath.Base(name)
	filePath := filepath.Join(m.baseDir, cleanName)
	if _, err := os.Stat(filePath); errors.Is(err, os.ErrNotExist) && !strings.HasSuffix(strings.ToLower(cleanName), ".json") {
		cleanName += ".json"
		filePath = filepath.Join(m.baseDir, cleanName)
	}
	data, err := os.ReadFile(filePath)
	if err != nil {
		return nil, err
	}

	var parsed map[string]any
	if err := json.Unmarshal(data, &parsed); err != nil {
		parsed = make(map[string]any)
	}

	if disabled != nil {
		parsed["disabled"] = *disabled
	}
	if priority != nil {
		parsed["priority"] = *priority
	}
	if excludedModels != nil {
		parsed["excluded_models"] = excludedModels
	}

	newData, err := json.MarshalIndent(parsed, "", "  ")
	if err != nil {
		return nil, err
	}

	if err := os.WriteFile(filePath, newData, 0644); err != nil {
		return nil, err
	}

	info, err := os.Stat(filePath)
	if err != nil {
		return nil, err
	}

	item := parseAuthFile(cleanName, info.Size(), info.ModTime(), newData)
	return &item, nil
}

func (m *Manager) Delete(name string) error {
	m.mu.Lock()
	defer m.mu.Unlock()

	cleanName := filepath.Base(name)
	filePath := filepath.Join(m.baseDir, cleanName)
	if _, err := os.Stat(filePath); errors.Is(err, os.ErrNotExist) && !strings.HasSuffix(strings.ToLower(cleanName), ".json") {
		cleanName += ".json"
		filePath = filepath.Join(m.baseDir, cleanName)
	}
	return os.Remove(filePath)
}

func (m *Manager) OpenDir() error {
	path := m.baseDir
	var cmd *exec.Cmd
	switch runtime.GOOS {
	case "darwin":
		cmd = exec.Command("open", path)
	case "windows":
		cmd = exec.Command("explorer", path)
	default:
		cmd = exec.Command("xdg-open", path)
	}
	return cmd.Start()
}

func (m *Manager) ReadFile(name string) ([]byte, error) {
	m.mu.Lock()
	defer m.mu.Unlock()

	cleanName := filepath.Base(name)
	filePath := filepath.Join(m.baseDir, cleanName)
	if _, err := os.Stat(filePath); errors.Is(err, os.ErrNotExist) && !strings.HasSuffix(strings.ToLower(cleanName), ".json") {
		cleanName += ".json"
		filePath = filepath.Join(m.baseDir, cleanName)
	}
	return os.ReadFile(filePath)
}

func parseAuthFile(name string, size int64, modTime time.Time, data []byte) AuthFileItem {
	var raw map[string]any
	_ = json.Unmarshal(data, &raw)

	provider := ""
	for _, key := range []string{"provider", "type", "account_type"} {
		if val, ok := raw[key].(string); ok && val != "" {
			provider = strings.ToLower(val)
			break
		}
	}
	if provider == "anthropic" {
		provider = "claude"
	} else if provider == "anti-gravity" {
		provider = "antigravity"
	}
	if provider == "" {
		lower := strings.ToLower(name)
		if strings.Contains(lower, "antigravity") {
			provider = "antigravity"
		} else if strings.Contains(lower, "claude") {
			provider = "claude"
		} else if strings.Contains(lower, "codex") || strings.Contains(lower, "chatgpt") {
			provider = "codex"
		} else if strings.Contains(lower, "kimi") {
			provider = "kimi"
		} else if strings.Contains(lower, "xai") || strings.Contains(lower, "grok") {
			provider = "xai"
		} else {
			provider = "unknown"
		}
	}

	account := ""
	for _, key := range []string{"label", "email", "account", "user", "username"} {
		if val, ok := raw[key].(string); ok && val != "" {
			account = val
			break
		}
	}
	if account == "" {
		account = strings.TrimSuffix(name, ".json")
	}

	disabled := false
	if d, ok := raw["disabled"].(bool); ok {
		disabled = d
	}

	priority := 0
	if p, ok := raw["priority"].(float64); ok {
		priority = int(p)
	}

	var excluded []string
	if ex, ok := raw["excluded_models"].([]any); ok {
		for _, item := range ex {
			if s, ok := item.(string); ok {
				excluded = append(excluded, s)
			}
		}
	}

	status := "active"
	if disabled {
		status = "disabled"
	}

	return AuthFileItem{
		Source:         "file",
		Name:           name,
		Provider:       provider,
		Account:        account,
		Status:         status,
		Disabled:       disabled,
		Size:           size,
		UpdatedAt:      modTime.Format(time.RFC3339),
		Priority:       priority,
		ExcludedModels: excluded,
	}
}
