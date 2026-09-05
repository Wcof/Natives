package host

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/secrets"
)

const gatewaySecretRef = "gateway:access-key"

func (e *Engine) startGateway(ctx context.Context, raw json.RawMessage) (domain.Snapshot, error) {
	e.gatewayMu.Lock()
	defer e.gatewayMu.Unlock()
	expectedRevision, err := expected(raw)
	if err != nil {
		return domain.Snapshot{}, err
	}
	return e.startGatewayWithState(ctx, expectedRevision, "starting")
}

func (e *Engine) restartGateway(ctx context.Context, raw json.RawMessage) (domain.Snapshot, error) {
	e.gatewayMu.Lock()
	defer e.gatewayMu.Unlock()
	expectedRevision, err := expected(raw)
	if err != nil {
		return domain.Snapshot{}, err
	}
	snapshot, err := e.repo.Load()
	if err != nil {
		return domain.Snapshot{}, err
	}
	if snapshot.Gateway.State == "running" {
		stopCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
		_ = e.runtime.Stop(stopCtx)
		cancel()
	}
	return e.startGatewayWithState(ctx, expectedRevision, "restarting")
}

func (e *Engine) startGatewayWithState(ctx context.Context, expectedRevision *int64, state string) (domain.Snapshot, error) {
	// Ensure at least default secret exists
	secret, err := e.secrets.Get(gatewaySecretRef)
	if err == secrets.ErrNotFound {
		secret, err = randomSecret()
		if err == nil {
			err = e.secrets.Set(gatewaySecretRef, secret)
		}
	}
	if err != nil {
		return domain.Snapshot{}, err
	}
	starting, err := e.repo.Update(expectedRevision, func(snapshot *domain.Snapshot) error {
		snapshot.Gateway.State = state
		snapshot.Gateway.ErrorCode = ""
		snapshot.Gateway.AccessKeyMask = maskSecret(secret)
		return nil
	})
	if err != nil {
		return domain.Snapshot{}, err
	}
	port, err := e.runtime.Start(ctx, starting, e.secrets, e.authStore, e.runtimeConfigPath)
	if err != nil {
		failed, updateErr := e.repo.Update(nil, func(snapshot *domain.Snapshot) error {
			snapshot.Gateway.State = "failed"
			snapshot.Gateway.ErrorCode = "gateway_start_failed"
			return nil
		})
		if updateErr != nil {
			return domain.Snapshot{}, updateErr
		}
		e.emitSnapshot("model_gateway_state_changed", failed)
		return failed, &SafeError{Code: "gateway_start_failed", Message: "本地模型代理启动失败"}
	}
		running, err := e.repo.Update(nil, func(snapshot *domain.Snapshot) error {
			snapshot.Gateway.State = "running"
			snapshot.Gateway.Port = port
			snapshot.Gateway.BaseURL = fmt.Sprintf("http://127.0.0.1:%d", port)
			snapshot.Gateway.ErrorCode = ""
			snapshot.Gateway.PID = os.Getpid()
			snapshot.Gateway.KernelVersion = getLocalKernelVersion()
			snapshot.Gateway.LatestKernelVersion = getCachedLatestKernelVersion()
			snapshot.Gateway.Version = "v0.2.25"
			return nil
		})
	if err == nil {
		e.emitSnapshot("model_gateway_state_changed", running)
	}
	return running, err
}

func (e *Engine) Restore(ctx context.Context) error {
	snapshot, err := e.repo.Load()
	if err != nil {
		return err
	}
	if !snapshot.Gateway.Resident {
		if snapshot.Gateway.State != "stopped" {
			_, err = e.repo.Update(&snapshot.Revision, resetGateway)
		}
		return err
	}
	if snapshot.Gateway.State != "running" && snapshot.Gateway.State != "starting" && snapshot.Gateway.State != "restarting" {
		return nil
	}
	_, err = e.startGatewayWithState(ctx, &snapshot.Revision, "restarting")
	return err
}

func (e *Engine) Resident() bool {
	snapshot, err := e.repo.Load()
	return err == nil && snapshot.Gateway.Resident
}

