package domain

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestRepositoryCreatesAndRevisionChecksAtomicState(t *testing.T) {
	path := filepath.Join(t.TempDir(), "nested", "state.json")
	repo := NewRepository(path)
	initial, err := repo.Load()
	if err != nil {
		t.Fatal(err)
	}
	if initial.Revision != 1 || len(initial.Providers) != 0 || initial.SchemaVersion != SchemaVersion {
		t.Fatalf("unexpected initial snapshot: %#v", initial)
	}
	expected := initial.Revision
	updated, err := repo.Update(&expected, func(snapshot *Snapshot) error {
		snapshot.Resident = true
		return nil
	})
	if err != nil || updated.Revision != 2 || !updated.Resident {
		t.Fatalf("unexpected update: %#v %v", updated, err)
	}
	if _, err = repo.Update(&expected, func(*Snapshot) error { return nil }); err == nil {
		t.Fatal("stale revision was accepted")
	}
	info, err := os.Stat(path)
	if err != nil || info.Mode().Perm()&0o077 != 0 {
		t.Fatalf("state file permissions are not private: %v %v", info, err)
	}
}

func TestRepositoryMigratesProxyStateIdempotently(t *testing.T) {
	path := filepath.Join(t.TempDir(), "state.json")
	legacy := map[string]any{
		"schemaVersion": 3,
		"revision":      7,
		"providers": []any{
			map[string]any{
				"id": "provider_custom", "kind": "custom", "name": "Synthetic",
				"baseUrl": "https://example.test/v1", "protocol": "openai_chat",
				"enabled": true, "secretRef": "provider-api-key:provider_custom",
				"models": []any{map[string]any{"id": "model-a", "displayName": "A", "alias": "legacy-route", "enabled": true, "manual": true}},
			},
			map[string]any{"id": "codex", "kind": "oauth", "oauthProvider": "codex", "name": "codex", "enabled": true, "models": []any{}},
		},
		"accounts": []any{map[string]any{"id": "acct", "provider": "codex", "label": "legacy", "enabled": true, "secretRef": "oauth:acct"}},
		"gateway": map[string]any{
			"state": "running", "resident": true, "port": 8317,
			"accessKeys": []any{map[string]any{"id": "key_default", "secretRef": "gateway:access-key", "enabled": true}},
			"settings":   map[string]any{"routingStrategy": "round_robin", "requestRetry": 3},
		},
		"defaultModel": map[string]any{"providerId": "provider_custom", "modelId": "model-a"},
	}
	data, err := json.Marshal(legacy)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(path, data, 0o600); err != nil {
		t.Fatal(err)
	}
	repo := NewRepository(path)
	migrated, err := repo.Load()
	if err != nil {
		t.Fatal(err)
	}
	if migrated.SchemaVersion != SchemaVersion || !migrated.Resident {
		t.Fatalf("schema or resident not migrated: %#v", migrated)
	}
	if len(migrated.Providers) != 1 || migrated.Providers[0].ID != "provider_custom" {
		t.Fatalf("retired OAuth provider must not survive: %#v", migrated.Providers)
	}
	if len(migrated.Providers[0].Models) != 1 || migrated.Providers[0].Models[0].ID != "model-a" {
		t.Fatalf("custom model missing: %#v", migrated.Providers[0].Models)
	}
	if migrated.DefaultModel.ProviderID != "provider_custom" || len(migrated.Accounts) != 1 {
		t.Fatalf("default model or quota account lost: %#v", migrated)
	}
	persisted, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	text := string(persisted)
	for _, retired := range []string{"gateway", "accessKeys", "alias", "oauthProvider"} {
		if strings.Contains(text, `"`+retired+`"`) {
			t.Fatalf("retired field %q leaked into schema 4 state: %s", retired, text)
		}
	}
	second, err := repo.Load()
	if err != nil {
		t.Fatal(err)
	}
	if second.Revision != migrated.Revision {
		t.Fatalf("repeated migration changed revision: %d -> %d", migrated.Revision, second.Revision)
	}
}

func TestRepositoryClearsDefaultPointingAtRetiredProvider(t *testing.T) {
	path := filepath.Join(t.TempDir(), "state.json")
	legacy := map[string]any{
		"schemaVersion": 3, "revision": 2,
		"providers":    []any{map[string]any{"id": "codex", "kind": "oauth", "name": "codex", "enabled": true, "models": []any{}}},
		"gateway":      map[string]any{"resident": false},
		"defaultModel": map[string]any{"providerId": "codex", "modelId": "model-a"},
	}
	data, _ := json.Marshal(legacy)
	if err := os.WriteFile(path, data, 0o600); err != nil {
		t.Fatal(err)
	}
	migrated, err := NewRepository(path).Load()
	if err != nil {
		t.Fatal(err)
	}
	if migrated.DefaultModel.ProviderID != "" {
		t.Fatalf("dangling default must be cleared: %#v", migrated.DefaultModel)
	}
}
