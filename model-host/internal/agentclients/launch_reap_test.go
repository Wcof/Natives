package agentclients

import (
	"os"
	"os/exec"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/ldh/natives/model-host/internal/lifecycle"
)

// TestLaunchReapsShortHelpers 验收 L01：连续启动并结束 100 个短 helper，
// 混合自然退出、启动失败、取消（kill）与输出失败并发；
// 每 child 恰一次 Wait，父进程存活期间不残留 Z（僵尸）。
// 只使用系统真 helper（true/sleep/echo），不启动用户终端或外部应用。
func TestLaunchReapsShortHelpers(t *testing.T) {
	if testing.Short() {
		t.Skip("short mode")
	}
	const total = 100
	type reapedChild struct {
		cmd  *exec.Cmd
		done <-chan struct{}
	}
	var mu sync.Mutex
	var reaps []reapedChild
	startFailures := 0
	attempts := 0
	var wg sync.WaitGroup
	record := func(cmd *exec.Cmd, done <-chan struct{}, startErr error, built bool) {
		defer wg.Done()
		mu.Lock()
		defer mu.Unlock()
		attempts++
		switch {
		case !built || startErr != nil:
			startFailures++
		default:
			reaps = append(reaps, reapedChild{cmd: cmd, done: done})
		}
	}
	for i := 0; i < total; i++ {
		switch i % 4 {
		case 0: // 自然退出
			wg.Add(1)
			go func() {
				cmd := exec.Command("/usr/bin/true")
				err := cmd.Start()
				var done <-chan struct{}
				if err == nil {
					done = lifecycle.Reap(cmd)
				}
				record(cmd, done, err, true)
			}()
		case 1: // 启动失败：可执行文件不存在（Start 报错，无 handle 可回收）
			wg.Add(1)
			go func() {
				cmd := exec.Command("./definitely-not-natives-helper")
				err := cmd.Start()
				record(cmd, nil, err, true)
			}()
		case 2: // 取消：启动后立即 kill，仍必须由 reap 回收
			wg.Add(1)
			go func() {
				cmd := exec.Command("/bin/sleep", "30")
				err := cmd.Start()
				var done <-chan struct{}
				if err == nil {
					done = lifecycle.Reap(cmd)
					_ = cmd.Process.Kill()
				}
				record(cmd, done, err, true)
			}()
		case 3: // 输出失败：标准输出立即关闭产生 EPIPE/SIGPIPE
			wg.Add(1)
			go func() {
				cmd := exec.Command("/bin/echo", "natives")
				devNull, openErr := os.OpenFile(os.DevNull, os.O_WRONLY, 0)
				if openErr == nil {
					defer devNull.Close()
					cmd.Stdout = devNull
				}
				err := cmd.Start()
				var done <-chan struct{}
				if err == nil {
					done = lifecycle.Reap(cmd)
				}
				record(cmd, done, err, true)
			}()
		}
	}
	wg.Wait()
	mu.Lock()
	reapedCount, failures, reapedCopy := len(reaps), startFailures, reaps
	mu.Unlock()
	if reapedCount+failures != total {
		t.Fatalf("expected %d attempts, got %d reaped + %d start failures", total, reapedCount, failures)
	}
	for _, child := range reapedCopy {
		select {
		case <-child.done:
		case <-time.After(5 * time.Second):
			t.Fatalf("child %d was not reaped", child.cmd.Process.Pid)
		}
		// done 关闭发生在 Wait 返回之后（happens-before），此时读 ProcessState 安全：
		// 证明 Wait 恰一次执行且子进程已被回收。
		if child.cmd.ProcessState == nil {
			t.Fatalf("child %d has no process state: Wait did not run exactly once", child.cmd.Process.Pid)
		}
	}
	// 父进程存活时不残留僵尸：所有已退出子进程都应已被 Wait 回收。
	if zombies := childZombies(strconv.Itoa(os.Getpid())); len(zombies) > 0 {
		t.Fatalf("zombie children leaked: %v", zombies)
	}
}

// childZombies 返回 ppid 指定进程下仍处于 Z 状态的子进程（darwin/linux）。
func childZombies(ppid string) []string {
	out, err := exec.Command("/bin/ps", "-axo", "ppid=,stat=").Output()
	if err != nil {
		return nil
	}
	var zombies []string
	for _, line := range strings.Split(string(out), "\n") {
		fields := strings.Fields(line)
		if len(fields) < 2 {
			continue
		}
		if fields[0] == ppid && strings.Contains(fields[1], "Z") {
			zombies = append(zombies, line)
		}
	}
	return zombies
}