func (e *Engine) stopGateway(ctx context.Context, raw json.RawMessage) (domain.Snapshot, error) {
	e.gatewayMu.Lock()
	defer e.gatewayMu.Unlock()
	expectedRevision, err := expected(raw)
	if err != nil {
		return domain.Snapshot{}, err
	}
	if _, err = e.repo.Update(expectedRevision, func(snapshot *domain.Snapshot) error {
		snapshot.Gateway.State = "stopping"
		return nil
	}); err != nil {
		return domain.Snapshot{}, err
	}
	stopCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	if err = e.runtime.Stop(stopCtx); err != nil {
		return domain.Snapshot{}, &SafeError{Code: "gateway_stop_failed", Message: "本地模型代理停止失败"}
	}
	stopped, err := e.repo.Update(nil, resetGateway)
	if err == nil {
		e.emitSnapshot("model_gateway_state_changed", stopped)
	}
	return stopped, err
}

func (e *Engine) setResident(raw json.RawMessage) (domain.Snapshot, error) {
	var input struct {
		ExpectedRevision *int64 `json:"expectedRevision"`
		Resident         *bool  `json:"resident"`
	}
	if json.Unmarshal(raw, &input) != nil || input.Resident == nil {
		return domain.Snapshot{}, invalid("常驻设置参数无效")
	}
	if err := requireRevision(input.ExpectedRevision); err != nil {
		return domain.Snapshot{}, err
	}
	snapshot, err := e.repo.Update(input.ExpectedRevision, func(snapshot *domain.Snapshot) error { snapshot.Gateway.Resident = *input.Resident; return nil })
	if err == nil {
		e.emitSnapshot("model_gateway_state_changed", snapshot)
	}
	return snapshot, err
}

func (e *Engine) updateGatewaySettings(raw json.RawMessage) (domain.Snapshot, error) {
	var input struct {
		ExpectedRevision *int64                 `json:"expectedRevision"`
		Settings         domain.GatewaySettings `json:"settings"`
	}
	if json.Unmarshal(raw, &input) != nil {
		return domain.Snapshot{}, invalid("网关高级设置参数无效")
	}
	if err := requireRevision(input.ExpectedRevision); err != nil {
		return domain.Snapshot{}, err
	}
	if err := validateGatewaySettings(input.Settings); err != nil {
		return domain.Snapshot{}, err
	}
	snapshot, err := e.repo.Update(input.ExpectedRevision, func(snapshot *domain.Snapshot) error {
		snapshot.Gateway.Settings = input.Settings
		if input.Settings.PreferredPort > 0 {
			snapshot.Gateway.PreferredPort = input.Settings.PreferredPort
		}
		return nil
	})
	return e.reconfigureGateway(snapshot, err)
}

func (e *Engine) createGatewayKey(raw json.RawMessage) (map[string]any, error) {
	e.gatewayMu.Lock()
	defer e.gatewayMu.Unlock()
	e.secretMu.Lock()
	defer e.secretMu.Unlock()

	var input struct {
		ExpectedRevision *int64 `json:"expectedRevision"`
		Name             string `json:"name"`
	}
	if err := json.Unmarshal(raw, &input); err != nil {
		return nil, invalid("创建密钥参数无效")
	}
	if err := requireRevision(input.ExpectedRevision); err != nil {
		return nil, err
	}
	name := strings.TrimSpace(input.Name)
	if name == "" {
		name = "新建密钥"
	}
	if len([]rune(name)) > 80 {
		return nil, invalid("访问密钥名称不能超过 80 个字符")
	}

	keyID, err := randomID("key_")
	if err != nil {
		return nil, err
	}
	secretRef := "gateway:key:" + keyID
	secret, err := randomSecret()
	if err != nil {
		return nil, err
	}
	if err = e.secrets.Set(secretRef, secret); err != nil {
		return nil, err
	}

	now := time.Now().UTC().Format(time.RFC3339)
	snapshot, err := e.repo.Update(input.ExpectedRevision, func(snapshot *domain.Snapshot) error {
		if len(snapshot.Gateway.AccessKeys) >= 64 {
			return invalid("访问密钥数量已达上限 (64)")
		}
		keyRecord := domain.GatewayAccessKey{
			ID:        keyID,
			Name:      name,
			SecretRef: secretRef,
			Mask:      maskSecret(secret),
			CreatedAt: now,
			UpdatedAt: now,
			Enabled:   true,
		}
		snapshot.Gateway.AccessKeys = append(snapshot.Gateway.AccessKeys, keyRecord)
		return nil
	})
	if err != nil {
		_ = e.secrets.Delete(secretRef)
		return nil, err
	}

	reconfigured, err := e.reconfigureGateway(snapshot, nil)
	if err != nil {
		return nil, err
	}
	return map[string]any{"snapshot": reconfigured, "accessKey": secret}, nil
}

