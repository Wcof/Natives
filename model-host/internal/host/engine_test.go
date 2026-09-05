package host

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/nativeio"
	"github.com/ldh/natives/model-host/internal/secrets"
	"github.com/ldh/natives/model-host/internal/usage"
)

func TestCustomProviderCRUDKeepsAPIKeyOutOfState(t *testing.T) {
	path := filepath.Join(t.TempDir(), "state.json")
	secretStore := secrets.NewMemoryStore()
	engine, err := NewEngine(domain.NewRepository(path), secretStore, nil)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(engine.Close)
	raw := json.RawMessage(`{"expectedRevision":1,"name":"Local","baseUrl":"http://127.0.0.1:11434/v1","protocol":"openai_chat","apiKey":"super-secret-value"}`)
	snapshot, err := engine.createProvider(raw)
	if err != nil {
		t.Fatal(err)
	}
	if snapshot.Revision != 2 || len(snapshot.Providers) != 6 || snapshot.Providers[5].CredentialMask != "••••alue" {
		t.Fatalf("unexpected provider snapshot: %#v", snapshot)
	}
	state, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(state), "super-secret-value") {
		t.Fatal("API key leaked into metadata state")
	}
	if len(secretStore.Values) != 1 {
		t.Fatalf("API key missing from secret store: %#v", secretStore.Values)
	}
	providerID := snapshot.Providers[5].ID
	_, err = engine.updateProvider(json.RawMessage(`{"expectedRevision":1,"providerId":"` + providerID + `","name":"Local","baseUrl":"http://127.0.0.1:11434/v1","protocol":"openai_chat","apiKey":"must-not-win"}`))
	if err == nil {
		t.Fatal("stale provider update unexpectedly succeeded")
	}
	stored, _ := secretStore.Get(snapshot.Providers[5].SecretRef)
	if stored != "super-secret-value" {
		t.Fatalf("revision conflict changed keychain secret: %q", stored)
	}
}

func TestProviderProbeUsesUnsavedFormWithoutPersistingIt(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if got := request.Header.Get("Authorization"); got != "Bearer unsaved-key" {
			t.Errorf("probe used the wrong key: %q", got)
		}
		_, _ = writer.Write([]byte(`{"data":[{"id":"probe-model"}]}`))
	}))
	defer server.Close()
	secretStore := secrets.NewMemoryStore()
	engine, err := NewEngine(domain.NewRepository(filepath.Join(t.TempDir(), "state.json")), secretStore, nil)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(engine.Close)
	created, err := engine.createProvider(json.RawMessage(`{"expectedRevision":1,"name":"Stored","baseUrl":"http://127.0.0.1:9/v1","protocol":"openai_chat","apiKey":"stored-key"}`))
	if err != nil {
		t.Fatal(err)
	}
	provider := created.Providers[len(created.Providers)-1]
	raw, _ := json.Marshal(map[string]any{"providerId": provider.ID, "baseUrl": server.URL + "/v1", "protocol": "openai_chat", "apiKey": "unsaved-key"})
	result, err := engine.testProvider(context.Background(), raw)
	if err != nil {
		t.Fatal(err)
	}
	if result["modelCount"] != 1 {
		t.Fatalf("unexpected probe result: %#v", result)
	}
	after, err := engine.repo.Load()
	if err != nil || after.Revision != created.Revision {
		t.Fatalf("probe changed metadata: revision=%d err=%v", after.Revision, err)
	}
	if stored, _ := secretStore.Get(provider.SecretRef); stored != "stored-key" {
		t.Fatalf("probe changed stored key: %q", stored)
	}
}

