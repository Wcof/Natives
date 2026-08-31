package host

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"slices"
	"strings"
	"time"

	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/netpolicy"
	"github.com/ldh/natives/model-host/internal/secrets"
)

const maxModelResponseBytes = 5 << 20

func (e *Engine) refreshModels(ctx context.Context, raw json.RawMessage) (domain.Snapshot, error) {
	var input struct {
		ExpectedRevision *int64 `json:"expectedRevision"`
		ProviderID       string `json:"providerId"`
	}
	if json.Unmarshal(raw, &input) != nil || strings.TrimSpace(input.ProviderID) == "" {
		return domain.Snapshot{}, invalid("供应商 ID 不能为空")
	}
	if err := requireRevision(input.ExpectedRevision); err != nil {
		return domain.Snapshot{}, err
	}
	snapshot, err := e.repo.Load()
	if err != nil {
		return domain.Snapshot{}, err
	}
	provider, err := findProvider(&snapshot, input.ProviderID)
	if err != nil {
		return domain.Snapshot{}, err
	}
	models, err := e.discoverModels(ctx, *provider, e.secrets)
	if err != nil {
		return domain.Snapshot{}, err
	}
	return e.repo.Update(input.ExpectedRevision, func(current *domain.Snapshot) error {
		target, findErr := findProvider(current, input.ProviderID)
		if findErr != nil {
			return findErr
		}
		for _, discovered := range models {
			index := slices.IndexFunc(target.Models, func(model domain.Model) bool { return model.ID == discovered.ID })
			if index < 0 {
				if len(target.Models) >= maxModelsPerProvider {
					break
				}
				target.Models = append(target.Models, discovered)
				continue
			}
			if !target.Models[index].Manual {
				discovered.Enabled = target.Models[index].Enabled
				target.Models[index] = discovered
			}
		}
		target.UpdatedAt = time.Now().UTC().Format(time.RFC3339Nano)
		return nil
	})
}

func (e *Engine) testProvider(ctx context.Context, raw json.RawMessage) (map[string]any, error) {
	var input struct {
		ProviderID string          `json:"providerId"`
		BaseURL    string          `json:"baseUrl"`
		Protocol   domain.Protocol `json:"protocol"`
		APIKey     string          `json:"apiKey"`
		AllowLAN   bool            `json:"allowLan"`
	}
	if json.Unmarshal(raw, &input) != nil {
		return nil, invalid("测试参数无效")
	}
	provider := domain.Provider{Kind: "custom"}
	if strings.TrimSpace(input.ProviderID) != "" {
		snapshot, err := e.repo.Load()
		if err != nil {
			return nil, err
		}
		stored, err := findProvider(&snapshot, input.ProviderID)
		if err != nil {
			return nil, err
		}
		provider = *stored
	}
	if strings.TrimSpace(input.BaseURL) != "" {
		provider.BaseURL = strings.TrimSpace(input.BaseURL)
	}
	if input.Protocol != "" {
		provider.Protocol = input.Protocol
	}
	provider.AllowLAN = input.AllowLAN
	if err := validateProtocol(provider.Protocol); err != nil {
		return nil, err
	}
	if err := validateBaseURL(provider.BaseURL, provider.AllowLAN); err != nil {
		return nil, err
	}
	secretStore := e.secrets
	input.APIKey = strings.TrimSpace(input.APIKey)
	if input.APIKey != "" {
		if len(input.APIKey) > 65536 {
			return nil, invalid("API Key 过长")
		}
		memory := secrets.NewMemoryStore()
		provider.SecretRef = "provider-test"
		if err := memory.Set(provider.SecretRef, input.APIKey); err != nil {
			return nil, err
		}
		secretStore = memory
	}
	models, err := e.discoverModels(ctx, provider, secretStore)
	if err != nil {
		return nil, err
	}
	return map[string]any{"connected": true, "modelCount": len(models)}, nil
}

