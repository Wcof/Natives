package host

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/usage"
)

func addInferenceProvider(t *testing.T, engine *Engine, baseURL, protocol, secret string) (domain.Snapshot, string) {
	t.Helper()
	current, err := engine.repo.Load()
	if err != nil {
		t.Fatal(err)
	}
	snapshot := dispatchJSON(t, engine, "model_provider_create", map[string]any{
		"expectedRevision": current.Revision, "name": "Synthetic", "baseUrl": baseURL,
		"protocol": protocol, "apiKey": secret, "enabled": true,
	}).(domain.Snapshot)
	providerID := snapshot.Providers[0].ID
	snapshot = dispatchJSON(t, engine, "model_models_upsert", map[string]any{
		"expectedRevision": snapshot.Revision, "providerId": providerID,
		"model": map[string]any{"id": "model-a", "displayName": "Model A", "enabled": true},
	}).(domain.Snapshot)
	snapshot = dispatchJSON(t, engine, "model_default_model_set", map[string]any{
		"expectedRevision": snapshot.Revision, "providerId": providerID, "modelId": "model-a",
	}).(domain.Snapshot)
	return snapshot, providerID
}

func startAndWait(t *testing.T, engine *Engine, owner, text string) map[string]any {
	t.Helper()
	options := dispatchJSON(t, engine, "model_inference_options", nil).(map[string]any)
	defaultModel := options["defaultModel"].(map[string]any)
	if defaultModel["unavailable"] == true {
		t.Fatalf("default model unavailable: %#v", defaultModel)
	}
	started := dispatchJSON(t, engine, "model_inference_start", map[string]any{
		"owner": owner, "text": text, "selection": "default",
		"expectedConfigToken": defaultModel["configToken"],
	}).(map[string]string)
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		result := dispatchJSON(t, engine, "model_inference_status", map[string]any{
			"owner": owner, "id": started["id"], "text": text,
		}).(map[string]any)
		if result["done"] == true {
			return result
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatal("inference did not finish")
	return nil
}

func waitForEvents(t *testing.T, engine *Engine, count int64) []usage.Event {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	var events []usage.Event
	for time.Now().Before(deadline) {
		result, err := engine.usageStore.GetEvents(usage.Filter{Range: "all", Limit: 20})
		if err != nil {
			t.Fatal(err)
		}
		events = result.Events
		if int64(len(events)) >= count {
			return events
		}
		time.Sleep(20 * time.Millisecond)
	}
	t.Fatalf("expected %d usage events, got %d", count, len(events))
	return nil
}

func TestInferenceStreamsDirectlyAndRecordsUsageOnce(t *testing.T) {
	engine, _, _ := newEngineHarness(t)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/v1/chat/completions" {
			t.Errorf("unexpected path: %s", r.URL.Path)
		}
		if r.Header.Get("Authorization") != "Bearer sk-test" {
			t.Errorf("missing provider credential")
		}
		var body map[string]any
		if json.NewDecoder(r.Body).Decode(&body) != nil || body["stream"] != true || body["model"] != "model-a" {
			t.Errorf("unexpected request body: %#v", body)
		}
		w.Header().Set("Content-Type", "text/event-stream")
		flusher, _ := w.(http.Flusher)
		chunks := []string{
			`{"choices":[{"delta":{"reasoning_content":"thinking"}}]}`,
			`{"choices":[{"delta":{"content":"Hel"}}]}`,
			`{"choices":[{"delta":{"content":"lo"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15,"prompt_tokens_details":{"cached_tokens":2},"completion_tokens_details":{"reasoning_tokens":1}}}`,
			`[DONE]`,
		}
		for _, chunk := range chunks {
			fmt.Fprintf(w, "data: %s\n\n", chunk)
			if flusher != nil {
				flusher.Flush()
			}
		}
	}))
	defer server.Close()
	addInferenceProvider(t, engine, server.URL+"/v1", "openai_chat", "sk-test")

	owner := "owner-stream-0123456789abcdef0123456789"
	result := startAndWait(t, engine, owner, "synthetic evidence")
	if result["failure"] != "" || result["text"] != "Hello" {
		t.Fatalf("unexpected inference result: %#v", result)
	}
	events := waitForEvents(t, engine, 1)
	event := events[0]
	if event.Result != usage.ResultSuccess || event.InputTokens != 8 || event.OutputTokens != 5 ||
		event.CacheReadTokens != 2 || event.ReasoningTokens != 1 || event.TotalTokens != 15 ||
		event.UsageStatus != "reported" {
		t.Fatalf("usage not normalized or not recorded once: %#v", event)
	}
	if event.Provider == "" || event.Model != "model-a" || event.Endpoint != "openai_chat" {
		t.Fatalf("missing direct identity: %#v", event)
	}
}