func TestRefreshModelsMergesByIDAndPreservesManualValues(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if request.URL.Path != "/v1/models" || request.Header.Get("Authorization") != "Bearer provider-key" {
			t.Errorf("unexpected discovery request: %s %q", request.URL.Path, request.Header.Get("Authorization"))
		}
		_, _ = writer.Write([]byte(`{"data":[{"id":"existing","display_name":"Upstream name"},{"id":"new-model"}]}`))
	}))
	defer server.Close()
	path := filepath.Join(t.TempDir(), "state.json")
	engine, err := NewEngine(domain.NewRepository(path), secrets.NewMemoryStore(), nil)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(engine.Close)
	created, err := engine.createProvider(json.RawMessage(`{"expectedRevision":1,"name":"Local","baseUrl":"` + server.URL + `/v1","protocol":"openai_chat","apiKey":"provider-key"}`))
	if err != nil {
		t.Fatal(err)
	}
	providerID := created.Providers[len(created.Providers)-1].ID
	manual, err := engine.upsertModel(json.RawMessage(`{"expectedRevision":2,"providerId":"` + providerID + `","model":{"id":"existing","displayName":"My name","contextLength":123,"enabled":true}}`))
	if err != nil {
		t.Fatal(err)
	}
	refreshed, err := engine.refreshModels(context.Background(), json.RawMessage(`{"expectedRevision":`+jsonNumber(manual.Revision)+`,"providerId":"`+providerID+`"}`))
	if err != nil {
		t.Fatal(err)
	}
	provider, _ := findProvider(&refreshed, providerID)
	if len(provider.Models) != 2 || provider.Models[0].DisplayName != "My name" || provider.Models[0].ContextLength != 123 || provider.Models[1].ID != "new-model" {
		t.Fatalf("unexpected merged models: %#v", provider.Models)
	}
}

func TestRefreshModelsFailurePreservesCatalogAndReturnsSafeErrors(t *testing.T) {
	for _, test := range []struct {
		name     string
		status   int
		body     string
		expected string
	}{
		{"unauthorized", http.StatusUnauthorized, `{}`, "upstream_unauthorized"},
		{"forbidden", http.StatusForbidden, `{}`, "upstream_unauthorized"},
		{"rate limited", http.StatusTooManyRequests, `{}`, "upstream_error"},
		{"server error", http.StatusInternalServerError, `{}`, "upstream_error"},
		{"oversized", http.StatusOK, strings.Repeat("x", maxModelResponseBytes+1), "upstream_response_too_large"},
	} {
		t.Run(test.name, func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
				writer.WriteHeader(test.status)
				_, _ = writer.Write([]byte(test.body))
			}))
			defer server.Close()
			repo := domain.NewRepository(filepath.Join(t.TempDir(), "state.json"))
			engine, err := NewEngine(repo, secrets.NewMemoryStore(), nil)
			if err != nil {
				t.Fatal(err)
			}
			defer engine.Close()
			created, err := engine.createProvider(json.RawMessage(`{"expectedRevision":1,"name":"Failure fixture","baseUrl":"` + server.URL + `/v1","protocol":"openai_chat","apiKey":"fixture-key"}`))
			if err != nil {
				t.Fatal(err)
			}
			providerID := created.Providers[len(created.Providers)-1].ID
			before, err := engine.upsertModel(json.RawMessage(`{"expectedRevision":2,"providerId":"` + providerID + `","model":{"id":"keep-model","displayName":"Keep","enabled":true}}`))
			if err != nil {
				t.Fatal(err)
			}
			_, refreshErr := engine.refreshModels(context.Background(), json.RawMessage(`{"expectedRevision":`+jsonNumber(before.Revision)+`,"providerId":"`+providerID+`"}`))
			var safe *SafeError
			if !errors.As(refreshErr, &safe) || safe.Code != test.expected {
				t.Fatalf("refresh error = %#v, want %s", refreshErr, test.expected)
			}
			after, _ := repo.Load()
			provider, _ := findProvider(&after, providerID)
			if after.Revision != before.Revision || len(provider.Models) != 1 || provider.Models[0].ID != "keep-model" {
				t.Fatalf("failed refresh changed catalog: before=%#v after=%#v", before, after)
			}
		})
	}
}

