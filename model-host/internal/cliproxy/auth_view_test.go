package cliproxy

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/ldh/natives/model-host/internal/secrets"
	sdkauth "github.com/router-for-me/CLIProxyAPI/v8/sdk/auth"
	coreauth "github.com/router-for-me/CLIProxyAPI/v8/sdk/cliproxy/auth"
	clipconfig "github.com/router-for-me/CLIProxyAPI/v8/sdk/config"
)

type stubSDKManager struct{ record *coreauth.Auth }

func (s *stubSDKManager) Login(ctx context.Context, provider string, cfg *clipconfig.Config, opts *sdkauth.LoginOptions) (*coreauth.Auth, string, error) {
	return s.record, "", nil
}

func TestViewFromAuthMasksTokenAndCarriesProjectFields(t *testing.T) {
	auth := &coreauth.Auth{
		ID: "acct-1", Provider: "codex", Label: "tester@example.test",
		Status: coreauth.StatusError, Disabled: true,
		LastError:  &coreauth.Error{Retryable: true},
		Attributes: map[string]string{"excluded_models": "gpt-3*,o1-mini"},
		Metadata:   map[string]any{"access_token": "super-secret-access-token"},
	}
	view := viewFromAuth(auth, func(value string) string {
		return "mask(" + value + ")"
	})
	if view.ID != "acct-1" || view.Provider != "codex" || view.Label != "tester@example.test" {
		t.Fatalf("unexpected identity fields: %+v", view)
	}
	if view.Status != AccountStatusError || !view.Disabled || !view.LastErrorRetryable {
		t.Fatalf("status mapping drifted: %+v", view)
	}
	if want := []string{"gpt-3*", "o1-mini"}; strings.Join(view.ExcludedModels, ",") != strings.Join(want, ",") {
		t.Fatalf("excluded models mapping drifted: %+v", view.ExcludedModels)
	}
	if view.Mask != "mask(super-secret-access-token)" {
		t.Fatalf("mask callback not applied: %q", view.Mask)
	}
	// 项目视图绝不携带明文。
	if strings.Contains(strings.ToLower(view.Mask), "secret") && !strings.HasPrefix(view.Mask, "mask(") {
		t.Fatalf("plaintext leaked into view: %q", view.Mask)
	}
}

func TestAccountViewOperationsRoundTripAndRollback(t *testing.T) {
	ctx := context.Background()
	store := NewAuthStore(secrets.NewMemoryStore(), nil, nil)
	if _, err := store.Save(ctx, &coreauth.Auth{ID: "acct-1", Provider: "codex", Status: coreauth.StatusActive, Metadata: map[string]any{"access_token": "tok"}}); err != nil {
		t.Fatal(err)
	}
	// SwapAccountDisabled：更新 + 精确回滚。
	rollback, err := store.SwapAccountDisabled(ctx, "acct-1", true)
	if err != nil {
		t.Fatal(err)
	}
	views, err := store.AccountViews(ctx)
	if err != nil || len(views) != 1 || !views[0].Disabled || views[0].Status != AccountStatusDisabled {
		t.Fatalf("disabled swap did not persist: %+v err=%v", views, err)
	}
	if err := rollback(ctx); err != nil {
		t.Fatal(err)
	}
	views, _ = store.AccountViews(ctx)
	if views[0].Disabled || views[0].Status != AccountStatusActive {
		t.Fatalf("rollback did not restore original record: %+v", views[0])
	}
	// SwapAccountExcludedModels：规则写 Attributes，空规则清除。
	rollback2, err := store.SwapAccountExcludedModels(ctx, "acct-1", []string{"gpt-3*"})
	if err != nil {
		t.Fatal(err)
	}
	views, _ = store.AccountViews(ctx)
	if len(views[0].ExcludedModels) != 1 || views[0].ExcludedModels[0] != "gpt-3*" {
		t.Fatalf("exclusion not persisted: %+v", views[0].ExcludedModels)
	}
	if err := rollback2(ctx); err != nil {
		t.Fatal(err)
	}
	views, _ = store.AccountViews(ctx)
	if len(views[0].ExcludedModels) != 0 {
		t.Fatalf("exclusion rollback drifted: %+v", views[0].ExcludedModels)
	}
	// 未知名直接拒绝。
	if _, err := store.SwapAccountDisabled(ctx, "missing", true); !errors.Is(err, errCredentialNotFound) {
		t.Fatalf("expected credential_not_found, got %v", err)
	}
	// 额度取数对未知账户返回空令牌（宿主按缺凭据诚实拒绝），不误报存储错误。
	token, _, _, matchedID, err := store.OAuthQuotaCredential(ctx, "codex", "missing")
	if err != nil || token != "" || matchedID != "" {
		t.Fatalf("expected empty quota credential for missing account, got %q/%q/%v", token, matchedID, err)
	}
}

func TestOAuthQuotaCredentialExtractsTokenAndProject(t *testing.T) {
	ctx := context.Background()
	store := NewAuthStore(secrets.NewMemoryStore(), nil, nil)
	if _, err := store.Save(ctx, &coreauth.Auth{ID: "acct-2", Provider: "Antigravity", Label: "user@example.test", Metadata: map[string]any{"access_token": "tok-2", "project_id": "proj-9"}}); err != nil {
		t.Fatal(err)
	}
	token, extraJSON, label, matched, err := store.OAuthQuotaCredential(ctx, "antigravity", "")
	if err != nil {
		t.Fatal(err)
	}
	if token != "tok-2" || label != "user@example.test" || matched != "acct-2" {
		t.Fatalf("unexpected quota credential: %q %q %q", token, label, matched)
	}
	if extraJSON != `{"project_id":"proj-9"}` {
		t.Fatalf("unexpected extra json: %q", extraJSON)
	}
}

func TestAdoptLoginReassignsToExistingAccountAndCleansUp(t *testing.T) {
	ctx := context.Background()
	store := NewAuthStore(secrets.NewMemoryStore(), nil, nil)
	flow := NewLoginFlow(store)
	flow.manager = &stubSDKManager{record: &coreauth.Auth{ID: "temporary-id", Provider: "codex", Label: "new", Metadata: map[string]any{"access_token": "fresh-token"}}}
	view, cleanup, err := flow.Login(ctx, "codex", "stable-account", func(v string) string { return "••••" })
	if err != nil {
		t.Fatal(err)
	}
	if view.ID != "stable-account" || view.Mask != "••••" {
		t.Fatalf("unexpected adopted view: %+v", view)
	}
	views, _ := store.AccountViews(ctx)
	if len(views) != 1 || views[0].ID != "stable-account" {
		t.Fatalf("reassignment did not replace temporary record: %+v", views)
	}
	if err := cleanup(ctx); err != nil {
		t.Fatal(err)
	}
	views, _ = store.AccountViews(ctx)
	if len(views) != 0 {
		t.Fatalf("cleanup did not remove adopted record: %+v", views)
	}
}