func (e *Engine) updateGatewayKey(raw json.RawMessage) (domain.Snapshot, error) {
	var input struct {
		ExpectedRevision *int64  `json:"expectedRevision"`
		KeyID            string  `json:"keyId"`
		Name             *string `json:"name"`
		Enabled          *bool   `json:"enabled"`
	}
	if json.Unmarshal(raw, &input) != nil || input.KeyID == "" {
		return domain.Snapshot{}, invalid("修改密钥参数无效")
	}
	if err := requireRevision(input.ExpectedRevision); err != nil {
		return domain.Snapshot{}, err
	}

	now := time.Now().UTC().Format(time.RFC3339)
	snapshot, err := e.repo.Update(input.ExpectedRevision, func(snapshot *domain.Snapshot) error {
		for i := range snapshot.Gateway.AccessKeys {
			if snapshot.Gateway.AccessKeys[i].ID == input.KeyID {
				if input.Name != nil {
					name := strings.TrimSpace(*input.Name)
					if len([]rune(name)) > 80 {
						return invalid("访问密钥名称不能超过 80 个字符")
					}
					if name != "" {
						snapshot.Gateway.AccessKeys[i].Name = name
					}
				}
				if input.Enabled != nil {
					if !*input.Enabled && snapshot.Gateway.AccessKeys[i].Enabled && enabledGatewayKeyCount(snapshot.Gateway.AccessKeys) == 1 {
						return invalid("至少需要保留一个已启用的访问密钥")
					}
					snapshot.Gateway.AccessKeys[i].Enabled = *input.Enabled
				}
				snapshot.Gateway.AccessKeys[i].UpdatedAt = now
				return nil
			}
		}
		return notFound()
	})
	return e.reconfigureGateway(snapshot, err)
}

func (e *Engine) deleteGatewayKey(raw json.RawMessage) (domain.Snapshot, error) {
	e.gatewayMu.Lock()
	defer e.gatewayMu.Unlock()
	e.secretMu.Lock()
	defer e.secretMu.Unlock()

	var input struct {
		ExpectedRevision *int64 `json:"expectedRevision"`
		KeyID            string `json:"keyId"`
	}
	if json.Unmarshal(raw, &input) != nil || input.KeyID == "" {
		return domain.Snapshot{}, invalid("删除密钥参数无效")
	}
	if err := requireRevision(input.ExpectedRevision); err != nil {
		return domain.Snapshot{}, err
	}

	var secretRefToDelete string
	snapshot, err := e.repo.Update(input.ExpectedRevision, func(snapshot *domain.Snapshot) error {
		var remaining []domain.GatewayAccessKey
		found := false
		for _, k := range snapshot.Gateway.AccessKeys {
			if k.ID == input.KeyID {
				secretRefToDelete = k.SecretRef
				found = true
			} else {
				remaining = append(remaining, k)
			}
		}
		if !found {
			return notFound()
		}
		if len(remaining) == 0 {
			return invalid("至少需要保留一个访问密钥")
		}
		if enabledGatewayKeyCount(remaining) == 0 {
			return invalid("至少需要保留一个已启用的访问密钥")
		}
		snapshot.Gateway.AccessKeys = remaining
		return nil
	})
	if err != nil {
		return domain.Snapshot{}, err
	}
	if secretRefToDelete != "" {
		_ = e.secrets.Delete(secretRefToDelete)
	}
	return e.reconfigureGateway(snapshot, nil)
}