func jsonNumber(value int64) string { return fmt.Sprintf("%d", value) }

func TestBaseURLSecurityPolicy(t *testing.T) {
	for _, test := range []struct {
		url      string
		allowLAN bool
		ok       bool
	}{
		{"https://api.example.com/v1", false, true},
		{"http://127.0.0.1:11434/v1", false, true},
		{"http://localhost:8317", false, true},
		{"http://192.168.1.20:8000", false, false},
		{"http://192.168.1.20:8000", true, true},
		{"http://example.com", true, false},
		{"https://169.254.1.2", true, false},
		{"file:///tmp/model", true, false},
		{"javascript:alert(1)", true, false},
		{"https://user:pass@example.com", false, false},
	} {
		err := validateBaseURL(test.url, test.allowLAN)
		if (err == nil) != test.ok {
			t.Fatalf("validateBaseURL(%q, %v) = %v", test.url, test.allowLAN, err)
		}
	}
}

func TestGatewayKeyIsExplicitOneTimeResult(t *testing.T) {
	path := filepath.Join(t.TempDir(), "state.json")
	secretStore := secrets.NewMemoryStore()
	engine, err := NewEngine(domain.NewRepository(path), secretStore, nil)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(engine.Close)
	result, err := engine.rotateGatewayKey(json.RawMessage(`{"expectedRevision":1}`))
	if err != nil {
		t.Fatal(err)
	}
	key := result["accessKey"].(string)
	if !strings.HasPrefix(key, "natives_") || len(key) < 40 {
		t.Fatalf("unexpected gateway key: %q", key)
	}
	state, _ := os.ReadFile(path)
	if strings.Contains(string(state), key) {
		t.Fatal("gateway key leaked into metadata state")
	}
	revealed, err := engine.revealGatewayKey()
	if err != nil || revealed["accessKey"] != key {
		t.Fatalf("unexpected reveal: %#v %v", revealed, err)
	}
}

func TestResidentGatewayRestoresAndNonResidentCloseStopsState(t *testing.T) {
	dir := t.TempDir()
	repo := domain.NewRepository(filepath.Join(dir, "state.json"))
	secretStore := secrets.NewMemoryStore()
	first, err := NewEngine(repo, secretStore, nil)
	if err != nil {
		t.Fatal(err)
	}
	first.runtimeConfigPath = filepath.Join(dir, "runtime.yaml")
	running, err := first.startGateway(context.Background(), json.RawMessage(`{"expectedRevision":1}`))
	if err != nil {
		t.Fatal(err)
	}
	oldKey, _ := secretStore.Get(gatewaySecretRef)
	rotated, err := first.rotateGatewayKey(json.RawMessage(`{"expectedRevision":` + jsonNumber(running.Revision) + `}`))
	if err != nil {
		t.Fatal(err)
	}
	running = rotated["snapshot"].(domain.Snapshot)
	newKey := rotated["accessKey"].(string)
	if gatewayStatus(t, running.Gateway.BaseURL, oldKey) != http.StatusUnauthorized || gatewayStatus(t, running.Gateway.BaseURL, newKey) != http.StatusOK {
		t.Fatal("rotated gateway key was not applied to the running proxy")
	}
	resident, err := first.setResident(json.RawMessage(`{"expectedRevision":` + jsonNumber(running.Revision) + `,"resident":true}`))
	if err != nil || !resident.Gateway.Resident {
		t.Fatalf("set resident = %#v %v", resident.Gateway, err)
	}
	first.Close()

	second, err := NewEngine(repo, secretStore, nil)
	if err != nil {
		t.Fatal(err)
	}
	second.runtimeConfigPath = filepath.Join(dir, "runtime.yaml")
	if err = second.Restore(context.Background()); err != nil {
		t.Fatal(err)
	}
	restored, _ := repo.Load()
	if restored.Gateway.State != "running" || restored.Gateway.Port == 0 {
		t.Fatalf("gateway was not restored: %#v", restored.Gateway)
	}
	nonResident, err := second.setResident(json.RawMessage(`{"expectedRevision":` + jsonNumber(restored.Revision) + `,"resident":false}`))
	if err != nil || nonResident.Gateway.Resident {
		t.Fatalf("disable resident = %#v %v", nonResident.Gateway, err)
	}
	second.Close()
	stopped, _ := repo.Load()
	if stopped.Gateway.State != "stopped" || stopped.Gateway.Port != 0 || stopped.Gateway.BaseURL != "" {
		t.Fatalf("non-resident close left running state: %#v", stopped.Gateway)
	}
}

