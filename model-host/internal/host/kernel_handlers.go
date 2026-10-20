package host

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"

	clipadapter "github.com/ldh/natives/model-host/internal/cliproxy"
	"github.com/ldh/natives/model-host/internal/domain"
)

const (
	githubReleasesLatestAPI = "https://api.github.com/repos/router-for-me/CLIProxyAPI/releases/latest"
	githubReleasesAtomURL   = "https://github.com/router-for-me/CLIProxyAPI/releases.atom"
	githubUpstreamRepoURL   = "https://github.com/router-for-me/CLIProxyAPI"
	// 内核更新换装后 worker 以该退出码请求桥接进程重建自己（加载新二进制）
	workerRestartExitCode = 75
	// 上游源码检出路径可用该环境变量覆盖；未设置时从 GitHub 缓存克隆
	kernelSourceEnv = "NATIVES_KERNEL_SOURCE"
)

var (
	// Set by the development updater at build time; never infer loaded code from disk.
	compiledKernelVersion     = ""
	cachedLatestKernelVersion = ""
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

func kernelVersionLess(current, latest string) bool {
	left, ok := kernelVersionParts(current)
	right, valid := kernelVersionParts(latest)
	if !ok || !valid {
		return false
	}
	for i := range left {
		if left[i] != right[i] {
			return left[i] < right[i]
		}
	}
	return false
}

// Only stable vMAJOR.MINOR.PATCH tags can be used as archive/build targets.
func kernelVersionParts(version string) ([3]int, bool) {
	var values [3]int
	if !strings.HasPrefix(version, "v") {
		return values, false
	}
	parts := strings.Split(strings.TrimPrefix(version, "v"), ".")
	if len(parts) != 3 {
		return values, false
	}
	for i, part := range parts {
		if part == "" || (len(part) > 1 && part[0] == '0') {
			return values, false
		}
		for _, c := range part {
			if c < '0' || c > '9' {
				return values, false
			}
		}
		n, err := strconv.Atoi(part)
		if err != nil {
			return values, false
		}
		values[i] = n
	}
	return values, true
}

// kernelRepoRoot 定位 Natives 仓库根（third_party/cliproxyapi 的宿主）。
// Chrome 拉起的 worker cwd 不可靠，优先从可执行文件路径推导
// （开发布局：<repo>/target/debug/model-host）；推导失败再回退 cwd 上溯。
func kernelRepoRoot() string {
	if exe, err := os.Executable(); err == nil {
		dir := filepath.Dir(exe)
		for _, rel := range []string{"../..", ".."} {
			candidate := filepath.Clean(filepath.Join(dir, rel))
			if info, err := os.Stat(filepath.Join(candidate, "third_party", "cliproxyapi")); err == nil && info.IsDir() {
				return candidate
			}
		}
	}
	dir, err := os.Getwd()
	if err != nil {
		dir = "."
	}
	for i := 0; i < 6; i++ {
		candidate := filepath.Join(dir, "third_party", "cliproxyapi")
		if info, err := os.Stat(candidate); err == nil && info.IsDir() {
			return dir
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			break
		}
		dir = parent
	}
	return ""
}

func getLocalKernelVersion() string {
	if compiledKernelVersion == "" {
		return clipadapter.CompiledKernelVersion()
	}
	return compiledKernelVersion
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

	return "", fmt.Errorf("无法从 GitHub 获取最新版本")
}

func (e *Engine) checkKernelUpdate() (any, error) {
	current := getLocalKernelVersion()
	fetch := e.kernelFetchLatest
	if fetch == nil {
		fetch = fetchOnlineLatestKernelVersion
	}

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	latest, err := fetch(ctx)
	if err == nil && latest != "" {
		setCachedLatestKernelVersion(latest)
	} else {
		latest = getCachedLatestKernelVersion()
	}

	currentParts, currentValid := kernelVersionParts(current)
	latestParts, latestValid := kernelVersionParts(latest)
	migrationRequired := currentValid && latestValid && latestParts[0] > currentParts[0]
	hasUpdate := !migrationRequired && latest != "" && current != "" && kernelVersionLess(current, latest)

	_, _ = e.repo.Update(nil, func(snapshot *domain.Snapshot) error {
		snapshot.Gateway.KernelVersion = current
		if latest != "" {
			snapshot.Gateway.LatestKernelVersion = latest
		}
		return nil
	})

	return map[string]any{
		"currentVersion":    current,
		"latestVersion":     latest,
		"hasUpdate":         hasUpdate,
		"migrationRequired": migrationRequired,
		"source":            githubUpstreamRepoURL,
	}, nil
}

// ===== 真实内核升级管线 =====
// 内核 = model-host 二进制（内嵌 Natives 裁剪版 CLIProxyAPI）。真实升级 = 从上游
// 源码导出 internal/sdk → 重放 natives-patches → go build → 原子换装二进制 →
// version.txt 落账 → worker 以退出码 75 请求桥接重建自己加载新内核。
// 仅开发源码树可用（生产安装包内无源码与工具链时如实报错）。

type kernelUpdateJob struct {
	mu        sync.Mutex
	phase     string // idle/queued/source/patches/build/swap/restarting/done/failed
	message   string
	errorCode string
	from, to  string
	startedAt time.Time
	updatedAt time.Time
}

var kernelRunningPhases = map[string]bool{
	"checking": true, "queued": true, "source": true, "patches": true,
	"build": true, "swap": true, "restarting": true,
}

func (j *kernelUpdateJob) set(phase, message string) {
	j.mu.Lock()
	defer j.mu.Unlock()
	j.phase = phase
	j.message = message
	j.updatedAt = time.Now()
}

func (j *kernelUpdateJob) snapshot() map[string]any {
	j.mu.Lock()
	defer j.mu.Unlock()
	return map[string]any{
		"phase":     j.phase,
		"message":   j.message,
		"errorCode": j.errorCode,
		"from":      j.from,
		"to":        j.to,
		"startedAt": j.startedAt.Unix(),
		"updatedAt": j.updatedAt.Unix(),
	}
}

func (j *kernelUpdateJob) fail(err error) {
	code, message := classify(err)
	j.mu.Lock()
	defer j.mu.Unlock()
	j.phase, j.message, j.errorCode = "failed", message, code
	j.updatedAt = time.Now()
}

type toolchain struct {
	gitBin string
	goBin  string
}

func findTool(name string, extra []string) (string, error) {
	if p, err := exec.LookPath(name); err == nil {
		return p, nil
	}
	home, _ := os.UserHomeDir()
	for _, candidate := range extra {
		if home != "" {
			candidate = strings.ReplaceAll(candidate, "$HOME", home)
		}
		if info, err := os.Stat(candidate); err == nil && !info.IsDir() && info.Mode()&0111 != 0 {
			return candidate, nil
		}
	}
	return "", fmt.Errorf("未找到 %s 工具链，无法在线升级内核（仅开发环境支持）", name)
}

func (t toolchain) run(ctx context.Context, dir, name string, args ...string) (string, error) {
	bin := t.gitBin
	if name == "go" {
		bin = t.goBin
	}
	cmd := exec.CommandContext(ctx, bin, args...)
	cmd.Dir = dir
	out, err := cmd.CombinedOutput()
	if err != nil {
		tail := string(out)
		if len(tail) > 800 {
			tail = tail[len(tail)-800:]
		}
		return tail, fmt.Errorf("%s %s: %w: %s", name, strings.Join(args, " "), err, tail)
	}
	return string(out), nil
}

// resolveKernelSource 返回含目标 tag 的上游 git 检出目录。
func (t toolchain) resolveKernelSource(ctx context.Context, tag string) (string, error) {
	if override := strings.TrimSpace(os.Getenv(kernelSourceEnv)); override != "" {
		if _, err := t.run(ctx, override, "git", "rev-parse", "-q", "--verify", fmt.Sprintf("refs/tags/%s^{commit}", tag)); err == nil {
			return override, nil
		}
		return "", fmt.Errorf("源码目录 %s 不含 %s（可用 %s 指向含该 tag 的上游检出）", override, tag, kernelSourceEnv)
	}

	cache := filepath.Join(kernelRepoRoot(), ".tmp", "kernel-update", "upstream")
	if err := os.MkdirAll(filepath.Dir(cache), 0700); err != nil {
		return "", err
	}
	if info, statErr := os.Stat(filepath.Join(cache, ".git")); statErr == nil && info.IsDir() {
		_, err := t.run(ctx, cache, "git", "fetch", "--depth", "1", "origin", fmt.Sprintf("refs/tags/%s:refs/tags/%s", tag, tag))
		if err != nil {
			return "", fmt.Errorf("获取上游 tag 失败: %w", err)
		}
		return cache, nil
	}
	if _, err := t.run(ctx, filepath.Dir(cache), "git", "clone", "--depth", "1", "--branch", tag, githubUpstreamRepoURL, cache); err != nil {
		return "", fmt.Errorf("克隆上游源码失败: %v", err)
	}
	return cache, nil
}

func (e *Engine) updateKernel(ctx context.Context) (any, error) {
	e.kernelMu.Lock()
	if e.kernelJob == nil {
		e.kernelJob = &kernelUpdateJob{phase: "idle"}
	}
	job := e.kernelJob
	e.kernelMu.Unlock()
	job.mu.Lock()
	if kernelRunningPhases[job.phase] {
		job.mu.Unlock()
		return map[string]any{"status": "running", "job": job.snapshot()}, nil
	}
	job.phase = "checking"
	job.errorCode = ""
	job.mu.Unlock()

	// mode/T09：开发身份核验先于任何网络/构建副作用——正式/未知身份在此
	// 稳定失败，fetch 与换装执行器计数保持 0（A29）。
	identityCheck := e.kernelIdentityCheck
	if identityCheck == nil {
		identityCheck = kernelDevIdentityOk
	}
	if err := identityCheck(); err != nil {
		job.fail(err)
		return nil, err
	}

	// 目标版本判定使用独立超时，不跟随请求 ctx（请求返回后任务继续执行）
	fetch := e.kernelFetchLatest
	if fetch == nil {
		fetch = fetchOnlineLatestKernelVersion
	}
	run := e.kernelRunUpdate
	if run == nil {
		run = e.runKernelUpdate
	}
	detectCtx, cancel := context.WithTimeout(e.kernelContext(), 10*time.Second)
	defer cancel()
	latest, _ := fetch(detectCtx)
	if latest == "" {
		latest = getCachedLatestKernelVersion()
	}
	if latest == "" {
		job.set("failed", "无法获取上游最新版本")
		return nil, fmt.Errorf("无法获取上游最新版本（网络不可用？）")
	}
	current := getLocalKernelVersion()
	currentParts, currentValid := kernelVersionParts(current)
	latestParts, valid := kernelVersionParts(latest)
	if !currentValid || !valid || currentParts[0] != latestParts[0] {
		err := &SafeError{Code: "kernel_migration_required", Message: "上游内核需要 SDK 与本地补丁迁移，请更新完整 Natives 产品"}
		job.fail(err)
		return nil, err
	}
	if current != "" && !kernelVersionLess(current, latest) {
		_, _ = e.repo.Update(nil, func(s *domain.Snapshot) error {
			s.Gateway.KernelVersion = current
			s.Gateway.LatestKernelVersion = latest
			return nil
		})
		job.set("idle", "内核已是当前版本")
		return map[string]any{"status": "up_to_date", "currentVersion": current, "latestVersion": latest}, nil
	}

	job.mu.Lock()
	job.phase = "queued"
	job.message = "等待开始"
	job.from = current
	job.to = latest
	job.startedAt = time.Now()
	job.updatedAt = job.startedAt
	job.mu.Unlock()

	// 任务与请求生命周期解耦：构建耗时可达数分钟
	go run(job, latest)

	return map[string]any{"status": "started", "job": job.snapshot()}, nil
}

func (e *Engine) kernelUpdateStatus() (any, error) {
	e.kernelMu.Lock()
	if e.kernelJob == nil {
		e.kernelJob = &kernelUpdateJob{phase: "idle"}
	}
	e.kernelMu.Unlock()
	return map[string]any{
		"job":           e.kernelJob.snapshot(),
		"kernelVersion": getLocalKernelVersion(),
	}, nil
}

func (e *Engine) runKernelUpdate(job *kernelUpdateJob, target string) {
	ctx, cancel := context.WithTimeout(e.kernelContext(), 20*time.Minute)
	defer cancel()
	fail := job.fail
	// mode/T09：身份已在 updateKernel 预留阶段核验（kernelDevIdentityOk），
	// 这里只做防御性复核，失败同样不触碰任何副作用。
	if err := kernelDevIdentityOk(); err != nil {
		fail(err)
		return
	}
	repoRoot := kernelRepoRoot()
	if repoRoot == "" {
		fail(fmt.Errorf("未找到内核源码树，仅开发环境支持在线升级"))
		return
	}
	exePath, err := os.Executable()
	if err != nil {
		fail(err)
		return
	}
	gitBin, err := findTool("git", []string{"/usr/bin/git", "/opt/homebrew/bin/git", "/usr/local/bin/git"})
	if err != nil {
		fail(err)
		return
	}
	goBin, err := findTool("go", []string{"/usr/local/go/bin/go", "/opt/homebrew/bin/go", "/usr/local/bin/go", "$HOME/go/bin/go"})
	if err != nil {
		fail(err)
		return
	}
	tools := toolchain{gitBin: gitBin, goBin: goBin}
	workRoot := filepath.Join(repoRoot, ".tmp", "kernel-update")
	if err := os.MkdirAll(workRoot, 0700); err != nil {
		fail(err)
		return
	}
	work, err := os.MkdirTemp(workRoot, "candidate-")
	if err != nil {
		fail(err)
		return
	}
	defer func() {
		if work != "" {
			_ = os.RemoveAll(work)
		}
	}()
	fork := filepath.Join(repoRoot, "third_party", "cliproxyapi")
	candidate := filepath.Join(work, "cliproxyapi")
	if err := os.Mkdir(candidate, 0700); err != nil {
		fail(err)
		return
	}
	job.set("source", fmt.Sprintf("获取上游 %s 源码", target))
	repo, err := tools.resolveKernelSource(ctx, target)
	if err != nil {
		fail(err)
		return
	}
	if err = tools.exportKernelSource(ctx, repo, target, candidate); err != nil {
		fail(err)
		return
	}
	metadata, err := os.ReadFile(filepath.Join(candidate, "go.mod"))
	if err != nil || !strings.HasPrefix(string(metadata), "module github.com/router-for-me/CLIProxyAPI/v8\n") {
		fail(fmt.Errorf("上游 SDK 模块路径不兼容，拒绝换装"))
		return
	}
	job.set("patches", "重放 Natives 本地补丁")
	if _, err = tools.run(ctx, candidate, "git", "init", "--quiet"); err != nil {
		fail(err)
		return
	}
	entries, err := os.ReadDir(filepath.Join(fork, "natives-patches"))
	if err != nil {
		fail(fmt.Errorf("缺少 natives-patches 补丁清单"))
		return
	}
	var names []string
	for _, entry := range entries {
		if !entry.IsDir() && strings.HasSuffix(entry.Name(), ".patch") {
			names = append(names, entry.Name())
		}
	}
	sort.Strings(names)
	if len(names) == 0 {
		fail(fmt.Errorf("拒绝无补丁的裸升级"))
		return
	}
	for _, name := range names {
		if _, err = tools.run(ctx, candidate, "git", "apply", "--whitespace=nowarn", filepath.Join(fork, "natives-patches", name)); err != nil {
			fail(fmt.Errorf("补丁 %s 应用失败: %v", name, err))
			return
		}
	}
	versionSource := "package cliproxy\n\n// Generated by the Natives development updater.\nconst NativesKernelVersion = " + strconv.Quote(target) + "\n"
	if err = os.WriteFile(filepath.Join(candidate, "sdk", "cliproxy", "natives_version.go"), []byte(versionSource), 0644); err != nil {
		fail(err)
		return
	}
	modelHostDir := filepath.Join(repoRoot, "model-host")
	modfile, err := stageKernelModfile(modelHostDir, work, candidate)
	if err != nil {
		fail(err)
		return
	}
	job.set("build", "验证并构建候选内核")
	if _, err = tools.run(ctx, modelHostDir, "go", "test", "-mod=readonly", "-modfile="+modfile, "-timeout=120s", "./..."); err != nil {
		fail(err)
		return
	}
	binary := filepath.Join(work, "model-host")
	if _, err = tools.run(ctx, modelHostDir, "go", "build", "-mod=readonly", "-modfile="+modfile, "-ldflags=-X github.com/ldh/natives/model-host/internal/host.compiledKernelVersion="+target, "-o", binary, "."); err != nil {
		fail(err)
		return
	}
	if err = os.WriteFile(filepath.Join(candidate, "version.txt"), []byte(target+"\n"), 0644); err != nil {
		fail(err)
		return
	}
	if err = ctx.Err(); err != nil {
		fail(err)
		return
	}
	job.set("swap", "换装已验证的开发内核")
	rollback, err := publishKernelSource(fork, candidate, work)
	if err != nil {
		// Keep any failed rollback's recovery tree; never discard its backups.
		work = ""
		fail(err)
		return
	}
	if err = os.Rename(binary, exePath); err != nil {
		if restoreErr := rollback(); restoreErr != nil {
			// Preserve recovery evidence rather than deleting the backup.
			work = ""
			fail(fmt.Errorf("换装失败且源码恢复失败: %v; %v", err, restoreErr))
			return
		}
		fail(fmt.Errorf("换装失败，源码已恢复: %v", err))
		return
	}
	// The running process still contains the old SDK. Only its replacement can
	// report the target version or restart the gateway using the new code.
	job.set("restarting", fmt.Sprintf("候选内核 %s 已换装，等待重新加载", target))
	select {
	case <-ctx.Done():
		return
	case <-time.After(2 * time.Second):
		os.Exit(workerRestartExitCode)
	}
}

func tailBytes(data []byte, n int) string {
	if len(data) <= n {
		return string(data)
	}
	return string(data[len(data)-n:])
}
