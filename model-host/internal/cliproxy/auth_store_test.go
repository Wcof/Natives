package cliproxy

import (
	"context"
	"strings"
	"testing"

	"github.com/ldh/natives/model-host/internal/secrets"
	coreauth "github.com/router-for-me/CLIProxyAPI/v7/sdk/cliproxy/auth"
)

func TestAuthStorePersistsOnlyToSecretStore(t *testing.T) {
	secretStore := secrets.NewMemoryStore()
	var indexed []string
	store := NewAuthStore(secretStore, nil, func(ids []string) error {
		indexed = append([]string(nil), ids...)
		return nil
	})
	auth := &coreauth.Auth{
		ID: "codex-account", Provider: "codex", Label: "user@example.com",
		Metadata: map[string]any{"access_token": "access-secret", "refresh_token": "refresh-secret"},
	}
	if _, err := store.Save(context.Background(), auth); err != nil {
		t.Fatal(err)
	}
	if len(indexed) != 1 || indexed[0] != auth.ID {
		t.Fatalf("unexpected index: %#v", indexed)
	}
	for key, value := range secretStore.Values {
		if !strings.HasPrefix(key, "oauth:") || !strings.Contains(value, "access-secret") {
			t.Fatalf("credential not isolated in secret store: %q %q", key, value)
		}
	}
	loaded, err := store.List(context.Background())
	if err != nil || len(loaded) != 1 || loaded[0].Provider != "codex" {
		t.Fatalf("unexpected load: %#v %v", loaded, err)
	}
	auth.Metadata["access_token"] = "refreshed-access-secret"
	if _, err = store.Save(context.Background(), auth); err != nil {
		t.Fatal(err)
	}
	refreshed := secretStore.Values["oauth:"+auth.ID]
	if !strings.Contains(refreshed, "refreshed-access-secret") || strings.Contains(refreshed, "\"access_token\":\"access-secret\"") {
		t.Fatalf("token refresh was not atomically persisted: %s", refreshed)
	}
	if err = store.Delete(context.Background(), auth.ID); err != nil {
		t.Fatal(err)
	}
	if len(secretStore.Values) != 0 || len(indexed) != 0 {
		t.Fatalf("delete left state: secrets=%#v index=%#v", secretStore.Values, indexed)
	}
}

func TestAuthStoreFailsClosedWhenKeychainUnavailable(t *testing.T) {
	secretStore := secrets.NewMemoryStore()
	secretStore.Unavailable = true
	store := NewAuthStore(secretStore, nil, nil)
	_, err := store.Save(context.Background(), &coreauth.Auth{ID: "x", Provider: "xai"})
	if err == nil {
		t.Fatal("save succeeded while keychain was unavailable")
	}
}