func TestRunningGatewayReloadsAfterModelConfigurationChanges(t *testing.T) {
	dir := t.TempDir()
	secretStore := secrets.NewMemoryStore()
	events := make(chan nativeio.Response, 4)
	engine, err := NewEngine(domain.NewRepository(filepath.Join(dir, "state.json")), secretStore, func(event nativeio.Response) { events <- event })
	if err != nil {
		t.Fatal(err)
	}
	engine.runtimeConfigPath = filepath.Join(dir, "runtime.yaml")
	t.Cleanup(engine.Close)
	created, err := engine.createProvider(json.RawMessage(`{"expectedRevision":1,"name":"Local","baseUrl":"http://127.0.0.1:9/v1","protocol":"openai_chat","apiKey":"provider-key"}`))
	if err != nil {
		t.Fatal(err)
	}
	providerID := created.Providers[len(created.Providers)-1].ID
	running, err := engine.startGateway(context.Background(), json.RawMessage(`{"expectedRevision":2}`))
	if err != nil {
		t.Fatal(err)
	}
	result, err := engine.dispatch(context.Background(), "model_models_upsert", json.RawMessage(`{"expectedRevision":`+jsonNumber(running.Revision)+`,"providerId":"`+providerID+`","model":{"id":"live-model","enabled":true}}`))
	if err != nil {
		t.Fatal(err)
	}
	reloaded := result.(domain.Snapshot)
	if reloaded.Gateway.State != "running" || reloaded.Revision < running.Revision+3 {
		t.Fatalf("running gateway did not reload: %#v", reloaded.Gateway)
	}
	key, _ := secretStore.Get(gatewaySecretRef)
	if gatewayStatus(t, reloaded.Gateway.BaseURL, key) != http.StatusOK {
		t.Fatal("reloaded gateway stopped responding")
	}
	select {
	case event := <-events:
		state := event.Result.(map[string]any)["snapshot"].(domain.Snapshot).Gateway.State
		if state != "running" && state != "restarting" {
			t.Fatalf("unexpected gateway event state: %s", state)
		}
	default:
		t.Fatal("gateway reload did not emit state changes")
	}
}

