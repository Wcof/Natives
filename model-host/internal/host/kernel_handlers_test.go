package host

import (
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func TestKernelUpdateRepeatedRequestReturnsRunning(t *testing.T) {
	e := &Engine{kernelJob: &kernelUpdateJob{phase: "build"}}
	done := make(chan error, 1)
	go func() {
		result, err := e.updateKernel(context.Background())
		if err == nil && result.(map[string]any)["status"] != "running" {
			t.Error("active update was not reported as running")
		}
		done <- err
	}()
	select {
	case err := <-done:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(250 * time.Millisecond):
		t.Fatal("repeated update deadlocked while reading job snapshot")
	}
}

func TestKernelUpdateRejectsMajorMigrationAndInvalidTags(t *testing.T) {
	for _, target := range []string{"v9.0.0", "v8.4.0-beta.1", "v8.bad.1", "v8.04.0", "--output=file"} {
		t.Run(target, func(t *testing.T) {
			// 本用例测试目标版本拒绝（身份守卫本体由身份专项测试覆盖），注入身份通过。
			e := &Engine{kernelFetchLatest: func(context.Context) (string, error) { return target, nil }, kernelRunUpdate: func(*kernelUpdateJob, string) { t.Error("incompatible target reached updater") }, kernelIdentityCheck: func() error { return nil }}
			if _, err := e.updateKernel(context.Background()); err == nil {
				t.Fatal("incompatible target accepted")
			}
			if e.kernelJob.snapshot()["phase"] != "failed" {
				t.Fatal("job did not report failure")
			}
		})
	}
}

func TestKernelUpdateReservesJobBeforeNetworkDetection(t *testing.T) {
	entered, release, ran := make(chan struct{}), make(chan struct{}), make(chan struct{})
	var detections atomic.Int32
	e := &Engine{kernelFetchLatest: func(context.Context) (string, error) {
		detections.Add(1)
		close(entered)
		<-release
		return "v8.999.999", nil
	}, kernelRunUpdate: func(*kernelUpdateJob, string) { close(ran) }, kernelIdentityCheck: func() error { return nil }}
	finished := make(chan error, 1)
	go func() { _, err := e.updateKernel(context.Background()); finished <- err }()
	<-entered
	result, err := e.updateKernel(context.Background())
	close(release)
	if err != nil || result.(map[string]any)["status"] != "running" {
		t.Fatalf("duplicate response: %v %v", result, err)
	}
	if err := <-finished; err != nil {
		t.Fatal(err)
	}
	<-ran
	if detections.Load() != 1 {
		t.Fatal("duplicate network/build job")
	}
}

func TestPublishKernelSourceRemovesObsoleteFilesAndRollsBack(t *testing.T) {
	work := t.TempDir()
	fork, candidate := filepath.Join(work, "fork"), filepath.Join(work, "candidate")
	for _, root := range []string{fork, candidate} {
		for _, dir := range []string{"internal", "sdk"} {
			if err := os.MkdirAll(filepath.Join(root, dir), 0700); err != nil {
				t.Fatal(err)
			}
		}
		for _, name := range []string{"go.mod", "go.sum", "version.txt"} {
			if err := os.WriteFile(filepath.Join(root, name), []byte(root), 0600); err != nil {
				t.Fatal(err)
			}
		}
	}
	old := filepath.Join(fork, "internal", "obsolete.go")
	newFile := filepath.Join(candidate, "internal", "new.go")
	os.WriteFile(old, []byte("old"), 0600)
	os.WriteFile(newFile, []byte("new"), 0600)
	rollback, err := publishKernelSource(fork, candidate, work)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(old); !os.IsNotExist(err) {
		t.Fatal("obsolete source survived publication")
	}
	if err := rollback(); err != nil {
		t.Fatal(err)
	}
	if data, err := os.ReadFile(old); err != nil || string(data) != "old" {
		t.Fatal("old source was not restored")
	}
	if _, err := os.Stat(filepath.Join(fork, "internal", "new.go")); !os.IsNotExist(err) {
		t.Fatal("candidate file survived rollback")
	}
}

func TestExportKernelSourceUsesMatchingArchiveFormat(t *testing.T) {
	git, err := exec.LookPath("git")
	if err != nil {
		t.Skip("git unavailable")
	}
	repo, destination := t.TempDir(), t.TempDir()
	tools := toolchain{gitBin: git}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	for _, dir := range []string{"internal", "sdk"} {
		os.Mkdir(filepath.Join(repo, dir), 0700)
		os.WriteFile(filepath.Join(repo, dir, "test.go"), []byte("package test\n"), 0600)
	}
	for _, name := range []string{"go.mod", "go.sum"} {
		os.WriteFile(filepath.Join(repo, name), []byte("fixture\n"), 0600)
	}
	// Git history belongs solely to this disposable test fixture.
	for _, args := range [][]string{{"init", "--quiet"}, {"add", "."}, {"-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "--quiet", "-m", "fixture"}, {"tag", "v7.0.0"}} {
		if _, err := tools.run(ctx, repo, "git", args...); err != nil {
			t.Fatal(err)
		}
	}
	if err := tools.exportKernelSource(ctx, repo, "v7.0.0", destination); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"internal/test.go", "sdk/test.go", "go.mod", "go.sum"} {
		if _, err := os.Stat(filepath.Join(destination, name)); err != nil {
			t.Fatal(err)
		}
	}
}

func TestKernelFailureDoesNotExposeToolOutput(t *testing.T) {
	job := &kernelUpdateJob{phase: "build"}
	job.fail(fmt.Errorf("build failed: /Users/private token=must-not-leak"))
	result := job.snapshot()
	if result["errorCode"] != "internal_error" || strings.Contains(result["message"].(string), "must-not-leak") || strings.Contains(result["message"].(string), "/Users/") {
		t.Fatalf("unsafe update error: %#v", result)
	}
}

// mode/T09（A29）：身份守卫与零副作用矩阵。身份判定可注入（kernelIdentityCheck），
// 正式/未知身份在任务预留阶段失败——fetch 与换装执行器计数均为 0；
// developmentKernelExecutable 的三重判定（路径/普通文件/受控源码根）用临时树验证，
// 符号链接与伪造源码根不放行；页面传入的任何 dev 字段不参与判定。

func TestKernelUpdateFormalIdentityHasZeroSideEffects(t *testing.T) {
	var fetches, runs atomic.Int32
	e := &Engine{
		kernelFetchLatest: func(context.Context) (string, error) {
			fetches.Add(1)
			return "v7.99.0", nil
		},
		kernelRunUpdate: func(*kernelUpdateJob, string) { runs.Add(1) },
		kernelIdentityCheck: func() error {
			return &SafeError{Code: "kernel_update_requires_product_update", Message: "仅仓库 target 中的开发 Host 支持内核换装，请更新完整 Natives 产品"}
		},
	}
	if _, err := e.updateKernel(context.Background()); err == nil {
		t.Fatal("formal identity must not start a kernel update")
	}
	if fetches.Load() != 0 || runs.Load() != 0 {
		t.Fatalf("formal identity side effects: fetches=%d runs=%d", fetches.Load(), runs.Load())
	}
	snapshot := e.kernelJob.snapshot()
	if snapshot["phase"] != "failed" {
		t.Fatalf("job phase = %#v", snapshot)
	}
}

func TestKernelUpdateUnknownIdentityPathSpoofingRejected(t *testing.T) {
	// 伪造 target 路径（把二进制复制到仓库 target 布局）不能替代受控源码根：
	// 无 third_party/cliproxyapi/go.mod 模块行或 natives-patches 时拒绝。
	root := t.TempDir()
	exeDir := filepath.Join(root, "target", "debug")
	if err := os.MkdirAll(exeDir, 0o755); err != nil {
		t.Fatal(err)
	}
	exe := filepath.Join(exeDir, "model-host")
	if err := os.WriteFile(exe, []byte("fake"), 0o755); err != nil {
		t.Fatal(err)
	}
	if developmentKernelExecutable(root, exe) {
		t.Fatal("forged repo layout must be rejected without controlled source root")
	}
	// 伪造源码根缺补丁清单同样拒绝。
	forkDir := filepath.Join(root, "third_party", "cliproxyapi")
	if err := os.MkdirAll(forkDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(forkDir, "go.mod"), []byte("module github.com/router-for-me/CLIProxyAPI/v8\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if developmentKernelExecutable(root, exe) {
		t.Fatal("missing natives-patches must be rejected")
	}
	if err := os.MkdirAll(filepath.Join(forkDir, "natives-patches"), 0o755); err != nil {
		t.Fatal(err)
	}
	if !developmentKernelExecutable(root, exe) {
		t.Fatal("controlled dev layout must be accepted")
	}
	// 符号链接伪装：同名 symlink 指向任意位置，拒绝。
	real := filepath.Join(t.TempDir(), "elsewhere")
	if err := os.WriteFile(real, []byte("evil"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.Remove(exe); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(real, exe); err != nil {
		t.Fatal(err)
	}
	if developmentKernelExecutable(root, exe) {
		t.Fatal("symlink spoofing at the target path must be rejected")
	}
}

func TestKernelUpdateIdentityRunsBeforeNetworkDetection(t *testing.T) {
	// 与 A29 语义一致：身份失败先于 fetch（updateKernel 预留阶段）。
	entered := make(chan struct{})
	e := &Engine{
		kernelFetchLatest: func(context.Context) (string, error) {
			close(entered)
			return "v7.99.0", nil
		},
		kernelIdentityCheck: func() error { return errors.New("identity rejected") },
	}
	if _, err := e.updateKernel(context.Background()); err == nil {
		t.Fatal("expected identity failure")
	}
	select {
	case <-entered:
		t.Fatal("network detection ran before identity check")
	default:
	}
}
