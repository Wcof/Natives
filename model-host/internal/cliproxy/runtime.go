package cliproxy

import (
	"context"
	"crypto/rand"
	"crypto/subtle"
	"encoding/hex"
	"errors"
	"fmt"
	"net"
	"net/http"
	"net/http/httputil"
	"net/url"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/netpolicy"
	"github.com/ldh/natives/model-host/internal/secrets"
	sdkauth "github.com/router-for-me/CLIProxyAPI/v7/sdk/auth"
	core "github.com/router-for-me/CLIProxyAPI/v7/sdk/cliproxy"
	coreauth "github.com/router-for-me/CLIProxyAPI/v7/sdk/cliproxy/auth"
	clipconfig "github.com/router-for-me/CLIProxyAPI/v7/sdk/config"
	"gopkg.in/yaml.v3"
)

type Runtime struct {
	mu      sync.Mutex
	service *core.Service
	cancel  context.CancelFunc
	done    chan error
	port    int
	front   *http.Server
	manager *coreauth.Manager
}

func (r *Runtime) Start(ctx context.Context, snapshot domain.Snapshot, secretStore secrets.Store, authStore *AuthStore, gatewayKey, configPath string) (int, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.service != nil {
		return r.port, nil
	}
	listener, port, err := listenLoopback(snapshot.Gateway.PreferredPort)
	if err != nil {
		return 0, err
	}
	internalPort, err := choosePort(0)
	if err != nil {
		_ = listener.Close()
		return 0, err
	}
	internalKey, err := ephemeralKey()
	if err != nil {
		_ = listener.Close()
		return 0, err
	}
	cfg, err := runtimeConfig(snapshot, secretStore, internalKey, internalPort, filepath.Join(filepath.Dir(configPath), "auth"))
	if err != nil {
		_ = listener.Close()
		return 0, err
	}
	if err = writePublicRuntimeConfig(configPath, port); err != nil {
		_ = listener.Close()
		return 0, err
	}
	manager := coreauth.NewManager(authStore, nil, nil)
	authManager := sdkauth.NewManager(authStore,
		sdkauth.NewCodexAuthenticator(), sdkauth.NewClaudeAuthenticator(),
		sdkauth.NewAntigravityAuthenticator(), sdkauth.NewKimiAuthenticator(), sdkauth.NewXAIAuthenticator(),
	)
	sdkauth.RegisterTokenStore(authStore)
	service, err := core.NewBuilder().
		WithConfig(cfg).
		WithConfigPath(configPath).
		WithCoreAuthManager(manager).
		WithAuthManager(authManager).
		WithWatcherFactory(func(string, string, func(*clipconfig.Config)) (*core.WatcherWrapper, error) {
			return &core.WatcherWrapper{}, nil
		}).
		Build()
	if err != nil {
		_ = listener.Close()
		return 0, fmt.Errorf("build model gateway: %w", err)
	}
	manager.SetRoundTripperProvider(fixedTransportProvider{transport: netpolicy.NewTransport(runtimePrivateHosts(snapshot))})
	runCtx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	r.service, r.cancel, r.done, r.port, r.manager = service, cancel, done, port, manager
	go func() { done <- service.Run(runCtx) }()
	if err = waitReady(ctx, internalPort, internalKey, done); err != nil {
		_ = listener.Close()
		cancel()
		stopCtx, stopCancel := context.WithTimeout(context.Background(), 3*time.Second)
		_ = service.Shutdown(stopCtx)
		stopCancel()
		r.service, r.cancel, r.done, r.manager, r.port = nil, nil, nil, nil, 0
		return 0, err
	}
	target, _ := url.Parse("http://127.0.0.1:" + strconv.Itoa(internalPort))
	proxy := httputil.NewSingleHostReverseProxy(target)
	direct := proxy.Director
	proxy.Director = func(request *http.Request) {
		direct(request)
		request.Header.Del("x-api-key")
		request.Header.Del("x-goog-api-key")
		request.Header.Set("Authorization", "Bearer "+internalKey)
	}
	front := &http.Server{Handler: gatewayHandler(gatewayKey, proxy), ReadHeaderTimeout: 5 * time.Second, IdleTimeout: 60 * time.Second, MaxHeaderBytes: 64 << 10}
	r.front = front
	go func() { _ = front.Serve(listener) }()
	return port, nil
}