func TestGatewayMultiKeyAndUsageAPIs(t *testing.T) {
	dir := t.TempDir()
	secretStore := secrets.NewMemoryStore()
	repo := domain.NewRepository(filepath.Join(dir, "state.json"))
	engine, err := NewEngine(repo, secretStore, nil)
	if err != nil {
		t.Fatal(err)
	}
	engine.runtimeConfigPath = filepath.Join(dir, "runtime.yaml")
	t.Cleanup(engine.Close)

	// 1. Gateway settings update
	settingsRes, err := engine.dispatch(context.Background(), "model_gateway_settings_update", json.RawMessage(`{"expectedRevision":1,"settings":{"routingStrategy":"fill_first","requestRetry":3,"preferredPort":8317}}`))
	if err != nil {
		t.Fatalf("settings update failed: %v", err)
	}
	snap := settingsRes.(domain.Snapshot)
	if snap.Gateway.Settings.RoutingStrategy != "fill_first" || snap.Gateway.Settings.RequestRetry != 3 {
		t.Fatalf("unexpected settings: %+v", snap.Gateway.Settings)
	}

	// 2. Gateway Key Create
	createKeyRes, err := engine.dispatch(context.Background(), "model_gateway_key_create", json.RawMessage(`{"expectedRevision":`+jsonNumber(snap.Revision)+`,"name":"Dev Key"}`))
	if err != nil {
		t.Fatalf("key create failed: %v", err)
	}
	createMap := createKeyRes.(map[string]any)
	newKeyText := createMap["accessKey"].(string)
	snap = createMap["snapshot"].(domain.Snapshot)
	if len(snap.Gateway.AccessKeys) != 2 || snap.Gateway.AccessKeys[1].Name != "Dev Key" {
		t.Fatalf("unexpected access keys: %+v", snap.Gateway.AccessKeys)
	}
	devKeyID := snap.Gateway.AccessKeys[1].ID

	// 3. Gateway Key Reveal
	revealRes, err := engine.dispatch(context.Background(), "model_gateway_key_reveal", json.RawMessage(`{"keyId":"`+devKeyID+`"}`))
	if err != nil {
		t.Fatalf("reveal failed: %v", err)
	}
	if revealRes.(map[string]string)["accessKey"] != newKeyText {
		t.Fatalf("revealed key mismatch: %v vs %v", revealRes, newKeyText)
	}

	// 4. Gateway Key Update
	updateKeyRes, err := engine.dispatch(context.Background(), "model_gateway_key_update", json.RawMessage(`{"expectedRevision":`+jsonNumber(snap.Revision)+`,"keyId":"`+devKeyID+`","name":"Renamed Key"}`))
	if err != nil {
		t.Fatalf("key update failed: %v", err)
	}
	snap = updateKeyRes.(domain.Snapshot)
	if snap.Gateway.AccessKeys[1].Name != "Renamed Key" {
		t.Fatalf("expected renamed key: %+v", snap.Gateway.AccessKeys[1])
	}

	// 5. Usage DB Status & APIs
	statusRes, err := engine.dispatch(context.Background(), "model_usage_status", nil)
	if err != nil {
		t.Fatalf("usage status failed: %v", err)
	}
	statusMap := statusRes.(map[string]any)
	if statusMap["dbPath"] == "" {
		t.Fatalf("expected dbPath in status, got %+v", statusMap)
	}

	// 6. Insert dummy event directly into usageStore to test overview and analytics
	err = engine.usageStore.InsertEvent(&usage.Event{
		ID:          "evt-test-1",
		RequestedAt: time.Now().UTC(),
		LatencyMs:   100,
		Provider:    "openai",
		Model:       "gpt-4o",
		Result:      usage.ResultSuccess,
		HTTPStatus:  200,
		InputTokens: 100,
		TotalTokens: 100,
		CostMicro:   250,
	})
	if err != nil {
		t.Fatalf("InsertEvent failed: %v", err)
	}

	overviewRes, err := engine.dispatch(context.Background(), "model_usage_overview", json.RawMessage(`{"range":"all"}`))
	if err != nil {
		t.Fatalf("overview failed: %v", err)
	}
	ov := overviewRes.(*usage.OverviewResult)
	if ov.TotalEventsCount != 1 || ov.Metrics.TotalRequests != 1 {
		t.Fatalf("unexpected overview metrics: %+v", ov)
	}

	analyticsRes, err := engine.dispatch(context.Background(), "model_usage_analysis", json.RawMessage(`{"range":"all"}`))
	if err != nil {
		t.Fatalf("analytics failed: %v", err)
	}
	an := analyticsRes.(*usage.AnalyticsResult)
	if len(an.ByModel) != 1 || an.ByModel[0].Requests != 1 {
		t.Fatalf("unexpected analytics result: %+v", an)
	}

	eventsRes, err := engine.dispatch(context.Background(), "model_usage_events", json.RawMessage(`{"range":"all","limit":10}`))
	if err != nil {
		t.Fatalf("events failed: %v", err)
	}
	ev := eventsRes.(*usage.EventsResult)
	if ev.Total != 1 || len(ev.Events) != 1 {
		t.Fatalf("unexpected events result: %+v", ev)
	}

	// 7. Pricing catalog
	pricingRes, err := engine.dispatch(context.Background(), "model_usage_pricing", nil)
	if err != nil {
		t.Fatalf("pricing failed: %v", err)
	}
	pr := pricingRes.(*usage.PricingResult)
	if len(pr.Prices) == 0 {
		t.Fatalf("expected non-empty pricing catalog")
	}

	// 8. Custom price upsert and delete
	upsertPriceRes, err := engine.dispatch(context.Background(), "model_usage_price_upsert", json.RawMessage(`{"providerId":"custom","modelId":"my-model","inputPriceMicro":1000,"outputPriceMicro":2000}`))
	if err != nil {
		t.Fatalf("price upsert failed: %v", err)
	}
	if len(upsertPriceRes.(*usage.PricingResult).Prices) == 0 {
		t.Fatalf("expected updated pricing")
	}
	if _, err := engine.dispatch(context.Background(), "model_usage_price_upsert", json.RawMessage(`{"modelId":"bad","inputPriceMicro":-1}`)); err == nil {
		t.Fatal("expected negative price to be rejected")
	}

	deletePriceRes, err := engine.dispatch(context.Background(), "model_usage_price_delete", json.RawMessage(`{"providerId":"custom","modelId":"my-model"}`))
	if err != nil {
		t.Fatalf("price delete failed: %v", err)
	}
	_ = deletePriceRes

	// 9. Key Delete
	delKeyRes, err := engine.dispatch(context.Background(), "model_gateway_key_delete", json.RawMessage(`{"expectedRevision":`+jsonNumber(snap.Revision)+`,"keyId":"`+devKeyID+`"}`))
	if err != nil {
		t.Fatalf("key delete failed: %v", err)
	}
	snap = delKeyRes.(domain.Snapshot)
	if len(snap.Gateway.AccessKeys) != 1 {
		t.Fatalf("expected 1 remaining key: %+v", snap.Gateway.AccessKeys)
	}
}

