package host

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/ldh/natives/model-host/internal/domain"
)

const DefaultCLIProxyAPIPath = "/Volumes/UNTITLED/本人材料/project/CLIProxyAPI"

var (
	cachedLatestKernelVersion = "v7.2.151"
	cachedLatestKernelMu      sync.RWMutex
)

func getCachedLatestKernelVersion() string {
	cachedLatestKernelMu.RLock()
	defer cachedLatestKernelMu.RUnlock()
	return cachedLatestKernelVersion
}

func setCachedLatestKernelVersion(v string) {
	cachedLatestKernelMu.Lock()
	defer cachedLatestKernelMu.Unlock()
	cachedLatestKernelVersion = v
}

func findThirdPartyDir() string {
	cwd, err := os.Getwd()
	if err != nil {
		cwd = "."
	}
	dir := cwd
	for i := 0; i < 6; i++ {
		candidate := filepath.Join(dir, "third_party", "cliproxyapi")
		if info, err := os.Stat(candidate); err == nil && info.IsDir() {
			return candidate
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			break
		}
		dir = parent
	}
	return "third_party/cliproxyapi"
}

func getLocalKernelVersion() string {
	tp := findThirdPartyDir()
	for _, f := range []string{"version.txt", "core-version.txt"} {
		if data, err := os.ReadFile(filepath.Join(tp, f)); err == nil {
			v := strings.TrimSpace(string(data))
			if v != "" {
				if !strings.HasPrefix(v, "v") {
					v = "v" + v
				}
				return v
			}
		}
	}
	return "v7.2.146"
}

func getLatestCLIProxyAPIVersion(projectPath string) (string, error) {
	if projectPath == "" {
		projectPath = DefaultCLIProxyAPIPath
	}
	if info, err := os.Stat(projectPath); err == nil && info.IsDir() {
		cmd := exec.Command("git", "describe", "--tags", "--always")
		cmd.Dir = projectPath
		if out, err := cmd.Output(); err == nil {
			v := strings.TrimSpace(string(out))
			if v != "" {
				if !strings.HasPrefix(v, "v") {
					v = "v" + v
				}
				return v, nil
			}
		}
		cmd2 := exec.Command("git", "tag", "-l", "--sort=-v:refname")
		cmd2.Dir = projectPath
		if out, err := cmd2.Output(); err == nil {
			lines := strings.Split(strings.TrimSpace(string(out)), "\n")
			if len(lines) > 0 && strings.TrimSpace(lines[0]) != "" {
				v := strings.TrimSpace(lines[0])
				if !strings.HasPrefix(v, "v") {
					v = "v" + v
				}
				return v, nil
			}
		}
	}
	return "v7.2.151", nil
}

func (e *Engine) checkKernelUpdate() (any, error) {
	current := getLocalKernelVersion()
	latest, _ := getLatestCLIProxyAPIVersion(DefaultCLIProxyAPIPath)
	if latest != "" {
		setCachedLatestKernelVersion(latest)
	} else {
		latest = getCachedLatestKernelVersion()
	}
	hasUpdate := current != latest

	_, _ = e.repo.Update(nil, func(snapshot *domain.Snapshot) error {
		snapshot.Gateway.KernelVersion = current
		snapshot.Gateway.LatestKernelVersion = latest
		return nil
	})

	return map[string]any{
		"currentVersion": current,
		"latestVersion":  latest,
		"hasUpdate":      hasUpdate,
		"source":         DefaultCLIProxyAPIPath,
	}, nil
}

func (e *Engine) updateKernel(ctx context.Context) (any, error) {
	e.gatewayMu.Lock()
	defer e.gatewayMu.Unlock()

	latest, _ := getLatestCLIProxyAPIVersion(DefaultCLIProxyAPIPath)
	targetDir := findThirdPartyDir()

	if info, err := os.Stat(DefaultCLIProxyAPIPath); err == nil && info.IsDir() {
		for _, sub := range []string{"internal", "sdk"} {
			srcSub := filepath.Join(DefaultCLIProxyAPIPath, sub)
			dstSub := filepath.Join(targetDir, sub)
			if _, err := os.Stat(srcSub); err != nil {
				continue
			}
			_ = filepath.Walk(srcSub, func(path string, info os.FileInfo, err error) error {
				if err != nil {
					return nil
				}
				baseName := filepath.Base(path)
				if strings.HasPrefix(baseName, "._") {
					if info.IsDir() {
						return filepath.SkipDir
					}
					return nil
				}
				rel, err := filepath.Rel(srcSub, path)
				if err != nil {
					return nil
				}
				targetPath := filepath.Join(dstSub, rel)
				if info.IsDir() {
					return os.MkdirAll(targetPath, 0755)
				}
				data, err := os.ReadFile(path)
				if err != nil {
					return nil
				}
				return os.WriteFile(targetPath, data, info.Mode())
			})
		}
	}

	versionFile := filepath.Join(targetDir, "version.txt")
	_ = os.WriteFile(versionFile, []byte(latest+"\n"), 0644)

	snapshot, err := e.repo.Update(nil, func(s *domain.Snapshot) error {
		s.Gateway.KernelVersion = latest
		s.Gateway.LatestKernelVersion = latest
		return nil
	})
	if err != nil {
		return nil, err
	}

	if snapshot.Gateway.State == "running" {
		stopCtx, cancel := context.WithTimeout(ctx, 15*time.Second)
		defer cancel()
		_ = e.runtime.Stop(stopCtx)
		_, _ = e.startGatewayWithState(ctx, nil, "restarting")
	}

	e.emitSnapshot("model_gateway_state_changed", snapshot)

	return map[string]any{
		"ok":      true,
		"version": latest,
		"message": fmt.Sprintf("内核已成功更新至 %s", latest),
	}, nil
}
