// Package inference calls a configured provider directly. Each wire protocol
// has its own request builder and stream/usage parser; there is no local
// self-proxy and no model alias routing.
package inference

import (
	"bytes"
	"context"
	"errors"
	"io"
	"net"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/netpolicy"
)

const (
	MaxResponseBytes = 64 * 1024
	MaxStreamBytes   = 1024 * 1024
	MaxLineBytes     = 64 * 1024
	MaxOutputRunes   = 8000

	FailureAuth       = "INFERENCE_AUTH"
	FailureQuota      = "INFERENCE_QUOTA"
	FailureRateLimit  = "INFERENCE_RATE_LIMIT"
	FailureModel      = "INFERENCE_MODEL"
	FailureUpstream   = "INFERENCE_UPSTREAM"
	FailureTimeout    = "INFERENCE_TIMEOUT"
	FailureCancelled  = "INFERENCE_CANCELLED"
	FailureConnection = "INFERENCE_CONNECTION"
	FailureEmpty      = "INFERENCE_EMPTY"
	FailureLength     = "INFERENCE_LENGTH"
	FailureTruncated  = "INFERENCE_TRUNCATED"
	FailureStream     = "INFERENCE_STREAM"
)

type Request struct {
	ProviderID   string
	ProviderName string
	Protocol     domain.Protocol
	BaseURL      string
	APIKey       string
	AllowLAN     bool
	ModelID      string
	SystemPrompt string
	Text         string
	MaxTokens    int
	OnPhase      func(string)
}

type Usage struct {
	InputTokens      int64
	OutputTokens     int64
	CacheReadTokens  int64
	CacheWriteTokens int64
	ReasoningTokens  int64
	TotalTokens      int64
}

type Result struct {
	Text        string
	Failure     string
	HTTPStatus  int
	Usage       *Usage
	ServiceTier string
}

func Run(ctx context.Context, request Request) Result {
	if request.MaxTokens <= 0 {
		request.MaxTokens = 1600
	}
	endpoint, body, headers, err := buildRequest(request)
	if err != nil {
		return Result{Failure: FailureUpstream}
	}
	parsed, err := url.Parse(endpoint)
	if err != nil || parsed.Hostname() == "" {
		return Result{Failure: FailureUpstream}
	}
	privateHosts := map[string]bool{}
	host := strings.ToLower(parsed.Hostname())
	allowPrivate := request.AllowLAN || host == "localhost"
	if ip := net.ParseIP(host); ip != nil && ip.IsLoopback() {
		allowPrivate = true
	}
	privateHosts[host] = allowPrivate
	client := &http.Client{
		Transport: &http.Transport{
			Proxy:                 nil,
			ForceAttemptHTTP2:     true,
			TLSHandshakeTimeout:   10 * time.Second,
			ResponseHeaderTimeout: 60 * time.Second,
			DialContext:           netpolicy.NewTransport(privateHosts).DialContext,
		},
		Timeout: 90 * time.Second,
		CheckRedirect: func(next *http.Request, _ []*http.Request) error {
			if !strings.EqualFold(next.URL.Scheme, parsed.Scheme) || !strings.EqualFold(next.URL.Host, parsed.Host) {
				return http.ErrUseLastResponse
			}
			return validateEndpoint(next.URL, request.AllowLAN)
		},
	}
	defer client.CloseIdleConnections()
	requestCtx, cancel := context.WithCancelCause(ctx)
	defer cancel(nil)
	req, err := http.NewRequestWithContext(requestCtx, http.MethodPost, endpoint, bytes.NewReader(body))
	if err != nil {
		return Result{Failure: FailureUpstream}
	}
	for name, value := range headers {
		req.Header.Set(name, value)
	}
	response, err := client.Do(req)
	if err != nil {
		return Result{Failure: transportFailure(requestCtx, ctx, err)}
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		_, _ = io.Copy(io.Discard, io.LimitReader(response.Body, 64*1024))
		return Result{Failure: statusFailure(response.StatusCode), HTTPStatus: response.StatusCode}
	}
	idle := time.AfterFunc(30*time.Second, func() { cancel(context.DeadlineExceeded) })
	defer idle.Stop()
	reader := activityReader{reader: response.Body, idle: idle}
	isStream := strings.Contains(strings.ToLower(response.Header.Get("Content-Type")), "text/event-stream")
	if isStream {
		parsedResult := parseStream(request.Protocol, reader, request.OnPhase)
		if len([]rune(parsedResult.Text)) > MaxOutputRunes {
			parsedResult.Text = clipText(parsedResult.Text)
			parsedResult.Failure = FailureLength
		}
		parsedResult.HTTPStatus = response.StatusCode
		// A cancellation/deadline surfaces as a read error inside the stream
		// parser; transport cause must win over that generic stream failure.
		if requestCtx.Err() != nil && (parsedResult.Failure == "" || parsedResult.Failure == FailureStream || parsedResult.Failure == FailureConnection) {
			parsedResult.Failure = transportFailure(requestCtx, ctx, requestCtx.Err())
		}
		return parsedResult
	}
	data, err := io.ReadAll(io.LimitReader(reader, MaxResponseBytes+1))
	if err != nil {
		return Result{Failure: transportFailure(requestCtx, ctx, err)}
	}
	if len(data) > MaxResponseBytes {
		return Result{Failure: FailureLength}
	}
	parsedResult := parseResponse(request.Protocol, data)
	parsedResult.HTTPStatus = response.StatusCode
	if len([]rune(parsedResult.Text)) > MaxOutputRunes {
		parsedResult.Text = clipText(parsedResult.Text)
		parsedResult.Failure = FailureLength
	}
	return parsedResult
}

