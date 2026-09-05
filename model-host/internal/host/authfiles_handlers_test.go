package host

import (
	"context"
	"encoding/json"
	"path/filepath"
	"testing"

	"github.com/ldh/natives/model-host/internal/authfiles"
	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/secrets"
	coreauth "github.com/router-for-me/CLIProxyAPI/v7/sdk/cliproxy/auth"
)

func TestOAuthAccountAppearsInCredentialListAndProvidesQuotaMetadata(t *testing.T) {
	repo := domain.NewRepository(filepath.Join(t.TempDir(), "state.json"))
	revision := int64(1)
	_, err := repo.Update(&revision, func(snapshot *domain.Snapshot) error {
		snapshot.Accounts = append(snapshot.Accounts, domain.Account{
			ID: "antigravity-user.json", Provider: "antigravity", Label: "user@example.test",
			Enabled: true, Status: "active", UpdatedAt: "2026-09-05T00:00:00Z",
		})
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	engine, err := NewEngine(repo, secrets.NewMemoryStore(), nil)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(engine.Close)
	engine.authFiles, err = authfiles.NewManager(filepath.Join(t.TempDir(), "auth"))
	if err != nil {
		t.Fatal(err)
	}

	listed, err := engine.listAuthFiles()
	if err != nil {
		t.Fatal(err)
	}
	items := listed.([]authfiles.AuthFileItem)
	if len(items) != 1 || items[0].AccountID != "antigravity-user.json" || items[0].Source != "keychain" {
		t.Fatalf("OAuth account missing from credential projection: %#v", items)
	}

	token, metadata := oauthQuotaCredential([]*coreauth.Auth{{
		ID: "antigravity-user.json", Provider: "antigravity",
		Metadata: map[string]any{"access_token": "secret-token", "project_id": "project-123"},
	}}, "antigravity", "antigravity-user.json")
	if token != "secret-token" || metadata != `{"project_id":"project-123"}` {
		t.Fatalf("quota credential was not extracted: token=%q metadata=%q", token, metadata)
	}
}

func TestAccountModelsPersistExclusionsToOAuthCredential(t *testing.T) {
	repo := domain.NewRepository(filepath.Join(t.TempDir(), "state.json"))
	revision := int64(1)
	_, err := repo.Update(&revision, func(snapshot *domain.Snapshot) error {
		snapshot.Accounts = append(snapshot.Accounts, domain.Account{ID: "ag.json", Provider: "antigravity", Label: "user", Enabled: true, Status: "active"})
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	engine, err := NewEngine(repo, secrets.NewMemoryStore(), nil)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(engine.Close)
	credential := &coreauth.Auth{ID: "ag.json", Provider: "antigravity", Metadata: map[string]any{"access_token": "token"}}
	if _, err = engine.authStore.Save(context.Background(), credential); err != nil {
		t.Fatal(err)
	}
	result, err := engine.accountModels(json.RawMessage(`{"accountId":"ag.json"}`))
	if err != nil {
		t.Fatal(err)
	}
	models := result["models"].([]accountModel)
	if len(models) < 2 {
		t.Fatalf("kernel model definitions missing: %#v", models)
	}
	snapshot, _ := repo.Load()
	input, _ := json.Marshal(map[string]any{"accountId": "ag.json", "expectedRevision": snapshot.Revision, "enabledModelIds": []string{models[0].ID}})
	if _, err = engine.updateAccountModels(input); err != nil {
		t.Fatal(err)
	}
	saved, err := engine.accountCredential(context.Background(), "ag.json")
	if err != nil {
		t.Fatal(err)
	}
	if saved.Attributes["excluded_models"] == "" || matchesAnyRule(models[0].ID, credentialExcludedModels(saved)) {
		t.Fatalf("model exclusions were not persisted correctly: %#v", saved.Attributes)
	}
}
