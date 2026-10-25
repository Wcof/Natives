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

	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/secrets"
)

const (
	antigravityOAuthClientIDEnv     = "NATIVES_ANTIGRAVITY_OAUTH_CLIENT_ID"
	antigravityOAuthClientSecretEnv = "NATIVES_ANTIGRAVITY_OAUTH_CLIENT_SECRET"
	antigravityOAuthClientIDDefault = "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com"
	antigravityOAuthClientSecretDef = "GOCSPX-LOCAL-ONLY-REDACTED"
)

func antigravityOAuthCredentials() (string, string, error) {
	clientID := strings.TrimSpace(os.Getenv(antigravityOAuthClientIDEnv))
	clientSecret := strings.TrimSpace(os.Getenv(antigravityOAuthClientSecretEnv))
	if clientID == "" {
		clientID = antigravityOAuthClientIDDefault
	}
	if clientSecret == "" {
		clientSecret = antigravityOAuthClientSecretDef
	}
	if strings.Contains(clientID, "PLACEHOLDER") || strings.Contains(clientSecret, "REDACTED") {
		return "", "", errors.New("antigravity oauth client credentials not configured")
	}
	return clientID, clientSecret, nil
}

// oauthQuotaCredential reads an existing OAuth credential for quota inquiry
// only. It never feeds inference and never creates or rotates accounts.
func (e *Engine) oauthQuotaCredential(_ context.Context, provider, accountID string) (token, extraJSON, label, matchedID string, err error) {
	snapshot, err := e.repo.Load()
	if err != nil {
		return "", "", "", "", err
	}
	var account *domain.Account
	if strings.TrimSpace(accountID) != "" {
		account, _ = findAccount(&snapshot, strings.TrimSpace(accountID))
	} else {
		for index := range snapshot.Accounts {
			candidate := &snapshot.Accounts[index]
			if !candidate.Enabled || candidate.Provider != provider {
				continue
			}
			account = candidate
			break
		}
	}
	if account == nil {
		return "", "", "", "", nil
	}
	raw, err := e.secrets.Get(account.SecretRef)
	if errors.Is(err, secrets.ErrNotFound) {
		return "", "", account.Label, account.ID, nil
	}
	if err != nil {
		return "", "", "", "", err
	}
	var payload map[string]any
	if json.Unmarshal([]byte(raw), &payload) == nil {
		extraJSON = raw
		token = tokenFromPayload(payload)
	}
	return token, extraJSON, account.Label, account.ID, nil
}

func tokenFromPayload(payload map[string]any) string {
	for _, key := range []string{"access_token", "accessToken", "token"} {
		if value, ok := payload[key].(string); ok && value != "" {
			return value
		}
	}
	if credential, ok := payload["credential"].(map[string]any); ok {
		for _, key := range []string{"access_token", "accessToken", "token"} {
			if value, ok := credential[key].(string); ok && value != "" {
				return value
			}
		}
	}
	return ""
}

func (e *Engine) refreshAntigravityQuotaToken(ctx context.Context, accountID string) (string, error) {
	snapshot, err := e.repo.Load()
	if err != nil {
		return "", err
	}
	account, err := findAccount(&snapshot, accountID)
	if err != nil {
		return "", err
	}
	raw, err := e.secrets.Get(account.SecretRef)
	if err != nil {
		return "", err
	}
	var payload map[string]any
	if json.Unmarshal([]byte(raw), &payload) != nil {
		return "", errors.New("stored credential is invalid")
	}
	refreshToken, _ := payload["refresh_token"].(string)
	if refreshToken == "" {
		return "", errors.New("missing refresh token")
	}
	clientID, clientSecret, err := antigravityOAuthCredentials()
	if err != nil {
		return "", err
	}
	form := url.Values{}
	form.Set("client_id", clientID)
	form.Set("client_secret", clientSecret)
	form.Set("grant_type", "refresh_token")
	form.Set("refresh_token", refreshToken)
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, "https://oauth2.googleapis.com/token", strings.NewReader(form.Encode()))
	if err != nil {
		return "", err
	}
	request.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	client := &http.Client{Timeout: 10 * time.Second}
	response, err := client.Do(request)
	if err != nil {
		return "", err
	}
	defer response.Body.Close()
	body, _ := io.ReadAll(io.LimitReader(response.Body, 32*1024))
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return "", fmt.Errorf("token refresh HTTP %d", response.StatusCode)
	}
	var refreshed struct {
		AccessToken string `json:"access_token"`
		ExpiresIn   int64  `json:"expires_in"`
	}
	if json.Unmarshal(body, &refreshed) != nil || refreshed.AccessToken == "" {
		return "", errors.New("invalid token refresh response")
	}
	payload["access_token"] = refreshed.AccessToken
	payload["timestamp"] = time.Now().UnixMilli()
	if refreshed.ExpiresIn > 0 {
		payload["expired"] = time.Now().Add(time.Duration(refreshed.ExpiresIn) * time.Second).Format(time.RFC3339)
	}
	encoded, err := json.Marshal(payload)
	if err != nil {
		return "", err
	}
	if err := e.secrets.Set(account.SecretRef, string(encoded)); err != nil {
		return "", err
	}
	return refreshed.AccessToken, nil
}
