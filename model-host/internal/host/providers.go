package host

import (
	"encoding/json"
	"errors"
	"net"
	"net/url"
	"slices"
	"strings"
	"time"

	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/secrets"
)

const (
	maxCustomProviders   = 100
	maxModelsPerProvider = 5000
)

type providerInput struct {
	ExpectedRevision *int64          `json:"expectedRevision"`
	ProviderID       string          `json:"providerId"`
	Name             string          `json:"name"`
	BaseURL          string          `json:"baseUrl"`
	Protocol         domain.Protocol `json:"protocol"`
	APIKey           string          `json:"apiKey"`
	Enabled          *bool           `json:"enabled"`
	AllowLAN         bool            `json:"allowLan"`
}

func parseProvider(raw json.RawMessage) (providerInput, error) {
	var input providerInput
	if err := json.Unmarshal(raw, &input); err != nil {
		return input, invalid("供应商参数无效")
	}
	input.ProviderID = strings.TrimSpace(input.ProviderID)
	input.Name = strings.TrimSpace(input.Name)
	input.BaseURL = strings.TrimRight(strings.TrimSpace(input.BaseURL), "/")
	if len(input.BaseURL) > 2048 || len(input.APIKey) > 65536 {
		return input, invalid("供应商地址或 API Key 超过大小限制")
	}
	return input, nil
}

func (e *Engine) createProvider(raw json.RawMessage) (domain.Snapshot, error) {
	e.secretMu.Lock()
	defer e.secretMu.Unlock()
	input, err := parseProvider(raw)
	if err != nil {
		return domain.Snapshot{}, err
	}
	if err = requireRevision(input.ExpectedRevision); err != nil {
		return domain.Snapshot{}, err
	}
	if input.Name == "" || len(input.Name) > 80 {
		return domain.Snapshot{}, invalid("供应商名称不能为空且不能超过 80 个字符")
	}
	if err = validateProtocol(input.Protocol); err != nil {
		return domain.Snapshot{}, err
	}
	if err = validateBaseURL(input.BaseURL, input.AllowLAN); err != nil {
		return domain.Snapshot{}, err
	}
	id, err := randomID("provider_")
	if err != nil {
		return domain.Snapshot{}, err
	}
	secretRef := "provider-api-key:" + id
	if strings.TrimSpace(input.APIKey) != "" {
		if err = e.secrets.Set(secretRef, strings.TrimSpace(input.APIKey)); err != nil {
			return domain.Snapshot{}, err
		}
	}
	enabled := true
	if input.Enabled != nil {
		enabled = *input.Enabled
	}
	snapshot, err := e.repo.Update(input.ExpectedRevision, func(snapshot *domain.Snapshot) error {
		customCount := 0
		for _, provider := range snapshot.Providers {
			if provider.Kind == "custom" {
				customCount++
			}
		}
		if customCount >= maxCustomProviders {
			return invalid("自定义供应商数量已达到上限")
		}
		snapshot.Providers = append(snapshot.Providers, domain.Provider{
			ID: id, Kind: "custom", Name: input.Name, BaseURL: input.BaseURL,
			Protocol: input.Protocol, Enabled: enabled, AllowLAN: input.AllowLAN,
			SecretRef: secretRef, CredentialMask: maskSecret(input.APIKey), Models: []domain.Model{},
			UpdatedAt: time.Now().UTC().Format(time.RFC3339Nano),
		})
		return nil
	})
	if err != nil && strings.TrimSpace(input.APIKey) != "" {
		_ = e.secrets.Delete(secretRef)
	}
	return snapshot, err
}

