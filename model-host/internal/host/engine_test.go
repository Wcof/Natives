package host

import (
	"context"
	"encoding/json"
	"path/filepath"
	"strings"
	"testing"

	"github.com/ldh/natives/model-host/internal/authfiles"
	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/nativeio"
	"github.com/ldh/natives/model-host/internal/secrets"
	"github.com/ldh/natives/model-host/internal/usage"
)

func newEngineHarness(t *testing.T) (*Engine, secrets.Store, *[]nativeio.Response) {
	t.Helper()
	t.Setenv("NATIVES_MODEL_HOST_CONFIG_DIR", t.TempDir())
	store := secrets.NewMemoryStore()
	events := &[]nativeio.Response{}
	engine, err := NewEngine(domain.NewRepository(filepath.Join(t.TempDir(), "state.json")), store, func(response nativeio.Response) {
		*events = append(*events, response)
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(engine.Close)
	if engine.aiQuery != nil {
		engine.aiQuery.Close()
		engine.aiQuery = nil
	}
	engine.authFiles, err = authfiles.NewManager(filepath.Join(t.TempDir(), "auth"))
	if err != nil {
		t.Fatal(err)
	}
	return engine, store, events
}

func dispatchJSON(t *testing.T, engine *Engine, method string, params any) any {
	t.Helper()
	var raw json.RawMessage
	if params != nil {
		data, err := json.Marshal(params)
		if err != nil {
			t.Fatal(err)
		}
		raw = data
	}
	result, err := engine.dispatch(context.Background(), method, raw)
	if err != nil {
		t.Fatalf("%s: %v", method, err)
	}
	return result
}

func TestFreshInstallUsageNeedsNoProviderOrProxy(t *testing.T) {
	engine, _, _ := newEngineHarness(t)
	snapshot := dispatchJSON(t, engine, "model_snapshot", nil).(domain.Snapshot)
	if len(snapshot.Providers) != 0 || len(snapshot.Accounts) != 0 || snapshot.DefaultModel != (domain.DefaultModelRef{}) || snapshot.Resident {
		t.Fatalf("fresh state must be empty: %#v", snapshot)
	}
	events, err := engine.usageStore.GetEvents(usage.Filter{Range: "all", Limit: 10})
	if err != nil {
		t.Fatal(err)
	}
	if events.Total != 0 || len(events.Events) != 0 {
		t.Fatalf("fresh usage must be empty, got %#v", events)
	}
	options := dispatchJSON(t, engine, "model_inference_options", nil).(map[string]any)
	defaultModel := options["defaultModel"].(map[string]any)
	if defaultModel["unavailable"] != true || defaultModel["reasonCode"] != "not_set" {
		t.Fatalf("fresh default must be honestly unavailable: %#v", defaultModel)
	}
}

func TestProviderLifecycleAndDefaultModel(t *testing.T) {
	engine, store, _ := newEngineHarness(t)
	snapshot := dispatchJSON(t, engine, "model_provider_create", map[string]any{
		"expectedRevision": 1, "name": "Synthetic", "baseUrl": "https://example.test/v1",
		"protocol": "openai_chat", "apiKey": "sk-test", "enabled": true, "allowLan": false,
	}).(domain.Snapshot)
	if len(snapshot.Providers) != 1 || snapshot.Providers[0].CredentialMask == "" {
		t.Fatalf("provider not created: %#v", snapshot)
	}
	providerID := snapshot.Providers[0].ID
	if key, err := store.Get(snapshot.Providers[0].SecretRef); err != nil || key != "sk-test" {
		t.Fatalf("secret not stored in keychain adapter: %q %v", key, err)
	}
	snapshot = dispatchJSON(t, engine, "model_models_upsert", map[string]any{
		"expectedRevision": snapshot.Revision, "providerId": providerID,
		"model": map[string]any{"id": "model-a", "displayName": "Model A", "enabled": true},
	}).(domain.Snapshot)
	if len(snapshot.Providers[0].Models) != 1 {
		t.Fatalf("model not added: %#v", snapshot.Providers[0])
	}
	snapshot = dispatchJSON(t, engine, "model_default_model_set", map[string]any{
		"expectedRevision": snapshot.Revision, "providerId": providerID, "modelId": "model-a",
	}).(domain.Snapshot)
	if snapshot.DefaultModel.ModelID != "model-a" {
		t.Fatalf("default model not set: %#v", snapshot.DefaultModel)
	}
	options := dispatchJSON(t, engine, "model_inference_options", nil).(map[string]any)
	defaultModel := options["defaultModel"].(map[string]any)
	if defaultModel["unavailable"] == true || defaultModel["configToken"] == "" {
		t.Fatalf("default model option unavailable: %#v", defaultModel)
	}
	if _, exists := defaultModel["route"]; exists {
		t.Fatal("retired route must not be exposed")
	}

	snapshot = dispatchJSON(t, engine, "model_models_delete", map[string]any{
		"expectedRevision": snapshot.Revision, "providerId": providerID, "modelId": "model-a",
	}).(domain.Snapshot)
	if snapshot.DefaultModel.ProviderID != "" {
		t.Fatalf("deleting a model must clear its default: %#v", snapshot.DefaultModel)
	}
	dispatchJSON(t, engine, "model_provider_delete", map[string]any{
		"expectedRevision": snapshot.Revision, "providerId": providerID,
	})
	if _, err := store.Get("provider-api-key:" + providerID); err == nil {
		t.Fatal("provider secret should be removed with the provider")
	}
}

func TestResidentLifecycleSettingAndRetiredMethods(t *testing.T) {
	engine, _, _ := newEngineHarness(t)
	initial := dispatchJSON(t, engine, "model_snapshot", nil).(domain.Snapshot)
	snapshot := dispatchJSON(t, engine, "model_host_set_resident", map[string]any{
		"expectedRevision": initial.Revision, "resident": true,
	}).(domain.Snapshot)
	if !snapshot.Resident {
		t.Fatalf("resident not persisted: %#v", snapshot)
	}
	data, _ := json.Marshal(snapshot)
	for _, retired := range []string{"gateway", "kernelVersion", "accessKeys"} {
		if strings.Contains(string(data), retired) {
			t.Fatalf("snapshot still exposes %q: %s", retired, data)
		}
	}
	for _, method := range []string{"model_gateway_start", "model_gateway_set_resident", "model_kernel_update", "model_oauth_start", "model_agent_clients_sync", "model_account_models"} {
		if _, err := engine.dispatch(context.Background(), method, json.RawMessage(`{}`)); err == nil {
			t.Fatalf("retired method %s must be rejected", method)
		}
	}
}

func TestProviderRejectsUnsafeAddresses(t *testing.T) {
	engine, _, _ := newEngineHarness(t)
	for _, baseURL := range []string{"http://10.0.0.5/v1", "http://169.254.1.1/v1", "file:///tmp/model"} {
		if _, err := engine.dispatch(context.Background(), "model_provider_create", mustJSON(t, map[string]any{
			"expectedRevision": 1, "name": "Unsafe", "baseUrl": baseURL,
			"protocol": "openai_chat", "apiKey": "sk", "enabled": true,
		})); err == nil {
			t.Fatalf("unsafe base URL %s was accepted", baseURL)
		}
	}
	// Loopback HTTP remains allowed for local test providers.
	if _, err := engine.dispatch(context.Background(), "model_provider_create", mustJSON(t, map[string]any{
		"expectedRevision": 1, "name": "Local", "baseUrl": "http://127.0.0.1:9999/v1",
		"protocol": "openai_chat", "apiKey": "sk", "enabled": true,
	})); err != nil {
		t.Fatalf("loopback provider rejected: %v", err)
	}
}

func mustJSON(t *testing.T, value any) json.RawMessage {
	t.Helper()
	data, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}
	return data
}
