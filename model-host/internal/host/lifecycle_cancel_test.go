package host

import (
	"context"
	"encoding/json"
	"testing"
	"time"

	"github.com/ldh/natives/model-host/internal/nativeio"
)

// TestCancelInferenceForOwners 验收 L07：客户端断开只取消该 owner 的推理任务，
// 其他 owner 与 Engine 任务不受影响；取消后的任务从登记表移除。
func TestCancelInferenceForOwners(t *testing.T) {
	engine, _, _ := newAIQueryTestHarness(t)
	ownerA := "owner-a-0123456789abcdef0123456789abcdef0123"
	ownerB := "owner-b-0123456789abcdef0123456789abcdef0123"

	ctxA, cancelA := context.WithCancel(context.Background())
	ctxB, cancelB := context.WithCancel(context.Background())
	jobA := &inferenceJob{owner: ownerA, cancel: cancelA, created: time.Now()}
	jobB := &inferenceJob{owner: ownerB, cancel: cancelB, created: time.Now()}
	engine.inferenceMu.Lock()
	if engine.inferenceJobs == nil {
		engine.inferenceJobs = map[string]*inferenceJob{}
	}
	engine.inferenceJobs["job-a"] = jobA
	engine.inferenceJobs["job-b"] = jobB
	engine.inferenceMu.Unlock()

	engine.CancelInferenceForOwners([]string{ownerA})

	select {
	case <-ctxA.Done():
	default:
		t.Fatal("owner A job context must be cancelled")
	}
	select {
	case <-ctxB.Done():
		t.Fatal("owner B job context must stay live")
	default:
	}
	engine.inferenceMu.Lock()
	_, aGone := engine.inferenceJobs["job-a"]
	_, bPresent := engine.inferenceJobs["job-b"]
	delete(engine.inferenceJobs, "job-b")
	engine.inferenceMu.Unlock()
	if aGone {
		t.Fatal("owner A job should be removed from registry")
	}
	if !bPresent {
		t.Fatal("owner B job must be untouched in registry")
	}
	cancelB()

	// 空 owner 列表：不触碰任何任务。
	engine.CancelInferenceForOwners(nil)
}

// TestInferenceOwnerExtraction 验证读 goroutine 用 InferenceOwner 提取 owner
// 与 parseInference 同源：合法请求返回 owner，非法参数返回空。
func TestInferenceOwnerExtraction(t *testing.T) {
	valid := json.RawMessage(`{"owner":"0123456789abcdef0123456789abcdef01","id":"","text":"hi"}`)
	if owner := InferenceOwner(valid); owner != "0123456789abcdef0123456789abcdef01" {
		t.Fatalf("unexpected owner: %q", owner)
	}
	if owner := InferenceOwner(json.RawMessage(`{"text":"missing owner"}`)); owner != "" {
		t.Fatalf("invalid params must yield empty owner, got %q", owner)
	}
	if owner := InferenceOwner(nil); owner != "" {
		t.Fatalf("nil params must yield empty owner, got %q", owner)
	}
}

// TestEngineHandleRejectsAfterClose 验收 L4：Close 之后 Handle 原子拒绝新请求，
// 不触碰已关闭存储。
func TestEngineHandleRejectsAfterClose(t *testing.T) {
	engine, _, _ := newAIQueryTestHarness(t)
	engine.Close()
	response := engine.Handle(context.Background(), nativeio.Request{ID: "late", Method: "model_snapshot"})
	if response.OK || response.ErrorCode != "host_stopping" {
		t.Fatalf("expected host_stopping rejection after close, got %+v", response)
	}
	// 重复 Close 幂等。
	engine.Close()
}
