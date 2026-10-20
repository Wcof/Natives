package host

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"strings"
	"time"

	"github.com/ldh/natives/model-host/internal/authfiles"
	clipadapter "github.com/ldh/natives/model-host/internal/cliproxy"
	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/nativeio"
)

// Google Antigravity 公共 OAuth client 标识：Google 公开发布、随上游
// CLIProxyAPI（third_party/cliproxyapi，同值）开源分发的 client 凭据，
// 不属于用户 Secret（refresh/access token 才是，始终只在 OS Keychain，R-S4）。
// 豁免依据与轮换结论见 docs/adr/0034-google-public-oauth-client-exemption.md。
// 仓库内只保留占位符；真实值通过环境变量 NATIVES_ANTIGRAVITY_OAUTH_CLIENT_ID /
// NATIVES_ANTIGRAVITY_OAUTH_CLIENT_SECRET 在本机注入（不入库、不被 git 追踪）。
const (
	antigravityOAuthClientIDEnv     = "NATIVES_ANTIGRAVITY_OAUTH_CLIENT_ID"
	antigravityOAuthClientSecretEnv = "NATIVES_ANTIGRAVITY_OAUTH_CLIENT_SECRET"
	// Antigravity OAuth client ID 是公开标识；secret 不入库，必须由本机环境变量提供。
	antigravityOAuthClientIDDefault = "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com"
	antigravityOAuthClientSecretDef = "GOCSPX-LOCAL-ONLY-REDACTED"
)

func antigravityOAuthCredentials() (clientID string, clientSecret string, err error) {
	clientID = strings.TrimSpace(os.Getenv(antigravityOAuthClientIDEnv))
	clientSecret = strings.TrimSpace(os.Getenv(antigravityOAuthClientSecretEnv))
	if clientID == "" {
		clientID = antigravityOAuthClientIDDefault
	}
	if clientSecret == "" {
		clientSecret = antigravityOAuthClientSecretDef
	}
	if strings.Contains(clientID, "PLACEHOLDER") || strings.Contains(clientSecret, "REDACTED") {
		return "", "", errors.New("antigravity oauth client credentials not configured; set NATIVES_ANTIGRAVITY_OAUTH_CLIENT_ID / NATIVES_ANTIGRAVITY_OAUTH_CLIENT_SECRET")
	}
	return clientID, clientSecret, nil
}

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

	// Join file-based credentials with their OAuth accounts so the 账号模型
	// dialog can manage per-account model exclusions from the same row.
	matched := make(map[string]bool, len(snapshot.Accounts))
	for index := range items {
		item := &items[index]
		for _, account := range snapshot.Accounts {
			if matched[account.ID] || !authFileMatchesAccount(item, account) {
				continue
			}
			item.AccountID = account.ID
			matched[account.ID] = true
			break
		}
	}
	// Accounts without a backing file still appear so they stay manageable.
	for _, account := range snapshot.Accounts {
		if matched[account.ID] {
			continue
		}
		items = append(items, authfiles.AuthFileItem{
			AccountID: account.ID, Source: "keychain", Name: account.Label,
			Provider: account.Provider, Account: account.Label, Status: account.Status,
			Disabled: !account.Enabled, UpdatedAt: account.UpdatedAt,
		})
	}
	return items, nil
}

// authFileMatchesAccount correlates an auth file with an OAuth account:
// same provider and the account label (email) appears in the file name or
// embedded account field.
func authFileMatchesAccount(item *authfiles.AuthFileItem, account domain.Account) bool {
	if normalizeAuthProvider(item.Provider) != normalizeAuthProvider(account.Provider) {
		return false
	}
	label := strings.ToLower(strings.TrimSpace(account.Label))
	if label == "" {
		return false
	}
	return strings.Contains(strings.ToLower(item.Name), label) ||
		strings.EqualFold(strings.TrimSpace(item.Account), label)
}

