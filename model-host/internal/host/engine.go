package host

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"sync"
	"time"

	clipadapter "github.com/ldh/natives/model-host/internal/cliproxy"
	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/nativeio"
	"github.com/ldh/natives/model-host/internal/secrets"
	sdkauth "github.com/router-for-me/CLIProxyAPI/v7/sdk/auth"
	clipcore "github.com/router-for-me/CLIProxyAPI/v7/sdk/cliproxy"
	coreauth "github.com/router-for-me/CLIProxyAPI/v7/sdk/cliproxy/auth"
	clipconfig "github.com/router-for-me/CLIProxyAPI/v7/sdk/config"
)

type loginManager interface {
	Login(context.Context, string, *clipconfig.Config, *sdkauth.LoginOptions) (*coreauth.Auth, string, error)
}

type Engine struct {
	repo              *domain.Repository
	secrets           secrets.Store
	authStore         *clipadapter.AuthStore
	auth              loginManager
	config            *clipconfig.Config
	runtime           clipadapter.Runtime
	runtimeConfigPath string
	emit              func(nativeio.Response)
	mu                sync.Mutex
	gatewayMu         sync.Mutex
	secretMu          sync.Mutex
	sessions          map[string]context.CancelFunc
	oauthTimeout      time.Duration
}

func NewEngine(repo *domain.Repository, secretStore secrets.Store, emit func(nativeio.Response)) (*Engine, error) {
	snapshot, err := repo.Load()
	if err != nil {
		return nil, err
	}
	ids := make([]string, 0, len(snapshot.Accounts))
	for _, account := range snapshot.Accounts {
		ids = append(ids, account.ID)
	}
	authStore := clipadapter.NewAuthStore(secretStore, ids, nil)
	authStore.SetOnSave(func(auth *coreauth.Auth) error { return syncAccountFromCredential(repo, emit, auth) })
	authManager := sdkauth.NewManager(authStore,
		sdkauth.NewCodexAuthenticator(),
		sdkauth.NewClaudeAuthenticator(),
		sdkauth.NewAntigravityAuthenticator(),
		sdkauth.NewKimiAuthenticator(),
		sdkauth.NewXAIAuthenticator(),
	)
	runtimeConfigPath, err := domain.DefaultRuntimeConfigPath()
	if err != nil {
		return nil, err
	}
	engine := &Engine{
		repo: repo, secrets: secretStore, authStore: authStore, auth: authManager,
		config: &clipconfig.Config{Host: "127.0.0.1"}, emit: emit,
		sessions: make(map[string]context.CancelFunc), runtimeConfigPath: runtimeConfigPath, oauthTimeout: 6 * time.Minute,
	}
	clipcore.SetGlobalModelRegistryHook(oauthCatalogHook{engine: engine})
	return engine, nil
}

func (e *Engine) Handle(ctx context.Context, request nativeio.Request) nativeio.Response {
	result, err := e.dispatch(ctx, request.Method, request.Params)
	if err == nil {
		return nativeio.Response{ID: request.ID, OK: true, Result: result}
	}
	code, message := classify(err)
	return nativeio.Response{ID: request.ID, OK: false, Error: message, ErrorCode: code}
}

func (e *Engine) Close() {
	clipcore.SetGlobalModelRegistryHook(nil)
	e.mu.Lock()
	for id, cancel := range e.sessions {
		cancel()
		delete(e.sessions, id)
	}
	e.mu.Unlock()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	e.gatewayMu.Lock()
	defer e.gatewayMu.Unlock()
	_ = e.runtime.Stop(ctx)
	snapshot, err := e.repo.Load()
	if err == nil && !snapshot.Gateway.Resident && snapshot.Gateway.State != "stopped" {
		_, _ = e.repo.Update(nil, resetGateway)
	}
}

func (e *Engine) dispatch(ctx context.Context, method string, raw json.RawMessage) (any, error) {
	switch method {
	case "model_snapshot", "model_gateway_status":
		return e.repo.Load()
	case "model_provider_create":
		return e.reconfigureGateway(e.createProvider(raw))
	case "model_provider_update":
		return e.reconfigureGateway(e.updateProvider(raw))
	case "model_provider_delete":
		return e.reconfigureGateway(e.deleteProvider(raw))
	case "model_provider_set_enabled":
		return e.reconfigureGateway(e.setProviderEnabled(raw))
	case "model_provider_test":
		return e.testProvider(ctx, raw)
	case "model_models_upsert":
		return e.reconfigureGateway(e.upsertModel(raw))
	case "model_models_delete":
		return e.reconfigureGateway(e.deleteModel(raw))
	case "model_models_set_enabled":
		return e.reconfigureGateway(e.setModelEnabled(raw))
	case "model_models_refresh":
		return e.reconfigureGateway(e.refreshModels(ctx, raw))
	case "model_gateway_set_resident":
		return e.setResident(raw)
	case "model_gateway_rotate_access_key":
		return e.rotateGatewayKey(raw)
	case "model_gateway_reveal_access_key":
		return e.revealGatewayKey()
	case "model_gateway_start":
		return e.startGateway(ctx, raw)
	case "model_gateway_stop":
		return e.stopGateway(ctx, raw)
	case "model_oauth_start", "model_account_reauth":
		return e.startOAuth(ctx, raw)
	case "model_oauth_cancel":
		return e.cancelOAuth(raw)
	case "model_account_set_enabled":
		return e.setAccountEnabled(raw)
	case "model_account_delete":
		return e.deleteAccount(raw)
	default:
		return nil, invalid("不支持的模型设置操作")
	}
}

func expected(raw json.RawMessage) (*int64, error) {
	var value struct {
		ExpectedRevision *int64 `json:"expectedRevision"`
	}
	if len(raw) > 0 && string(raw) != "null" {
		if err := json.Unmarshal(raw, &value); err != nil {
			return nil, invalid("请求参数无效")
		}
	}
	if value.ExpectedRevision == nil {
		return nil, invalid("写操作缺少配置版本")
	}
	return value.ExpectedRevision, nil
}

func requireRevision(value *int64) error {
	if value == nil {
		return invalid("写操作缺少配置版本")
	}
	return nil
}

func randomID(prefix string) (string, error) {
	bytes := make([]byte, 16)
	if _, err := rand.Read(bytes); err != nil {
		return "", fmt.Errorf("generate id: %w", err)
	}
	return prefix + hex.EncodeToString(bytes), nil
}

func randomSecret() (string, error) {
	bytes := make([]byte, 32)
	if _, err := rand.Read(bytes); err != nil {
		return "", err
	}
	return "natives_" + hex.EncodeToString(bytes), nil
}

func maskSecret(value string) string {
	value = strings.TrimSpace(value)
	if len(value) <= 4 {
		return "••••"
	}
	return "••••" + value[len(value)-4:]
}

func findProvider(snapshot *domain.Snapshot, id string) (*domain.Provider, error) {
	for index := range snapshot.Providers {
		if snapshot.Providers[index].ID == id {
			return &snapshot.Providers[index], nil
		}
	}
	return nil, notFound()
}

func findAccount(snapshot *domain.Snapshot, id string) (*domain.Account, error) {
	for index := range snapshot.Accounts {
		if snapshot.Accounts[index].ID == id {
			return &snapshot.Accounts[index], nil
		}
	}
	return nil, notFound()
}

func (e *Engine) emitSnapshot(event string, snapshot domain.Snapshot) {
	if e.emit != nil {
		e.emit(nativeio.Response{OK: true, Event: event, Result: map[string]any{"snapshot": snapshot}})
	}
}

var errSecretMissing = errors.New("secret is not configured")
