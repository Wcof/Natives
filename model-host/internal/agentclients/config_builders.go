package agentclients

import (
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"

	"github.com/pelletier/go-toml/v2"
	"gopkg.in/yaml.v3"
)

// BuildChanges renders the per-client config writes pointing the client at the
// local gateway. existingPreserved keeps unrelated user keys intact.
func BuildChanges(clientID, home, baseURL, apiKey, model string, mappings *ClaudeMappings) ([]Change, string, error) {
	base := strings.TrimRight(baseURL, "/")
	switch clientID {
	case "claude-code":
		return buildClaudeCode(home, base, apiKey, model, mappings)
	case "zcode":
		return buildZCode(home, base, apiKey, model)
	case "codex":
		return buildCodex(home, base, apiKey, model)
	case "opencode":
		return buildOpenCode(home, base, apiKey, model)
	case "openclaw":
		return buildOpenClaw(home, base, apiKey, model)
	case "kimi-code":
		return buildKimiCode(home, base, apiKey, model)
	case "grok-build":
		return buildGrokBuild(home, base, apiKey, model)
	case "deepseek-harness":
		return buildDeepSeekHarness(home, base, apiKey, model)
	case "hermes":
		return buildHermes(home, base, apiKey, model)
	case "pi":
		return buildPi(home, base, apiKey, model)
	case "claude-desktop":
		return buildClaudeDesktop(home, base, apiKey, model)
	default:
		return nil, "", fmt.Errorf("暂不支持为 %s 生成配置", clientID)
	}
}

func buildClaudeCode(home, base, apiKey, model string, mappings *ClaudeMappings) ([]Change, string, error) {
	path := filepath.Join(home, ".claude", "settings.json")
	document, err := decodeJSONFile(path)
	if err != nil {
		return nil, "", err
	}
	if mappings == nil {
		mappings = &ClaudeMappings{}
	}
	mappings.normalize()
	document["model"] = mappings.with1M(mappings.Sonnet, mappings.Sonnet1M)
	env := nestedMap(document, "env")
	env["ANTHROPIC_BASE_URL"] = base
	env["ANTHROPIC_AUTH_TOKEN"] = apiKey
	delete(env, "ANTHROPIC_API_KEY")
	env["ANTHROPIC_MODEL"] = model
	env["ANTHROPIC_DEFAULT_SONNET_MODEL"] = mappings.with1M(mappings.Sonnet, mappings.Sonnet1M)
	env["ANTHROPIC_DEFAULT_HAIKU_MODEL"] = mappings.with1M(mappings.Haiku, mappings.Haiku1M)
	env["ANTHROPIC_DEFAULT_OPUS_MODEL"] = mappings.with1M(mappings.Opus, mappings.Opus1M)
	env["CLAUDE_CODE_SUBAGENT_MODEL"] = mappings.Haiku
	env["CLAUDE_CODE_MAX_CONTEXT_TOKENS"] = strconv.Itoa(mappings.MaxContextTokens)
	env["CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY"] = "1"
	env["CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"] = strconv.Itoa(mappings.AutoCompactPct)
	if mappings.DisableAutoCompact {
		env["DISABLE_AUTO_COMPACT"] = "1"
	} else {
		delete(env, "DISABLE_AUTO_COMPACT")
	}
	content, err := marshalJSONIndent(document)
	if err != nil {
		return nil, "", err
	}
	return []Change{{Path: path, Content: content}}, path, nil
}

func buildZCode(home, base, apiKey, model string) ([]Change, string, error) {
	targets := []struct{ path, modelKey string }{
		{filepath.Join(home, ".zcode", "v2", "config.json"), "model"},
		{filepath.Join(home, ".zcode", "cli", "config.json"), "model.main"},
	}
	var changes []Change
	var primary string
	for _, target := range targets {
		document, err := decodeJSONFile(target.path)
		if err != nil {
			return nil, "", err
		}
		providers := nestedMap(document, "provider")
		providers[ProviderID] = map[string]any{
			"enabled":     true,
			"name":        ProviderName,
			"source":      "custom",
			"kind":        "anthropic",
			"defaultKind": "anthropic",
			"apiFormat":   "anthropic-messages",
			"npm":         "@ai-sdk/anthropic",
			"options":     map[string]any{"baseURL": base, "apiKey": apiKey},
			"models":      map[string]any{model: map[string]any{"name": model}},
		}
		managed := ProviderID + "/" + model
		if target.modelKey == "model" {
			document["model"] = managed
		} else {
			nestedMap(document, "model")["main"] = managed
		}
		content, err := marshalJSONIndent(document)
		if err != nil {
			return nil, "", err
		}
		changes = append(changes, Change{Path: target.path, Content: content})
		if primary == "" {
			primary = target.path
		}
	}
	return changes, primary, nil
}

