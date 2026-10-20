package lifecycle

import (
	"context"
	"os/exec"
	"sync"
	"testing"
	"time"
)

func TestRegistryGoRunsAndStops(t *testing.T) {
	registry := NewRegistry()
	started := make(chan struct{})
	finished := make(chan struct{})
	task := registry.Go("collect", func(ctx context.Context) {
		close(started)
		select {
		case <-ctx.Done():
		case <-finished:
		}
	})
	if task == nil {
		t.Fatal("expected task to register")
	}
	<-started
	if registry.Count() != 1 {
		t.Fatalf("expected 1 registered task, got %d", registry.Count())
	}
	close(finished)
	select {
	case <-task.Done():
	case <-time.After(2 * time.Second):
		t.Fatal("task did not finish")
	}
	time.Sleep(50 * time.Millisecond)
	if registry.Count() != 0 {
		t.Fatalf("expected registry to drain to 0, got %d", registry.Count())
	}
}

func TestRegistryRejectsTasksAfterStop(t *testing.T) {
	registry := NewRegistry()
	release := make(chan struct{})
	registry.Go("blocker", func(ctx context.Context) {
		<-ctx.Done()
		<-release
	})
	shutdown := NewShutdown(2 * time.Second)
	stopCtx, cancel := shutdown.Context()
	defer cancel()
	resultCh := make(chan StopResult, 1)
	go func() { resultCh <- registry.Stop(stopCtx) }()
	time.Sleep(50 * time.Millisecond)
	if task := registry.Go("late", func(context.Context) {}); task != nil {
		t.Fatal("stopping registry must reject new tasks")
	}
	close(release)
	select {
	case result := <-resultCh:
		if len(result.Cancelled) != 1 || result.Cancelled[0] != "blocker" {
			t.Fatalf("unexpected result: %+v", result)
		}
		if len(result.TimedOut) != 0 {
			t.Fatalf("unexpected timeouts: %+v", result)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("Stop did not return")
	}
	if !registry.Stopping() {
		t.Fatal("registry must stay in stopping")
	}
}

func TestRegistryStopReportsTimeout(t *testing.T) {
	registry := NewRegistry()
	registry.Go("uncooperative", func(ctx context.Context) {
		<-ctx.Done()
		time.Sleep(5 * time.Second) // ignore deadline pressure: must be reported
	})
	shutdown := NewShutdown(300 * time.Millisecond)
	stopCtx, cancel := shutdown.Context()
	defer cancel()
	result := registry.Stop(stopCtx)
	if len(result.TimedOut) != 1 || result.TimedOut[0] != "uncooperative" {
		t.Fatalf("expected timeout report, got %+v", result)
	}
}

func TestRegistryConcurrentGoAndStop(t *testing.T) {
	registry := NewRegistry()
	var wg sync.WaitGroup
	for i := 0; i < 50; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			if task := registry.Go("racer", func(ctx context.Context) {
				<-ctx.Done()
			}); task != nil {
				time.Sleep(time.Millisecond)
			}
		}()
	}
	time.Sleep(20 * time.Millisecond)
	shutdown := NewShutdown(2 * time.Second)
	stopCtx, cancel := shutdown.Context()
	defer cancel()
	_ = registry.Stop(stopCtx)
	wg.Wait()
	if task := registry.Go("after-stop", func(context.Context) {}); task != nil {
		t.Fatal("tasks must be rejected after Stop begins")
	}
}

func TestReapReapsExitedChild(t *testing.T) {
	cmd := exec.Command("true")
	if err := cmd.Start(); err != nil {
		t.Skipf("cannot spawn true: %v", err)
	}
	done := Reap(cmd)
	select {
	case <-done:
	case <-time.After(2 * time.Second):
		t.Fatal("child was not reaped in time")
	}
	// Wait 恰一次：重复等待必须仍能安全结束（Reap 内部只 Wait 一次），
	// 这里通过 ProcessState 验证子进程确实被回收。
	if cmd.ProcessState == nil {
		t.Fatal("expected child to be reaped exactly once")
	}
}

func TestReapIgnoresUnstarted(t *testing.T) {
	select {
	case <-Reap(exec.Command("true")):
	case <-time.After(time.Second):
		t.Fatal("unstarted command must close done immediately")
	}
	select {
	case <-Reap(nil):
	case <-time.After(time.Second):
		t.Fatal("nil command must close done immediately")
	}
}

func TestShutdownRemainingNeverNegative(t *testing.T) {
	shutdown := NewShutdown(10 * time.Millisecond)
	time.Sleep(20 * time.Millisecond)
	if shutdown.Remaining() != 0 {
		t.Fatalf("expected zero remaining, got %v", shutdown.Remaining())
	}
	ctx, cancel := shutdown.Context()
	defer cancel()
	select {
	case <-ctx.Done():
	case <-time.After(time.Second):
		t.Fatal("expired shutdown context must be done")
	}
}