func buildRequest(request Request) (string, []byte, map[string]string, error) {
	switch request.Protocol {
	case domain.ProtocolOpenAIChat:
		return buildOpenAIChat(request)
	case domain.ProtocolOpenAIResponses:
		return buildOpenAIResponses(request)
	case domain.ProtocolAnthropic:
		return buildAnthropic(request)
	case domain.ProtocolGemini:
		return buildGemini(request)
	default:
		return "", nil, nil, errors.New("unsupported protocol")
	}
}

func parseResponse(protocol domain.Protocol, data []byte) Result {
	switch protocol {
	case domain.ProtocolOpenAIChat:
		return parseOpenAIChatResponse(data)
	case domain.ProtocolOpenAIResponses:
		return parseOpenAIResponsesResponse(data)
	case domain.ProtocolAnthropic:
		return parseAnthropicResponse(data)
	case domain.ProtocolGemini:
		return parseGeminiResponse(data)
	default:
		return Result{Failure: FailureUpstream}
	}
}

func parseStream(protocol domain.Protocol, reader io.Reader, progress func(string)) Result {
	switch protocol {
	case domain.ProtocolOpenAIChat:
		return parseOpenAIChatStream(reader, progress)
	case domain.ProtocolOpenAIResponses:
		return parseOpenAIResponsesStream(reader, progress)
	case domain.ProtocolAnthropic:
		return parseAnthropicStream(reader, progress)
	case domain.ProtocolGemini:
		return parseGeminiStream(reader, progress)
	default:
		return Result{Failure: FailureUpstream}
	}
}

func endpoint(base, suffix string) (string, error) {
	base = strings.TrimRight(strings.TrimSpace(base), "/")
	if base == "" {
		return "", errors.New("empty base url")
	}
	if strings.HasSuffix(base, suffix) {
		return base, nil
	}
	return base + suffix, nil
}

func geminiEndpoint(base, modelID, method string) (string, error) {
	base = strings.TrimRight(strings.TrimSpace(base), "/")
	if base == "" {
		return "", errors.New("empty base url")
	}
	modelID = strings.TrimPrefix(strings.TrimSpace(modelID), "models/")
	if modelID == "" {
		return "", errors.New("empty model")
	}
	suffix := "/models/" + url.PathEscape(modelID) + ":" + method
	if method == "streamGenerateContent" {
		return base + suffix + "?alt=sse", nil
	}
	return base + suffix, nil
}

func validateEndpoint(target *url.URL, allowLAN bool) error {
	host := strings.ToLower(target.Hostname())
	parsed, err := url.Parse(target.Scheme + "://" + target.Host)
	if err != nil || parsed.Hostname() == "" || parsed.User != nil || parsed.RawQuery != "" || parsed.Fragment != "" {
		return errors.New("invalid redirect")
	}
	loopback := host == "localhost"
	private := false
	if ip := net.ParseIP(host); ip != nil {
		loopback = ip.IsLoopback()
		private = ip.IsPrivate()
		if ip.IsUnspecified() || ip.IsMulticast() || ip.IsLinkLocalUnicast() || ip.IsLinkLocalMulticast() {
			return errors.New("unsafe redirect")
		}
		if !loopback && private && !allowLAN {
			return errors.New("private redirect requires LAN authorization")
		}
	}
	httpAllowed := loopback || (allowLAN && (private || strings.HasSuffix(host, ".local")))
	if target.Scheme != "https" && !(target.Scheme == "http" && httpAllowed) {
		return errors.New("redirect must use HTTPS")
	}
	return nil
}

func statusFailure(status int) string {
	switch status {
	case http.StatusUnauthorized, http.StatusForbidden:
		return FailureAuth
	case http.StatusPaymentRequired:
		return FailureQuota
	case http.StatusNotFound:
		return FailureModel
	case http.StatusTooManyRequests:
		return FailureRateLimit
	case http.StatusRequestTimeout, http.StatusGatewayTimeout:
		return FailureTimeout
	default:
		return FailureUpstream
	}
}

func transportFailure(requestCtx, parentCtx context.Context, err error) string {
	cause := context.Cause(requestCtx)
	if errors.Is(cause, context.DeadlineExceeded) || errors.Is(parentCtx.Err(), context.DeadlineExceeded) {
		return FailureTimeout
	}
	if requestCtx.Err() != nil || parentCtx.Err() != nil {
		return FailureCancelled
	}
	var netErr net.Error
	if errors.As(err, &netErr) && netErr.Timeout() {
		return FailureTimeout
	}
	return FailureConnection
}

type activityReader struct {
	reader io.Reader
	idle   *time.Timer
}

func (r activityReader) Read(buffer []byte) (int, error) {
	n, err := r.reader.Read(buffer)
	if n > 0 && r.idle != nil {
		r.idle.Reset(30 * time.Second)
	}
	return n, err
}

func clipText(text string) string {
	runes := []rune(text)
	if len(runes) > MaxOutputRunes {
		return string(runes[:MaxOutputRunes])
	}
	return text
}

func parseNumber(value float64) int64 {
	if value < 0 {
		return 0
	}
	return int64(value)
}

func computedTotal(usage *Usage) {
	if usage == nil {
		return
	}
	if usage.TotalTokens == 0 {
		usage.TotalTokens = usage.InputTokens + usage.OutputTokens + usage.CacheReadTokens + usage.CacheWriteTokens
	}
}