func (e *Engine) updateProvider(raw json.RawMessage) (domain.Snapshot, error) {
	e.secretMu.Lock()
	defer e.secretMu.Unlock()
	input, err := parseProvider(raw)
	if err != nil {
		return domain.Snapshot{}, err
	}
	if err = requireRevision(input.ExpectedRevision); err != nil {
		return domain.Snapshot{}, err
	}
	if input.ProviderID == "" || input.Name == "" || len(input.Name) > 80 {
		return domain.Snapshot{}, invalid("供应商 ID 和名称不能为空")
	}
	if err = validateProtocol(input.Protocol); err != nil {
		return domain.Snapshot{}, err
	}
	if err = validateBaseURL(input.BaseURL, input.AllowLAN); err != nil {
		return domain.Snapshot{}, err
	}
	current, err := e.repo.Load()
	if err != nil {
		return domain.Snapshot{}, err
	}
	if input.ExpectedRevision != nil && *input.ExpectedRevision != current.Revision {
		return domain.Snapshot{}, errors.New("revision_conflict")
	}
	provider, err := findProvider(&current, input.ProviderID)
	if err != nil {
		return domain.Snapshot{}, err
	}
	if provider.Kind != "custom" {
		return domain.Snapshot{}, invalid("OAuth 供应商不能按自定义供应商编辑")
	}
	newKey := strings.TrimSpace(input.APIKey)
	oldKey, oldKeyErr := e.secrets.Get(provider.SecretRef)
	if oldKeyErr != nil && oldKeyErr != secrets.ErrNotFound {
		return domain.Snapshot{}, oldKeyErr
	}
	if newKey != "" {
		if err = e.secrets.Set(provider.SecretRef, newKey); err != nil {
			return domain.Snapshot{}, err
		}
	}
	snapshot, err := e.repo.Update(input.ExpectedRevision, func(snapshot *domain.Snapshot) error {
		provider, findErr := findProvider(snapshot, input.ProviderID)
		if findErr != nil {
			return findErr
		}
		if provider.Kind != "custom" {
			return invalid("OAuth 供应商不能按自定义供应商编辑")
		}
		if newKey != "" {
			provider.CredentialMask = maskSecret(input.APIKey)
		}
		provider.Name, provider.BaseURL, provider.Protocol = input.Name, input.BaseURL, input.Protocol
		provider.AllowLAN = input.AllowLAN
		if input.Enabled != nil {
			provider.Enabled = *input.Enabled
		}
		provider.UpdatedAt = time.Now().UTC().Format(time.RFC3339Nano)
		return nil
	})
	if err != nil && newKey != "" {
		if oldKeyErr == nil {
			_ = e.secrets.Set(provider.SecretRef, oldKey)
		} else {
			_ = e.secrets.Delete(provider.SecretRef)
		}
	}
	return snapshot, err
}

func (e *Engine) deleteProvider(raw json.RawMessage) (domain.Snapshot, error) {
	e.secretMu.Lock()
	defer e.secretMu.Unlock()
	var input struct {
		ExpectedRevision *int64 `json:"expectedRevision"`
		ProviderID       string `json:"providerId"`
	}
	if json.Unmarshal(raw, &input) != nil || input.ProviderID == "" {
		return domain.Snapshot{}, invalid("供应商 ID 不能为空")
	}
	if err := requireRevision(input.ExpectedRevision); err != nil {
		return domain.Snapshot{}, err
	}
	current, err := e.repo.Load()
	if err != nil {
		return domain.Snapshot{}, err
	}
	if input.ExpectedRevision != nil && *input.ExpectedRevision != current.Revision {
		return domain.Snapshot{}, errors.New("revision_conflict")
	}
	provider, err := findProvider(&current, input.ProviderID)
	if err != nil {
		return domain.Snapshot{}, err
	}
	if provider.Kind != "custom" {
		return domain.Snapshot{}, invalid("内置 OAuth 供应商不能删除")
	}
	secretRef := provider.SecretRef
	oldKey, oldKeyErr := e.secrets.Get(secretRef)
	if oldKeyErr != nil && oldKeyErr != secrets.ErrNotFound {
		return domain.Snapshot{}, oldKeyErr
	}
	if err = e.secrets.Delete(secretRef); err != nil {
		return domain.Snapshot{}, err
	}
	snapshot, err := e.repo.Update(input.ExpectedRevision, func(snapshot *domain.Snapshot) error {
		provider, findErr := findProvider(snapshot, input.ProviderID)
		if findErr != nil {
			return findErr
		}
		if provider.Kind != "custom" {
			return invalid("内置 OAuth 供应商不能删除")
		}
		snapshot.Providers = slices.DeleteFunc(snapshot.Providers, func(value domain.Provider) bool { return value.ID == input.ProviderID })
		return nil
	})
	if err != nil && oldKeyErr == nil {
		_ = e.secrets.Set(secretRef, oldKey)
	}
	return snapshot, err
}

func (e *Engine) setProviderEnabled(raw json.RawMessage) (domain.Snapshot, error) {
	var input struct {
		ExpectedRevision *int64 `json:"expectedRevision"`
		ProviderID       string `json:"providerId"`
		Enabled          *bool  `json:"enabled"`
	}
	if json.Unmarshal(raw, &input) != nil || input.ProviderID == "" || input.Enabled == nil {
		return domain.Snapshot{}, invalid("供应商状态参数无效")
	}
	if err := requireRevision(input.ExpectedRevision); err != nil {
		return domain.Snapshot{}, err
	}
	return e.repo.Update(input.ExpectedRevision, func(snapshot *domain.Snapshot) error {
		provider, err := findProvider(snapshot, input.ProviderID)
		if err == nil {
			provider.Enabled = *input.Enabled
		}
		return err
	})
}

