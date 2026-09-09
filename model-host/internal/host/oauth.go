package host

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"slices"
	"strings"
	"time"

	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/nativeio"
	sdkauth "github.com/router-for-me/CLIProxyAPI/v7/sdk/auth"
	coreauth "github.com/router-for-me/CLIProxyAPI/v7/sdk/cliproxy/auth"
)

func (e *Engine) startOAuth(parent context.Context, raw json.RawMessage) (map[string]string, error) {
	var input struct {
		ExpectedRevision *int64 `json:"expectedRevision"`
		Provider         string `json:"provider"`
		AccountID        string `json:"accountId"`
	}
	if json.Unmarshal(raw, &input) != nil || !slices.Contains(domain.OAuthProviders, input.Provider) {
		return nil, invalid("不支持的 OAuth 供应商")
	}
	if err := requireRevision(input.ExpectedRevision); err != nil {
		return nil, err
	}
	snapshot, err := e.repo.Load()
	if err != nil {
		return nil, err
	}
	if snapshot.Revision != *input.ExpectedRevision {
		return nil, fmt.Errorf("revision_conflict")
	}
	if input.AccountID != "" {
		account, findErr := findAccount(&snapshot, input.AccountID)
		if findErr != nil {
			return nil, findErr
		}
		if account.Provider != input.Provider {
			return nil, invalid("重新授权账户与供应商不匹配")
		}
	}
	sessionID, err := randomID("oauth_")
	if err != nil {
		return nil, err
	}
	ctx, cancel := context.WithTimeout(parent, e.oauthTimeout)
	e.mu.Lock()
	if len(e.sessions) > 0 {
		e.mu.Unlock()
		cancel()
		return nil, &SafeError{Code: "oauth_in_progress", Message: "已有 OAuth 授权正在进行"}
	}
	e.sessions[sessionID] = cancel
	e.mu.Unlock()
	go e.runOAuth(ctx, sessionID, input.Provider, input.AccountID)
	return map[string]string{"sessionId": sessionID, "provider": input.Provider, "accountId": input.AccountID, "state": "pending"}, nil
}

func (e *Engine) runOAuth(ctx context.Context, sessionID, provider, targetAccountID string) {
	defer func() { e.mu.Lock(); delete(e.sessions, sessionID); e.mu.Unlock() }()
	record, _, err := e.auth.Login(ctx, provider, e.config, &sdkauth.LoginOptions{NoBrowser: false})
	if err != nil {
		state := "failed"
		code := "oauth_failed"
		if errors.Is(ctx.Err(), context.DeadlineExceeded) {
			state, code = "timeout", "oauth_timeout"
		} else if ctx.Err() != nil {
			state, code = "cancelled", "oauth_cancelled"
		}
		e.emit(nativeio.Response{OK: true, Event: "model_oauth_state_changed", Result: map[string]any{"sessionId": sessionID, "provider": provider, "state": state, "errorCode": code}})
		return
	}
	e.secretMu.Lock()
	defer e.secretMu.Unlock()
	if targetAccountID != "" && record.ID != targetAccountID {
		temporaryID := record.ID
		record = record.Clone()
		record.ID = targetAccountID
		if _, saveErr := e.authStore.Save(context.Background(), record); saveErr != nil {
			_ = e.authStore.Delete(context.Background(), temporaryID)
			e.emit(nativeio.Response{OK: true, Event: "model_oauth_state_changed", Result: map[string]any{"sessionId": sessionID, "provider": provider, "state": "failed", "errorCode": "credential_replace_failed"}})
			return
		}
		if deleteErr := e.authStore.Delete(context.Background(), temporaryID); deleteErr != nil {
			e.emit(nativeio.Response{OK: true, Event: "model_oauth_state_changed", Result: map[string]any{"sessionId": sessionID, "provider": provider, "state": "failed", "errorCode": "credential_cleanup_failed"}})
			return
		}
	}
	mask := ""
	if token, ok := record.Metadata["access_token"].(string); ok {
		mask = maskSecret(token)
	}
	snapshot, updateErr := e.repo.Update(nil, func(snapshot *domain.Snapshot) error {
		for index := range snapshot.Accounts {
			if snapshot.Accounts[index].ID == record.ID {
				snapshot.Accounts[index].Label = safeLabel(record.Label, provider)
				snapshot.Accounts[index].Status = "active"
				snapshot.Accounts[index].Mask = mask
				snapshot.Accounts[index].UpdatedAt = time.Now().UTC().Format(time.RFC3339Nano)
				return nil
			}
		}
		if len(snapshot.Accounts) >= 100 {
			return invalid("OAuth 账户数量已达到上限")
		}
		snapshot.Accounts = append(snapshot.Accounts, domain.Account{ID: record.ID, Provider: provider, Label: safeLabel(record.Label, provider), Enabled: true, Status: "active", SecretRef: "oauth:" + record.ID, Mask: mask, UpdatedAt: time.Now().UTC().Format(time.RFC3339Nano)})
		return nil
	})
	if updateErr != nil {
		_ = e.authStore.Delete(context.Background(), record.ID)
		e.emit(nativeio.Response{OK: true, Event: "model_oauth_state_changed", Result: map[string]any{"sessionId": sessionID, "provider": provider, "state": "failed", "errorCode": "metadata_save_failed"}})
		return
	}
	if syncErr := e.runtime.SyncAuth(context.Background(), record); syncErr != nil {
		e.emit(nativeio.Response{OK: true, Event: "model_oauth_state_changed", Result: map[string]any{"sessionId": sessionID, "provider": provider, "state": "failed", "errorCode": "runtime_sync_failed"}})
		return
	}
	snapshot, _ = e.repo.Load()
	e.emit(nativeio.Response{OK: true, Event: "model_oauth_state_changed", Result: map[string]any{"sessionId": sessionID, "provider": provider, "state": "succeeded", "snapshot": snapshot}})
}

