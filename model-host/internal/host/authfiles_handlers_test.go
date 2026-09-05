package host

import (
	"context"
	"encoding/json"
	"os"
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
	// A file-backed credential matching the account label must be joined to
	// the account row (accountId stamped), and the keychain row deduped.
	fileContent := `{"provider":"antigravity","email":"user@example.test","access_token":"t"}`
	if err := os.WriteFile(filepath.Join(engine.authFiles.BaseDir(), "antigravity-user@example.test.json"), []byte(fileContent), 0o644); err != nil {
		t.Fatal(err)
	}

	listed, err := engine.listAuthFiles()
	if err != nil {
		t.Fatal(err)
	}
	items := listed.([]authfiles.AuthFileItem)
	var fileRow, keychainRow *authfiles.AuthFileItem
	for index := range items {
		switch items[index].Source {
		case "file":
			fileRow = &items[index]
		case "keychain":
			keychainRow = &items[index]
		}
	}
	if fileRow == nil || fileRow.AccountID != "antigravity-user.json" {
		t.Fatalf("file row must be joined to its OAuth account: %#v", items)
	}
	if keychainRow != nil {
		t.Fatalf("matched account must not emit a duplicate keychain row: %#v", items)
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

	func TestAccountModelsPersistExclusionsToAuthFile(t *testing.T) {
		tempDir := t.TempDir()
		repo := domain.NewRepository(filepath.Join(tempDir, "state.json"))
		afm, err := authfiles.NewManager(filepath.Join(tempDir, "auth"))
		if err != nil {
			t.Fatal(err)
		}
		_, err = afm.Import("gemini-test.json", []byte(`{"provider":"gemini","account":"test@example.com"}`))
		if err != nil {
			t.Fatal(err)
		}
		engine, err := NewEngine(repo, secrets.NewMemoryStore(), nil)
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(engine.Close)
		engine.authFiles = afm

		// 1. Query models for the auth file by name
		res, err := engine.accountModels(json.RawMessage(`{"name":"gemini-test.json"}`))
		if err != nil {
			t.Fatalf("accountModels for auth file failed: %v", err)
		}
		models := res["models"].([]accountModel)
		if len(models) == 0 {
			t.Fatalf("expected static models for gemini, got 0")
		}
		targetModel := models[0].ID

		// 2. Update models to enable only targetModel
		updInput, _ := json.Marshal(map[string]any{
			"name":            "gemini-test.json",
			"enabledModelIds": []string{targetModel},
		})
		updRes, err := engine.updateAccountModels(updInput)
		if err != nil {
			t.Fatalf("updateAccountModels for auth file failed: %v", err)
		}
		updModels := updRes["models"].([]accountModel)
		for _, m := range updModels {
			if m.ID == targetModel && !m.Enabled {
				t.Fatalf("model %s should be enabled", m.ID)
			}
			if m.ID != targetModel && m.Enabled {
				t.Fatalf("model %s should be disabled", m.ID)
			}
		}

		// 3. Verify on-disk file has excluded_models
		item, err := afm.List()
		if err != nil || len(item) == 0 {
			t.Fatalf("failed to read updated auth file: %v", err)
		}
		if len(item[0].ExcludedModels) == 0 {
			t.Fatalf("excluded_models not written to auth file: %#v", item[0])
		}
	}
