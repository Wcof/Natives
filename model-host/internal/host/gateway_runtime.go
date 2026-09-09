package host

import (
	"context"
	"fmt"
	"net/url"
	"os"
	"strings"
	"time"

	"github.com/ldh/natives/model-host/internal/domain"
)

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
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	err = e.runtime.Stop(ctx)
	if err == nil {
		var port int
		port, err = e.runtime.Start(ctx, restarting, e.secrets, e.authStore, e.runtimeConfigPath)
		if err == nil {
			snapshot, err = e.repo.Update(nil, func(current *domain.Snapshot) error {
				current.Gateway.State = "running"
				current.Gateway.Port = port
				current.Gateway.PreferredPort = port
				current.Gateway.Settings.PreferredPort = port
				current.Gateway.BaseURL = fmt.Sprintf("http://127.0.0.1:%d", port)
				current.Gateway.PID = os.Getpid()
				current.Gateway.KernelVersion = getLocalKernelVersion()
				current.Gateway.LatestKernelVersion = getCachedLatestKernelVersion()
				current.Gateway.Version = "v0.2.25"
				return nil
			})
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

func enabledGatewayKeyCount(keys []domain.GatewayAccessKey) int {
	count := 0
	for _, key := range keys {
		if key.Enabled {
			count++
		}
	}
	return count
}

func validateGatewaySettings(settings domain.GatewaySettings) error {
	if settings.PreferredPort < 0 || settings.PreferredPort > 65535 {
		return invalid("代理监听端口必须在 0 到 65535 之间")
	}
	if settings.RoutingStrategy != "" && settings.RoutingStrategy != "round_robin" && settings.RoutingStrategy != "fill_first" {
		return invalid("路由策略无效")
	}
	if settings.SessionAffinityTTL < 0 || settings.SessionAffinityTTL > 7*24*60*60 {
		return invalid("会话粘性时长必须在 0 到 7 天之间")
	}
	for _, value := range []int{settings.RequestRetry, settings.MaxRetryCredentials, settings.StreamingBootstrapRetries} {
		if value < 0 || value > 100 {
			return invalid("重试参数必须在 0 到 100 之间")
		}
	}
	if settings.MaxRetryIntervalSeconds < 0 || settings.MaxRetryIntervalSeconds > 3600 {
		return invalid("最大重试等待必须在 0 到 3600 秒之间")
	}
	if strings.TrimSpace(settings.ProxyURL) == "" {
		return nil
	}
	parsed, err := url.Parse(settings.ProxyURL)
	if err != nil || parsed.Host == "" || (parsed.Scheme != "http" && parsed.Scheme != "https" && parsed.Scheme != "socks5") {
		return invalid("上游代理 URL 仅支持 HTTP、HTTPS 或 SOCKS5")
	}
	if parsed.User != nil {
		return invalid("代理凭证不能保存在 URL 中")
	}
	return nil
}

func resetGateway(snapshot *domain.Snapshot) error {
	snapshot.Gateway.State = "stopped"
	snapshot.Gateway.Port = 0
	snapshot.Gateway.BaseURL = ""
	snapshot.Gateway.ErrorCode = ""
	snapshot.Gateway.PID = 0
	return nil
}
