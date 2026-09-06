package host

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/ldh/natives/model-host/internal/domain"
)

const (
	githubReleasesLatestAPI = "https://api.github.com/repos/router-for-me/CLIProxyAPI/releases/latest"
	githubReleasesAtomURL   = "https://github.com/router-for-me/CLIProxyAPI/releases.atom"
)

var (
	cachedLatestKernelVersion = "v7.2.152"
	cachedLatestKernelMu      sync.RWMutex
)

func getCachedLatestKernelVersion() string {
	cachedLatestKernelMu.RLock()
	v := cachedLatestKernelVersion
	cachedLatestKernelMu.RUnlock()
	local := getLocalKernelVersion()
	if kernelVersionLess(v, local) {
		return local
	}
	return v
}

func setCachedLatestKernelVersion(v string) {
	cachedLatestKernelMu.Lock()
	defer cachedLatestKernelMu.Unlock()
	cachedLatestKernelVersion = v
}

func kernelVersionLess(current, latest string) bool {
	parse := func(raw string) []int {
		raw = strings.TrimPrefix(strings.TrimSpace(raw), "v")
		parts := strings.Split(raw, ".")
		values := make([]int, len(parts))
		for i, part := range parts {
			values[i], _ = strconv.Atoi(part)
		}
		return values
	}
	left, right := parse(current), parse(latest)
	for i := 0; i < len(left) || i < len(right); i++ {
		lv, rv := 0, 0
		if i < len(left) { lv = left[i] }
		if i < len(right) { rv = right[i] }
		if lv != rv { return lv < rv }
	}
	return false
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

// fetchOnlineLatestKernelVersion fetches the latest release tag from GitHub online
func fetchOnlineLatestKernelVersion(ctx context.Context) (string, error) {
	client := &http.Client{Timeout: 8 * time.Second}

	// 1. Try GitHub Releases API
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, githubReleasesLatestAPI, nil)
	if err == nil {
		req.Header.Set("User-Agent", "Natives-Model-Host")
		req.Header.Set("Accept", "application/vnd.github.v3+json")
		if resp, err := client.Do(req); err == nil {
			defer resp.Body.Close()
			if resp.StatusCode == http.StatusOK {
				body, _ := io.ReadAll(io.LimitReader(resp.Body, 512*1024))
				var release struct {
					TagName string `json:"tag_name"`
				}
				if json.Unmarshal(body, &release) == nil && release.TagName != "" {
					tag := strings.TrimSpace(release.TagName)
					if !strings.HasPrefix(tag, "v") {
						tag = "v" + tag
					}
					return tag, nil
				}
			}
		}
	}

	// 2. Fallback to GitHub Releases Atom feed
	atomReq, err := http.NewRequestWithContext(ctx, http.MethodGet, githubReleasesAtomURL, nil)
	if err == nil {
		atomReq.Header.Set("User-Agent", "Natives-Model-Host")
		atomReq.Header.Set("Accept", "application/atom+xml,text/xml")
		if resp, err := client.Do(atomReq); err == nil {
			defer resp.Body.Close()
			if resp.StatusCode == http.StatusOK {
				body, _ := io.ReadAll(io.LimitReader(resp.Body, 512*1024))
				content := string(body)
				if idx := strings.Index(content, "/releases/tag/"); idx != -1 {
					sub := content[idx+len("/releases/tag/"):]
					end := strings.IndexAny(sub, "\"< \r\n")
					if end != -1 {
						tag := strings.TrimSpace(sub[:end])
						if tag != "" {
							if !strings.HasPrefix(tag, "v") {
								tag = "v" + tag
							}
							return tag, nil
						}
					}
				}
			}
		}
	}

	return getCachedLatestKernelVersion(), nil
}

func (e *Engine) checkKernelUpdate() (any, error) {
	current := getLocalKernelVersion()

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	latest, err := fetchOnlineLatestKernelVersion(ctx)
	if err == nil && latest != "" {
		setCachedLatestKernelVersion(latest)
	} else {
		latest = getCachedLatestKernelVersion()
	}

	hasUpdate := latest != "" && kernelVersionLess(current, latest)

	_, _ = e.repo.Update(nil, func(snapshot *domain.Snapshot) error {
		snapshot.Gateway.KernelVersion = current
		snapshot.Gateway.LatestKernelVersion = latest
		return nil
	})

	return map[string]any{
		"currentVersion": current,
		"latestVersion":  latest,
		"hasUpdate":      hasUpdate,
		"source":         "https://github.com/router-for-me/CLIProxyAPI",
	}, nil
}

func (e *Engine) updateKernel(ctx context.Context) (any, error) {
	e.gatewayMu.Lock()
	defer e.gatewayMu.Unlock()

	latest, _ := fetchOnlineLatestKernelVersion(ctx)
	if latest == "" {
		latest = getCachedLatestKernelVersion()
	}

	targetDir := findThirdPartyDir()
	_ = os.MkdirAll(targetDir, 0755)
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
		"message": fmt.Sprintf("内核版本记录已同步为 %s", latest),
	}, nil
}
