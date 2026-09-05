package agentclients

import (
	"context"
	"os"
	"os/exec"
	"strconv"
	"strings"
	"time"
)

// Status is the per-client detection snapshot surfaced to the settings page.
// Field names follow the reference GUI's camelCase contract.
type Status struct {
	ID                 string         `json:"id"`
	Name               string         `json:"name"`
	SupportedPlatform  bool           `json:"supportedPlatform"`
	Installed          bool           `json:"installed"`
	LaunchTargets      []LaunchTarget `json:"launchTargets"`
	Version            string         `json:"version"`
	AppVersion         string         `json:"appVersion,omitempty"`
	PluginVersion      string         `json:"pluginVersion,omitempty"`
	ConfigPaths        []string       `json:"configPaths"`
	ConfigExists       bool           `json:"configExists"`
	Configured         bool           `json:"configured"`
	ModificationState  string         `json:"modificationState"`
	AppliedModel       string         `json:"appliedModel,omitempty"`
	CurrentModel       string         `json:"currentModel,omitempty"`
	ModelPicker        bool           `json:"modelPicker"`
	BackupAvailable    bool           `json:"backupAvailable"`
	Warnings           []string       `json:"warnings"`
	Error              string         `json:"error,omitempty"`
}

const versionProbeTimeout = 5 * time.Second

// DetectStatus probes one client: config files, CLI/app presence, version.
func DetectStatus(definition Definition) Status {
	home := homeDir()
	status := Status{
		ID:                definition.ID,
		Name:              definition.Name,
		SupportedPlatform: true,
		LaunchTargets:     definition.LaunchTargets,
		ModelPicker:       definition.ModelPicker,
		Warnings:          []string{},
	}
	for _, path := range definition.ConfigPaths(home) {
		status.ConfigPaths = append(status.ConfigPaths, path)
		if fileExists(path) {
			status.ConfigExists = true
		}
	}

	var versionErr string
	if len(definition.CLIExecutables) > 0 {
		if exe := lookupHook(definition.CLIExecutables); len(exe) > 0 {
			if version, err := versionProbe(exe[0]); err == nil && version != "" {
				status.Version = version
				status.Installed = true
			} else if err != nil {
				versionErr = err.Error()
			}
		}
	}
	if !status.Installed && definition.AppProbe != nil {
		for _, app := range definition.AppProbe(home) {
			if pathExists(app) {
				status.Installed = true
				status.AppVersion = probeAppBundleVersion(app)
				break
			}
		}
	}
	if !status.Installed && status.ConfigExists {
		// Config present without a runnable binary still counts as installed
		// so the user can manage leftover configuration explicitly.
		status.Installed = true
	}
	if versionErr != "" && !status.Installed {
		status.Error = versionErr
	}

	state, err := ReadStateFile(definition.PrimaryPath(home))
	if err == nil && state != nil {
		status.ModificationState = "applied"
		status.AppliedModel = state.Model
		status.BackupAvailable = len(state.BackupFiles) > 0
		status.Configured = true
		for _, record := range state.BackupFiles {
			if !record.ExistedBefore {
				continue
			}
			if _, err := os.Stat(record.BackupPath); err == nil {
				status.BackupAvailable = true
				break
			}
		}
	} else {
		status.ModificationState = "unconfigured"
		status.Configured = false
	}
	if status.ConfigExists && state == nil {
		status.Warnings = append(status.Warnings, "配置文件存在但未由本工具管理")
	}
	return status
}

// DetectAll probes every client with a bounded worker pool.
func DetectAll(concurrency int) []Status {
	definitions := Definitions()
	if concurrency <= 0 {
		concurrency = 4
	}
	jobs := make(chan int, len(definitions))
	results := make([]Status, len(definitions))
	for index := range definitions {
		jobs <- index
	}
	close(jobs)
	done := make(chan struct{})
	for worker := 0; worker < concurrency; worker++ {
		go func() {
			defer func() { done <- struct{}{} }()
			for index := range jobs {
				results[index] = DetectStatus(definitions[index])
			}
		}()
	}
	for worker := 0; worker < concurrency; worker++ {
		<-done
	}
	return results
}

// probeVersion runs `<exe> --version` and returns the first digit-bearing line.
func probeVersion(exe string) (string, error) {
	ctx, cancel := context.WithTimeout(context.Background(), versionProbeTimeout)
	defer cancel()
	cmd := exec.CommandContext(ctx, exe, "--version")
	cmd.Stdin = nil
	output, err := cmd.CombinedOutput()
	if ctx.Err() == context.DeadlineExceeded {
		return "", errVersionTimeout
	}
	if err != nil && len(strings.TrimSpace(string(output))) == 0 {
		return "", err
	}
	return normalizeVersion(string(output)), nil
}

var errVersionTimeout = errorString("version probe timed out")

type errorString string

func (e errorString) Error() string { return string(e) }

// lookupHook lets tests stub executable discovery.
var lookupHook = executableCandidates

// versionProbe lets tests stub version probing.
var versionProbe = probeVersion

func normalizeVersion(raw string) string {
	for _, line := range strings.Split(raw, "\n") {
		line = strings.TrimSpace(line)
		if len(line) == 0 || len(line) > 256 {
			continue
		}
		if !strings.ContainsAny(line, "0123456789") {
			continue
		}
		if strings.ContainsFunc(line, func(r rune) bool { return r < 0x20 }) {
			continue
		}
		return line
	}
	return ""
}

func probeAppBundleVersion(appPath string) string {
	if !strings.HasSuffix(appPath, ".app") {
		return ""
	}
	infoPlist := appPath + "/Contents/Info.plist"
	data, err := os.ReadFile(infoPlist)
	if err != nil {
		return ""
	}
	// Extract CFBundleShortVersionString without spawning plutil for speed.
	content := string(data)
	marker := "CFBundleShortVersionString"
	index := strings.Index(content, marker)
	if index < 0 {
		return ""
	}
	tail := content[index+len(marker):]
	start := strings.Index(tail, "<string>")
	if start < 0 {
		return ""
	}
	end := strings.Index(tail[start:], "</string>")
	if end < 0 {
		return ""
	}
	return strings.TrimSpace(tail[start+len("<string>") : start+end])
}

func fileExists(path string) bool {
	info, err := os.Stat(path)
	return err == nil && !info.IsDir()
}

func pathExists(path string) bool {
	_, err := os.Stat(path)
	return err == nil
}

func atoiDefault(value string, fallback int) int {
	parsed, err := strconv.Atoi(strings.TrimSpace(value))
	if err != nil {
		return fallback
	}
	return parsed
}
