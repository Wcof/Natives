package host

import (
	"context"
	"encoding/json"
	"strings"
	"testing"

	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/secrets"
)

func TestCreateGlobalAIConnectionAtomically(t *testing.T) {
	e, keychain, _ := newEngineHarness(t)
	input := map[string]any{"expectedRevision": 1, "name": "Global AI", "baseUrl": "https://example.test/v1", "protocol": "openai_chat", "apiKey": "synthetic-key", "modelId": "  model-a  ", "setAsDefault": true}
	s := dispatchJSON(t, e, "model_provider_create", input).(domain.Snapshot)
	if s.Revision != 2 || len(s.Providers) != 1 || len(s.Providers[0].Models) != 1 || s.DefaultModel.ProviderID != s.Providers[0].ID || s.DefaultModel.ModelID != "model-a" {
		t.Fatalf("setup must commit exactly once: %+v", s)
	}
	options := dispatchJSON(t, e, "model_inference_options", nil).(map[string]any)
	if options["defaultModel"].(map[string]any)["modelId"] != "model-a" {
		t.Fatal("modules cannot resolve the global default")
	}
	if secret, _ := keychain.Get(s.Providers[0].SecretRef); secret != "synthetic-key" {
		t.Fatal("credential not stored by the secret owner")
	}
	input["expectedRevision"] = s.Revision
	input["setAsDefault"] = false
	input["modelId"] = "model-b"
	s = dispatchJSON(t, e, "model_provider_create", input).(domain.Snapshot)
	if s.DefaultModel.ModelID != "model-a" {
		t.Fatal("adding a connection replaced the global default without a choice")
	}
}

func TestGlobalConnectionSetupFailuresPreserveState(t *testing.T) {
	for _, tc := range []struct {
		name     string
		model    string
		enabled  bool
		revision int
		locked   bool
	}{
		{"missing_model", "", true, 2, false},
		{"blank_model", "  ", true, 2, false},
		{"long_model", strings.Repeat("x", 201), true, 2, false},
		{"disabled", "model", false, 2, false},
		{"conflict", "model", true, 99, false},
		{"locked_keychain", "model", true, 2, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			e, keychain, _ := newEngineHarness(t)
			original := dispatchJSON(t, e, "model_provider_create", map[string]any{"expectedRevision": 1, "name": "Existing", "baseUrl": "https://example.test/v1", "protocol": "openai_chat", "apiKey": "existing-synthetic", "modelId": "original", "setAsDefault": true}).(domain.Snapshot)
			store := keychain.(*secrets.MemoryStore)
			store.Unavailable = tc.locked
			raw, _ := json.Marshal(map[string]any{"expectedRevision": tc.revision, "name": "Fixture", "baseUrl": "https://example.test/v1", "protocol": "openai_chat", "apiKey": "synthetic", "modelId": tc.model, "enabled": tc.enabled, "setAsDefault": true})
			if _, err := e.dispatch(context.Background(), "model_provider_create", raw); err == nil {
				t.Fatal("invalid setup succeeded")
			}
			s, err := e.repo.Load()
			if err != nil || s.Revision != original.Revision || len(s.Providers) != 1 || s.DefaultModel != original.DefaultModel || len(store.Values) != 1 || store.Values[s.Providers[0].SecretRef] != "existing-synthetic" {
				t.Fatalf("failed setup left partial state or a secret: %+v %v", s, err)
			}
		})
	}
}
