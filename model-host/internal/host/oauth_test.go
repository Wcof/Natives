package host

import (
	"context"
	"encoding/json"
	"net/http"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/nativeio"
	"github.com/ldh/natives/model-host/internal/secrets"
	sdkauth "github.com/router-for-me/CLIProxyAPI/v7/sdk/auth"
	clipcore "github.com/router-for-me/CLIProxyAPI/v7/sdk/cliproxy"
	coreauth "github.com/router-for-me/CLIProxyAPI/v7/sdk/cliproxy/auth"
	clipconfig "github.com/router-for-me/CLIProxyAPI/v7/sdk/config"
)

type fakeLoginManager struct {
	login func(context.Context, string) (*coreauth.Auth, error)
}

func (f fakeLoginManager) Login(ctx context.Context, provider string, _ *clipconfig.Config, _ *sdkauth.LoginOptions) (*coreauth.Auth, string, error) {
	record, err := f.login(ctx, provider)
	return record, "keychain", err
}

func TestOAuthFiveProvidersPersistToSecretStore(t *testing.T) {
	dir := t.TempDir()
	secretStore := secrets.NewMemoryStore()
	events := make(chan nativeio.Response, 10)
	engine, err := NewEngine(domain.NewRepository(filepath.Join(dir, "state.json")), secretStore, func(event nativeio.Response) { events <- event })
	if err != nil {
		t.Fatal(err)
	}
	defer engine.Close()
	engine.auth = fakeLoginManager{login: func(_ context.Context, provider string) (*coreauth.Auth, error) {
		record := &coreauth.Auth{ID: provider + "-account", Provider: provider, Label: provider + "@example.test", Status: coreauth.StatusActive, Metadata: map[string]any{"access_token": provider + "-access-secret", "refresh_token": provider + "-refresh-secret"}}
		_, saveErr := engine.authStore.Save(context.Background(), record)
		return record, saveErr
	}}
	snapshot, _ := engine.repo.Load()
	for _, provider := range domain.OAuthProviders {
		result, startErr := engine.startOAuth(context.Background(), json.RawMessage(`{"provider":"`+provider+`","expectedRevision":`+jsonNumber(snapshot.Revision)+`}`))
		if startErr != nil || result["state"] != "pending" {
			t.Fatalf("start %s = %#v %v", provider, result, startErr)
		}
		select {
		case event := <-events:
			payload := event.Result.(map[string]any)
			if payload["state"] != "succeeded" || payload["provider"] != provider {
				t.Fatalf("unexpected OAuth event: %#v", payload)
			}
			snapshot = payload["snapshot"].(domain.Snapshot)
		case <-time.After(time.Second):
			t.Fatalf("OAuth fixture timed out for %s", provider)
		}
	}
	if len(snapshot.Accounts) != len(domain.OAuthProviders) || len(secretStore.Values) != len(domain.OAuthProviders) {
		t.Fatalf("OAuth accounts were not persisted: accounts=%d secrets=%d", len(snapshot.Accounts), len(secretStore.Values))
	}
	state, _ := os.ReadFile(filepath.Join(dir, "state.json"))
	if strings.Contains(string(state), "access-secret") || strings.Contains(string(state), "refresh-secret") {
		t.Fatal("OAuth secret leaked into metadata")
	}
}