func buildCodex(home, base, apiKey, model string) ([]Change, string, error) {
	configPath := filepath.Join(home, ".codex", "config.toml")
	document := map[string]any{}
	if data, err := os.ReadFile(configPath); err == nil && len(data) > 0 {
		if err := toml.Unmarshal(data, &document); err != nil {
			return nil, "", fmt.Errorf("config.toml 解析失败: %w", err)
		}
	} else if err != nil && !os.IsNotExist(err) {
		return nil, "", err
	}
	document["model_provider"] = ProviderID
	document["model"] = model
	document["model_providers"] = map[string]any{
		ProviderID: map[string]any{
			"name":                      ProviderName,
			"base_url":                  base + "/v1",
			"wire_api":                  "responses",
			"experimental_bearer_token": apiKey,
		},
	}
	configContent, err := toml.Marshal(document)
	if err != nil {
		return nil, "", err
	}
	authPath := filepath.Join(home, ".codex", "auth.json")
	authContent, err := marshalJSONIndent(map[string]any{"auth_mode": "apikey", "OPENAI_API_KEY": apiKey})
	if err != nil {
		return nil, "", err
	}
	return []Change{
		{Path: configPath, Content: configContent},
		{Path: authPath, Content: authContent},
	}, configPath, nil
}

func buildOpenCode(home, base, apiKey, model string) ([]Change, string, error) {
	path := filepath.Join(envOr("XDG_CONFIG_HOME", filepath.Join(home, ".config")), "opencode", "opencode.json")
	document, err := decodeJSONFile(path)
	if err != nil {
		return nil, "", err
	}
	nestedMap(document, "provider")[ProviderID] = map[string]any{
		"npm":     "@ai-sdk/openai-compatible",
		"name":    ProviderName,
		"options": map[string]any{"baseURL": base + "/v1", "apiKey": apiKey},
		"models":  map[string]any{model: map[string]any{"name": model}},
	}
	document["model"] = ProviderID + "/" + model
	content, err := marshalJSONIndent(document)
	if err != nil {
		return nil, "", err
	}
	return []Change{{Path: path, Content: content}}, path, nil
}

func buildOpenClaw(home, base, apiKey, model string) ([]Change, string, error) {
	path := filepath.Join(home, ".openclaw", "openclaw.json")
	document, err := decodeJSONFile(path)
	if err != nil {
		return nil, "", err
	}
	models := nestedMap(document, "models")
	models["mode"] = "merge"
	models["providers"] = map[string]any{
		ProviderID: map[string]any{
			"baseUrl": base + "/v1",
			"apiKey":  apiKey,
			"api":     "openai-completions",
			"models":  []map[string]any{{"id": model, "name": model}},
		},
	}
	agents := nestedMap(document, "agents", "defaults", "model")
	agents["primary"] = ProviderID + "/" + model
	content, err := marshalJSONIndent(document)
	if err != nil {
		return nil, "", err
	}
	return []Change{{Path: path, Content: content}}, path, nil
}

func buildKimiCode(home, base, apiKey, model string) ([]Change, string, error) {
	path := filepath.Join(envOr("KIMI_CODE_HOME", filepath.Join(home, ".kimi-code")), "config.toml")
	document := map[string]any{}
	if data, err := os.ReadFile(path); err == nil && len(data) > 0 {
		if err := toml.Unmarshal(data, &document); err != nil {
			return nil, "", fmt.Errorf("config.toml 解析失败: %w", err)
		}
	} else if err != nil && !os.IsNotExist(err) {
		return nil, "", err
	}
	managed := ProviderID + "/" + model
	document["default_model"] = managed
	document["providers"] = map[string]any{
		ProviderID: map[string]any{"type": "openai", "base_url": base + "/v1", "api_key": apiKey},
	}
	models, _ := document["models"].(map[string]any)
	if models == nil {
		models = map[string]any{}
	}
	for key := range models {
		if strings.HasPrefix(key, ProviderID+"/") {
			delete(models, key)
		}
	}
	models[managed] = map[string]any{
		"provider":         ProviderID,
		"model":            model,
		"display_name":     model,
		"max_context_size": 200000,
		"capabilities":     []string{"tool_use"},
	}
	document["models"] = models
	content, err := toml.Marshal(document)
	if err != nil {
		return nil, "", err
	}
	return []Change{{Path: path, Content: content}}, path, nil
}

func buildGrokBuild(home, base, apiKey, model string) ([]Change, string, error) {
	path := filepath.Join(home, ".grok", "config.toml")
	document := map[string]any{}
	if data, err := os.ReadFile(path); err == nil && len(data) > 0 {
		if err := toml.Unmarshal(data, &document); err != nil {
			return nil, "", fmt.Errorf("config.toml 解析失败: %w", err)
		}
	} else if err != nil && !os.IsNotExist(err) {
		return nil, "", err
	}
	managed := ProviderID + "/" + model
	nestedMap(document, "models")["default"] = managed
	nestedMap(document, "model")[managed] = map[string]any{
		"model":          model,
		"base_url":       base + "/v1",
		"name":           model,
		"api_key":        apiKey,
		"api_backend":    "chat_completions",
		"context_window": 200000,
	}
	content, err := toml.Marshal(document)
	if err != nil {
		return nil, "", err
	}
	return []Change{{Path: path, Content: content}}, path, nil
}