func (e *Engine) rotateGatewayKeyByID(raw json.RawMessage) (map[string]any, error) {
	e.gatewayMu.Lock()
	defer e.gatewayMu.Unlock()
	e.secretMu.Lock()
	defer e.secretMu.Unlock()

	var input struct {
		ExpectedRevision *int64 `json:"expectedRevision"`
		KeyID            string `json:"keyId"`
	}
	if err := json.Unmarshal(raw, &input); err != nil {
		return nil, invalid("重置密钥参数无效")
	}
	if err := requireRevision(input.ExpectedRevision); err != nil {
		return nil, err
	}

	secretRef := gatewaySecretRef
	if input.KeyID != "" {
		current, err := e.repo.Load()
		if err != nil {
			return nil, err
		}
		found := false
		for _, k := range current.Gateway.AccessKeys {
			if k.ID == input.KeyID {
				secretRef = k.SecretRef
				found = true
				break
			}
		}
		if !found {
			return nil, notFound()
		}
	}

	previous, previousErr := e.secrets.Get(secretRef)
	if previousErr != nil && previousErr != secrets.ErrNotFound {
		return nil, previousErr
	}
	secret, err := randomSecret()
	if err != nil {
		return nil, err
	}
	if err = e.secrets.Set(secretRef, secret); err != nil {
		return nil, err
	}

	now := time.Now().UTC().Format(time.RFC3339)
	snapshot, err := e.repo.Update(input.ExpectedRevision, func(snapshot *domain.Snapshot) error {
		if input.KeyID == "" {
			snapshot.Gateway.AccessKeyMask = maskSecret(secret)
		} else {
			for i := range snapshot.Gateway.AccessKeys {
				if snapshot.Gateway.AccessKeys[i].ID == input.KeyID {
					snapshot.Gateway.AccessKeys[i].Mask = maskSecret(secret)
					snapshot.Gateway.AccessKeys[i].UpdatedAt = now
					break
				}
			}
		}
		if snapshot.Gateway.State == "running" {
			snapshot.Gateway.State = "restarting"
		}
		return nil
	})
	if err != nil {
		if previousErr == nil {
			_ = e.secrets.Set(secretRef, previous)
		} else {
			_ = e.secrets.Delete(secretRef)
		}
		return nil, err
	}
	if snapshot.Gateway.State == "restarting" {
		stopCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		stopErr := e.runtime.Stop(stopCtx)
		cancel()
		if stopErr == nil {
			var port int
			port, err = e.runtime.Start(context.Background(), snapshot, e.secrets, e.authStore, e.runtimeConfigPath)
			if err == nil {
				snapshot, err = e.repo.Update(nil, func(current *domain.Snapshot) error {
					current.Gateway.State = "running"
					current.Gateway.Port = port
					current.Gateway.BaseURL = fmt.Sprintf("http://127.0.0.1:%d", port)
					return nil
				})
			}
		} else {
			err = stopErr
		}
		if err != nil {
			_, _ = e.repo.Update(nil, func(current *domain.Snapshot) error {
				current.Gateway.State = "failed"
				current.Gateway.ErrorCode = "gateway_restart_failed"
				return nil
			})
			return nil, &SafeError{Code: "gateway_restart_failed", Message: "访问密钥已更新，但代理重启失败，请重试"}
		}
	}
	e.emitSnapshot("model_gateway_state_changed", snapshot)
	return map[string]any{"snapshot": snapshot, "accessKey": secret}, nil
}

func (e *Engine) revealGatewayKeyByID(raw json.RawMessage) (map[string]string, error) {
	var input struct {
		KeyID string `json:"keyId"`
	}
	if len(raw) > 0 && string(raw) != "null" {
		_ = json.Unmarshal(raw, &input)
	}

	secretRef := gatewaySecretRef
	if input.KeyID != "" {
		current, err := e.repo.Load()
		if err != nil {
			return nil, err
		}
		found := false
		for _, k := range current.Gateway.AccessKeys {
			if k.ID == input.KeyID {
				secretRef = k.SecretRef
				found = true
				break
			}
		}
		if !found {
			return nil, notFound()
		}
	}

	secret, err := e.secrets.Get(secretRef)
	if err != nil {
		if err == secrets.ErrNotFound {
			return nil, errSecretMissing
		}
		return nil, err
	}
	return map[string]string{"accessKey": strings.TrimSpace(secret)}, nil
}

func (e *Engine) rotateGatewayKey(raw json.RawMessage) (map[string]any, error) {
	return e.rotateGatewayKeyByID(raw)
}

func (e *Engine) revealGatewayKey() (map[string]string, error) {
	return e.revealGatewayKeyByID(nil)
}
