package cliproxy

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/secrets"
)

func TestRuntimeUsesAuthenticatedLoopbackAndPublicConfigHasNoSecrets(t *testing.T) {
	secretStore := secrets.NewMemoryStore()
	secretStore.Values["provider"] = "upstream-secret"
	snapshot := domain.NewSnapshot()
	snapshot.Providers = append(snapshot.Providers, domain.Provider{
		ID: "custom", Kind: "custom", Name: "Custom", BaseURL: "https://example.invalid/v1",
		Protocol: domain.ProtocolOpenAIChat, Enabled: true, SecretRef: "provider",
		Models: []domain.Model{{ID: "demo-model", DisplayName: "Demo", Enabled: true}},
	})
	configPath := filepath.Join(t.TempDir(), "runtime.yaml")
	runtime := &Runtime{}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	port, err := runtime.Start(ctx, snapshot, secretStore, NewAuthStore(secretStore, nil, nil), "gateway-secret", configPath)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		stopCtx, stopCancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer stopCancel()
		_ = runtime.Stop(stopCtx)
	})
	endpoint := "http://127.0.0.1:" + strconv.Itoa(port) + "/v1/models"
	unauthorized, err := http.Get(endpoint)
	if err != nil {
		t.Fatal(err)
	}
	_ = unauthorized.Body.Close()
	if unauthorized.StatusCode != http.StatusUnauthorized {
		t.Fatalf("unauthenticated status = %d", unauthorized.StatusCode)
	}
	request, _ := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	request.Header.Set("Authorization", "Bearer gateway-secret")
	response, err := http.DefaultClient.Do(request)
	if err != nil {
		t.Fatal(err)
	}
	body, _ := io.ReadAll(response.Body)
	_ = response.Body.Close()
	if response.StatusCode != http.StatusOK || !strings.Contains(string(body), "demo-model") {
		t.Fatalf("models response = %d %s", response.StatusCode, body)
	}
	management, _ := http.NewRequestWithContext(ctx, http.MethodGet, "http://127.0.0.1:"+strconv.Itoa(port)+"/management.html", nil)
	management.Header.Set("Authorization", "Bearer gateway-secret")
	managementResponse, err := http.DefaultClient.Do(management)
	if err != nil {
		t.Fatal(err)
	}
	_ = managementResponse.Body.Close()
	if managementResponse.StatusCode != http.StatusNotFound {
		t.Fatalf("management route was exposed: %d", managementResponse.StatusCode)
	}
	publicConfig, err := os.ReadFile(configPath)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(publicConfig), "secret") || strings.Contains(string(publicConfig), "example.invalid") {
		t.Fatalf("runtime config leaked private configuration: %s", publicConfig)
	}
}

func TestRuntimeStreamsToolsUsageAndPropagatesCancellation(t *testing.T) {
	cancelled := make(chan struct{}, 1)
	upstream := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		writer.Header().Set("Content-Type", "text/event-stream")
		_, _ = writer.Write([]byte("data: {\"id\":\"chunk-1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"stream-model\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"demo\",\"arguments\":\"{}\"}}]},\"finish_reason\":null}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\n"))
		writer.(http.Flusher).Flush()
		<-request.Context().Done()
		cancelled <- struct{}{}
	}))
	defer upstream.Close()
	secretStore := secrets.NewMemoryStore()
	secretStore.Values["stream-key"] = "upstream-stream-secret"
	snapshot := domain.NewSnapshot()
	snapshot.Providers = append(snapshot.Providers, domain.Provider{
		ID: "stream", Kind: "custom", Name: "Stream", BaseURL: upstream.URL + "/v1",
		Protocol: domain.ProtocolOpenAIChat, Enabled: true, SecretRef: "stream-key",
		Models: []domain.Model{{ID: "stream-model", Enabled: true}},
	})
	runtime := &Runtime{}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	port, err := runtime.Start(ctx, snapshot, secretStore, NewAuthStore(secretStore, nil, nil), "gateway-secret", filepath.Join(t.TempDir(), "runtime.yaml"))
	if err != nil {
		t.Fatal(err)
	}
	defer runtime.Stop(context.Background())
	requestCtx, cancelRequest := context.WithCancel(ctx)
	request, _ := http.NewRequestWithContext(requestCtx, http.MethodPost, "http://127.0.0.1:"+strconv.Itoa(port)+"/v1/chat/completions", bytes.NewBufferString(`{"model":"stream-model","stream":true,"stream_options":{"include_usage":true},"messages":[{"role":"user","content":"hello"}],"tools":[{"type":"function","function":{"name":"demo","parameters":{"type":"object"}}}]}`))
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("Authorization", "Bearer gateway-secret")
	response, err := http.DefaultClient.Do(request)
	if err != nil {
		t.Fatal(err)
	}
	line, err := bufio.NewReader(response.Body).ReadString('\n')
	if err != nil || !strings.Contains(line, "tool_calls") || !strings.Contains(line, "usage") {
		t.Fatalf("unexpected stream chunk: %q %v", line, err)
	}
	cancelRequest()
	_ = response.Body.Close()
	select {
	case <-cancelled:
	case <-time.After(2 * time.Second):
		t.Fatal("downstream cancellation did not reach upstream")
	}
}

