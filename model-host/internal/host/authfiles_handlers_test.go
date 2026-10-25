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
)

func TestLegacyOAuthAccountIsQuotaReadOnlyCredential(t *testing.T) {
	repo := domain.NewRepository(filepath.Join(t.TempDir(), "state.json"))
	revision := int64(1)
	_, err := repo.Update(&revision, func(snapshot *domain.Snapshot) error {
		snapshot.Accounts = append(snapshot.Accounts, domain.Account{
			ID: "antigravity-user.json", Provider: "antigravity", Label: "user@example.test",
			Enabled: true, Status: "active", SecretRef: "oauth:antigravity-user.json",
			UpdatedAt: "2026-09-05T00:00:00Z",
		})
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	store := secrets.NewMemoryStore()
	if err := store.Set("oauth:antigravity-user.json", `{"access_token":"secret-token","project_id":"project-123"}`); err != nil {
		t.Fatal(err)
	}
	engine, err := NewEngine(repo, store, nil)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(engine.Close)
	engine.authFiles, err = authfiles.NewManager(filepath.Join(t.TempDir(), "auth"))
	if err != nil {
		t.Fatal(err)
	}
	// A matching file is joined to the account row; the keychain row is deduped.
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
		t.Fatalf("file row must be joined to its legacy account: %#v", items)
	}
	if keychainRow != nil {
		t.Fatalf("matched account must not emit a duplicate keychain row: %#v", items)
	}

	token, extra, label, matched, err := engine.oauthQuotaCredential(context.Background(), "antigravity", "antigravity-user.json")
	if err != nil {
		t.Fatal(err)
	}
	if token != "secret-token" || label != "user@example.test" || matched != "antigravity-user.json" {
		t.Fatalf("quota credential not extracted: token=%q label=%q id=%q", token, label, matched)
	}
	var payload map[string]any
	if json.Unmarshal([]byte(extra), &payload) != nil || payload["project_id"] != "project-123" {
		t.Fatalf("extra metadata not preserved: %q", extra)
	}
}

func TestAuthFileQuotaCredentialManagement(t *testing.T) {
	engine, _, _ := newEngineHarness(t)
	imported := dispatchJSON(t, engine, "model_auth_files_import", map[string]any{
		"name": "gemini-test.json", "content": `{"provider":"gemini","account":"test@example.com","access_token":"t"}`,
	}).(*authfiles.AuthFileItem)
	if imported.Provider != "gemini" || imported.Account != "test@example.com" {
		t.Fatalf("auth file metadata not parsed: %#v", imported)
	}
	updated := dispatchJSON(t, engine, "model_auth_files_update", map[string]any{
		"name": "gemini-test.json", "disabled": true,
	}).(*authfiles.AuthFileItem)
	if !updated.Disabled {
		t.Fatalf("auth file disable not persisted: %#v", updated)
	}
	list := dispatchJSON(t, engine, "model_auth_files_list", nil).([]authfiles.AuthFileItem)
	if len(list) != 1 || !list[0].Disabled {
		t.Fatalf("auth file list mismatch: %#v", list)
	}
	dispatchJSON(t, engine, "model_auth_files_delete", map[string]any{"name": "gemini-test.json"})
	list = dispatchJSON(t, engine, "model_auth_files_list", nil).([]authfiles.AuthFileItem)
	if len(list) != 0 {
		t.Fatalf("auth file was not deleted: %#v", list)
	}
}

func TestRetiredAccountManagementMethodsAreRejected(t *testing.T) {
	engine, _, _ := newEngineHarness(t)
	for _, method := range []string{"model_account_set_enabled", "model_account_delete", "model_account_reauth", "model_account_models_update"} {
		if _, err := engine.dispatch(context.Background(), method, json.RawMessage(`{}`)); err == nil {
			t.Fatalf("retired method %s must be rejected", method)
		}
	}
}
