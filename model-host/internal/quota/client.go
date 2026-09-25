package quota

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

type QuotaWindow struct {
	Name             string   `json:"name"`
	Group            string   `json:"group,omitempty"`
	RemainingPercent *float64 `json:"remainingPercent"`
	ResetTime        string   `json:"resetTime,omitempty"`
	Description      string   `json:"description,omitempty"`
	Models           []string `json:"models,omitempty"`
}

type QuotaResult struct {
	Provider     string        `json:"provider"`
	Name         string        `json:"name"`
	Account      string        `json:"account,omitempty"`
	Status       string        `json:"status"` // "success" | "error"
	Error        string        `json:"error,omitempty"`
	Plan         string        `json:"plan,omitempty"`
	ResetCredits *int          `json:"resetCredits,omitempty"`
	Windows      []QuotaWindow `json:"windows"`
	FetchedAt    int64         `json:"fetchedAt"`
}

type Client struct {
	httpClient *http.Client
}

func NewClient() *Client {
	return &Client{
		httpClient: &http.Client{Timeout: 15 * time.Second},
	}
}

func (c *Client) Query(ctx context.Context, provider, name, token, extraJSON string) (*QuotaResult, error) {
	cleanProvider := strings.ToLower(provider)
	res := &QuotaResult{
		Provider:  cleanProvider,
		Name:      name,
		Status:    "success",
		FetchedAt: time.Now().UnixMilli(),
		Windows:   []QuotaWindow{},
	}

	switch cleanProvider {
	case "codex", "chatgpt":
		err := c.queryCodex(ctx, token, res)
		if err != nil {
			res.Status = "error"
			res.Error = err.Error()
		}
	case "claude", "anthropic":
		err := c.queryClaude(ctx, token, res)
		if err != nil {
			res.Status = "error"
			res.Error = err.Error()
		}
	case "antigravity":
		err := c.queryAntigravity(ctx, token, extraJSON, res)
		if err != nil {
			res.Status = "error"
			res.Error = err.Error()
		}
	case "xai", "grok":
		err := c.queryXAI(ctx, token, res)
		if err != nil {
			res.Status = "error"
			res.Error = err.Error()
		}
	case "kimi":
		err := c.queryKimi(ctx, token, res)
		if err != nil {
			res.Status = "error"
			res.Error = err.Error()
		}
	default:
		res.Status = "error"
		res.Error = fmt.Sprintf("unsupported quota provider: %s", provider)
	}

	return res, nil
}