func (r *Runtime) Stop(ctx context.Context) error {
	r.mu.Lock()
	service, cancel, done, front := r.service, r.cancel, r.done, r.front
	if service == nil {
		r.mu.Unlock()
		return nil
	}
	r.service, r.cancel, r.done, r.front, r.manager, r.port = nil, nil, nil, nil, nil, 0
	r.mu.Unlock()
	if front != nil {
		_ = front.Shutdown(ctx)
	}
	cancel()
	err := service.Shutdown(ctx)
	select {
	case runErr := <-done:
		if err == nil && runErr != nil && !errors.Is(runErr, context.Canceled) {
			err = runErr
		}
	case <-ctx.Done():
		if err == nil {
			err = ctx.Err()
		}
	}
	return err
}

func (r *Runtime) SyncAuth(ctx context.Context, auth *coreauth.Auth) error {
	r.mu.Lock()
	manager := r.manager
	r.mu.Unlock()
	if manager == nil || auth == nil {
		return nil
	}
	_, err := manager.Register(ctx, auth)
	return err
}

func (r *Runtime) RemoveAuth(ctx context.Context, id string) {
	r.mu.Lock()
	manager := r.manager
	r.mu.Unlock()
	if manager != nil {
		manager.Remove(ctx, id)
	}
}

func runtimeConfig(snapshot domain.Snapshot, secretStore secrets.Store, gatewayKey string, port int, authDir string) (*clipconfig.Config, error) {
	configMap := map[string]any{
		"host": "127.0.0.1", "port": port, "api-keys": []string{gatewayKey},
		"auth-dir": authDir, "debug": false, "logging-to-file": false,
		"request-log": false, "usage-statistics-enabled": false,
	}
	var openAI, claude, gemini []map[string]any
	for _, provider := range snapshot.Providers {
		if provider.Kind != "custom" || !provider.Enabled {
			continue
		}
		key, err := secretStore.Get(provider.SecretRef)
		if errors.Is(err, secrets.ErrNotFound) {
			continue
		}
		if err != nil {
			return nil, err
		}
		models := runtimeModels(provider.Models)
		switch provider.Protocol {
		case domain.ProtocolAnthropic:
			claude = append(claude, map[string]any{"api-key": key, "base-url": provider.BaseURL, "models": models})
		case domain.ProtocolGemini:
			gemini = append(gemini, map[string]any{"api-key": key, "base-url": provider.BaseURL, "models": models})
		default:
			openAI = append(openAI, map[string]any{"name": provider.ID, "base-url": provider.BaseURL, "api-key-entries": []map[string]any{{"api-key": key}}, "models": models})
		}
	}
	if len(openAI) > 0 {
		configMap["openai-compatibility"] = openAI
	}
	if len(claude) > 0 {
		configMap["claude-api-key"] = claude
	}
	if len(gemini) > 0 {
		configMap["gemini-api-key"] = gemini
	}
	raw, err := yaml.Marshal(configMap)
	if err != nil {
		return nil, errors.New("build in-memory model configuration failed")
	}
	cfg, err := clipconfig.ParseConfigBytes(raw)
	for index := range raw {
		raw[index] = 0
	}
	if err != nil {
		return nil, errors.New("in-memory model configuration is invalid")
	}
	return cfg, nil
}

func runtimeModels(models []domain.Model) []map[string]any {
	result := make([]map[string]any, 0, len(models))
	for _, model := range models {
		if !model.Enabled {
			continue
		}
		alias := strings.TrimSpace(model.Alias)
		if alias == "" {
			alias = model.ID
		}
		result = append(result, map[string]any{"name": model.ID, "alias": alias, "display-name": model.DisplayName, "max-context-length": model.ContextLength})
	}
	return result
}

func choosePort(preferred int) (int, error) {
	for _, candidate := range []int{preferred, 0} {
		if candidate < 0 || candidate > 65535 {
			continue
		}
		listener, err := net.Listen("tcp", net.JoinHostPort("127.0.0.1", strconv.Itoa(candidate)))
		if err != nil {
			continue
		}
		port := listener.Addr().(*net.TCPAddr).Port
		_ = listener.Close()
		return port, nil
	}
	return 0, errors.New("no loopback port is available")
}

