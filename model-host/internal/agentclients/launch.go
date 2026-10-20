package agentclients

import (
	"fmt"
	"os/exec"
	"runtime"
	"strings"

	"github.com/ldh/natives/model-host/internal/lifecycle"
)

// Launch starts the installed client. Desktop apps are opened detached; CLI
// clients get a fresh terminal window on macOS, matching the reference GUI.
// 每个启动器子进程（open/osascript/cmd/terminal）异步 reap：短命 helper 退出后
// 由父进程回收，不产生僵尸；用户打开的终端与应用不是本进程子进程，生命周期
// 不绑定 Host（方案 L2.3/L2.4）。
func Launch(definition Definition, target, workingDirectory string) error {
	home := homeDir()
	if target == "" && len(definition.LaunchTargets) > 0 {
		target = definition.LaunchTargets[0].ID
	}

	if definition.AppProbe != nil && target == "app" {
		for _, app := range definition.AppProbe(home) {
			if pathExists(app) {
				return launchDetachedApp(app)
			}
		}
		return fmt.Errorf("未找到 %s 应用", definition.Name)
	}

	candidates := executableCandidates(definition.CLIExecutables)
	if len(candidates) == 0 {
		return fmt.Errorf("未找到 %s 可执行文件", definition.Name)
	}
	return launchInTerminal(candidates[0], workingDirectory)
}

func launchDetachedApp(appPath string) error {
	var cmd *exec.Cmd
	switch runtime.GOOS {
	case "darwin":
		cmd = exec.Command("open", appPath)
	case "windows":
		cmd = exec.Command("cmd", "/D", "/C", "start", "", appPath)
	default:
		cmd = exec.Command(appPath)
	}
	cmd.Stdin = nil
	cmd.Stdout = nil
	cmd.Stderr = nil
	if err := cmd.Start(); err != nil {
		return err
	}
	lifecycle.Reap(cmd)
	return nil
}

func launchInTerminal(exe, workingDirectory string) error {
	switch runtime.GOOS {
	case "darwin":
		cdPrefix := ""
		if workingDirectory != "" && pathExists(workingDirectory) {
			cdPrefix = fmt.Sprintf("cd %s && ", shellQuote(workingDirectory))
		}
		script := fmt.Sprintf(`tell application "Terminal"
	activate
	do script "%sexec '%s'"
end tell`, cdPrefix, strings.ReplaceAll(exe, "'", "'\\''"))
		return startReaped(exec.Command("osascript", "-e", script))
	case "windows":
		cmd := exec.Command("cmd", "/D", "/K", "call", exe)
		if workingDirectory != "" && pathExists(workingDirectory) {
			cmd.Dir = workingDirectory
		}
		return startReaped(cmd)
	default:
		for _, terminal := range []struct {
			name string
			args []string
		}{
			{"gnome-terminal", []string{"--"}},
			{"konsole", []string{"-e"}},
			{"xfce4-terminal", []string{"-e"}},
			{"x-terminal-emulator", []string{"-e"}},
			{"xterm", []string{"-e"}},
		} {
			if _, err := exec.LookPath(terminal.name); err == nil {
				cmd := exec.Command(terminal.name, append(terminal.args, exe)...)
				if workingDirectory != "" && pathExists(workingDirectory) {
					cmd.Dir = workingDirectory
				}
				return startReaped(cmd)
			}
		}
		return fmt.Errorf("未找到可用的终端模拟器")
	}
}

// startReaped 启动启动器 helper 并异步回收：Start 失败原样返回错误；
// 成功后 Wait 恰一次由 Reap 负责。
func startReaped(cmd *exec.Cmd) error {
	if err := cmd.Start(); err != nil {
		return err
	}
	lifecycle.Reap(cmd)
	return nil
}

func shellQuote(value string) string {
	return "'" + strings.ReplaceAll(value, "'", "'\\''") + "'"
}