func (c *Client) queryCodex(ctx context.Context, token string, res *QuotaResult) error {
	req, err := http.NewRequestWithContext(ctx, "GET", "https://chatgpt.com/backend-api/wham/usage", nil)
	if err != nil {
		return err
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("User-Agent", "codex_cli_rs/0.76.0 (Debian 13.0.0; x86_64) WindowsTerminal")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode >= 400 {
		bodyBytes, _ := io.ReadAll(io.LimitReader(resp.Body, 64*1024))
		return quotaHTTPError(resp.StatusCode, bodyBytes)
	}

	var data struct {
		PlanType        string         `json:"plan_type"`
		PrimaryWindow   map[string]any `json:"primary_window"`
		SecondaryWindow map[string]any `json:"secondary_window"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&data); err != nil {
		return err
	}

	res.Plan = data.PlanType
	if data.PrimaryWindow != nil {
		p := parseWindow(data.PrimaryWindow, "5 小时额度 (5-Hour Limit)", []string{"GPT-4o", "o1", "o3"})
		res.Windows = append(res.Windows, p)
	}
	if data.SecondaryWindow != nil {
		p := parseWindow(data.SecondaryWindow, "周额度 (Weekly Limit)", []string{"GPT-4o", "o1", "o3"})
		res.Windows = append(res.Windows, p)
	}
	return nil
}

func (c *Client) queryClaude(ctx context.Context, token string, res *QuotaResult) error {
	req, err := http.NewRequestWithContext(ctx, "GET", "https://api.anthropic.com/api/oauth/usage", nil)
	if err != nil {
		return err
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("anthropic-beta", "oauth-2025-04-20")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode >= 400 {
		bodyBytes, _ := io.ReadAll(io.LimitReader(resp.Body, 64*1024))
		return quotaHTTPError(resp.StatusCode, bodyBytes)
	}

	var data struct {
		FiveHour map[string]any `json:"five_hour"`
		SevenDay map[string]any `json:"seven_day"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&data); err != nil {
		return err
	}

	if data.FiveHour != nil {
		p := parseWindow(data.FiveHour, "5 小时额度 (Five Hour Limit)", []string{"Claude 3.7 Sonnet", "Claude 3.5 Haiku"})
		res.Windows = append(res.Windows, p)
	}
	if data.SevenDay != nil {
		p := parseWindow(data.SevenDay, "周额度 (Weekly Limit)", []string{"Claude 3.7 Sonnet", "Claude 3.5 Haiku"})
		res.Windows = append(res.Windows, p)
	}
	return nil
}

// antigravityQuotaEndpoints 按优先级排列：daily 是 canary 端点但对部分账号
// 返回 403 VALIDATION_REQUIRED（账号验证门禁，浏览器验证也未必解除），
// 生产端点 cloudcode-pa 对同一 token 通常直接可用（借鉴 EasyCLIProxyAPI 的
// 三端点回退链）。
var antigravityQuotaEndpoints = []string{
	"https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
	"https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:retrieveUserQuotaSummary",
	"https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
}

func (c *Client) queryAntigravity(ctx context.Context, token, extraJSON string, res *QuotaResult) error {
	projectID := ""
	if extraJSON != "" {
		var m map[string]any
		_ = json.Unmarshal([]byte(extraJSON), &m)
		if p, ok := m["project_id"].(string); ok {
			projectID = p
		}
	}
	bodyData, _ := json.Marshal(map[string]any{"project": projectID})

	var lastErr error = errors.New("无可用额度端点")
	for _, endpoint := range antigravityQuotaEndpoints {
		req, reqErr := http.NewRequestWithContext(ctx, "POST", endpoint, bytes.NewReader(bodyData))
		if reqErr != nil {
			return reqErr
		}
		req.Header.Set("Authorization", "Bearer "+token)
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("User-Agent", "antigravity/cli/1.0.13 (aidev_client; os_type=darwin; arch=arm64)")

		resp, err := c.httpClient.Do(req)
		if err != nil {
			lastErr = err
			continue
		}
		bodyBytes, _ := io.ReadAll(io.LimitReader(resp.Body, 256*1024))
		resp.Body.Close()

		if resp.StatusCode >= 400 {
			lastErr = quotaHTTPError(resp.StatusCode, bodyBytes)
			continue // 验证门禁等错误：回退下一个端点
		}

		var data struct {
			Buckets []struct {
				RemainingFraction      *float64 `json:"remaining_fraction"`
				RemainingFractionCamel *float64 `json:"remainingFraction"`
				ResetTime              string   `json:"reset_time"`
				ResetTimeCamel         string   `json:"resetTime"`
				DisplayName            string   `json:"display_name"`
				DisplayNameCamel       string   `json:"displayName"`
				ModelGroup             string   `json:"model_group"`
			} `json:"buckets"`
			Groups []struct {
				Name        string `json:"name"`
				DisplayName string `json:"displayName"`
				Buckets     []struct {
					RemainingFraction      *float64 `json:"remaining_fraction"`
					RemainingFractionCamel *float64 `json:"remainingFraction"`
					ResetTime              string   `json:"reset_time"`
					ResetTimeCamel         string   `json:"resetTime"`
					DisplayName            string   `json:"display_name"`
					DisplayNameCamel       string   `json:"displayName"`
				} `json:"buckets"`
			} `json:"groups"`
		}
		if err := json.Unmarshal(bodyBytes, &data); err != nil {
			lastErr = errors.New("Antigravity 返回了无法识别的额度数据")
			continue
		}

		// Parse buckets from root or groups；模型组（Gemini / Claude 与 GPT）
		// 各自拥有独立的周/5小时窗口，必须保留 Group 维度，不得折叠
		addBucket := func(name string, group string, frac *float64, reset string) {
			var rem *float64
			if frac != nil {
				val := *frac * 100.0
				if val < 0 {
					val = 0
				}
				if val > 100 {
					val = 100
				}
				rem = &val
			}
			res.Windows = append(res.Windows, QuotaWindow{
				Name:             name,
				Group:            group,
				RemainingPercent: rem,
				ResetTime:        reset,
			})
		}

		for _, b := range data.Buckets {
			name := firstNonEmpty(b.DisplayName, b.DisplayNameCamel)
			if name == "" {
				name = "Gemini Models"
			}
			addBucket(name, "", firstPercent(b.RemainingFraction, b.RemainingFractionCamel), firstNonEmpty(b.ResetTime, b.ResetTimeCamel))
		}
		for _, g := range data.Groups {
			group := firstNonEmpty(g.DisplayName, g.Name)
			for _, b := range g.Buckets {
				name := firstNonEmpty(b.DisplayName, b.DisplayNameCamel)
				if name == "" {
					name = group
				}
				addBucket(name, group, firstPercent(b.RemainingFraction, b.RemainingFractionCamel), firstNonEmpty(b.ResetTime, b.ResetTimeCamel))
			}
		}

		if len(res.Windows) == 0 {
			lastErr = errors.New("Antigravity 返回成功，但没有可识别的额度分组")
			continue
		}
		return nil
	}
	return lastErr
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		if value != "" {
			return value
		}
	}
	return ""
}