func TestRuntimeStreamingParityAgainstMockUpstream(t *testing.T) {
	upstream := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		writer.Header().Set("Content-Type", "text/event-stream")
		switch {
		case strings.HasSuffix(request.URL.Path, "/chat/completions"):
			_, _ = writer.Write([]byte("data: {\"id\":\"chunk\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"response-model\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"stream-ok\"},\"finish_reason\":null}]}\n\ndata: [DONE]\n\n"))
		case request.URL.Path == "/v1/messages":
			_, _ = writer.Write([]byte("event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-model\",\"content\":[],\"stop_reason\":null,\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\nevent: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"stream-ok\"}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"))
		case strings.Contains(request.URL.Path, ":streamGenerateContent"):
			_, _ = writer.Write([]byte("data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"stream-ok\"}]},\"finishReason\":\"STOP\",\"index\":0}],\"usageMetadata\":{\"promptTokenCount\":1,\"candidatesTokenCount\":1,\"totalTokenCount\":2}}\n\n"))
		default:
			http.NotFound(writer, request)
		}
	}))
	defer upstream.Close()
	secretStore := secrets.NewMemoryStore()
	for ref, key := range map[string]string{"responses-key": "responses-secret", "claude-key": "claude-secret", "gemini-key": "gemini-secret"} {
		secretStore.Values[ref] = key
	}
	snapshot := domain.NewSnapshot()
	snapshot.Providers = append(snapshot.Providers,
		domain.Provider{ID: "responses", Kind: "custom", Name: "Responses", BaseURL: upstream.URL + "/v1", Protocol: domain.ProtocolOpenAIResponses, Enabled: true, SecretRef: "responses-key", Models: []domain.Model{{ID: "response-model", Enabled: true}}},
		domain.Provider{ID: "claude", Kind: "custom", Name: "Claude", BaseURL: upstream.URL, Protocol: domain.ProtocolAnthropic, Enabled: true, SecretRef: "claude-key", Models: []domain.Model{{ID: "claude-model", Enabled: true}}},
		domain.Provider{ID: "gemini", Kind: "custom", Name: "Gemini", BaseURL: upstream.URL, Protocol: domain.ProtocolGemini, Enabled: true, SecretRef: "gemini-key", Models: []domain.Model{{ID: "gemini-model", Enabled: true}}},
	)
	runtime := &Runtime{}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	port, err := runtime.Start(ctx, snapshot, secretStore, NewAuthStore(secretStore, nil, nil), "gateway-secret", filepath.Join(t.TempDir(), "runtime.yaml"))
	if err != nil {
		t.Fatal(err)
	}
	defer runtime.Stop(context.Background())
	baseURL := "http://127.0.0.1:" + strconv.Itoa(port)
	for _, fixture := range []struct{ path, body, header string }{
		{"/v1/responses", `{"model":"response-model","input":"hello","stream":true}`, "Authorization"},
		{"/v1/messages", `{"model":"claude-model","max_tokens":32,"stream":true,"messages":[{"role":"user","content":"hello"}]}`, "x-api-key"},
		{"/v1beta/models/gemini-model:streamGenerateContent?alt=sse", `{"contents":[{"role":"user","parts":[{"text":"hello"}]}]}`, "x-goog-api-key"},
	} {
		request, _ := http.NewRequestWithContext(ctx, http.MethodPost, baseURL+fixture.path, bytes.NewBufferString(fixture.body))
		request.Header.Set("Content-Type", "application/json")
		value := "gateway-secret"
		if fixture.header == "Authorization" {
			value = "Bearer " + value
		}
		request.Header.Set(fixture.header, value)
		response, requestErr := http.DefaultClient.Do(request)
		if requestErr != nil {
			t.Fatal(requestErr)
		}
		body, readErr := io.ReadAll(response.Body)
		_ = response.Body.Close()
		if readErr != nil || response.StatusCode != http.StatusOK || !strings.Contains(string(body), "stream-ok") {
			t.Fatalf("%s stream response = %d %q %v", fixture.path, response.StatusCode, body, readErr)
		}
	}
}