func listenLoopback(preferred int) (net.Listener, int, error) {
	for _, candidate := range []int{preferred, 0} {
		if candidate < 0 || candidate > 65535 {
			continue
		}
		listener, err := net.Listen("tcp", net.JoinHostPort("127.0.0.1", strconv.Itoa(candidate)))
		if err == nil {
			return listener, listener.Addr().(*net.TCPAddr).Port, nil
		}
	}
	return nil, 0, errors.New("no loopback port is available")
}

func gatewayHandler(key string, proxy http.Handler) http.Handler {
	return http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if !validGatewayKey(request, key) {
			writer.WriteHeader(http.StatusUnauthorized)
			return
		}
		if !allowedGatewayPath(request.URL.Path) {
			http.NotFound(writer, request)
			return
		}
		request.Body = http.MaxBytesReader(writer, request.Body, 32<<20)
		proxy.ServeHTTP(writer, request)
	})
}

func validGatewayKey(request *http.Request, key string) bool {
	provided := strings.TrimPrefix(request.Header.Get("Authorization"), "Bearer ")
	if provided == request.Header.Get("Authorization") {
		provided = request.Header.Get("x-api-key")
		if provided == "" {
			provided = request.Header.Get("x-goog-api-key")
		}
	}
	return subtle.ConstantTimeCompare([]byte(provided), []byte(key)) == 1
}

func allowedGatewayPath(path string) bool {
	return path == "/v1/models" || path == "/v1/chat/completions" || path == "/v1/responses" || path == "/v1/messages" || path == "/v1/messages/count_tokens" || path == "/v1beta/models" || strings.HasPrefix(path, "/v1beta/models/")
}

func ephemeralKey() (string, error) {
	value := make([]byte, 32)
	if _, err := rand.Read(value); err != nil {
		return "", err
	}
	return hex.EncodeToString(value), nil
}

type fixedTransportProvider struct{ transport http.RoundTripper }

func (p fixedTransportProvider) RoundTripperFor(*coreauth.Auth) http.RoundTripper { return p.transport }

func runtimePrivateHosts(snapshot domain.Snapshot) map[string]bool {
	hosts := make(map[string]bool)
	for _, provider := range snapshot.Providers {
		if provider.Kind != "custom" {
			continue
		}
		parsed, err := url.Parse(provider.BaseURL)
		if err != nil {
			continue
		}
		host := strings.ToLower(parsed.Hostname())
		allow := provider.AllowLAN || host == "localhost"
		if ip := net.ParseIP(host); ip != nil && ip.IsLoopback() {
			allow = true
		}
		if allow {
			hosts[host] = true
		}
	}
	return hosts
}

func waitReady(ctx context.Context, port int, key string, done <-chan error) error {
	client := &http.Client{Timeout: 500 * time.Millisecond}
	deadline := time.NewTimer(8 * time.Second)
	defer deadline.Stop()
	ticker := time.NewTicker(100 * time.Millisecond)
	defer ticker.Stop()
	endpoint := "http://127.0.0.1:" + strconv.Itoa(port) + "/v1/models"
	for {
		select {
		case err := <-done:
			if err == nil {
				return errors.New("model gateway stopped during startup")
			}
			return fmt.Errorf("model gateway startup failed: %w", err)
		case <-ctx.Done():
			return ctx.Err()
		case <-deadline.C:
			return errors.New("model gateway readiness timed out")
		case <-ticker.C:
			req, _ := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
			req.Header.Set("Authorization", "Bearer "+key)
			response, err := client.Do(req)
			if err == nil {
				_ = response.Body.Close()
				if response.StatusCode >= 200 && response.StatusCode < 500 {
					return nil
				}
			}
		}
	}
}

func writePublicRuntimeConfig(path string, port int) error {
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return err
	}
	contents := []byte("host: 127.0.0.1\nport: " + strconv.Itoa(port) + "\n")
	file, err := os.OpenFile(path, os.O_CREATE|os.O_TRUNC|os.O_WRONLY, 0o600)
	if err != nil {
		return err
	}
	if _, err = file.Write(contents); err == nil {
		err = file.Sync()
	}
	closeErr := file.Close()
	if err == nil {
		err = closeErr
	}
	return err
}
