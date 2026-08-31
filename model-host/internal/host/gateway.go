package host

import (
	"context"
	"encoding/json"
	"fmt"
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

func (e *Engine) startGatewayWithState(ctx context.Context, expectedRevision *int64, state string) (domain.Snapshot, error) {
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
	port, err := e.runtime.Start(ctx, starting, e.secrets, e.authStore, secret, e.runtimeConfigPath)
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
		snapshot.Gateway.PreferredPort = port
		snapshot.Gateway.BaseURL = fmt.Sprintf("http://127.0.0.1:%d", port)
		snapshot.Gateway.ErrorCode = ""
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

func (e *Engine) rotateGatewayKey(raw json.RawMessage) (map[string]any, error) {
	e.gatewayMu.Lock()
	defer e.gatewayMu.Unlock()
	e.secretMu.Lock()
	defer e.secretMu.Unlock()
	expectedRevision, err := expected(raw)
	if err != nil {
		return nil, err
	}
	previous, previousErr := e.secrets.Get(gatewaySecretRef)
	if previousErr != nil && previousErr != secrets.ErrNotFound {
		return nil, previousErr
	}
	secret, err := randomSecret()
	if err != nil {
		return nil, err
	}
	if err = e.secrets.Set(gatewaySecretRef, secret); err != nil {
		return nil, err
	}
	snapshot, err := e.repo.Update(expectedRevision, func(snapshot *domain.Snapshot) error {
		snapshot.Gateway.AccessKeyMask = maskSecret(secret)
		if snapshot.Gateway.State == "running" {
			snapshot.Gateway.State = "restarting"
		}
		return nil
	})
	if err != nil {
		if previousErr == nil {
			_ = e.secrets.Set(gatewaySecretRef, previous)
		} else {
			_ = e.secrets.Delete(gatewaySecretRef)
		}
		return nil, err
	}
	if snapshot.Gateway.State == "restarting" {
		stopCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		stopErr := e.runtime.Stop(stopCtx)
		cancel()
		if stopErr == nil {
			var port int
			port, err = e.runtime.Start(context.Background(), snapshot, e.secrets, e.authStore, secret, e.runtimeConfigPath)
			if err == nil {
				snapshot, err = e.repo.Update(nil, func(current *domain.Snapshot) error {
					current.Gateway.State = "running"
					current.Gateway.Port = port
					current.Gateway.PreferredPort = port
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

func (e *Engine) reconfigureGateway(snapshot domain.Snapshot, mutationErr error) (domain.Snapshot, error) {
	if mutationErr != nil || snapshot.Gateway.State != "running" {
		return snapshot, mutationErr
	}
	e.gatewayMu.Lock()
	defer e.gatewayMu.Unlock()
	restarting, err := e.repo.Update(nil, func(current *domain.Snapshot) error {
		current.Gateway.State = "restarting"
		current.Gateway.ErrorCode = ""
		return nil
	})
	if err != nil {
		return domain.Snapshot{}, err
	}
	e.emitSnapshot("model_gateway_state_changed", restarting)
	key, err := e.secrets.Get(gatewaySecretRef)
	if err == nil {
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()
		err = e.runtime.Stop(ctx)
		if err == nil {
			var port int
			port, err = e.runtime.Start(ctx, restarting, e.secrets, e.authStore, key, e.runtimeConfigPath)
			if err == nil {
				snapshot, err = e.repo.Update(nil, func(current *domain.Snapshot) error {
					current.Gateway.State = "running"
					current.Gateway.Port = port
					current.Gateway.PreferredPort = port
					current.Gateway.BaseURL = fmt.Sprintf("http://127.0.0.1:%d", port)
					return nil
				})
			}
		}
	}
	if err != nil {
		failed, updateErr := e.repo.Update(nil, func(current *domain.Snapshot) error {
			current.Gateway.State = "failed"
			current.Gateway.ErrorCode = "gateway_restart_failed"
			return nil
		})
		if updateErr == nil {
			e.emitSnapshot("model_gateway_state_changed", failed)
		}
		return failed, &SafeError{Code: "gateway_restart_failed", Message: "配置已保存，但代理重启失败，请重试"}
	}
	e.emitSnapshot("model_gateway_state_changed", snapshot)
	return snapshot, nil
}

func (e *Engine) revealGatewayKey() (map[string]string, error) {
	secret, err := e.secrets.Get(gatewaySecretRef)
	if err != nil {
		if err == secrets.ErrNotFound {
			return nil, errSecretMissing
		}
		return nil, err
	}
	return map[string]string{"accessKey": strings.TrimSpace(secret)}, nil
}

func resetGateway(snapshot *domain.Snapshot) error {
	snapshot.Gateway.State = "stopped"
	snapshot.Gateway.Port = 0
	snapshot.Gateway.BaseURL = ""
	snapshot.Gateway.ErrorCode = ""
	return nil
}
