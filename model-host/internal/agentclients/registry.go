// Package agentclients detects local AI coding-agent clients, points their
// configuration at the local gateway, and launches them (ADR-0024 scope:
// management features mirrored from the reference GUI, macOS-first).
package agentclients

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"
)

type LaunchTarget struct {
	ID    string `json:"id"`
	Label string `json:"label"`
}

// Definition describes one managed agent client and how to probe it.
type Definition struct {
	ID             string
	Name           string
	ConfigPaths    func(home string) []string
	PrimaryPath    func(home string) string
	CLIExecutables []string
	AppProbe       func(home string) []string
	LaunchTargets  []LaunchTarget
	// ModelPicker reports whether the client accepts a managed model selection.
	ModelPicker bool
}

func homeConfig(name string) func(string) []string {
	return func(home string) []string { return []string{filepath.Join(home, name)} }
}

func envOr(name, fallback string) string {
	if value := os.Getenv(name); value != "" {
		return value
	}
	return fallback
}

// Definitions returns the managed client registry in stable display order.
func Definitions() []Definition {
	return []Definition{
		{
			ID: "claude-code", Name: "Claude Code", ModelPicker: true,
			ConfigPaths:    homeConfig(".claude/settings.json"),
			PrimaryPath:    func(home string) string { return filepath.Join(home, ".claude", "settings.json") },
			CLIExecutables: []string{"claude"},
			LaunchTargets:  []LaunchTarget{{ID: "cli", Label: "Claude Code"}},
		},
		{
			ID: "claude-desktop", Name: "Claude Desktop", ModelPicker: false,
			ConfigPaths: func(home string) []string {
				dir := claudeDesktopDir(home)
				return []string{
					filepath.Join(dir, "claude_desktop_config.json"),
					filepath.Join(dir, "Claude-3p", "claude_desktop_config.json"),
				}
			},
			PrimaryPath: func(home string) string {
				return filepath.Join(claudeDesktopDir(home), "claude_desktop_config.json")
			},
			AppProbe: func(home string) []string {
				if runtime.GOOS == "darwin" {
					return []string{"/Applications/Claude.app"}
				}
				return nil
			},
			LaunchTargets: []LaunchTarget{{ID: "app", Label: "Claude Desktop"}},
		},
		{
			ID: "codex", Name: "Codex", ModelPicker: true,
			ConfigPaths:    homeConfig(".codex/config.toml"),
			PrimaryPath:    func(home string) string { return filepath.Join(home, ".codex", "config.toml") },
			CLIExecutables: []string{"codex"},
			LaunchTargets:  []LaunchTarget{{ID: "cli", Label: "Codex CLI"}},
		},
		{
			ID: "deepseek-harness", Name: "DeepSeek Harness", ModelPicker: true,
			ConfigPaths: func(home string) []string {
				base := envOr("DSH_HOME", filepath.Join(home, ".dsh"))
				return []string{filepath.Join(base, "settings.yaml"), filepath.Join(base, ".credentials.yaml")}
			},
			PrimaryPath: func(home string) string {
				return filepath.Join(envOr("DSH_HOME", filepath.Join(home, ".dsh")), "settings.yaml")
			},
			LaunchTargets: []LaunchTarget{{ID: "cli", Label: "DeepSeek Harness"}},
		},
		{
			ID: "opencode", Name: "OpenCode", ModelPicker: true,
			ConfigPaths: func(home string) []string {
				base := envOr("XDG_CONFIG_HOME", filepath.Join(home, ".config"))
				return []string{filepath.Join(base, "opencode", "opencode.json"), filepath.Join(base, "opencode", "opencode.jsonc")}
			},
			PrimaryPath: func(home string) string {
				return filepath.Join(envOr("XDG_CONFIG_HOME", filepath.Join(home, ".config")), "opencode", "opencode.json")
			},
			CLIExecutables: []string{"opencode", "opencode-cli"},
			LaunchTargets:  []LaunchTarget{{ID: "cli", Label: "OpenCode CLI"}},
		},
		{
			ID: "pi", Name: "Pi", ModelPicker: true,
			ConfigPaths: func(home string) []string {
				base := envOr("PI_CODING_AGENT_DIR", filepath.Join(home, ".pi", "agent"))
				return []string{filepath.Join(base, "settings.json"), filepath.Join(base, "cliproxyapi.json")}
			},
			PrimaryPath: func(home string) string {
				return filepath.Join(envOr("PI_CODING_AGENT_DIR", filepath.Join(home, ".pi", "agent")), "settings.json")
			},
			CLIExecutables: []string{"pi"},
			LaunchTargets:  []LaunchTarget{{ID: "cli", Label: "Pi"}},
		},
		{
			ID: "grok-build", Name: "Grok Build", ModelPicker: true,
			ConfigPaths:    homeConfig(".grok/config.toml"),
			PrimaryPath:    func(home string) string { return filepath.Join(home, ".grok", "config.toml") },
			CLIExecutables: []string{"grok"},
			LaunchTargets:  []LaunchTarget{{ID: "cli", Label: "Grok Build"}},
		},
		{
			ID: "zcode", Name: "ZCode", ModelPicker: true,
			ConfigPaths: func(home string) []string {
				return []string{filepath.Join(home, ".zcode", "v2", "config.json"), filepath.Join(home, ".zcode", "cli", "config.json")}
			},
			PrimaryPath: func(home string) string { return filepath.Join(home, ".zcode", "v2", "config.json") },
			AppProbe: func(home string) []string {
				switch runtime.GOOS {
				case "darwin":
					return []string{"/Applications/ZCode.app"}
				case "windows":
					return []string{filepath.Join(envOr("LOCALAPPDATA", filepath.Join(home, "AppData", "Local")), "Programs", "ZCode", "ZCode.exe")}
				default:
					return []string{"/opt/ZCode/zcode"}
				}
			},
			CLIExecutables: []string{"zcode"},
			LaunchTargets:  []LaunchTarget{{ID: "app", Label: "ZCode"}},
		},
		{
			ID: "kimi-code", Name: "Kimi Code", ModelPicker: true,
			ConfigPaths: func(home string) []string {
				base := envOr("KIMI_CODE_HOME", filepath.Join(home, ".kimi-code"))
				return []string{filepath.Join(base, "config.toml")}
			},
			PrimaryPath: func(home string) string {
				return filepath.Join(envOr("KIMI_CODE_HOME", filepath.Join(home, ".kimi-code")), "config.toml")
			},
			CLIExecutables: []string{"kimi"},
			LaunchTargets:  []LaunchTarget{{ID: "cli", Label: "Kimi Code"}},
		},
		{
			ID: "openclaw", Name: "OpenClaw", ModelPicker: true,
			ConfigPaths:    homeConfig(".openclaw/openclaw.json"),
			PrimaryPath:    func(home string) string { return filepath.Join(home, ".openclaw", "openclaw.json") },
			CLIExecutables: []string{"openclaw"},
			LaunchTargets:  []LaunchTarget{{ID: "cli", Label: "OpenClaw"}},
		},
		{
			ID: "hermes", Name: "Hermes Agent", ModelPicker: true,
			ConfigPaths: func(home string) []string {
				base := envOr("HERMES_HOME", filepath.Join(home, ".hermes"))
				return []string{filepath.Join(base, "config.yaml")}
			},
			PrimaryPath: func(home string) string {
				return filepath.Join(envOr("HERMES_HOME", filepath.Join(home, ".hermes")), "config.yaml")
			},
			CLIExecutables: []string{"hermes"},
			LaunchTargets:  []LaunchTarget{{ID: "cli", Label: "Hermes Agent"}},
		},
	}
}