func (e *Engine) cancelOAuth(raw json.RawMessage) (map[string]string, error) {
	var input struct {
		ExpectedRevision *int64 `json:"expectedRevision"`
		SessionID        string `json:"sessionId"`
	}
	if json.Unmarshal(raw, &input) != nil || input.SessionID == "" {
		return nil, invalid("OAuth 会话 ID 不能为空")
	}
	if err := requireRevision(input.ExpectedRevision); err != nil {
		return nil, err
	}
	snapshot, err := e.repo.Load()
	if err != nil {
		return nil, err
	}
	if snapshot.Revision != *input.ExpectedRevision {
		return nil, fmt.Errorf("revision_conflict")
	}
	e.mu.Lock()
	cancel := e.sessions[input.SessionID]
	e.mu.Unlock()
	if cancel == nil {
		return nil, notFound()
	}
	cancel()
	return map[string]string{"sessionId": input.SessionID, "state": "cancelling"}, nil
}

func (e *Engine) setAccountEnabled(raw json.RawMessage) (domain.Snapshot, error) {
	e.secretMu.Lock()
	defer e.secretMu.Unlock()
	var input struct {
		ExpectedRevision *int64 `json:"expectedRevision"`
		AccountID        string `json:"accountId"`
		Enabled          *bool  `json:"enabled"`
	}
	if json.Unmarshal(raw, &input) != nil || input.AccountID == "" || input.Enabled == nil {
		return domain.Snapshot{}, invalid("账户状态参数无效")
	}
	if err := requireRevision(input.ExpectedRevision); err != nil {
		return domain.Snapshot{}, err
	}
	current, err := e.repo.Load()
	if err != nil {
		return domain.Snapshot{}, err
	}
	account, err := findAccount(&current, input.AccountID)
	if err != nil {
		return domain.Snapshot{}, err
	}
	credentials, err := e.authStore.List(context.Background())
	if err != nil {
		return domain.Snapshot{}, err
	}
	credentialIndex := slices.IndexFunc(credentials, func(credential *coreauth.Auth) bool { return credential.ID == input.AccountID })
	if credentialIndex < 0 {
		return domain.Snapshot{}, &SafeError{Code: "secret_not_configured", Message: "账户凭证不存在，请重新授权"}
	}
	originalEnabled := account.Enabled
	if _, err = e.repo.Update(input.ExpectedRevision, func(snapshot *domain.Snapshot) error {
		account, err := findAccount(snapshot, input.AccountID)
		if err == nil {
			account.Enabled = *input.Enabled
			if !*input.Enabled {
				removeAccountModels(snapshot, input.AccountID)
			}
		}
		return err
	}); err != nil {
		return domain.Snapshot{}, err
	}
	credential := credentials[credentialIndex].Clone()
	credential.Disabled = !*input.Enabled
	credential.Status = coreauth.StatusActive
	if credential.Disabled {
		credential.Status = coreauth.StatusDisabled
	}
	if _, err = e.authStore.Save(context.Background(), credential); err == nil {
		err = e.runtime.SyncAuth(context.Background(), credential)
	}
	if err != nil {
		_, _ = e.repo.Update(nil, func(snapshot *domain.Snapshot) error {
			account, findErr := findAccount(snapshot, input.AccountID)
			if findErr == nil {
				account.Enabled = originalEnabled
			}
			return findErr
		})
		_, _ = e.authStore.Save(context.Background(), credentials[credentialIndex])
		_ = e.runtime.SyncAuth(context.Background(), credentials[credentialIndex])
		return domain.Snapshot{}, err
	}
	return e.repo.Load()
}

