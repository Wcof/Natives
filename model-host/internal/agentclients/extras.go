package agentclients

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"

	"github.com/pelletier/go-toml/v2"
)

// CurrentModel reads the managed model back out of a client's config file.
func CurrentModel(clientID, home string) string {
	switch clientID {
	case "claude-code":
		document, err := decodeJSONFile(filepath.Join(home, ".claude", "settings.json"))
		if err != nil {
			return ""
		}
		if model, _ := document["model"].(string); model != "" {
			return model
		}
		if env := nestedMap(document, "env"); env != nil {
			if model, _ := env["ANTHROPIC_MODEL"].(string); model != "" {
				return model
			}
		}
	case "codex":
		document, err := decodeTOMLFile(filepath.Join(home, ".codex", "config.toml"))
		if err != nil {
			return ""
		}
		if model, _ := document["model"].(string); model != "" {
			return model
		}
	case "zcode":
		document, err := decodeJSONFile(filepath.Join(home, ".zcode", "v2", "config.json"))
		if err == nil {
			if model, _ := document["model"].(string); model != "" {
				return strings.TrimPrefix(model, ProviderID+"/")
			}
		}
		cli, err := decodeJSONFile(filepath.Join(home, ".zcode", "cli", "config.json"))
		if err == nil {
			if modelMap := nestedMap(cli, "model"); modelMap != nil {
				if model, _ := modelMap["main"].(string); model != "" {
					return strings.TrimPrefix(model, ProviderID+"/")
				}
			}
		}
	case "kimi-code":
		document, err := decodeTOMLFile(filepath.Join(envOr("KIMI_CODE_HOME", filepath.Join(home, ".kimi-code")), "config.toml"))
		if err != nil {
			return ""
		}
		if model, _ := document["default_model"].(string); model != "" {
			return strings.TrimPrefix(model, ProviderID+"/")
		}
	case "grok-build":
		document, err := decodeTOMLFile(filepath.Join(home, ".grok", "config.toml"))
		if err != nil {
			return ""
		}
		if models := nestedMap(document, "models"); models != nil {
			if model, _ := models["default"].(string); model != "" {
				return strings.TrimPrefix(model, ProviderID+"/")
			}
		}
	case "opencode":
		document, err := decodeJSONFile(filepath.Join(envOr("XDG_CONFIG_HOME", filepath.Join(home, ".config")), "opencode", "opencode.json"))
		if err != nil {
			return ""
		}
		if model, _ := document["model"].(string); model != "" {
			return strings.TrimPrefix(model, ProviderID+"/")
		}
	}
	return ""
}

// ClaudeCodeMappings reads applied role mappings from settings.json env.
func ClaudeCodeMappings(home string) *ClaudeMappings {
	document, err := decodeJSONFile(filepath.Join(home, ".claude", "settings.json"))
	if err != nil {
		return nil
	}
	env := nestedMap(document, "env")
	if env == nil {
		return nil
	}
	pick := func(key string) string { value, _ := env[key].(string); return strings.TrimSuffix(value, "[1m]") }
	boolKey := func(key string) bool { value, _ := env[key].(string); return value == "1" }
	intKey := func(key string) int {
		value, _ := env[key].(string)
		return atoiDefault(value, 0)
	}
	mappings := &ClaudeMappings{
		Opus:               pick("ANTHROPIC_DEFAULT_OPUS_MODEL"),
		Sonnet:             pick("ANTHROPIC_DEFAULT_SONNET_MODEL"),
		Haiku:              pick("ANTHROPIC_DEFAULT_HAIKU_MODEL"),
		Opus1M:             strings.HasSuffix(stringOf(env["ANTHROPIC_DEFAULT_OPUS_MODEL"]), "[1m]"),
		Sonnet1M:           strings.HasSuffix(stringOf(env["ANTHROPIC_DEFAULT_SONNET_MODEL"]), "[1m]"),
		Haiku1M:            strings.HasSuffix(stringOf(env["ANTHROPIC_DEFAULT_HAIKU_MODEL"]), "[1m]"),
		MaxContextTokens:   intKey("CLAUDE_CODE_MAX_CONTEXT_TOKENS"),
		AutoCompactPct:     intKey("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"),
		DisableAutoCompact: boolKey("DISABLE_AUTO_COMPACT"),
	}
	if mappings.Opus == "" && mappings.Sonnet == "" && mappings.Haiku == "" {
		return nil
	}
	mappings.normalize()
	return mappings
}

func stringOf(value any) string {
	text, _ := value.(string)
	return text
}

// CodexAuthMode reports how the codex client authenticates.
func CodexAuthMode(home string) string {
	document, err := decodeJSONFile(filepath.Join(home, ".codex", "auth.json"))
	if err != nil {
		return ""
	}
	if mode, _ := document["auth_mode"].(string); mode != "" {
		return mode
	}
	return ""
}

// ClearCodexConfig removes the managed codex files and reports what was deleted.
func ClearCodexConfig(home string) ([]string, error) {
	targets := []string{
		filepath.Join(home, ".codex", "config.toml"),
		filepath.Join(home, ".codex", "auth.json"),
		filepath.Join(home, ".codex", "cpa-gui-model-catalog.json"),
	}
	var deleted []string
	for _, path := range targets {
		if err := os.Remove(path); err == nil {
			deleted = append(deleted, path)
		} else if !os.IsNotExist(err) {
			return deleted, err
		}
	}
	_ = os.Remove(StatePath(filepath.Join(home, ".codex", "config.toml")))
	return deleted, nil
}