func TestInferenceWithoutUsageIsNotRecordedAsZeroSuccess(t *testing.T) {
	engine, _, _ := newEngineHarness(t)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, `{"choices":[{"message":{"content":"ok"},"finish_reason":"stop"}]}`)
	}))
	defer server.Close()
	addInferenceProvider(t, engine, server.URL+"/v1", "openai_chat", "sk-test")

	result := startAndWait(t, engine, "owner-missing-0123456789abcdef012345", "synthetic evidence")
	if result["failure"] != "" || result["text"] != "ok" {
		t.Fatalf("unexpected result: %#v", result)
	}
	event := waitForEvents(t, engine, 1)[0]
	if event.UsageStatus != "missing" || event.TotalTokens != 0 {
		t.Fatalf("missing usage must be explicit, not zero consumption: %#v", event)
	}
}

func TestInferenceCancelRecordsOneCancelledEvent(t *testing.T) {
	engine, _, _ := newEngineHarness(t)
	released := make(chan struct{})
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/event-stream")
		flusher, _ := w.(http.Flusher)
		fmt.Fprint(w, "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n")
		if flusher != nil {
			flusher.Flush()
		}
		<-r.Context().Done()
		close(released)
	}))
	defer server.Close()
	_, providerID := addInferenceProvider(t, engine, server.URL+"/v1", "openai_chat", "sk-test")

	owner := "owner-cancel-0123456789abcdef01234567"
	started := dispatchJSON(t, engine, "model_inference_start", map[string]any{
		"owner": owner, "text": "synthetic evidence", "providerId": providerID, "modelId": "model-a",
	}).(map[string]string)
	time.Sleep(50 * time.Millisecond)
	dispatchJSON(t, engine, "model_inference_cancel", map[string]any{"owner": owner, "id": started["id"], "text": "synthetic evidence"})
	select {
	case <-released:
	case <-time.After(5 * time.Second):
		t.Fatal("provider request was not cancelled")
	}
	event := waitForEvents(t, engine, 1)[0]
	if event.Result != usage.ResultCancelled || event.UsageStatus != "missing" {
		t.Fatalf("cancelled request must remain explicit: %#v", event)
	}
	time.Sleep(50 * time.Millisecond)
	events := waitForEvents(t, engine, 1)
	if len(events) != 1 {
		t.Fatalf("cancelled request was recorded more than once: %d", len(events))
	}
}

func TestInferenceStatusMapping(t *testing.T) {
	engine, _, _ := newEngineHarness(t)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Error(w, "denied", http.StatusUnauthorized)
	}))
	defer server.Close()
	addInferenceProvider(t, engine, server.URL+"/v1", "openai_chat", "sk-test")
	result := startAndWait(t, engine, "owner-auth-0123456789abcdef0123456789", "synthetic evidence")
	if result["failure"] != "INFERENCE_AUTH" {
		t.Fatalf("401 must map to INFERENCE_AUTH: %#v", result)
	}
	event := waitForEvents(t, engine, 1)[0]
	if event.Result != usage.ResultFailed || event.HTTPStatus != http.StatusUnauthorized {
		t.Fatalf("failed request usage state wrong: %#v", event)
	}
}

func TestInferenceRejectsDefaultChangeAfterPreview(t *testing.T) {
	engine, _, _ := newEngineHarness(t)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {}))
	defer server.Close()
	snapshot, _ := addInferenceProvider(t, engine, server.URL+"/v1", "openai_chat", "sk-test")
	options := dispatchJSON(t, engine, "model_inference_options", nil).(map[string]any)["defaultModel"].(map[string]any)
	token := options["configToken"].(string)
	if _, err := engine.dispatch(context.Background(), "model_inference_start", mustJSON(t, map[string]any{
		"owner": "owner-default-0123456789abcdef012345", "text": "synthetic evidence",
		"selection": "default", "expectedConfigToken": "stale",
	})); err == nil {
		t.Fatal("stale default token must be rejected")
	}
	if _, err := engine.dispatch(context.Background(), "model_inference_start", mustJSON(t, map[string]any{
		"owner": "owner-default-0123456789abcdef012345", "text": "synthetic evidence",
		"selection": "default", "expectedConfigToken": token,
	})); err != nil {
		t.Fatalf("valid default token rejected: %v", err)
	}
	_ = snapshot
	// Keep the started job from racing the test cleanup.
	engine.cancelInference()
}

func TestInferenceRequestDoesNotContainGatewayFields(t *testing.T) {
	engine, _, _ := newEngineHarness(t)
	_, err := engine.dispatch(context.Background(), "model_inference_start", json.RawMessage(`{"owner":"owner-unknown-0123456789abcdef0123456","text":"x","route":"legacy"}`))
	if err == nil || !strings.Contains(err.Error(), "推理请求无效") {
		t.Fatalf("legacy route field must be rejected: %v", err)
	}
}
