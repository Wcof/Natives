package host

import (
	"context"
	"encoding/json"
	"errors"
	"slices"
	"sort"
	"strings"

	clipcore "github.com/router-for-me/CLIProxyAPI/v7/sdk/cliproxy"
	coreauth "github.com/router-for-me/CLIProxyAPI/v7/sdk/cliproxy/auth"
)

type accountModel struct {
	ID          string `json:"id"`
	DisplayName string `json:"displayName,omitempty"`
	Enabled     bool   `json:"enabled"`
}

func (e *Engine) accountModels(raw json.RawMessage) (map[string]any, error) {
	var input struct {
		AccountID string `json:"accountId"`
	}
	if json.Unmarshal(raw, &input) != nil || strings.TrimSpace(input.AccountID) == "" {
		return nil, invalid("账户 ID 不能为空")
	}
	snapshot, err := e.repo.Load()
	if err != nil {
		return nil, err
	}
	account, err := findAccount(&snapshot, input.AccountID)
	if err != nil {
		return nil, err
	}
	credential, err := e.accountCredential(context.Background(), account.ID)
	if err != nil {
		return nil, err
	}
	rules := credentialExcludedModels(credential)
	models := make(map[string]accountModel)
	for _, model := range clipcore.StaticModelDefinitions(account.Provider) {
		if model != nil && strings.TrimSpace(model.ID) != "" {
			models[model.ID] = accountModel{ID: model.ID, DisplayName: model.DisplayName, Enabled: !matchesAnyRule(model.ID, rules)}
		}
	}
	provider, _ := findProvider(&snapshot, account.Provider)
	if provider != nil {
		for _, model := range provider.Models {
			if slices.Contains(model.AccountIDs, account.ID) {
				models[model.ID] = accountModel{ID: model.ID, DisplayName: model.DisplayName, Enabled: !matchesAnyRule(model.ID, rules)}
			}
		}
	}
	result := make([]accountModel, 0, len(models))
	for _, model := range models {
		result = append(result, model)
	}
	sort.Slice(result, func(i, j int) bool { return result[i].ID < result[j].ID })
	return map[string]any{"accountId": account.ID, "provider": account.Provider, "models": result}, nil
}

func (e *Engine) updateAccountModels(raw json.RawMessage) (map[string]any, error) {
	var input struct {
		ExpectedRevision *int64   `json:"expectedRevision"`
		AccountID        string   `json:"accountId"`
		EnabledModelIDs  []string `json:"enabledModelIds"`
	}
	if json.Unmarshal(raw, &input) != nil || strings.TrimSpace(input.AccountID) == "" {
		return nil, invalid("账户模型参数无效")
	}
	if err := requireRevision(input.ExpectedRevision); err != nil {
		return nil, err
	}
	snapshot, err := e.repo.Load()
	if err != nil {
		return nil, err
	}
	if snapshot.Revision != *input.ExpectedRevision {
		return nil, errors.New("revision_conflict")
	}
	account, err := findAccount(&snapshot, input.AccountID)
	if err != nil {
		return nil, err
	}
	credential, err := e.accountCredential(context.Background(), account.ID)
	if err != nil {
		return nil, err
	}
	definitionInput, err := json.Marshal(map[string]string{"accountId": account.ID})
	if err != nil {
		return nil, err
	}
	definitions, err := e.accountModels(definitionInput)
	if err != nil {
		return nil, err
	}
	allModels := definitions["models"].([]accountModel)
	rules := exclusionsForEnabled(credentialExcludedModels(credential), allModels, input.EnabledModelIDs)
	credential = credential.Clone()
	previous := credential.Clone()
	if credential.Metadata == nil {
		credential.Metadata = make(map[string]any)
	}
	credential.Metadata["excluded_models"] = rules
	if credential.Attributes == nil {
		credential.Attributes = make(map[string]string)
	}
	if len(rules) == 0 {
		delete(credential.Attributes, "excluded_models")
	} else {
		credential.Attributes["excluded_models"] = strings.Join(rules, ",")
	}
	if _, err = e.authStore.Save(context.Background(), credential); err == nil {
		err = e.runtime.SyncAuth(context.Background(), credential)
	}
	if err != nil {
		_, _ = e.authStore.Save(context.Background(), previous)
		_ = e.runtime.SyncAuth(context.Background(), previous)
		return nil, err
	}
	return e.accountModels(raw)
}

func (e *Engine) accountCredential(ctx context.Context, accountID string) (*coreauth.Auth, error) {
	credentials, err := e.authStore.List(ctx)
	if err != nil {
		return nil, err
	}
	for _, credential := range credentials {
		if credential.ID == accountID {
			return credential, nil
		}
	}
	return nil, &SafeError{Code: "secret_not_configured", Message: "账户凭证不存在，请重新授权"}
}

func credentialExcludedModels(credential *coreauth.Auth) []string {
	if credential == nil {
		return nil
	}
	if value := credential.Attributes["excluded_models"]; strings.TrimSpace(value) != "" {
		return strings.Split(value, ",")
	}
	raw, _ := credential.Metadata["excluded_models"].([]any)
	result := make([]string, 0, len(raw))
	for _, value := range raw {
		if model, ok := value.(string); ok {
			result = append(result, model)
		}
	}
	if typed, ok := credential.Metadata["excluded_models"].([]string); ok {
		result = append(result, typed...)
	}
	return result
}

func exclusionsForEnabled(current []string, models []accountModel, enabledIDs []string) []string {
	enabled := make(map[string]bool, len(enabledIDs))
	for _, id := range enabledIDs {
		enabled[strings.ToLower(strings.TrimSpace(id))] = true
	}
	result := make([]string, 0)
	for _, rule := range current {
		rule = strings.ToLower(strings.TrimSpace(rule))
		if rule == "" {
			continue
		}
		keep := true
		for _, model := range models {
			if enabled[strings.ToLower(model.ID)] && matchWildcard(rule, model.ID) {
				keep = false
				break
			}
		}
		if keep {
			result = append(result, rule)
		}
	}
	for _, model := range models {
		if !enabled[strings.ToLower(model.ID)] {
			result = append(result, strings.ToLower(model.ID))
		}
	}
	sort.Strings(result)
	return slices.Compact(result)
}

func matchesAnyRule(model string, rules []string) bool {
	for _, rule := range rules {
		if matchWildcard(rule, model) {
			return true
		}
	}
	return false
}

func matchWildcard(pattern, value string) bool {
	pattern, value = strings.ToLower(strings.TrimSpace(pattern)), strings.ToLower(strings.TrimSpace(value))
	parts := strings.Split(pattern, "*")
	if len(parts) == 1 {
		return pattern == value
	}
	position := 0
	for index, part := range parts {
		if part == "" {
			continue
		}
		offset := strings.Index(value[position:], part)
		if offset < 0 || (index == 0 && offset != 0) {
			return false
		}
		position += offset + len(part)
	}
	return strings.HasSuffix(pattern, "*") || strings.HasSuffix(value, parts[len(parts)-1])
}