// quotaHTTPError 把上游 4xx/5xx 转成可操作的信息。Google 系端点的
// VALIDATION_REQUIRED 类错误会附带 validation_url（用户在浏览器完成验证后
// 即可恢复），必须透传给 UI，而不是只报一个 "HTTP 403"。
func quotaHTTPError(status int, body []byte) error {
	var payload struct {
		Error struct {
			Message string `json:"message"`
			Status  string `json:"status"`
			Details []struct {
				Reason   string            `json:"reason"`
				Metadata map[string]string `json:"metadata"`
			} `json:"details"`
		} `json:"error"`
	}
	if err := json.Unmarshal(body, &payload); err == nil && payload.Error.Message != "" {
		message := strings.TrimSpace(payload.Error.Message)
		reason := strings.TrimSpace(payload.Error.Status)
		for _, detail := range payload.Error.Details {
			if validationURL := strings.TrimSpace(detail.Metadata["validation_url"]); validationURL != "" {
				return fmt.Errorf("HTTP %d %s: %s（请在浏览器打开并完成验证后重试：%s）",
					status, reason, message, validationURL)
			}
			if detail.Reason != "" {
				reason = detail.Reason
			}
		}
		return fmt.Errorf("HTTP %d %s: %s", status, reason, message)
	}
	// 非 JSON 错误体：保留一段原文便于定位
	snippet := strings.TrimSpace(string(body))
	if snippet != "" {
		if len(snippet) > 200 {
			snippet = snippet[:200]
		}
		return fmt.Errorf("HTTP %d: %s", status, snippet)
	}
	return fmt.Errorf("HTTP %d", status)
}

func firstPercent(values ...*float64) *float64 {
	for _, value := range values {
		if value != nil {
			return value
		}
	}
	return nil
}

func (c *Client) queryXAI(ctx context.Context, token string, res *QuotaResult) error {
	req, err := http.NewRequestWithContext(ctx, "GET", "https://cli-chat-proxy.grok.com/v1/billing?format=credits", nil)
	if err != nil {
		return err
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("x-xai-token-auth", "xai-grok-cli")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode >= 400 {
		bodyBytes, _ := io.ReadAll(io.LimitReader(resp.Body, 64*1024))
		return quotaHTTPError(resp.StatusCode, bodyBytes)
	}

	rem := 100.0
	res.Windows = append(res.Windows, QuotaWindow{
		Name:             "Grok Weekly Credits",
		RemainingPercent: &rem,
		Models:           []string{"grok-2", "grok-3"},
	})
	return nil
}

func (c *Client) queryKimi(ctx context.Context, token string, res *QuotaResult) error {
	req, err := http.NewRequestWithContext(ctx, "GET", "https://api.kimi.com/coding/v1/usages", nil)
	if err != nil {
		return err
	}
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode >= 400 {
		bodyBytes, _ := io.ReadAll(io.LimitReader(resp.Body, 64*1024))
		return quotaHTTPError(resp.StatusCode, bodyBytes)
	}

	rem := 100.0
	res.Windows = append(res.Windows, QuotaWindow{
		Name:             "Kimi Daily Quota",
		RemainingPercent: &rem,
		Models:           []string{"moonshot-v1-auto"},
	})
	return nil
}

func parseWindow(raw map[string]any, defaultName string, models []string) QuotaWindow {
	var rem *float64
	if used, ok := raw["used_percent"].(float64); ok {
		val := 100.0 - used
		if val < 0 {
			val = 0
		}
		if val > 100 {
			val = 100
		}
		rem = &val
	} else if r, ok := raw["remaining_percent"].(float64); ok {
		rem = &r
	}

	reset := ""
	for _, key := range []string{"reset_at", "resetAt"} {
		switch value := raw[key].(type) {
		case string:
			reset = value
		case float64:
			reset = time.Unix(int64(value), 0).UTC().Format(time.RFC3339)
		}
		if reset != "" {
			break
		}
	}
	if reset == "" {
		for _, key := range []string{"reset_after_seconds", "resetAfterSeconds"} {
			if seconds, ok := raw[key].(float64); ok && seconds >= 0 {
				reset = time.Now().Add(time.Duration(seconds) * time.Second).UTC().Format(time.RFC3339)
				break
			}
		}
	}

	return QuotaWindow{
		Name:             defaultName,
		RemainingPercent: rem,
		ResetTime:        reset,
		Models:           models,
	}
}
