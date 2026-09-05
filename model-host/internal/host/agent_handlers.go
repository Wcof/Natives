package host

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"strings"

	"github.com/ldh/natives/model-host/internal/agentclients"
)

const fallbackGatewayAPIKey = "123456"

func homeDir() string {
	home, err := os.UserHomeDir()
	if err != nil {
		return "."
	}
	return home
}

type agentClientParams struct {
	Client string `json:"client"`
	Model  string `json:"model"`
	Target string `json:"target"`
}

// agentGatewayContext resolves the local gateway origin and effective API key
// the way the reference GUI does: first configured access key, else "123456".
func (e *Engine) agentGatewayContext() (baseURL, apiKey string, err error) {
	snapshot, err := e.repo.Load()
	if err != nil {
		return "", "", err
	}
	if snapshot.Gateway.State != "running" || snapshot.Gateway.BaseURL == "" {
		return "", "", invalid("本地代理未运行，请先在「本地代理」页启动后再管理智能体配置")
	}
	baseURL = strings.TrimRight(snapshot.Gateway.BaseURL, "/")
	apiKey = fallbackGatewayAPIKey
	for _, key := range snapshot.Gateway.AccessKeys {
		if !key.Enabled || key.SecretRef == "" {
			continue
		}
		if secret, err := e.secrets.Get(key.SecretRef); err == nil && strings.TrimSpace(secret) != "" {
			apiKey = strings.TrimSpace(secret)
			break
		}
	}
	return baseURL, apiKey, nil
}

func (e *Engine) listAgentClients() (any, error) {
	return map[string]any{"clients": agentclients.DetectAll(4)}, nil
}

func (e *Engine) agentClientModels(ctx context.Context, raw json.RawMessage) (any, error) {
	var params agentClientParams
	_ = json.Unmarshal(raw, &params)
	baseURL, apiKey, err := e.agentGatewayContext()
	if err != nil {
		return nil, err
	}
	models, err := agentclients.FetchModels(ctx, baseURL, apiKey)
	if err != nil {
		return nil, err
	}
	return map[string]any{"models": models}, nil
}

func (e *Engine) applyAgentClientConfig(raw json.RawMessage, mode string) (any, error) {
	var params agentClientParams
	if err := json.Unmarshal(raw, &params); err != nil || strings.TrimSpace(params.Client) == "" {
		return nil, invalid("参数无效: client 不能为空")
	}
	definition, ok := agentclients.DefinitionByID(params.Client)
	if !ok {
		return nil, notFound()
	}
	home := homeDir()
	primary := definition.PrimaryPath(home)

	var message string
	switch mode {
	case "apply", "default":
		if strings.TrimSpace(params.Model) == "" {
			return nil, invalid("参数无效: model 不能为空")
		}
		baseURL, apiKey, err := e.agentGatewayContext()
		if err != nil {
			return nil, err
		}
		changes, primaryPath, err := agentclients.BuildChanges(params.Client, home, baseURL, apiKey, params.Model)
		if err != nil {
			return nil, err
		}
		if primaryPath != "" {
			primary = primaryPath
		}
		if err := agentclients.CommitTransaction(params.Client, primary, params.Model, changes); err != nil {
			return nil, err
		}
		if mode == "default" {
			message = fmt.Sprintf("已为 %s 写入默认网关配置", definition.Name)
		} else {
			message = fmt.Sprintf("已应用配置修改：%s 将使用模型 %s", definition.Name, params.Model)
		}
	case "close":
		var err error
		message, err = agentclients.CloseModification(primary)
		if err != nil {
			return nil, err
		}
	default:
		return nil, invalid("未支持的配置操作")
	}
	return map[string]any{
		"outcome": mode,
		"client":  params.Client,
		"message": message,
	}, nil
}

func (e *Engine) launchAgentClient(raw json.RawMessage) (any, error) {
	var params agentClientParams
	if err := json.Unmarshal(raw, &params); err != nil || strings.TrimSpace(params.Client) == "" {
		return nil, invalid("参数无效: client 不能为空")
	}
	definition, ok := agentclients.DefinitionByID(params.Client)
	if !ok {
		return nil, notFound()
	}
	if err := agentclients.Launch(definition, params.Target); err != nil {
		return nil, err
	}
	return map[string]any{"launched": params.Client, "target": params.Target}, nil
}