func normalizeAuthProvider(provider string) string {
	normalized := strings.ToLower(strings.TrimSpace(provider))
	switch normalized {
	case "anthropic":
		return "claude"
	case "anti-gravity":
		return "antigravity"
	default:
		return normalized
	}
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
	// mode/T07：凭据查找与令牌短时取用经适配层意图操作，SDK 对象不进本包。
	accountLabel := ""
	matchedID := ""
	if token == "" {
		var credErr error
		token, extraJSON, accountLabel, matchedID, credErr = e.authStore.OAuthQuotaCredential(ctx, p.Provider, p.AccountID)
		if credErr != nil {
			return nil, credErr
		}
	}
	if strings.TrimSpace(token) == "" {
		return nil, &SafeError{Code: "secret_not_configured", Message: "账户凭证不存在，请重新授权"}
	}
	result, queryErr := e.quotaClient.Query(ctx, p.Provider, p.Name, token, extraJSON)
	// 如果查询遇到 401（未认证/token过期），且为 antigravity 且有 refresh_token，自动换发新令牌重试并写回 Keychain
	if (queryErr != nil || (result != nil && result.Status == "error" && strings.Contains(result.Error, "401"))) &&
		strings.EqualFold(p.Provider, "antigravity") && matchedID != "" {
		if newTok, rErr := refreshAntigravityOAuthToken(ctx, e.authStore, matchedID); rErr == nil && newTok != "" {
			result, queryErr = e.quotaClient.Query(ctx, p.Provider, p.Name, newTok, extraJSON)
		}
	}
	if queryErr == nil && result != nil {
		if result.Account == "" {
			result.Account = accountLabel
		}
		// 真实结果写入额度缓存：顶栏面板读取展示最近一次真实余额
		saveQuotaCacheEntry(result)
		if result.Status == "success" && e.emit != nil {
			e.emit(nativeio.Response{OK: true, Event: "model_quota_updated"})
		}
	}
	return result, queryErr
}

// refreshAntigravityOAuthToken 用存储中的 refresh token 换发新访问令牌并
// 写回（mode/T07：令牌读取/写回经适配层意图操作，HTTP 换发留在本文件）。
func refreshAntigravityOAuthToken(ctx context.Context, store *clipadapter.AuthStore, accountID string) (string, error) {
	refreshToken, err := store.RefreshToken(ctx, accountID)
	if err != nil {
		return "", err
	}
	if strings.TrimSpace(refreshToken) == "" {
		return "", errors.New("missing refresh token")
	}

	clientID, clientSecret, credErr := antigravityOAuthCredentials()
	if credErr != nil {
		return "", credErr
	}

	form := url.Values{}
	form.Set("client_id", clientID)
	form.Set("client_secret", clientSecret)
	form.Set("grant_type", "refresh_token")
	form.Set("refresh_token", refreshToken)

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, "https://oauth2.googleapis.com/token", strings.NewReader(form.Encode()))
	if err != nil {
		return "", err
	}
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	req.Header.Set("User-Agent", "Go-http-client/2.0")

	client := &http.Client{Timeout: 10 * time.Second}
	resp, err := client.Do(req)
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()

	bodyBytes, _ := io.ReadAll(io.LimitReader(resp.Body, 32*1024))
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return "", fmt.Errorf("token refresh HTTP %d: %s", resp.StatusCode, strings.TrimSpace(string(bodyBytes)))
	}

	var data struct {
		AccessToken string `json:"access_token"`
		ExpiresIn   int64  `json:"expires_in"`
	}
	if err := json.Unmarshal(bodyBytes, &data); err != nil || data.AccessToken == "" {
		return "", errors.New("invalid token refresh response")
	}

	now := time.Now()
	values := map[string]any{
		"access_token": data.AccessToken,
		"timestamp":    now.UnixMilli(),
	}
	if data.ExpiresIn > 0 {
		values["expired"] = now.Add(time.Duration(data.ExpiresIn) * time.Second).Format(time.RFC3339)
	}
	if err := store.SaveRefreshedTokens(ctx, accountID, values); err != nil {
		return "", err
	}
	return data.AccessToken, nil
}
