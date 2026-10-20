package host

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"slices"
	"strings"
	"time"

	clipadapter "github.com/ldh/natives/model-host/internal/cliproxy"
	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/nativeio"
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
	// mode/T07：登录、凭据重指与掩码提取在适配层完成；业务只见 AccountView。
	view, cleanup, err := e.auth.Login(ctx, provider, targetAccountID, maskSecret)
	if err != nil {
		state := "failed"
		code := "oauth_failed"
		if errors.Is(err, clipadapter.ErrCredentialReplaceFailed) {
			code = "credential_replace_failed"
		} else if errors.Is(err, clipadapter.ErrCredentialCleanupFailed) {
			code = "credential_cleanup_failed"
		} else if errors.Is(ctx.Err(), context.DeadlineExceeded) {
			state, code = "timeout", "oauth_timeout"
		} else if ctx.Err() != nil {
			state, code = "cancelled", "oauth_cancelled"
		}
		e.emit(nativeio.Response{OK: true, Event: "model_oauth_state_changed", Result: map[string]any{"sessionId": sessionID, "provider": provider, "state": state, "errorCode": code}})
		return
	}
	e.secretMu.Lock()
	defer e.secretMu.Unlock()
	snapshot, updateErr := e.repo.Update(nil, func(snapshot *domain.Snapshot) error {
		for index := range snapshot.Accounts {
			if snapshot.Accounts[index].ID == view.ID {
				snapshot.Accounts[index].Label = safeLabel(view.Label, provider)
				snapshot.Accounts[index].Status = "active"
				snapshot.Accounts[index].Mask = view.Mask
				snapshot.Accounts[index].UpdatedAt = time.Now().UTC().Format(time.RFC3339Nano)
				return nil
			}
		}
		if len(snapshot.Accounts) >= 100 {
			return invalid("OAuth 账户数量已达到上限")
		}
		snapshot.Accounts = append(snapshot.Accounts, domain.Account{ID: view.ID, Provider: provider, Label: safeLabel(view.Label, provider), Enabled: true, Status: "active", SecretRef: "oauth:" + view.ID, Mask: view.Mask, UpdatedAt: time.Now().UTC().Format(time.RFC3339Nano)})
		return nil
	})
	if updateErr != nil {
		_ = cleanup(context.Background())
		e.emit(nativeio.Response{OK: true, Event: "model_oauth_state_changed", Result: map[string]any{"sessionId": sessionID, "provider": provider, "state": "failed", "errorCode": "metadata_save_failed"}})
		return
	}
	if syncErr := e.runtime.SyncAccount(context.Background(), e.authStore, view.ID); syncErr != nil {
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
	views, err := e.authStore.AccountViews(context.Background())
	if err != nil {
		return domain.Snapshot{}, err
	}
	if !slices.ContainsFunc(views, func(view clipadapter.AccountView) bool { return view.ID == input.AccountID }) {
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
	// mode/T07：克隆/状态变更/持久化/补偿在适配层（SwapAccountDisabled）。
	rollback, swapErr := e.authStore.SwapAccountDisabled(context.Background(), input.AccountID, !*input.Enabled)
	if swapErr == nil {
		if syncErr := e.runtime.SyncAccount(context.Background(), e.authStore, input.AccountID); syncErr != nil {
			_ = rollback(context.Background())
			swapErr = syncErr
		}
	}
	if swapErr != nil {
		_, _ = e.repo.Update(nil, func(snapshot *domain.Snapshot) error {
			account, findErr := findAccount(snapshot, input.AccountID)
			if findErr == nil {
				account.Enabled = originalEnabled
			}
			return findErr
		})
		if rollback != nil {
			_ = rollback(context.Background())
			_ = e.runtime.SyncAccount(context.Background(), e.authStore, input.AccountID)
		}
		return domain.Snapshot{}, swapErr
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
	// mode/T07：删除与补偿在适配层（DeleteAccount 返回恢复原记录的 undo）。
	undo, deleteErr := e.authStore.DeleteAccount(context.Background(), input.AccountID)
	if deleteErr != nil {
		return domain.Snapshot{}, deleteErr
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
		if undo != nil {
			_ = undo(context.Background())
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

// syncAccountFromView 消费适配层的项目视图（mode/T07）：SDK 状态枚举与
// 明文令牌不再进入本包；掩码由适配层按注入的 maskSecret 预先计算。
func syncAccountFromView(repo *domain.Repository, emit func(nativeio.Response), view clipadapter.AccountView) error {
	current, err := repo.Load()
	if err != nil {
		return err
	}
	if slices.IndexFunc(current.Accounts, func(account domain.Account) bool { return account.ID == view.ID }) < 0 {
		return nil
	}
	snapshot, err := repo.Update(nil, func(snapshot *domain.Snapshot) error {
		account, findErr := findAccount(snapshot, view.ID)
		if findErr != nil {
			return findErr
		}
		account.Status = "active"
		if view.Status == clipadapter.AccountStatusError && !view.LastErrorRetryable {
			account.Status = "needs_reauth"
		}
		if view.Mask != "" {
			account.Mask = view.Mask
		}
		account.UpdatedAt = time.Now().UTC().Format(time.RFC3339Nano)
		return nil
	})
	if err == nil && emit != nil {
		emit(nativeio.Response{OK: true, Event: "model_account_state_changed", Result: map[string]any{"snapshot": snapshot}})
	}
	return err
}