func buildDeepSeekHarness(home, base, apiKey, model string) ([]Change, string, error) {
	baseDir := envOr("DSH_HOME", filepath.Join(home, ".dsh"))
	settingsPath := filepath.Join(baseDir, "settings.yaml")
	document := map[string]any{}
	if data, err := os.ReadFile(settingsPath); err == nil && len(data) > 0 {
		if err := yaml.Unmarshal(data, &document); err != nil {
			return nil, "", fmt.Errorf("settings.yaml 解析失败: %w", err)
		}
	} else if err != nil && !os.IsNotExist(err) {
		return nil, "", err
	}
	providers := nestedMap(document, "llm-pi-ai", "providers")
	providers["easy-cliproxyapi"] = map[string]any{
		"displayName": ProviderName,
		"apiKeyEnv":   "EASYCLIPROXYAPI_API_KEY",
		"api":         "openai-completions",
		"baseURL":     base + "/v1",
		"models":      []map[string]any{{"id": model, "name": model}},
	}
	nestedMap(document, "agent-default-model")["provider"] = "easy-cliproxyapi"
	nestedMap(document, "agent-default-model")["model"] = model
	settingsContent, err := yaml.Marshal(document)
	if err != nil {
		return nil, "", err
	}
	credentialsPath := filepath.Join(baseDir, ".credentials.yaml")
	credentialsContent := []byte(fmt.Sprintf("EASYCLIPROXYAPI_API_KEY: %s\n", apiKey))
	return []Change{
		{Path: settingsPath, Content: settingsContent},
		{Path: credentialsPath, Content: credentialsContent, Mode: 0o600},
	}, settingsPath, nil
}

func buildHermes(home, base, apiKey, model string) ([]Change, string, error) {
	path := filepath.Join(envOr("HERMES_HOME", filepath.Join(home, ".hermes")), "config.yaml")
	document := map[string]any{}
	if data, err := os.ReadFile(path); err == nil && len(data) > 0 {
		if err := yaml.Unmarshal(data, &document); err != nil {
			return nil, "", fmt.Errorf("config.yaml 解析失败: %w", err)
		}
	} else if err != nil && !os.IsNotExist(err) {
		return nil, "", err
	}
	providers, _ := document["custom_providers"].([]any)
	replaced := false
	for index, raw := range providers {
		entry, ok := raw.(map[string]any)
		if !ok || entry["name"] != ProviderID {
			continue
		}
		providers[index] = map[string]any{
			"name": ProviderID, "base_url": base + "/v1", "api_key": apiKey,
			"api_mode": "chat_completions", "model": model,
			"models": map[string]any{model: map[string]any{}},
		}
		replaced = true
	}
	if !replaced {
		document["custom_providers"] = append(providers, map[string]any{
			"name": ProviderID, "base_url": base + "/v1", "api_key": apiKey,
			"api_mode": "chat_completions", "model": model,
			"models": map[string]any{model: map[string]any{}},
		})
	}
	document["model"] = map[string]any{"default": model, "provider": ProviderID}
	content, err := yaml.Marshal(document)
	if err != nil {
		return nil, "", err
	}
	return []Change{{Path: path, Content: content}}, path, nil
}

func buildPi(home, base, apiKey, model string) ([]Change, string, error) {
	agentDir := envOr("PI_CODING_AGENT_DIR", filepath.Join(home, ".pi", "agent"))
	providerPath := filepath.Join(agentDir, "cliproxyapi.json")
	providerContent, err := marshalJSONIndent(map[string]any{"baseUrl": base, "apiKey": apiKey})
	if err != nil {
		return nil, "", err
	}
	settingsPath := filepath.Join(agentDir, "settings.json")
	document, err := decodeJSONFile(settingsPath)
	if err != nil {
		return nil, "", err
	}
	document["defaultProvider"] = "cliproxyapi"
	document["defaultModel"] = model
	settingsContent, err := marshalJSONIndent(document)
	if err != nil {
		return nil, "", err
	}
	return []Change{
		{Path: providerPath, Content: providerContent},
		{Path: settingsPath, Content: settingsContent},
	}, settingsPath, nil
}

func buildClaudeDesktop(home, base, apiKey, model string) ([]Change, string, error) {
	path := filepath.Join(claudeDesktopDir(home), "claude_desktop_config.json")
	document, err := decodeJSONFile(path)
	if err != nil {
		return nil, "", err
	}
	document["deploymentMode"] = "3p"
	document["inferenceGatewayApiKey"] = apiKey
	document["inferenceGatewayAuthScheme"] = "bearer"
	document["inferenceGatewayBaseUrl"] = base
	content, err := marshalJSONIndent(document)
	if err != nil {
		return nil, "", err
	}
	return []Change{{Path: path, Content: content}}, path, nil
}