// DefinitionByID returns the client definition with the given id.
func DefinitionByID(id string) (Definition, bool) {
	for _, definition := range Definitions() {
		if definition.ID == id {
			return definition, true
		}
	}
	return Definition{}, false
}

func claudeDesktopDir(home string) string {
	switch runtime.GOOS {
	case "darwin":
		return filepath.Join(home, "Library", "Application Support", "Claude")
	case "windows":
		base := envOr("LOCALAPPDATA", filepath.Join(home, "AppData", "Local"))
		return filepath.Join(base, "Claude")
	default:
		base := envOr("XDG_CONFIG_HOME", filepath.Join(home, ".config"))
		return filepath.Join(base, "Claude")
	}
}

// executableCandidates resolves CLI executables across PATH and common
// user-local install directories, mirroring the reference probe order.
func executableCandidates(names []string) []string {
	var candidates []string
	seen := map[string]bool{}
	add := func(path string) {
		if path != "" && !seen[path] && isExecutableFile(path) {
			seen[path] = true
			candidates = append(candidates, path)
		}
	}
	dirs := filepath.SplitList(os.Getenv("PATH"))
	switch runtime.GOOS {
	case "darwin":
		dirs = append(dirs, "/opt/homebrew/bin", "/usr/local/bin", filepath.Join(homeDir(), ".local", "bin"))
	case "linux":
		dirs = append(dirs, filepath.Join(homeDir(), ".local", "bin"), "/usr/local/bin")
	case "windows":
		dirs = append(dirs, filepath.Join(envOr("APPDATA", ""), "npm"))
	}
	for _, name := range names {
		for _, dir := range dirs {
			if runtime.GOOS == "windows" {
				for _, ext := range []string{".exe", ".cmd", ".bat", ""} {
					add(filepath.Join(dir, name+ext))
				}
				continue
			}
			add(filepath.Join(dir, name))
		}
		for _, dir := range extraToolBins() {
			if runtime.GOOS == "windows" {
				add(filepath.Join(dir, name+".exe"))
				continue
			}
			add(filepath.Join(dir, name))
		}
	}
	return candidates
}

func extraToolBins() []string {
	home := homeDir()
	var dirs []string
	for _, path := range []string{
		filepath.Join(home, ".local", "bin"),
		filepath.Join(home, ".npm-global", "bin"),
		filepath.Join(home, ".cargo", "bin"),
		filepath.Join(home, "bin"),
	} {
		if path != "" {
			dirs = append(dirs, path)
		}
	}
	if runtime.GOOS == "darwin" {
		dirs = append(dirs, "/opt/homebrew/bin")
	}
	return dirs
}

func isExecutableFile(path string) bool {
	info, err := os.Stat(path)
	if err != nil || info.IsDir() {
		return false
	}
	if runtime.GOOS == "windows" {
		return true
	}
	return info.Mode()&0o111 != 0
}

func homeDir() string {
	home, err := os.UserHomeDir()
	if err != nil {
		return "."
	}
	return strings.TrimRight(home, "/\\")
}
