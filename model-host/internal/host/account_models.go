package host

import (
	"context"
	"encoding/json"
	"errors"
	"slices"
	"sort"
	"strings"

	clipadapter "github.com/ldh/natives/model-host/internal/cliproxy"
	"github.com/ldh/natives/model-host/internal/domain"
)

type accountModel struct {
	ID          string `json:"id"`
	DisplayName string `json:"displayName,omitempty"`
	Enabled     bool   `json:"enabled"`
}

func (e *Engine) accountModels(raw json.RawMessage) (map[string]any, error) {
	var input struct {
		AccountID string `json:"accountId"`
		Name      string `json:"name"`
		Provider  string `json:"provider"`
	}
	if json.Unmarshal(raw, &input) != nil || (strings.TrimSpace(input.AccountID) == "" && strings.TrimSpace(input.Name) == "") {
		return nil, invalid("账户 ID 或文件名不能为空")
	}
	snapshot, err := e.repo.Load()
	if err != nil {
		return nil, err
	}
	var account *domain.Account
	if strings.TrimSpace(input.AccountID) != "" {
		account, _ = findAccount(&snapshot, input.AccountID)
	}
	if account != nil {
		view, err := e.accountView(context.Background(), account.ID)
		if err != nil {
			return nil, err
		}
		rules := view.ExcludedModels
		models := make(map[string]accountModel)
		for _, model := range clipadapter.StaticModelDefinitions(account.Provider) {
			if strings.TrimSpace(model.ID) != "" {
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

	targetName := strings.TrimSpace(input.Name)
	if targetName == "" {
		targetName = strings.TrimSpace(input.AccountID)
	}
	provider := strings.TrimSpace(input.Provider)
	var rules []string
	var matchedAccountID string

	if e.authFiles != nil {
		items, err := e.authFiles.List()
		if err == nil {
			for _, item := range items {
				if item.Name == targetName || item.Name == targetName+".json" || item.AccountID == targetName {
					targetName = item.Name
					if provider == "" {
						provider = item.Provider
					}
					rules = item.ExcludedModels
					matchedAccountID = item.AccountID
					break
				}
			}
		}
	}
	if provider == "" {
		provider = normalizeAuthProvider(targetName)
	}
	models := make(map[string]accountModel)
	for _, model := range clipadapter.StaticModelDefinitions(provider) {
		if strings.TrimSpace(model.ID) != "" {
			models[model.ID] = accountModel{ID: model.ID, DisplayName: model.DisplayName, Enabled: !matchesAnyRule(model.ID, rules)}
		}
	}
	providerConfig, _ := findProvider(&snapshot, provider)
	if providerConfig != nil {
		for _, model := range providerConfig.Models {
			models[model.ID] = accountModel{ID: model.ID, DisplayName: model.DisplayName, Enabled: !matchesAnyRule(model.ID, rules)}
		}
	}
	result := make([]accountModel, 0, len(models))
	for _, model := range models {
		result = append(result, model)
	}
	sort.Slice(result, func(i, j int) bool { return result[i].ID < result[j].ID })
	resp := map[string]any{
		"name":     targetName,
		"provider": provider,
		"models":   result,
	}
	if matchedAccountID != "" {
		resp["accountId"] = matchedAccountID
	}
	return resp, nil
}

func (e *Engine) updateAccountModels(raw json.RawMessage) (map[string]any, error) {
	var input struct {
		ExpectedRevision *int64   `json:"expectedRevision"`
		AccountID        string   `json:"accountId"`
		Name             string   `json:"name"`
		Provider         string   `json:"provider"`
		EnabledModelIDs  []string `json:"enabledModelIds"`
	}
	if json.Unmarshal(raw, &input) != nil || (strings.TrimSpace(input.AccountID) == "" && strings.TrimSpace(input.Name) == "") {
		return nil, invalid("账户模型参数无效")
	}
	snapshot, err := e.repo.Load()
	if err != nil {
		return nil, err
	}
	var account *domain.Account
	if strings.TrimSpace(input.AccountID) != "" {
		account, _ = findAccount(&snapshot, input.AccountID)
	}
	if account != nil {
		if err := requireRevision(input.ExpectedRevision); err != nil {
			return nil, err
		}
		if snapshot.Revision != *input.ExpectedRevision {
			return nil, errors.New("revision_conflict")
		}
		view, err := e.accountView(context.Background(), account.ID)
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
		rules := exclusionsForEnabled(view.ExcludedModels, allModels, input.EnabledModelIDs)
		// mode/T07：克隆/元数据变更/持久化/补偿在适配层（SwapAccountExcludedModels）。
		rollback, swapErr := e.authStore.SwapAccountExcludedModels(context.Background(), account.ID, rules)
		if swapErr == nil {
			if syncErr := e.runtime.SyncAccount(context.Background(), e.authStore, account.ID); syncErr != nil {
				_ = rollback(context.Background())
				swapErr = syncErr
			}
		}
		if swapErr != nil {
			if rollback != nil {
				_ = rollback(context.Background())
				_ = e.runtime.SyncAccount(context.Background(), e.authStore, account.ID)
			}
			return nil, swapErr
		}
		return e.accountModels(raw)
	}

	if e.authFiles == nil {
		return nil, errors.New("auth files manager not initialized")
	}
	targetName := strings.TrimSpace(input.Name)
	if targetName == "" {
		targetName = strings.TrimSpace(input.AccountID)
	}
	definitionInput, err := json.Marshal(map[string]string{
		"name":     targetName,
		"provider": input.Provider,
	})
	if err != nil {
		return nil, err
	}
	definitions, err := e.accountModels(definitionInput)
	if err != nil {
		return nil, err
	}
	allModels, _ := definitions["models"].([]accountModel)
	var currentRules []string
	items, err := e.authFiles.List()
	if err == nil {
		for _, item := range items {
			if item.Name == targetName || item.Name == targetName+".json" || item.AccountID == targetName {
				targetName = item.Name
				currentRules = item.ExcludedModels
				break
			}
		}
	}
	rules := exclusionsForEnabled(currentRules, allModels, input.EnabledModelIDs)
	if _, err := e.authFiles.Update(targetName, nil, nil, rules); err != nil {
		return nil, err
	}
	return e.accountModels(definitionInput)
}

// accountView 返回账户凭据的项目侧视图（mode/T07：不再取 SDK 对象）。
func (e *Engine) accountView(ctx context.Context, accountID string) (clipadapter.AccountView, error) {
	views, err := e.authStore.AccountViews(ctx)
	if err != nil {
		return clipadapter.AccountView{}, err
	}
	for _, view := range views {
		if view.ID == accountID {
			return view, nil
		}
	}
	return clipadapter.AccountView{}, &SafeError{Code: "secret_not_configured", Message: "账户凭证不存在，请重新授权"}
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