func (e *Engine) upsertModel(raw json.RawMessage) (domain.Snapshot, error) {
	var input struct {
		ExpectedRevision *int64       `json:"expectedRevision"`
		ProviderID       string       `json:"providerId"`
		Model            domain.Model `json:"model"`
	}
	if json.Unmarshal(raw, &input) != nil || input.ProviderID == "" || strings.TrimSpace(input.Model.ID) == "" {
		return domain.Snapshot{}, invalid("模型参数无效")
	}
	if err := requireRevision(input.ExpectedRevision); err != nil {
		return domain.Snapshot{}, err
	}
	input.Model.ID = strings.TrimSpace(input.Model.ID)
	input.Model.DisplayName = strings.TrimSpace(input.Model.DisplayName)
	input.Model.Alias = strings.TrimSpace(input.Model.Alias)
	if len(input.Model.ID) > 200 || len(input.Model.DisplayName) > 200 || len(input.Model.Alias) > 200 || input.Model.ContextLength < 0 || input.Model.ContextLength > 100_000_000 {
		return domain.Snapshot{}, invalid("模型字段超过允许范围")
	}
	if input.Model.DisplayName == "" {
		input.Model.DisplayName = input.Model.ID
	}
	input.Model.Manual = true
	return e.repo.Update(input.ExpectedRevision, func(snapshot *domain.Snapshot) error {
		provider, err := findProvider(snapshot, input.ProviderID)
		if err != nil {
			return err
		}
		for index := range provider.Models {
			if provider.Models[index].ID == input.Model.ID {
				provider.Models[index] = input.Model
				return nil
			}
		}
		if len(provider.Models) >= maxModelsPerProvider {
			return invalid("模型数量已达到上限")
		}
		provider.Models = append(provider.Models, input.Model)
		return nil
	})
}

func (e *Engine) deleteModel(raw json.RawMessage) (domain.Snapshot, error) {
	var input struct {
		ExpectedRevision *int64 `json:"expectedRevision"`
		ProviderID       string `json:"providerId"`
		ModelID          string `json:"modelId"`
	}
	if json.Unmarshal(raw, &input) != nil || input.ProviderID == "" || input.ModelID == "" {
		return domain.Snapshot{}, invalid("模型参数无效")
	}
	if err := requireRevision(input.ExpectedRevision); err != nil {
		return domain.Snapshot{}, err
	}
	return e.repo.Update(input.ExpectedRevision, func(snapshot *domain.Snapshot) error {
		provider, err := findProvider(snapshot, input.ProviderID)
		if err != nil {
			return err
		}
		provider.Models = slices.DeleteFunc(provider.Models, func(model domain.Model) bool { return model.ID == input.ModelID })
		return nil
	})
}

func (e *Engine) setModelEnabled(raw json.RawMessage) (domain.Snapshot, error) {
	var input struct {
		ExpectedRevision *int64 `json:"expectedRevision"`
		ProviderID       string `json:"providerId"`
		ModelID          string `json:"modelId"`
		Enabled          *bool  `json:"enabled"`
	}
	if json.Unmarshal(raw, &input) != nil || input.ProviderID == "" || input.ModelID == "" || input.Enabled == nil {
		return domain.Snapshot{}, invalid("模型状态参数无效")
	}
	if err := requireRevision(input.ExpectedRevision); err != nil {
		return domain.Snapshot{}, err
	}
	return e.repo.Update(input.ExpectedRevision, func(snapshot *domain.Snapshot) error {
		provider, err := findProvider(snapshot, input.ProviderID)
		if err != nil {
			return err
		}
		for index := range provider.Models {
			if provider.Models[index].ID == input.ModelID {
				provider.Models[index].Enabled = *input.Enabled
				return nil
			}
		}
		return notFound()
	})
}

func validateProtocol(protocol domain.Protocol) error {
	if !slices.Contains([]domain.Protocol{domain.ProtocolOpenAIChat, domain.ProtocolOpenAIResponses, domain.ProtocolAnthropic, domain.ProtocolGemini}, protocol) {
		return invalid("不支持的 API 格式")
	}
	return nil
}

func validateBaseURL(raw string, allowLAN bool) error {
	parsed, err := url.Parse(raw)
	if err != nil || parsed.Hostname() == "" || parsed.User != nil || parsed.RawQuery != "" || parsed.Fragment != "" {
		return invalid("Base URL 无效")
	}
	host := strings.ToLower(parsed.Hostname())
	loopback := host == "localhost"
	private := false
	if ip := net.ParseIP(host); ip != nil {
		loopback = ip.IsLoopback()
		private = ip.IsPrivate()
		if ip.IsUnspecified() || ip.IsMulticast() || ip.IsLinkLocalUnicast() || ip.IsLinkLocalMulticast() {
			return invalid("Base URL 地址不安全")
		}
		if !loopback && private && !allowLAN {
			return invalid("局域网地址需要显式授权")
		}
	}
	httpAllowed := loopback || (allowLAN && (private || strings.HasSuffix(host, ".local")))
	if parsed.Scheme != "https" && !(parsed.Scheme == "http" && httpAllowed) {
		return invalid("Base URL 必须使用 HTTPS；本机或已授权局域网地址可使用 HTTP")
	}
	return nil
}