func (e *Engine) discoverModels(parent context.Context, provider domain.Provider, secretStore secrets.Store) ([]domain.Model, error) {
	if provider.Kind != "custom" {
		return nil, invalid("OAuth 供应商的模型由登录账户管理")
	}
	key, err := secretStore.Get(provider.SecretRef)
	if errors.Is(err, secrets.ErrNotFound) {
		return nil, &SafeError{Code: "secret_not_configured", Message: "请先配置 API Key"}
	}
	if err != nil {
		return nil, err
	}
	ctx, cancel := context.WithTimeout(parent, 10*time.Second)
	defer cancel()
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, modelEndpoint(provider), nil)
	if err != nil {
		return nil, invalid("模型列表地址无效")
	}
	switch provider.Protocol {
	case domain.ProtocolGemini:
		request.Header.Set("x-goog-api-key", key)
	case domain.ProtocolAnthropic:
		request.Header.Set("x-api-key", key)
		request.Header.Set("anthropic-version", "2023-06-01")
	default:
		request.Header.Set("Authorization", "Bearer "+key)
	}
	parsedEndpoint := request.URL
	endpointHost := strings.ToLower(parsedEndpoint.Hostname())
	allowPrivate := provider.AllowLAN || endpointHost == "localhost"
	if ip := net.ParseIP(endpointHost); ip != nil && ip.IsLoopback() {
		allowPrivate = true
	}
	privateHosts := map[string]bool{endpointHost: allowPrivate}
	client := &http.Client{Timeout: 10 * time.Second, Transport: netpolicy.NewTransport(privateHosts), CheckRedirect: func(request *http.Request, _ []*http.Request) error {
		if !strings.EqualFold(request.URL.Scheme, parsedEndpoint.Scheme) || !strings.EqualFold(request.URL.Host, parsedEndpoint.Host) {
			return errors.New("cross-origin model discovery redirect")
		}
		return validateBaseURL(request.URL.Scheme+"://"+request.URL.Host, provider.AllowLAN)
	}}
	response, err := client.Do(request)
	if err != nil {
		return nil, &SafeError{Code: "upstream_unreachable", Message: "无法连接供应商，请检查地址和网络"}
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		code := "upstream_error"
		if response.StatusCode == http.StatusUnauthorized || response.StatusCode == http.StatusForbidden {
			code = "upstream_unauthorized"
		}
		return nil, &SafeError{Code: code, Message: "供应商拒绝了模型列表请求"}
	}
	limited := io.LimitReader(response.Body, maxModelResponseBytes+1)
	body, err := io.ReadAll(limited)
	if err != nil {
		return nil, &SafeError{Code: "upstream_invalid_response", Message: "读取模型列表失败"}
	}
	if len(body) > maxModelResponseBytes {
		return nil, &SafeError{Code: "upstream_response_too_large", Message: "模型列表响应超过大小限制"}
	}
	models, err := parseModelList(provider.Protocol, body)
	if err != nil {
		return nil, &SafeError{Code: "upstream_invalid_response", Message: "供应商返回了无效的模型列表"}
	}
	return models, nil
}

func modelEndpoint(provider domain.Provider) string {
	base := strings.TrimRight(provider.BaseURL, "/")
	if strings.HasSuffix(base, "/models") {
		return base
	}
	if provider.Protocol == domain.ProtocolGemini && !strings.HasSuffix(base, "/v1beta") {
		return base + "/v1beta/models"
	}
	return base + "/models"
}

func parseModelList(protocol domain.Protocol, body []byte) ([]domain.Model, error) {
	var payload struct {
		Data []struct {
			ID          string `json:"id"`
			DisplayName string `json:"display_name"`
		} `json:"data"`
		Models []struct {
			Name            string `json:"name"`
			DisplayName     string `json:"displayName"`
			InputTokenLimit int64  `json:"inputTokenLimit"`
		} `json:"models"`
	}
	if err := json.Unmarshal(body, &payload); err != nil {
		return nil, err
	}
	models := make([]domain.Model, 0, len(payload.Data)+len(payload.Models))
	seen := make(map[string]bool)
	add := func(id, display string, contextLength int64) {
		id = strings.TrimSpace(strings.TrimPrefix(id, "models/"))
		if id == "" || len(id) > 200 || seen[id] || len(models) >= 5000 {
			return
		}
		seen[id] = true
		if strings.TrimSpace(display) == "" {
			display = id
		}
		if runes := []rune(strings.TrimSpace(display)); len(runes) > 200 {
			display = string(runes[:200])
		}
		models = append(models, domain.Model{ID: id, DisplayName: strings.TrimSpace(display), ContextLength: contextLength, Enabled: true})
	}
	if protocol == domain.ProtocolGemini {
		for _, model := range payload.Models {
			add(model.Name, model.DisplayName, model.InputTokenLimit)
		}
	} else {
		for _, model := range payload.Data {
			add(model.ID, model.DisplayName, 0)
		}
	}
	if len(models) == 0 {
		return nil, fmt.Errorf("empty model list")
	}
	return models, nil
}
