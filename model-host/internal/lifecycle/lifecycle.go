// Package lifecycle 集中 Model Host 内部的任务登记、子进程 reap 与退出期限原语。
//
// 边界（方案 L1）：只容纳有真实调用方的最小原语；业务数据、模型路由与 resident
// 判断仍归 Engine／现有领域模块。不引入新依赖，不建通用 supervisor：
//   - Registry：后台任务登记（与 stopping 判定原子）、取消、按共同 deadline 等待；
//   - Reap：已 Start 子进程的异步 Wait（恰一次），只回收不终止；
//   - Shutdown：单一绝对 deadline，各阶段共享剩余时间。
package lifecycle

import (
	"context"
	"os/exec"
	"sync"
	"time"
)

// Task 是一次已登记的后台任务。done 在任务函数返回后关闭。
type Task struct {
	name string
	// cancel 取消任务 ctx；在 Registry.Go 内创建，Stop 或任务退出时调用。
	cancel context.CancelFunc
	done   chan struct{}
}

// Name 返回登记名（诊断与退出记录用）。
func (t *Task) Name() string { return t.name }

// Done 在任务函数完全返回后关闭（等待与清理对称）。
func (t *Task) Done() <-chan struct{} { return t.done }

// Registry 登记可取消的后台任务。进入 stopping 后拒绝新任务；Add/Stop 判定原子，
// 避免 Add/Wait 竞态。业务锁不得在持有期间调用 Stop（Stop 会等待）。
type Registry struct {
	mu       sync.Mutex
	stopping bool
	tasks    map[*Task]struct{}
}

// NewRegistry 创建空登记表。
func NewRegistry() *Registry { return &Registry{tasks: make(map[*Task]struct{})} }

// Go 原子地登记并在独立 goroutine 运行 fn（ctx 取消贯穿任务）。
// 已进入 stopping 时返回 nil 且不运行 fn；调用方必须处理 nil。
func (r *Registry) Go(name string, fn func(ctx context.Context)) *Task {
	r.mu.Lock()
	if r.stopping {
		r.mu.Unlock()
		return nil
	}
	ctx, cancel := context.WithCancel(context.Background())
	task := &Task{name: name, cancel: cancel, done: make(chan struct{})}
	r.tasks[task] = struct{}{}
	r.mu.Unlock()
	go func() {
		defer func() {
			r.mu.Lock()
			delete(r.tasks, task)
			r.mu.Unlock()
			cancel()
			close(task.done)
		}()
		fn(ctx)
	}()
	return task
}

// Count 返回当前在册任务数（诊断用）。
func (r *Registry) Count() int {
	r.mu.Lock()
	defer r.mu.Unlock()
	return len(r.tasks)
}

// Stopping 报告是否已进入 stopping（新任务被拒绝）。
func (r *Registry) Stopping() bool {
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.stopping
}

// StopResult 记录一次 Stop 的等待结果。
type StopResult struct {
	Cancelled []string // 已取消并等待的任务名
	TimedOut  []string // 在 deadline 内未退出的任务名
}

// Stop 原子进入 stopping（拒绝新任务），取消全部在册任务，并按 ctx（共同 deadline
// 的剩余时间）等待退出。不吞失败：超时任务列在 TimedOut，由调用方决定记录与后续。
func (r *Registry) Stop(ctx context.Context) StopResult {
	r.mu.Lock()
	r.stopping = true
	tasks := make([]*Task, 0, len(r.tasks))
	for task := range r.tasks {
		tasks = append(tasks, task)
	}
	r.mu.Unlock()
	result := StopResult{}
	for _, task := range tasks {
		task.cancel()
	}
	for _, task := range tasks {
		select {
		case <-task.done:
			result.Cancelled = append(result.Cancelled, task.name)
		case <-ctx.Done():
			r.mu.Lock()
			pending := make([]*Task, 0, len(r.tasks))
			for pendingTask := range r.tasks {
				pending = append(pending, pendingTask)
			}
			r.mu.Unlock()
			for _, pendingTask := range pending {
				result.TimedOut = append(result.TimedOut, pendingTask.name)
			}
			return result
		}
	}
	return result
}

// Reap 异步 Wait 一个已 Start 的短命启动器子进程（open/osascript/xdg-open 等），
// 防止长寿父进程下产生僵尸。只回收，不终止：用户打开的终端或外部应用的生命周期
// 不绑定 Host。Wait 与释放 child handle 各只执行一次——调用 Start 后不得再自行
// Wait。返回的 done 在子进程被回收后关闭；cmd 为 nil 或尚未 Start 时立即关闭。
func Reap(cmd *exec.Cmd) <-chan struct{} {
	done := make(chan struct{})
	if cmd == nil || cmd.Process == nil {
		close(done)
		return done
	}
	go func() {
		defer close(done)
		_ = cmd.Wait()
	}()
	return done
}

// Shutdown 是退出流程的单一绝对 deadline：全部阶段共享剩余时间，
// 不得把多个局部 timeout 相加突破预算。
type Shutdown struct {
	deadline time.Time
}

// NewShutdown 从现在起给定总预算。
func NewShutdown(budget time.Duration) *Shutdown {
	return &Shutdown{deadline: time.Now().Add(budget)}
}

// Remaining 返回剩余预算（已超时返回 0）。
func (s *Shutdown) Remaining() time.Duration {
	remaining := time.Until(s.deadline)
	if remaining < 0 {
		return 0
	}
	return remaining
}

// Context 返回绑定剩余时间的 ctx（deadline 到期自动取消）。
func (s *Shutdown) Context() (context.Context, context.CancelFunc) {
	return context.WithDeadline(context.Background(), s.deadline)
}
