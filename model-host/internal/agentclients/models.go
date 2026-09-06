package agentclients

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

// ModelOption is one selectable model in the 使用模型 dropdown.
type ModelOption struct {
	Name          string `json:"name"`
	Alias         string `json:"alias,omitempty"`
	IsAlias       bool   `json:"isAlias,omitempty"`
	ContextWindow int64  `json:"contextWindow,omitempty"`
}

const modelsTimeout = 10 * time.Second

// FetchModels reads the gateway's OpenAI-compatible model catalog. The kernel
// serves GET /v1/models; parsing is tolerant per the reference GUI contract.
func FetchModels(ctx context.Context, baseURL, apiKey string) ([]ModelOption, error) {
	base := strings.TrimRight(baseURL, "/")
	url := base + "/v1/models"
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, err
	}
	request.Header.Set("Authorization", "Bearer "+apiKey)
	client := &http.Client{Timeout: modelsTimeout}
	response, err := client.Do(request)
	if err != nil {
		return nil, fmt.Errorf("无法连接本地代理: %w", err)
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 4<<20))
	if err != nil {
		return nil, err
	}
	if response.StatusCode >= 400 {
		return nil, fmt.Errorf("本地代理返回 HTTP %d", response.StatusCode)
	}
	return parseModelOptions(body)
}

func parseModelOptions(body []byte) ([]ModelOption, error) {
	var envelope struct {
		Data   []json.RawMessage `json:"data"`
		Models []json.RawMessage `json:"models"`
	}
	var flat []json.RawMessage
	if err := json.Unmarshal(body, &envelope); err == nil {
		flat = append(envelope.Data, envelope.Models...)
	} else if err := json.Unmarshal(body, &flat); err != nil {
		return nil, fmt.Errorf("模型列表响应无法解析")
	}
	if len(flat) == 0 {
		if err := json.Unmarshal(body, &flat); err != nil {
			return []ModelOption{}, nil
		}
	}
	options := make([]ModelOption, 0, len(flat))
	for _, raw := range flat {
		var item struct {
			ID           string      `json:"id"`
			Name         string      `json:"name"`
			Model        string      `json:"model"`
			DisplayName  string      `json:"display_name"`
			Alias        string      `json:"alias"`
			ContextValue json.Number `json:"context_length"`
			ContextWin   json.Number `json:"context_window"`
			MaxTokens    json.Number `json:"max_input_tokens"`
		}
		if err := json.Unmarshal(raw, &item); err != nil {
			continue
		}
		name := firstNonEmpty(item.ID, item.Name, item.Model)
		if name == "" {
			continue
		}
		option := ModelOption{Name: name, Alias: item.Alias, IsAlias: item.Alias != ""}
		display := firstNonEmpty(item.DisplayName)
		if display != "" && display != name {
			option.Alias = firstNonEmpty(option.Alias, display)
		}
		option.ContextWindow = firstNumber(item.ContextValue, item.ContextWin, item.MaxTokens)
		options = append(options, option)
	}
	return options, nil
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		if strings.TrimSpace(value) != "" {
			return strings.TrimSpace(value)
		}
	}
	return ""
}

func firstNumber(values ...json.Number) int64 {
	for _, value := range values {
		if value.String() == "" {
			continue
		}
		if parsed, err := value.Int64(); err == nil && parsed > 0 {
			return parsed
		}
	}
	return 0
}

func DefaultModelForClient(clientID string) string {
	switch clientID {
	case "claude-code", "claude-desktop":
		return "claude-3-5-sonnet-20241022"
	case "codex":
		return "gpt-4o"
	case "deepseek-harness":
		return "deepseek-chat"
	case "opencode", "openclaw":
		return "gpt-4o"
	case "kimi-code":
		return "kimi-k1.5"
	case "grok-build":
		return "grok-beta"
	case "hermes":
		return "hermes-3-llama-3.1-405b"
	case "pi", "zcode":
		return "claude-3-5-sonnet-20241022"
	default:
		return "gpt-4o"
	}
}

func FallbackModelsForClient(clientID string) []ModelOption {
	switch clientID {
	case "claude-code", "claude-desktop":
		return []ModelOption{
			{Name: "claude-3-5-sonnet-20241022", Alias: "Claude 3.5 Sonnet", ContextWindow: 200000},
			{Name: "claude-3-5-haiku-20241022", Alias: "Claude 3.5 Haiku", ContextWindow: 200000},
			{Name: "claude-3-opus-20240229", Alias: "Claude 3 Opus", ContextWindow: 200000},
		}
	case "codex":
		return []ModelOption{
			{Name: "gpt-4o", Alias: "GPT-4o", ContextWindow: 128000},
			{Name: "gpt-4o-mini", Alias: "GPT-4o Mini", ContextWindow: 128000},
			{Name: "o1", Alias: "OpenAI o1", ContextWindow: 128000},
			{Name: "o1-mini", Alias: "OpenAI o1 Mini", ContextWindow: 128000},
		}
	default:
		return []ModelOption{
			{Name: "gpt-4o", Alias: "GPT-4o", ContextWindow: 128000},
			{Name: "claude-3-5-sonnet-20241022", Alias: "Claude 3.5 Sonnet", ContextWindow: 200000},
			{Name: "deepseek-chat", Alias: "DeepSeek V3", ContextWindow: 64000},
		}
	}
}