func TestRuntimeReturnsPromptlyWhenUpstreamStreamEndsUnexpectedly(t *testing.T) {
	upstream := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
		connection, buffer, err := writer.(http.Hijacker).Hijack()
		if err != nil {
			t.Error(err)
			return
		}
		_, _ = buffer.WriteString("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\ndata: {\"incomplete\"")
		_ = buffer.Flush()
		_ = connection.Close()
	}))
	defer upstream.Close()
	secretStore := secrets.NewMemoryStore()
	secretStore.Values["eof-key"] = "upstream-secret"
	snapshot := domain.NewSnapshot()
	snapshot.Providers = append(snapshot.Providers, domain.Provider{ID: "eof", Kind: "custom", Name: "EOF", BaseURL: upstream.URL + "/v1", Protocol: domain.ProtocolOpenAIChat, Enabled: true, SecretRef: "eof-key", Models: []domain.Model{{ID: "eof-model", Enabled: true}}})
	runtime := &Runtime{}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	port, err := runtime.Start(ctx, snapshot, secretStore, NewAuthStore(secretStore, nil, nil), "gateway-secret", filepath.Join(t.TempDir(), "runtime.yaml"))
	if err != nil {
		t.Fatal(err)
	}
	defer runtime.Stop(context.Background())
	request, _ := http.NewRequestWithContext(ctx, http.MethodPost, "http://127.0.0.1:"+strconv.Itoa(port)+"/v1/chat/completions", bytes.NewBufferString(`{"model":"eof-model","stream":true,"messages":[{"role":"user","content":"hello"}]}`))
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("Authorization", "Bearer gateway-secret")
	started := time.Now()
	response, err := http.DefaultClient.Do(request)
	if err == nil {
		_, _ = io.ReadAll(response.Body)
		_ = response.Body.Close()
	}
	if time.Since(started) > 2*time.Second {
		t.Fatal("abnormal upstream EOF left the gateway request hanging")
	}
}

