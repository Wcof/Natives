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
	Client   string          `json:"client"`
	Model    string          `json:"model"`
	Target   string          `json:"target"`
	WorkingDirectory string   `json:"workingDirectory"`
	Mappings *agentclients.ClaudeMappings `json:"claudeCodeModelMappings"`
}

type agentSessionParams struct {
	Paths []string `json:"paths"`
}

type agentPiParams struct {
	Action string `json:"action"`
	Model  string `json:"model"`
}

// agentGatewayContext resolves the local gateway origin and effective API key
// the way the reference GUI does: first configured access key, else "123456".
func (e *Engine) agentGatewayContext() (baseURL, apiKey string, err error) {
	snapshot, err := e.repo.Load()
	if err != nil {
		return "", "", err
	}
	port := snapshot.Gateway.Port
	if port == 0 {
		port = snapshot.Gateway.Settings.PreferredPort
	}
	if port == 0 {
		port = snapshot.Gateway.PreferredPort
	}
	if port == 0 {
		port = 8317
	}
	baseURL = fmt.Sprintf("http://127.0.0.1:%d", port)
	apiKey = ""
	for _, key := range snapshot.Gateway.AccessKeys {
		if !key.Enabled || key.SecretRef == "" {
			continue
		}
		if secret, err := e.secrets.Get(key.SecretRef); err == nil && strings.TrimSpace(secret) != "" {
			apiKey = strings.TrimSpace(secret)
			break
		}
	}
	if apiKey == "" {
		if secret, err := e.secrets.Get(gatewaySecretRef); err == nil && strings.TrimSpace(secret) != "" {
			apiKey = strings.TrimSpace(secret)
		}
	}
	if apiKey == "" {
		apiKey = fallbackGatewayAPIKey
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
	if err == nil {
		models, fetchErr := agentclients.FetchModels(ctx, baseURL, apiKey)
		if fetchErr == nil && len(models) > 0 {
			return map[string]any{"models": models}, nil
		}
	}
	// Fallback to client-recommended models so the picker is never blocked
	return map[string]any{"models": agentclients.FallbackModelsForClient(params.Client)}, nil
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
		targetModel := strings.TrimSpace(params.Model)
		if targetModel == "" {
			targetModel = agentclients.DefaultModelForClient(params.Client)
		}
		baseURL, apiKey, err := e.agentGatewayContext()
		if err != nil {
			return nil, err
		}
		changes, primaryPath, err := agentclients.BuildChanges(params.Client, home, baseURL, apiKey, targetModel, params.Mappings)
		if err != nil {
			return nil, err
		}
		if primaryPath != "" {
			primary = primaryPath
		}
		if err := agentclients.CommitTransaction(params.Client, primary, targetModel, changes, params.Mappings); err != nil {
			return nil, err
		}
		if mode == "default" {
			message = fmt.Sprintf("已为 %s 写入默认网关配置", definition.Name)
		} else {
			message = fmt.Sprintf("已应用配置修改：%s 将使用模型 %s", definition.Name, targetModel)
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
	if err := agentclients.Launch(definition, params.Target, params.WorkingDirectory); err != nil {
		return nil, err
	}
	return map[string]any{"launched": params.Client, "target": params.Target}, nil
}

func (e *Engine) agentClientExtras(ctx context.Context, raw json.RawMessage, method string) (any, error) {
	home := homeDir()
	switch method {
	case "model_agent_codex_auth_check":
		return map[string]any{"authMode": agentclients.CodexAuthMode(home)}, nil
	case "model_agent_codex_clear":
		deleted, err := agentclients.ClearCodexConfig(home)
		if err != nil {
			return nil, err
		}
		return map[string]any{"deleted": deleted}, nil
	case "model_agent_codex_sessions_list":
		sessions, err := agentclients.ListCodexSessions(home)
		if err != nil {
			return nil, err
		}
		return map[string]any{"sessions": sessions}, nil
	case "model_agent_codex_sessions_delete":
		var params agentSessionParams
		if err := json.Unmarshal(raw, &params); err != nil || len(params.Paths) == 0 {
			return nil, invalid("参数无效: paths 不能为空")
		}
		deleted, err := agentclients.DeleteCodexSessions(home, params.Paths)
		if err != nil {
			return nil, err
		}
		return map[string]any{"deleted": deleted}, nil
	case "model_agent_pi_status":
		return agentclients.DetectPiProvider(home), nil
	case "model_agent_pi_action":
		var params agentPiParams
		if err := json.Unmarshal(raw, &params); err != nil || strings.TrimSpace(params.Action) == "" {
			return nil, invalid("参数无效: action 不能为空")
		}
		baseURL, apiKey, err := e.agentGatewayContext()
		if err != nil {
			return nil, err
		}
		output, err := agentclients.PiProviderAction(ctx, home, baseURL, apiKey, params.Model, params.Action)
		if err != nil {
			return nil, err
		}
		return map[string]any{"output": output}, nil
	default:
		return nil, invalid("未支持的智能体操作")
	}
}