func (e *Engine) deleteAccount(raw json.RawMessage) (domain.Snapshot, error) {
	e.secretMu.Lock()
	defer e.secretMu.Unlock()
	var input struct {
		ExpectedRevision *int64 `json:"expectedRevision"`
		AccountID        string `json:"accountId"`
	}
	if json.Unmarshal(raw, &input) != nil || input.AccountID == "" {
		return domain.Snapshot{}, invalid("账户 ID 不能为空")
	}
	if err := requireRevision(input.ExpectedRevision); err != nil {
		return domain.Snapshot{}, err
	}
	current, err := e.repo.Load()
	if err != nil {
		return domain.Snapshot{}, err
	}
	if input.ExpectedRevision != nil && *input.ExpectedRevision != current.Revision {
		return domain.Snapshot{}, fmt.Errorf("revision_conflict")
	}
	if _, err = findAccount(&current, input.AccountID); err != nil {
		return domain.Snapshot{}, err
	}
	credentials, err := e.authStore.List(context.Background())
	if err != nil {
		return domain.Snapshot{}, err
	}
	credentialIndex := slices.IndexFunc(credentials, func(credential *coreauth.Auth) bool { return credential.ID == input.AccountID })
	if err = e.authStore.Delete(context.Background(), input.AccountID); err != nil {
		return domain.Snapshot{}, err
	}
	snapshot, err := e.repo.Update(input.ExpectedRevision, func(snapshot *domain.Snapshot) error {
		if _, findErr := findAccount(snapshot, input.AccountID); findErr != nil {
			return findErr
		}
		snapshot.Accounts = slices.DeleteFunc(snapshot.Accounts, func(account domain.Account) bool { return account.ID == input.AccountID })
		removeAccountModels(snapshot, input.AccountID)
		return nil
	})
	if err != nil {
		if credentialIndex >= 0 {
			_, _ = e.authStore.Save(context.Background(), credentials[credentialIndex])
		}
		return domain.Snapshot{}, err
	}
	e.runtime.RemoveAuth(context.Background(), input.AccountID)
	return snapshot, nil
}

func safeLabel(label, provider string) string {
	label = strings.TrimSpace(label)
	if label == "" {
		return strings.ToUpper(provider) + " account"
	}
	if runes := []rune(label); len(runes) > 120 {
		return string(runes[:120])
	}
	return label
}

func syncAccountFromCredential(repo *domain.Repository, emit func(nativeio.Response), credential *coreauth.Auth) error {
	current, err := repo.Load()
	if err != nil {
		return err
	}
	if slices.IndexFunc(current.Accounts, func(account domain.Account) bool { return account.ID == credential.ID }) < 0 {
		return nil
	}
	snapshot, err := repo.Update(nil, func(snapshot *domain.Snapshot) error {
		account, findErr := findAccount(snapshot, credential.ID)
		if findErr != nil {
			return findErr
		}
		account.Status = "active"
		if credential.Status == coreauth.StatusError && (credential.LastError == nil || !credential.LastError.Retryable) {
			account.Status = "needs_reauth"
		}
		if token, ok := credential.Metadata["access_token"].(string); ok {
			account.Mask = maskSecret(token)
		}
		account.UpdatedAt = time.Now().UTC().Format(time.RFC3339Nano)
		return nil
	})
	if err == nil && emit != nil {
		emit(nativeio.Response{OK: true, Event: "model_account_state_changed", Result: map[string]any{"snapshot": snapshot}})
	}
	return err
}
