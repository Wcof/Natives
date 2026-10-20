package host

import (
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime/debug"
	"strconv"
	"strings"
)

func (e *Engine) kernelContext() context.Context {
	if e.kernelCtx != nil {
		return e.kernelCtx
	}
	return context.Background()
}

// developmentKernelExecutable 判定运行中的 Host 是否为受核验的开发构建
// （mode/T09，A29 / R-M10）：目录位置只是必要条件，不构成身份——还需
// ① 实际可执行文件是普通文件而非符号链接（防同名链接伪装）；
// ② 仓库根是受控源码（内核 fork 的 go.mod 模块行 + Natives 补丁清单）。
func developmentKernelExecutable(root, executable string) bool {
	matched := false
	for _, mode := range []string{"debug", "release"} {
		if executable == filepath.Join(root, "target", mode, "model-host") {
			matched = true
			break
		}
	}
	if !matched {
		return false
	}
	info, err := os.Lstat(executable)
	if err != nil || !info.Mode().IsRegular() || info.Mode()&os.ModeSymlink != 0 {
		return false
	}
	return controlledKernelSourceRoot(root)
}

// controlledKernelSourceRoot 校验仓库根持有受控内核源与补丁清单。
func controlledKernelSourceRoot(root string) bool {
	forkMod, err := os.ReadFile(filepath.Join(root, "third_party", "cliproxyapi", "go.mod"))
	if err != nil || !strings.Contains(string(forkMod), "module github.com/router-for-me/CLIProxyAPI/v8") {
		return false
	}
	info, err := os.Stat(filepath.Join(root, "third_party", "cliproxyapi", "natives-patches"))
	return err == nil && info.IsDir()
}

// runningBinaryIsDevHostBuild 校验当前进程二进制的构建来源：主模块必须是
// 本仓库 model-host（防任意二进制放到 target 路径冒名）。测试二进制的主
// 模块路径同样以该模块为根，满足本判定；无构建信息的二进制拒绝。
func runningBinaryIsDevHostBuild() bool {
	info, ok := debug.ReadBuildInfo()
	if !ok {
		return false
	}
	return strings.HasPrefix(info.Main.Path, "github.com/ldh/natives/model-host")
}

// kernelDevIdentityOk 是 updateKernel 的统一身份守卫：任一条件失败返回
// 稳定可行动错误，不产生任何下载/构建/换装副作用。
func kernelDevIdentityOk() error {
	repoRoot := kernelRepoRoot()
	if repoRoot == "" {
		return fmt.Errorf("未找到内核源码树，仅开发环境支持在线升级")
	}
	exePath, err := os.Executable()
	if err != nil {
		return err
	}
	if !developmentKernelExecutable(repoRoot, exePath) {
		return &SafeError{Code: "kernel_update_requires_product_update", Message: "仅仓库 target 中的开发 Host 支持内核换装，请更新完整 Natives 产品"}
	}
	if !runningBinaryIsDevHostBuild() {
		return &SafeError{Code: "kernel_update_requires_product_update", Message: "当前 Host 不是受核验的开发构建，请更新完整 Natives 产品"}
	}
	return nil
}

func (t toolchain) exportKernelSource(ctx context.Context, repo, tag, destination string) error {
	archive := exec.CommandContext(ctx, t.gitBin, "-C", repo, "archive", "--format=tar.gz", tag, "--", "internal", "sdk", "go.mod", "go.sum")
	pipe, err := archive.StdoutPipe()
	if err != nil {
		return err
	}
	defer pipe.Close()
	extract := exec.CommandContext(ctx, "tar", "-xzf", "-", "-C", destination)
	extract.Stdin = pipe
	if err = archive.Start(); err != nil {
		return fmt.Errorf("导出源码失败: %w", err)
	}
	extractErr := extract.Run()
	if extractErr != nil {
		_ = archive.Process.Kill()
	}
	archiveErr := archive.Wait() // Reap even when extraction fails.
	return errors.Join(extractErr, archiveErr)
}

func stageKernelModfile(hostDir, work, candidate string) (string, error) {
	data, err := os.ReadFile(filepath.Join(hostDir, "go.mod"))
	if err != nil {
		return "", err
	}
	const original = "replace github.com/router-for-me/CLIProxyAPI/v8 => ../third_party/cliproxyapi"
	if strings.Count(string(data), original) != 1 {
		return "", fmt.Errorf("内核 replace 契约不匹配")
	}
	modfile := filepath.Join(work, "candidate.mod")
	updated := strings.Replace(string(data), original, "replace github.com/router-for-me/CLIProxyAPI/v8 => "+strconv.Quote(candidate), 1)
	if err = os.WriteFile(modfile, []byte(updated), 0600); err != nil {
		return "", err
	}
	sums, err := os.ReadFile(filepath.Join(hostDir, "go.sum"))
	if err != nil {
		return "", err
	}
	if err = os.WriteFile(filepath.Join(work, "candidate.sum"), sums, 0600); err != nil {
		return "", err
	}
	return modfile, nil
}

// Replace only exported artifacts. Patches/notices remain in the original tree.
// Renaming entire directories ensures removed upstream files cannot survive.
func publishKernelSource(fork, candidate, work string) (func() error, error) {
	backup := filepath.Join(work, "previous")
	if err := os.Mkdir(backup, 0700); err != nil {
		return nil, err
	}
	var saved, installed []string
	rollback := func() error {
		var failures []error
		for i := len(installed) - 1; i >= 0; i-- {
			name := installed[i]
			if err := os.Rename(filepath.Join(fork, name), filepath.Join(candidate, name)); err != nil {
				failures = append(failures, err)
			}
		}
		for i := len(saved) - 1; i >= 0; i-- {
			name := saved[i]
			if err := os.Rename(filepath.Join(backup, name), filepath.Join(fork, name)); err != nil {
				failures = append(failures, err)
			}
		}
		return errors.Join(failures...)
	}
	for _, name := range []string{"internal", "sdk", "go.mod", "go.sum", "version.txt"} {
		if _, err := os.Stat(filepath.Join(candidate, name)); err != nil {
			return nil, errors.Join(err, rollback())
		}
		if err := os.Rename(filepath.Join(fork, name), filepath.Join(backup, name)); err != nil {
			return nil, errors.Join(err, rollback())
		}
		saved = append(saved, name)
		if err := os.Rename(filepath.Join(candidate, name), filepath.Join(fork, name)); err != nil {
			return nil, errors.Join(err, rollback())
		}
		installed = append(installed, name)
	}
	return rollback, nil
}
