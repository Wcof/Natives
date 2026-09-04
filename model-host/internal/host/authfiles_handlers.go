package host

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
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
	Provider string `json:"provider"`
	Name     string `json:"name"`
}

func (e *Engine) listAuthFiles() (any, error) {
	if e.authFiles == nil {
		return []any{}, nil
	}
	return e.authFiles.List()
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

	// Fallback to Keychain accounts if token is still empty
	if token == "" {
		snapshot, err := e.repo.Load()
		if err == nil {
			for _, account := range snapshot.Accounts {
				if strings.EqualFold(account.Provider, p.Provider) || (p.Name != "" && strings.Contains(account.Label, p.Name)) {
					if secret, err := e.secrets.Get(account.SecretRef); err == nil && secret != "" {
						token = secret
						break
					}
				}
			}
		}
	}

	return e.quotaClient.Query(ctx, p.Provider, p.Name, token, extraJSON)
}
