package host

import (
	"context"
	"encoding/json"
	"errors"
	"strings"

	"github.com/ldh/natives/model-host/internal/authfiles"
	coreauth "github.com/router-for-me/CLIProxyAPI/v7/sdk/cliproxy/auth"
)

type importAuthFileParams struct {
	Name    string `json:"name"`
	Content string `json:"content"`
}

type updateAuthFileParams struct {
	Name           string   `json:"name"`
	Disabled       *bool    `json:"disabled"`
	Priority       *int     `json:"priority"`
	ExcludedModels []string `json:"excludedModels"`
}

type deleteAuthFileParams struct {
	Name string `json:"name"`
}

type quotaQueryParams struct {
	Provider  string `json:"provider"`
	Name      string `json:"name"`
	AccountID string `json:"accountId"`
}

func (e *Engine) listAuthFiles() (any, error) {
	items := make([]authfiles.AuthFileItem, 0)
	if e.authFiles != nil {
		files, err := e.authFiles.List()
		if err != nil {
			return nil, err
		}
		items = append(items, files...)
	}
	snapshot, err := e.repo.Load()
	if err != nil {
		return nil, err
	}
	for _, account := range snapshot.Accounts {
		items = append(items, authfiles.AuthFileItem{
			AccountID: account.ID, Source: "keychain", Name: account.Label,
			Provider: account.Provider, Account: account.Label, Status: account.Status,
			Disabled: !account.Enabled, UpdatedAt: account.UpdatedAt,
		})
	}
	return items, nil
}

func (e *Engine) importAuthFile(raw json.RawMessage) (any, error) {
	if e.authFiles == nil {
		return nil, errors.New("auth files manager not initialized")
	}
	var p importAuthFileParams
	if err := json.Unmarshal(raw, &p); err != nil || p.Name == "" || p.Content == "" {
		return nil, invalid("参数无效: name 和 content 不能为空")
	}
	return e.authFiles.Import(p.Name, []byte(p.Content))
}

func (e *Engine) updateAuthFile(raw json.RawMessage) (any, error) {
	if e.authFiles == nil {
		return nil, errors.New("auth files manager not initialized")
	}
	var p updateAuthFileParams
	if err := json.Unmarshal(raw, &p); err != nil || p.Name == "" {
		return nil, invalid("参数无效: name 不能为空")
	}
	return e.authFiles.Update(p.Name, p.Disabled, p.Priority, p.ExcludedModels)
}

func (e *Engine) deleteAuthFile(raw json.RawMessage) (any, error) {
	if e.authFiles == nil {
		return nil, errors.New("auth files manager not initialized")
	}
	var p deleteAuthFileParams
	if err := json.Unmarshal(raw, &p); err != nil || p.Name == "" {
		return nil, invalid("参数无效: name 不能为空")
	}
	err := e.authFiles.Delete(p.Name)
	if err != nil {
		return nil, err
	}
	return map[string]any{"deleted": p.Name}, nil
}

func (e *Engine) openAuthDir() (any, error) {
	if e.authFiles == nil {
		return nil, errors.New("auth files manager not initialized")
	}
	err := e.authFiles.OpenDir()
	if err != nil {
		return nil, err
	}
	return map[string]any{"ok": true}, nil
}

func (e *Engine) queryQuota(ctx context.Context, raw json.RawMessage) (any, error) {
	if e.quotaClient == nil {
		return nil, errors.New("quota client not initialized")
	}
	var p quotaQueryParams
	if err := json.Unmarshal(raw, &p); err != nil {
		return nil, invalid("参数无效")
	}

	token := ""
	extraJSON := ""

	// If a file name is given, try reading token from the auth file in ~/.natives/auth/
	if p.Name != "" && e.authFiles != nil {
		data, err := e.authFiles.ReadFile(p.Name)
		if err == nil {
			var m map[string]any
			if json.Unmarshal(data, &m) == nil {
				extraJSON = string(data)
				for _, k := range []string{"access_token", "accessToken", "token"} {
					if t, ok := m[k].(string); ok && t != "" {
						token = t
						break
					}
				}
				if token == "" {
					if cred, ok := m["credential"].(map[string]any); ok {
						for _, k := range []string{"access_token", "accessToken", "token"} {
							if t, ok := cred[k].(string); ok && t != "" {
								token = t
								break
							}
						}
					}
				}
				if p.Provider == "" {
					if pr, ok := m["provider"].(string); ok {
						p.Provider = pr
					} else if pr, ok := m["type"].(string); ok {
						p.Provider = pr
					}
				}
			}
		}
	}

	// OAuth credentials are stored as one JSON payload in Keychain.
	if token == "" {
		credentials, err := e.authStore.List(ctx)
		if err != nil {
			return nil, err
		}
		token, extraJSON = oauthQuotaCredential(credentials, p.Provider, p.AccountID)
	}
	if strings.TrimSpace(token) == "" {
		return nil, &SafeError{Code: "secret_not_configured", Message: "账户凭证不存在，请重新授权"}
	}
	return e.quotaClient.Query(ctx, p.Provider, p.Name, token, extraJSON)
}

func oauthQuotaCredential(credentials []*coreauth.Auth, provider, accountID string) (string, string) {
	for _, credential := range credentials {
		if accountID != "" && credential.ID != accountID {
			continue
		}
		if accountID == "" && !strings.EqualFold(credential.Provider, provider) {
			continue
		}
		token, _ := credential.Metadata["access_token"].(string)
		projectID, _ := credential.Metadata["project_id"].(string)
		if projectID == "" {
			return token, ""
		}
		project, err := json.Marshal(map[string]string{"project_id": projectID})
		if err != nil {
			return token, ""
		}
		return token, string(project)
	}
	return "", ""
}
