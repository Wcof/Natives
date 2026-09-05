package agentclients

import (
	"fmt"
	"os/exec"
	"runtime"
	"strings"
)

// Launch starts the installed client. Desktop apps are opened detached; CLI
// clients get a fresh terminal window on macOS, matching the reference GUI.
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
	return cmd.Start()
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
		return exec.Command("osascript", "-e", script).Start()
	case "windows":
		cmd := exec.Command("cmd", "/D", "/K", "call", exe)
		if workingDirectory != "" && pathExists(workingDirectory) {
			cmd.Dir = workingDirectory
		}
		return cmd.Start()
	default:
		for _, terminal := range []struct{ name string; args []string }{
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
				return cmd.Start()
			}
		}
		return fmt.Errorf("未找到可用的终端模拟器")
	}
}

func shellQuote(value string) string {
	return "'" + strings.ReplaceAll(value, "'", "'\\''") + "'"
}
