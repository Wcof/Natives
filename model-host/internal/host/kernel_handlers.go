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
		if i < len(left) {
			lv = left[i]
		}
		if i < len(right) {
			rv = right[i]
		}
		if lv != rv {
			return lv < rv
		}
	}
	return false
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

func findThirdPartyDir() string {
	if root := kernelRepoRoot(); root != "" {
		return filepath.Join(root, "third_party", "cliproxyapi")
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
	return ""
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
	if e.kernelFetchLatest == nil {
		e.kernelFetchLatest = fetchOnlineLatestKernelVersion
	}

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	latest, err := e.kernelFetchLatest(ctx)
	if err == nil && latest != "" {
		setCachedLatestKernelVersion(latest)
	} else {
		latest = getCachedLatestKernelVersion()
	}

	hasUpdate := latest != "" && current != "" && kernelVersionLess(current, latest)

	_, _ = e.repo.Update(nil, func(snapshot *domain.Snapshot) error {
		snapshot.Gateway.KernelVersion = current
		if latest != "" {
			snapshot.Gateway.LatestKernelVersion = latest
		}
		return nil
	})

	return map[string]any{
		"currentVersion": current,
		"latestVersion":  latest,
		"hasUpdate":      hasUpdate,
		"source":         githubUpstreamRepoURL,
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
	from, to  string
	startedAt time.Time
	updatedAt time.Time
}

var kernelRunningPhases = map[string]bool{
	"queued": true, "source": true, "patches": true,
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
		"from":      j.from,
		"to":        j.to,
		"startedAt": j.startedAt.Unix(),
		"updatedAt": j.updatedAt.Unix(),
	}
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

func (t toolchain) tarGz(ctx context.Context, args ...string) (string, error) {
	tarBin, err := exec.LookPath("tar")
	if err != nil {
		return "", fmt.Errorf("未找到 tar: %w", err)
	}
	cmd := exec.CommandContext(ctx, tarBin, args...)
	out, err := cmd.CombinedOutput()
	return string(out), err
}

// resolveKernelSource 返回含目标 tag 的上游 git 检出目录。
func (t toolchain) resolveKernelSource(ctx context.Context, tag string) (string, error) {
	if override := strings.TrimSpace(os.Getenv(kernelSourceEnv)); override != "" {
		if _, err := t.run(ctx, override, "git", "fetch", "--depth", "1", "--force", "origin", fmt.Sprintf("refs/tags/%s:refs/tags/%s", tag, tag)); err != nil {
			// 本地检出可能已含该 tag，fetch 失败不致命，继续校验
			_ = err
		}
		if _, err := t.run(ctx, override, "git", "rev-parse", "-q", "--verify", fmt.Sprintf("refs/tags/%s^{commit}", tag)); err == nil {
			return override, nil
		}
		return "", fmt.Errorf("源码目录 %s 不含 %s（可用 %s 指向含该 tag 的上游检出）", override, tag, kernelSourceEnv)
	}

	cacheDir, err := os.UserCacheDir()
	if err != nil {
		return "", fmt.Errorf("无法定位缓存目录: %w", err)
	}
	cache := filepath.Join(cacheDir, "natives", "kernel-upstream")
	if info, statErr := os.Stat(filepath.Join(cache, ".git")); statErr == nil && info.IsDir() {
		if _, err := t.run(ctx, cache, "git", "fetch", "--depth", "1", "--force", "origin", fmt.Sprintf("refs/tags/%s:refs/tags/%s", tag, tag)); err == nil {
			return cache, nil
		}
		// 缓存仓库失效（浅克隆缺历史等）时重建
		_ = os.RemoveAll(cache)
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
		snap := job.snapshot()
		job.mu.Unlock()
		return map[string]any{"status": "running", "job": snap}, nil
	}
	job.mu.Unlock()

	// 目标版本判定使用独立超时，不跟随请求 ctx（请求返回后任务继续执行）
	if e.kernelFetchLatest == nil {
		e.kernelFetchLatest = fetchOnlineLatestKernelVersion
	}
	if e.kernelRunUpdate == nil {
		e.kernelRunUpdate = e.runKernelUpdate
	}
	detectCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	latest, _ := e.kernelFetchLatest(detectCtx)
	if latest == "" {
		latest = getCachedLatestKernelVersion()
	}
	if latest == "" {
		return nil, fmt.Errorf("无法获取上游最新版本（网络不可用？）")
	}
	current := getLocalKernelVersion()
	if current != "" && !kernelVersionLess(current, latest) {
		_, _ = e.repo.Update(nil, func(s *domain.Snapshot) error {
			s.Gateway.KernelVersion = current
			s.Gateway.LatestKernelVersion = latest
			return nil
		})
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
	go e.kernelRunUpdate(job, latest)

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
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Minute)
	defer cancel()

	fail := func(phase, format string, args ...any) {
		job.set(phase, fmt.Sprintf(format, args...))
	}
	restore := func(backupTar string) {
		fork := findThirdPartyDir()
		if backupTar != "" {
			_, _ = exec.Command("tar", "-xzf", backupTar, "-C", fork).CombinedOutput()
		}
	}

	tools := toolchain{}
	var backupTar, staging string
	defer func() {
		if staging != "" {
			_ = os.Remove(staging)
		}
	}()

	// 0) 工具链与源码树
	job.set("queued", "检查构建环境")
	repoRoot := kernelRepoRoot()
	if repoRoot == "" {
		fail("failed", "未找到内核源码树（third_party/cliproxyapi），仅开发环境支持在线升级")
		return
	}
	fork := filepath.Join(repoRoot, "third_party", "cliproxyapi")
	modelHostDir := filepath.Join(repoRoot, "model-host")
	exePath, err := os.Executable()
	if err != nil {
		fail("failed", "无法定位当前内核二进制: %v", err)
		return
	}
	gitBin, err := findTool("git", []string{"/usr/bin/git", "/opt/homebrew/bin/git", "/usr/local/bin/git"})
	if err != nil {
		fail("failed", "%v", err)
		return
	}
	tools.gitBin = gitBin
	goBin, err := findTool("go", []string{"/usr/local/go/bin/go", "/opt/homebrew/bin/go", "/usr/local/bin/go", "$HOME/go/bin/go"})
	if err != nil {
		fail("failed", "%v", err)
		return
	}
	tools.goBin = goBin

	// 1) 上游源码
	job.set("source", fmt.Sprintf("获取上游 %s 源码", target))
	repo, err := tools.resolveKernelSource(ctx, target)
	if err != nil {
		fail("failed", "%v", err)
		return
	}
	if _, err := tools.run(ctx, repo, "git", "rev-parse", "-q", "--verify", fmt.Sprintf("refs/tags/%s^{commit}", target)); err != nil {
		fail("failed", "上游不含 %s", target)
		return
	}

	// 2) 备份当前内核树（回滚兜底）
	job.set("source", "备份当前内核源码")
	backupTar = filepath.Join(os.TempDir(), fmt.Sprintf("natives-kernel-backup-%d.tar.gz", time.Now().UnixNano()))
	if out, err := tools.tarGz(ctx, "-czf", backupTar, "-C", fork, "internal", "sdk", "go.mod", "go.sum", "version.txt"); err != nil {
		fail("failed", "备份内核源码失败: %v: %s", err, out)
		return
	}

	// 3) 导出上游 internal/sdk（git archive 管道进 tar 解包）
	job.set("source", fmt.Sprintf("导出 %s internal/sdk", target))
	_ = os.RemoveAll(filepath.Join(fork, "internal"))
	_ = os.RemoveAll(filepath.Join(fork, "sdk"))
	archive := exec.CommandContext(ctx, tools.gitBin, "-C", repo, "archive", target, "--", "internal", "sdk", "go.mod", "go.sum")
	archiveStdout, pipeErr := archive.StdoutPipe()
	if pipeErr != nil {
		restore(backupTar)
		fail("failed", "建立源码导出管道失败: %v", pipeErr)
		return
	}
	extract := exec.CommandContext(ctx, "tar", "-xzf", "-", "-C", fork)
	extract.Stdin = archiveStdout
	extract.Stderr = archive.Stderr
	if err := archive.Start(); err != nil {
		restore(backupTar)
		fail("failed", "导出上游源码失败: %v", err)
		return
	}
	if err := extract.Run(); err != nil {
		restore(backupTar)
		fail("failed", "解包上游源码失败: %v", err)
		return
	}
	if err := archive.Wait(); err != nil {
		restore(backupTar)
		fail("failed", "导出上游源码失败: %v", err)
		return
	}
	if info, err := os.Stat(filepath.Join(fork, "internal")); err != nil || !info.IsDir() {
		restore(backupTar)
		fail("failed", "上游源码导出不完整")
		return
	}

	// 4) 重放 Natives 补丁
	job.set("patches", "重放 Natives 本地补丁")
	entries, err := os.ReadDir(filepath.Join(fork, "natives-patches"))
	if err != nil || len(entries) == 0 {
		restore(backupTar)
		fail("failed", "缺少 natives-patches 补丁清单，拒绝无补丁的裸升级")
		return
	}
	names := make([]string, 0, len(entries))
	for _, entry := range entries {
		if !entry.IsDir() && strings.HasSuffix(entry.Name(), ".patch") {
			names = append(names, entry.Name())
		}
	}
	sort.Strings(names)
	if len(names) == 0 {
		restore(backupTar)
		fail("failed", "缺少 natives-patches 补丁清单，拒绝无补丁的裸升级")
		return
	}
	for _, name := range names {
		if _, err := tools.run(ctx, fork, "git", "apply", "--whitespace=nowarn", filepath.Join("natives-patches", name)); err != nil {
			restore(backupTar)
			fail("failed", "补丁 %s 应用失败: %v", name, err)
			return
		}
	}

	// 5) 构建新内核二进制
	job.set("build", "正在构建新内核（约 1 分钟）")
	staging = filepath.Join(filepath.Dir(exePath), ".model-host-new")
	_ = os.Remove(staging)
	build := exec.CommandContext(ctx, tools.goBin, "build", "-o", staging, ".")
	build.Dir = modelHostDir
	build.Env = os.Environ()
	if out, err := build.CombinedOutput(); err != nil {
		restore(backupTar)
		fail("failed", "内核构建失败: %v: %s", err, tailBytes(out, 800))
		return
	}
	if info, err := os.Stat(staging); err != nil || info.Size() < 1<<20 {
		restore(backupTar)
		fail("failed", "构建产物异常，已回滚")
		return
	}

	// 6) 原子换装（保留旧二进制备份）
	job.set("swap", "换装内核二进制")
	backupBin := filepath.Join(filepath.Dir(exePath), ".model-host-bak")
	_ = os.Remove(backupBin)
	if err := os.Rename(exePath, backupBin); err != nil {
		restore(backupTar)
		fail("failed", "备份旧内核失败: %v", err)
		return
	}
	if err := os.Rename(staging, exePath); err != nil {
		_ = os.Rename(backupBin, exePath)
		staging = ""
		restore(backupTar)
		fail("failed", "换装新内核失败: %v", err)
		return
	}
	staging = ""

	// 7) 版本落账（构建成功后才写，避免假账）
	if err := os.WriteFile(filepath.Join(fork, "version.txt"), []byte(target+"\n"), 0644); err != nil {
		fail("failed", "写入版本记录失败: %v", err)
		return
	}

	// 8) 重启网关并广播快照
	job.set("restarting", "重启本地代理网关")
	snapshot, err := e.repo.Load()
	if err == nil {
		wasRunning := snapshot.Gateway.State == "running"
		if wasRunning {
			stopCtx, stopCancel := context.WithTimeout(ctx, 15*time.Second)
			_ = e.runtime.Stop(stopCtx)
			stopCancel()
		}
		snapshot, err = e.repo.Update(nil, func(s *domain.Snapshot) error {
			s.Gateway.KernelVersion = target
			s.Gateway.LatestKernelVersion = target
			return nil
		})
		if err == nil {
			if wasRunning {
				if _, err := e.startGatewayWithState(ctx, nil, "restarting"); err != nil {
					job.set("restarting", fmt.Sprintf("网关自动恢复失败，可手动启动: %v", err))
				}
			}
			e.emitSnapshot("model_gateway_state_changed", snapshot)
		}
	}

	job.set("done", fmt.Sprintf("内核已更新至 %s，正在加载新版本", target))
	// 数据落地后让位：worker 以 75 退出，桥接进程自动重建 worker 加载新二进制
	time.AfterFunc(2*time.Second, func() {
		os.Exit(workerRestartExitCode)
	})
}

func tailBytes(data []byte, n int) string {
	if len(data) <= n {
		return string(data)
	}
	return string(data[len(data)-n:])
}