func TestRuntimeProtocolParityAgainstMockUpstream(t *testing.T) {
	seen := make(map[string]bool)
	var seenMu sync.Mutex
	upstream := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		body, _ := io.ReadAll(request.Body)
		switch {
		case strings.HasSuffix(request.URL.Path, "/chat/completions"):
			authorization := request.Header.Get("Authorization")
			if !strings.HasPrefix(authorization, "Bearer openai-") {
				t.Errorf("OpenAI key was not injected: %q", authorization)
			}
			seenMu.Lock()
			if authorization == "Bearer openai-responses-secret" {
				seen["responses"] = strings.Contains(string(body), `"reasoning_effort":"high"`)
			} else {
				seen["openai"] = true
			}
			seenMu.Unlock()
			writer.Header().Set("Content-Type", "application/json")
			_, _ = writer.Write([]byte(`{"id":"chatcmpl-mock","object":"chat.completion","created":1,"model":"mock","choices":[{"index":0,"message":{"role":"assistant","content":"mock-ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}`))
		case request.URL.Path == "/v1/messages":
			if request.Header.Get("x-api-key") != "claude-secret" && request.Header.Get("Authorization") != "Bearer claude-secret" {
				t.Errorf("Anthropic key was not injected: x-api-key=%q authorization=%q", request.Header.Get("x-api-key"), request.Header.Get("Authorization"))
			}
			seenMu.Lock()
			seen["anthropic"] = true
			seenMu.Unlock()
			writer.Header().Set("Content-Type", "application/json")
			_, _ = writer.Write([]byte(`{"id":"msg_mock","type":"message","role":"assistant","model":"claude-model","content":[{"type":"text","text":"mock-ok"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}`))
		case strings.Contains(request.URL.Path, ":generateContent"):
			if request.Header.Get("x-goog-api-key") != "gemini-secret" {
				t.Errorf("Gemini key was not injected: %q", request.Header.Get("x-goog-api-key"))
			}
			seenMu.Lock()
			seen["gemini"] = true
			seenMu.Unlock()
			writer.Header().Set("Content-Type", "application/json")
			_, _ = writer.Write([]byte(`{"candidates":[{"content":{"role":"model","parts":[{"text":"mock-ok"}]},"finishReason":"STOP","index":0}],"usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1,"totalTokenCount":2},"modelVersion":"gemini-model"}`))
		default:
			t.Errorf("unexpected upstream path %s body=%s", request.URL.Path, body)
			http.NotFound(writer, request)
		}
	}))
	defer upstream.Close()
	secretStore := secrets.NewMemoryStore()
	snapshot := domain.NewSnapshot()
	providers := []domain.Provider{
		{ID: "chat", Kind: "custom", Name: "Chat", BaseURL: upstream.URL + "/v1", Protocol: domain.ProtocolOpenAIChat, Enabled: true, SecretRef: "chat-key", Models: []domain.Model{{ID: "chat-model", Enabled: true}}},
		{ID: "responses", Kind: "custom", Name: "Responses", BaseURL: upstream.URL + "/v1", Protocol: domain.ProtocolOpenAIResponses, Enabled: true, SecretRef: "responses-key", Models: []domain.Model{{ID: "response-model", Enabled: true}}},
		{ID: "claude", Kind: "custom", Name: "Claude", BaseURL: upstream.URL, Protocol: domain.ProtocolAnthropic, Enabled: true, SecretRef: "claude-key", Models: []domain.Model{{ID: "claude-model", Enabled: true}}},
		{ID: "gemini", Kind: "custom", Name: "Gemini", BaseURL: upstream.URL, Protocol: domain.ProtocolGemini, Enabled: true, SecretRef: "gemini-key", Models: []domain.Model{{ID: "gemini-model", Enabled: true}}},
	}
	snapshot.Providers = append(snapshot.Providers, providers...)
	secretStore.Values["chat-key"] = "openai-chat-secret"
	secretStore.Values["responses-key"] = "openai-responses-secret"
	secretStore.Values["claude-key"] = "claude-secret"
	secretStore.Values["gemini-key"] = "gemini-secret"
	runtime := &Runtime{}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	port, err := runtime.Start(ctx, snapshot, secretStore, NewAuthStore(secretStore, nil, nil), "gateway-secret", filepath.Join(t.TempDir(), "runtime.yaml"))
	if err != nil {
		t.Fatal(err)
	}
	defer runtime.Stop(context.Background())
	baseURL := "http://127.0.0.1:" + strconv.Itoa(port)
	requests := []struct{ path, body, authHeader, authValue string }{
		{"/v1/chat/completions", `{"model":"chat-model","messages":[{"role":"user","content":"hello"}]}`, "Authorization", "Bearer gateway-secret"},
		{"/v1/responses", `{"model":"response-model","input":"hello","reasoning":{"effort":"high"}}`, "Authorization", "Bearer gateway-secret"},
		{"/v1/messages", `{"model":"claude-model","max_tokens":32,"messages":[{"role":"user","content":"hello"}]}`, "x-api-key", "gateway-secret"},
		{"/v1beta/models/gemini-model:generateContent", `{"contents":[{"role":"user","parts":[{"text":"hello"}]}]}`, "x-goog-api-key", "gateway-secret"},
	}
	for _, fixture := range requests {
		request, _ := http.NewRequestWithContext(ctx, http.MethodPost, baseURL+fixture.path, bytes.NewBufferString(fixture.body))
		request.Header.Set("Content-Type", "application/json")
		request.Header.Set(fixture.authHeader, fixture.authValue)
		response, requestErr := http.DefaultClient.Do(request)
		if requestErr != nil {
			t.Fatal(requestErr)
		}
		responseBody, _ := io.ReadAll(response.Body)
		_ = response.Body.Close()
		if response.StatusCode != http.StatusOK || !json.Valid(responseBody) || !strings.Contains(string(responseBody), "mock-ok") {
			t.Fatalf("%s response = %d %s", fixture.path, response.StatusCode, responseBody)
		}
	}
	seenMu.Lock()
	defer seenMu.Unlock()
	if !seen["openai"] || !seen["responses"] || !seen["anthropic"] || !seen["gemini"] {
		t.Fatalf("not all upstream adapters ran: %#v", seen)
	}
}