func gatewayStatus(t *testing.T, baseURL, key string) int {
	t.Helper()
	request, _ := http.NewRequest(http.MethodGet, baseURL+"/v1/models", nil)
	request.Header.Set("Authorization", "Bearer "+key)
	response, err := http.DefaultClient.Do(request)
	if err != nil {
		t.Fatal(err)
	}
	_ = response.Body.Close()
	return response.StatusCode
}

func TestCheckAndPerformKernelUpdate(t *testing.T) {
	repo := domain.NewRepository(filepath.Join(t.TempDir(), "state.json"))
	engine, err := NewEngine(repo, secrets.NewMemoryStore(), nil)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(engine.Close)

	checkRes, err := engine.dispatch(context.Background(), "model_kernel_check_update", nil)
	if err != nil {
		t.Fatalf("model_kernel_check_update failed: %v", err)
	}
	checkMap := checkRes.(map[string]any)
	if checkMap["latestVersion"] == "" {
		t.Fatalf("expected latestVersion, got: %#v", checkMap)
	}

	updateRes, err := engine.dispatch(context.Background(), "model_kernel_update", nil)
	if err != nil {
		t.Fatalf("model_kernel_update failed: %v", err)
	}
	updateMap := updateRes.(map[string]any)
	if updateMap["ok"] != true || updateMap["version"] == "" {
		t.Fatalf("expected update ok with version, got: %#v", updateMap)
	}

	snap, err := repo.Load()
	if err != nil {
		t.Fatal(err)
	}
	if snap.Gateway.KernelVersion == "" || snap.Gateway.LatestKernelVersion == "" {
		t.Fatalf("expected KernelVersion and LatestKernelVersion in snapshot: %#v", snap.Gateway)
	}
}