func TestOAuthCatalogKeepsAccountModelIdentity(t *testing.T) {
	repo := domain.NewRepository(filepath.Join(t.TempDir(), "state.json"))
	revision := int64(1)
	_, err := repo.Update(&revision, func(snapshot *domain.Snapshot) error {
		snapshot.Accounts = append(snapshot.Accounts, domain.Account{ID: "account-a", Provider: "codex", Enabled: true}, domain.Account{ID: "account-b", Provider: "codex", Enabled: true})
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	events := make(chan nativeio.Response, 3)
	engine, err := NewEngine(repo, secrets.NewMemoryStore(), func(event nativeio.Response) { events <- event })
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(engine.Close)
	hook := oauthCatalogHook{engine: engine}
	hook.OnModelsRegistered(context.Background(), "codex", "account-a", []*clipcore.ModelInfo{{ID: "shared", DisplayName: "Shared", ContextLength: 100}})
	hook.OnModelsRegistered(context.Background(), "codex", "account-b", []*clipcore.ModelInfo{{ID: "shared", DisplayName: "Shared", ContextLength: 100}})
	hook.OnModelsRegistered(context.Background(), "codex", "account-a", []*clipcore.ModelInfo{{ID: "new-a", DisplayName: "New A", ContextLength: 200}})
	snapshot, _ := repo.Load()
	provider, _ := findProvider(&snapshot, "codex")
	shared := provider.Models[slices.IndexFunc(provider.Models, func(model domain.Model) bool { return model.ID == "shared" })]
	newA := provider.Models[slices.IndexFunc(provider.Models, func(model domain.Model) bool { return model.ID == "new-a" })]
	if !slices.Equal(shared.AccountIDs, []string{"account-b"}) || !slices.Equal(newA.AccountIDs, []string{"account-a"}) || newA.ContextLength != 200 {
		t.Fatalf("OAuth model identity was conflated: %#v", provider.Models)
	}
	if len(events) != 3 {
		t.Fatalf("expected catalog events, got %d", len(events))
	}
}

func TestOAuthDuplicateCancelAndTimeoutStates(t *testing.T) {
	events := make(chan nativeio.Response, 4)
	engine, err := NewEngine(domain.NewRepository(filepath.Join(t.TempDir(), "state.json")), secrets.NewMemoryStore(), func(event nativeio.Response) { events <- event })
	if err != nil {
		t.Fatal(err)
	}
	defer engine.Close()
	engine.oauthTimeout = time.Second
	engine.auth = fakeLoginManager{login: func(ctx context.Context, _ string) (*coreauth.Auth, error) { <-ctx.Done(); return nil, ctx.Err() }}
	started, err := engine.startOAuth(context.Background(), json.RawMessage(`{"provider":"codex","expectedRevision":1}`))
	if err != nil {
		t.Fatal(err)
	}
	if _, duplicateErr := engine.startOAuth(context.Background(), json.RawMessage(`{"provider":"claude","expectedRevision":1}`)); duplicateErr == nil {
		t.Fatal("a second OAuth session was accepted")
	}
	if _, err = engine.cancelOAuth(json.RawMessage(`{"sessionId":"` + started["sessionId"] + `","expectedRevision":1}`)); err != nil {
		t.Fatal(err)
	}
	if payload := (<-events).Result.(map[string]any); payload["state"] != "cancelled" {
		t.Fatalf("cancel event = %#v", payload)
	}
	for {
		engine.mu.Lock()
		remaining := len(engine.sessions)
		engine.mu.Unlock()
		if remaining == 0 {
			break
		}
		time.Sleep(time.Millisecond)
	}
	engine.oauthTimeout = 20 * time.Millisecond
	if _, err = engine.startOAuth(context.Background(), json.RawMessage(`{"provider":"xai","expectedRevision":1}`)); err != nil {
		t.Fatal(err)
	}
	select {
	case event := <-events:
		if payload := event.Result.(map[string]any); payload["state"] != "timeout" || payload["errorCode"] != "oauth_timeout" {
			t.Fatalf("timeout event = %#v", payload)
		}
	case <-time.After(time.Second):
		t.Fatal("OAuth timeout was not emitted")
	}
}

func TestReauthorizationReplacesCredentialWithoutChangingAccountIdentity(t *testing.T) {
	events := make(chan nativeio.Response, 8)
	secretStore := secrets.NewMemoryStore()
	repo := domain.NewRepository(filepath.Join(t.TempDir(), "state.json"))
	revision := int64(1)
	_, err := repo.Update(&revision, func(snapshot *domain.Snapshot) error {
		snapshot.Accounts = append(snapshot.Accounts, domain.Account{ID: "stable-account", Provider: "kimi", Label: "old", Enabled: true, Status: "active", SecretRef: "oauth:stable-account"})
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	engine, err := NewEngine(repo, secretStore, func(event nativeio.Response) { events <- event })
	if err != nil {
		t.Fatal(err)
	}
	defer engine.Close()
	old := &coreauth.Auth{ID: "stable-account", Provider: "kimi", Label: "old", Status: coreauth.StatusActive, Metadata: map[string]any{"access_token": "old-token"}}
	if _, err = engine.authStore.Save(context.Background(), old); err != nil {
		t.Fatal(err)
	}
	<-events
	engine.auth = fakeLoginManager{login: func(_ context.Context, _ string) (*coreauth.Auth, error) {
		fresh := &coreauth.Auth{ID: "temporary-account", Provider: "kimi", Label: "new", Status: coreauth.StatusActive, Metadata: map[string]any{"access_token": "new-token"}}
		_, saveErr := engine.authStore.Save(context.Background(), fresh)
		return fresh, saveErr
	}}
	snapshot, _ := repo.Load()
	if _, err = engine.startOAuth(context.Background(), json.RawMessage(`{"provider":"kimi","accountId":"stable-account","expectedRevision":`+jsonNumber(snapshot.Revision)+`}`)); err != nil {
		t.Fatal(err)
	}
	deadline := time.After(time.Second)
	for {
		select {
		case event := <-events:
			if event.Event != "model_oauth_state_changed" {
				continue
			}
			payload := event.Result.(map[string]any)
			if payload["state"] != "succeeded" {
				t.Fatalf("reauthorization failed: %#v", payload)
			}
			final := payload["snapshot"].(domain.Snapshot)
			if len(final.Accounts) != 1 || final.Accounts[0].ID != "stable-account" || final.Accounts[0].Label != "new" {
				t.Fatalf("reauthorization changed account identity: %#v", final.Accounts)
			}
			if _, exists := secretStore.Values["oauth:temporary-account"]; exists {
				t.Fatal("temporary OAuth credential was not removed")
			}
			stored := secretStore.Values["oauth:stable-account"]
			if !strings.Contains(stored, "new-token") || strings.Contains(stored, "old-token") {
				t.Fatal("stable account credential was not replaced")
			}
			return
		case <-deadline:
			t.Fatal("reauthorization timed out")
		}
	}
}

func TestCredentialRefreshUpdatesAccountStatusAndAccountMutations(t *testing.T) {
	events := make(chan nativeio.Response, 4)
	secretStore := secrets.NewMemoryStore()
	engine, err := NewEngine(domain.NewRepository(filepath.Join(t.TempDir(), "state.json")), secretStore, func(event nativeio.Response) { events <- event })
	if err != nil {
		t.Fatal(err)
	}
	defer engine.Close()
	snapshot, err := engine.repo.Update(ptrRevision(1), func(snapshot *domain.Snapshot) error {
		snapshot.Accounts = append(snapshot.Accounts, domain.Account{ID: "account", Provider: "codex", Label: "fixture", Enabled: true, Status: "active", SecretRef: "oauth:account"})
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	credential := &coreauth.Auth{ID: "account", Provider: "codex", Status: coreauth.StatusActive, Metadata: map[string]any{"access_token": "refreshed-token"}}
	if _, err = engine.authStore.Save(context.Background(), credential); err != nil {
		t.Fatal(err)
	}
	snapshot = (<-events).Result.(map[string]any)["snapshot"].(domain.Snapshot)
	if snapshot.Accounts[0].Mask != "••••oken" || snapshot.Accounts[0].Status != "active" {
		t.Fatalf("refresh did not update safe account metadata: %#v", snapshot.Accounts[0])
	}
	credential.Status = coreauth.StatusError
	credential.LastError = &coreauth.Error{HTTPStatus: http.StatusServiceUnavailable, Retryable: true}
	if _, err = engine.authStore.Save(context.Background(), credential); err != nil {
		t.Fatal(err)
	}
	snapshot = (<-events).Result.(map[string]any)["snapshot"].(domain.Snapshot)
	if snapshot.Accounts[0].Status != "active" {
		t.Fatalf("transient upstream failure required reauthorization: %#v", snapshot.Accounts[0])
	}
	credential.LastError.Retryable = false
	if _, err = engine.authStore.Save(context.Background(), credential); err != nil {
		t.Fatal(err)
	}
	snapshot = (<-events).Result.(map[string]any)["snapshot"].(domain.Snapshot)
	if snapshot.Accounts[0].Status != "needs_reauth" {
		t.Fatalf("refresh failure did not require reauthorization: %#v", snapshot.Accounts[0])
	}
	snapshot, err = engine.setAccountEnabled(json.RawMessage(`{"accountId":"account","enabled":false,"expectedRevision":` + jsonNumber(snapshot.Revision) + `}`))
	if err != nil || snapshot.Accounts[0].Enabled {
		t.Fatalf("disable account = %#v %v", snapshot.Accounts[0], err)
	}
	snapshot, err = engine.deleteAccount(json.RawMessage(`{"accountId":"account","expectedRevision":` + jsonNumber(snapshot.Revision) + `}`))
	if err != nil || len(snapshot.Accounts) != 0 || len(secretStore.Values) != 0 {
		t.Fatalf("delete account = %#v secrets=%#v %v", snapshot.Accounts, secretStore.Values, err)
	}
}

func ptrRevision(value int64) *int64 { return &value }