// CodexSession is one recorded session file in ~/.codex/sessions.
type CodexSession struct {
	Path      string `json:"path"`
	Name      string `json:"name"`
	SizeBytes int64  `json:"sizeBytes"`
	UpdatedAt string `json:"updatedAt"`
}

// ListCodexSessions enumerates session recordings under ~/.codex/sessions.
func ListCodexSessions(home string) ([]CodexSession, error) {
	root := filepath.Join(home, ".codex", "sessions")
	var sessions []CodexSession
	err := filepath.Walk(root, func(path string, info os.FileInfo, err error) error {
		if err != nil || info.IsDir() {
			return nil
		}
		if !strings.HasSuffix(info.Name(), ".jsonl") {
			return nil
		}
		sessions = append(sessions, CodexSession{
			Path:      path,
			Name:      strings.TrimSuffix(info.Name(), ".jsonl"),
			SizeBytes: info.Size(),
			UpdatedAt: info.ModTime().Format("2006-01-02 15:04"),
		})
		return nil
	})
	if os.IsNotExist(err) {
		return []CodexSession{}, nil
	}
	return sessions, err
}

// DeleteCodexSessions removes the given session files that live under the
// sessions root, refusing paths that escape it.
func DeleteCodexSessions(home string, paths []string) ([]string, error) {
	root := filepath.Join(home, ".codex", "sessions")
	var deleted []string
	for _, path := range paths {
		clean := filepath.Clean(path)
		if !strings.HasPrefix(clean, root+string(filepath.Separator)) {
			return deleted, fmt.Errorf("拒绝删除会话目录之外的路径: %s", path)
		}
		if err := os.Remove(clean); err == nil {
			deleted = append(deleted, clean)
		} else if !os.IsNotExist(err) {
			return deleted, err
		}
	}
	return deleted, nil
}

// PiProviderStatus reports whether the cliproxyapi provider plugin is present.
type PiProviderStatus struct {
	Installed       bool   `json:"installed"`
	InstalledVersion string `json:"installedVersion,omitempty"`
}

const piProviderPackageDir = "@router-for-me" + string(filepath.Separator) + "pi-cliproxyapi-provider"

func piNodeModulesDirs(home string) []string {
	agentDir := envOr("PI_CODING_AGENT_DIR", filepath.Join(home, ".pi", "agent"))
	return []string{
		filepath.Join(agentDir, "npm", "node_modules"),
		filepath.Join(agentDir, "node_modules"),
		filepath.Join(home, ".pi", "agent", "node_modules"),
	}
}

// DetectPiProvider locates the Pi cliproxyapi provider plugin installation.
func DetectPiProvider(home string) PiProviderStatus {
	for _, dir := range piNodeModulesDirs(home) {
		packagePath := filepath.Join(dir, piProviderPackageDir, "package.json")
		data, err := os.ReadFile(packagePath)
		if err != nil {
			continue
		}
		var manifest struct {
			Version string `json:"version"`
		}
		if json.Unmarshal(data, &manifest) == nil && manifest.Version != "" {
			return PiProviderStatus{Installed: true, InstalledVersion: manifest.Version}
		}
		return PiProviderStatus{Installed: true}
	}
	return PiProviderStatus{Installed: false}
}

// PiProviderAction installs, removes, repairs, or updates the Pi provider
// plugin through the pi CLI, then re-applies the managed provider config.
// Supported actions: install, uninstall, update, repair.
func PiProviderAction(ctx context.Context, home, baseURL, apiKey, model, action string) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	candidates := executableCandidates([]string{"pi"})
	if len(candidates) == 0 {
		return "", fmt.Errorf("未找到 pi 可执行文件")
	}
	var args []string
	switch action {
	case "install", "repair":
		args = []string{"install", "npm:@router-for-me/pi-cliproxyapi-provider"}
	case "update":
		args = []string{"install", "--force", "npm:@router-for-me/pi-cliproxyapi-provider"}
	case "uninstall":
		args = []string{"remove", "@router-for-me/pi-cliproxyapi-provider"}
	default:
		return "", fmt.Errorf("未支持的 Pi 插件操作: %s", action)
	}
	output, err := runCommand(ctx, candidates[0], args, 120*time.Second)
	if err != nil {
		return "", fmt.Errorf("pi %s 失败: %v", action, err)
	}
	if action != "uninstall" {
		if changes, _, buildErr := BuildChanges("pi", home, baseURL, apiKey, model, nil); buildErr == nil {
			for _, change := range changes {
				_ = os.MkdirAll(filepath.Dir(change.Path), 0o755)
				_ = os.WriteFile(change.Path, change.Content, change.Mode)
			}
		}
	}
	return output, nil
}

func runCommand(ctx context.Context, exe string, args []string, timeout time.Duration) (string, error) {
	ctx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()
	cmd := exec.CommandContext(ctx, exe, args...)
	output, err := cmd.CombinedOutput()
	if ctx.Err() == context.DeadlineExceeded {
		return string(output), fmt.Errorf("命令超时")
	}
	if err != nil {
		trimmed := strings.TrimSpace(string(output))
		if trimmed != "" {
			return string(output), fmt.Errorf("%w: %s", err, trimmed)
		}
		return string(output), err
	}
	return string(output), nil
}

func decodeTOMLFile(path string) (map[string]any, error) {
	document := map[string]any{}
	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return document, nil
		}
		return nil, err
	}
	if err := toml.Unmarshal(data, &document); err != nil {
		return nil, fmt.Errorf("%s 解析失败: %w", filepath.Base(path), err)
	}
	return document, nil
}
