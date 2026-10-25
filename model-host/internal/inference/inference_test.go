package inference

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/ldh/natives/model-host/internal/domain"
)

func TestProtocolsBuildDistinctRequests(t *testing.T) {
	request := Request{
		ProviderID: "provider", Protocol: domain.ProtocolOpenAIChat,
		BaseURL: "https://example.test/v1", APIKey: "sk", ModelID: "model-a",
		SystemPrompt: "system", Text: "user", MaxTokens: 512,
	}
	endpoint, body, headers, err := buildRequest(request)
	if err != nil || endpoint != "https://example.test/v1/chat/completions" {
		t.Fatalf("chat endpoint: %s %v", endpoint, err)
	}
	if headers["Authorization"] != "Bearer sk" || !strings.Contains(string(body), `"stream":true`) {
		t.Fatalf("chat request shape: %s %#v", body, headers)
	}

	request.Protocol = domain.ProtocolOpenAIResponses
	endpoint, body, _, err = buildRequest(request)
	if err != nil || endpoint != "https://example.test/v1/responses" || !strings.Contains(string(body), `"max_output_tokens":512`) {
		t.Fatalf("responses request: %s %s %v", endpoint, body, err)
	}

	request.Protocol = domain.ProtocolAnthropic
	endpoint, body, headers, err = buildRequest(request)
	if err != nil || endpoint != "https://example.test/v1/messages" || headers["x-api-key"] != "sk" || !strings.Contains(string(body), `"system":"system"`) {
		t.Fatalf("anthropic request: %s %s %#v %v", endpoint, body, headers, err)
	}

	request.Protocol = domain.ProtocolGemini
	endpoint, body, headers, err = buildRequest(request)
	if err != nil || endpoint != "https://example.test/v1/models/model-a:streamGenerateContent?alt=sse" || headers["x-goog-api-key"] != "sk" || !strings.Contains(string(body), `"maxOutputTokens":512`) {
		t.Fatalf("gemini request: %s %s %#v %v", endpoint, body, headers, err)
	}
}

func TestChatStreamNormalizesReasoningAndCache(t *testing.T) {
	stream := strings.Join([]string{
		`data: {"choices":[{"delta":{"reasoning_content":"think"}}]}`,
		`data: {"choices":[{"delta":{"content":"Hi"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":4,"total_tokens":14,"prompt_tokens_details":{"cached_tokens":3},"completion_tokens_details":{"reasoning_tokens":2}}}`,
		`data: [DONE]`,
		``,
	}, "\n\n")
	result := parseStream(domain.ProtocolOpenAIChat, strings.NewReader(stream), nil)
	if result.Failure != "" || result.Text != "Hi" {
		t.Fatalf("chat stream result: %#v", result)
	}
	if result.Usage.InputTokens != 7 || result.Usage.CacheReadTokens != 3 || result.Usage.OutputTokens != 4 ||
		result.Usage.ReasoningTokens != 2 || result.Usage.TotalTokens != 14 {
		t.Fatalf("chat usage normalization: %#v", result.Usage)
	}
}

func TestResponsesStreamSeparatesOutputFromReasoning(t *testing.T) {
	stream := strings.Join([]string{
		`data: {"type":"response.reasoning_summary_text.delta","delta":"thinking"}`,
		`data: {"type":"response.output_text.delta","delta":"Answer"}`,
		`data: {"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":20,"output_tokens":6,"total_tokens":26,"input_tokens_details":{"cached_tokens":4},"output_tokens_details":{"reasoning_tokens":3}}}}`,
		``,
	}, "\n\n")
	result := parseStream(domain.ProtocolOpenAIResponses, strings.NewReader(stream), nil)
	if result.Failure != "" || result.Text != "Answer" {
		t.Fatalf("responses stream result: %#v", result)
	}
	if result.Usage.InputTokens != 16 || result.Usage.CacheReadTokens != 4 || result.Usage.OutputTokens != 6 ||
		result.Usage.ReasoningTokens != 3 || result.Usage.TotalTokens != 26 {
		t.Fatalf("responses usage normalization: %#v", result.Usage)
	}
}

func TestAnthropicStreamCountsCacheBucketsOnce(t *testing.T) {
	stream := strings.Join([]string{
		`data: {"type":"message_start","message":{"usage":{"input_tokens":12,"cache_read_input_tokens":5,"cache_creation_input_tokens":2,"output_tokens":0}}}`,
		`data: {"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"think"}}`,
		`data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"Answer"}}`,
		`data: {"type":"message_delta","usage":{"output_tokens":7},"delta":{"stop_reason":"end_turn"}}`,
		`data: {"type":"message_stop"}`,
		``,
	}, "\n\n")
	result := parseStream(domain.ProtocolAnthropic, strings.NewReader(stream), nil)
	if result.Failure != "" || result.Text != "Answer" {
		t.Fatalf("anthropic stream result: %#v", result)
	}
	if result.Usage.InputTokens != 12 || result.Usage.CacheReadTokens != 5 || result.Usage.CacheWriteTokens != 2 ||
		result.Usage.OutputTokens != 7 || result.Usage.TotalTokens != 26 {
		t.Fatalf("anthropic usage normalization: %#v", result.Usage)
	}
}

func TestGeminiStreamKeepsThoughtsOutOfAnswerAndInsideOutput(t *testing.T) {
	stream := strings.Join([]string{
		`data: {"candidates":[{"content":{"parts":[{"text":"think","thought":true}]}}]}`,
		`data: {"candidates":[{"content":{"parts":[{"text":"Answer"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":20,"candidatesTokenCount":4,"thoughtsTokenCount":3,"cachedContentTokenCount":6,"totalTokenCount":27}}`,
		``,
	}, "\n\n")
	result := parseStream(domain.ProtocolGemini, strings.NewReader(stream), nil)
	if result.Failure != "" || result.Text != "Answer" {
		t.Fatalf("gemini stream result: %#v", result)
	}
	if result.Usage.InputTokens != 14 || result.Usage.CacheReadTokens != 6 || result.Usage.OutputTokens != 7 ||
		result.Usage.ReasoningTokens != 3 || result.Usage.TotalTokens != 27 {
		t.Fatalf("gemini usage normalization: %#v", result.Usage)
	}
}

func TestNonStreamOutputOverLimitIsAnExplicitFailure(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"choices": []map[string]any{{
				"message":       map[string]string{"content": strings.Repeat("x", MaxOutputRunes+1)},
				"finish_reason": "stop",
			}},
		})
	}))
	defer server.Close()
	result := Run(context.Background(), Request{
		Protocol: domain.ProtocolOpenAIChat, BaseURL: server.URL, APIKey: "sk",
		ModelID: "model-a", Text: "synthetic evidence", MaxTokens: 512,
	})
	if result.Failure != FailureLength || len([]rune(result.Text)) != MaxOutputRunes {
		t.Fatalf("over-limit non-stream output must be explicit: failure=%q runes=%d", result.Failure, len([]rune(result.Text)))
	}
}

func TestMissingUsageIsNilNotZero(t *testing.T) {
	for _, protocol := range []domain.Protocol{domain.ProtocolOpenAIChat, domain.ProtocolOpenAIResponses, domain.ProtocolAnthropic, domain.ProtocolGemini} {
		var payload []byte
		switch protocol {
		case domain.ProtocolOpenAIChat:
			payload = []byte(`{"choices":[{"message":{"content":"ok"},"finish_reason":"stop"}]}`)
		case domain.ProtocolOpenAIResponses:
			payload = []byte(`{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}]}`)
		case domain.ProtocolAnthropic:
			payload = []byte(`{"content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn"}`)
		case domain.ProtocolGemini:
			payload = []byte(`{"candidates":[{"content":{"parts":[{"text":"ok"}]},"finishReason":"STOP"}]}`)
		}
		result := parseResponse(protocol, payload)
		if result.Failure != "" || result.Text != "ok" {
			t.Fatalf("%s non-stream result: %#v", protocol, result)
		}
		if result.Usage != nil {
			t.Fatalf("%s missing usage must stay nil: %#v", protocol, result.Usage)
		}
	}
}

func TestTruncatedStreamsAreNotReportedAsSuccess(t *testing.T) {
	stream := "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":\"length\"}]}\n\n"
	result := parseStream(domain.ProtocolOpenAIChat, strings.NewReader(stream), nil)
	if result.Failure != FailureTruncated {
		t.Fatalf("length finish reason must be truncated: %#v", result)
	}
	unterminated := "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n"
	result = parseStream(domain.ProtocolOpenAIChat, strings.NewReader(unterminated), nil)
	if result.Failure != FailureStream {
		t.Fatalf("EOF without terminal event must fail: %#v", result)
	}
}

func TestStatusFailureMapping(t *testing.T) {
	cases := map[int]string{
		401: FailureAuth, 403: FailureAuth, 402: FailureQuota,
		404: FailureModel, 429: FailureRateLimit,
		408: FailureTimeout, 504: FailureTimeout,
		500: FailureUpstream, 503: FailureUpstream,
	}
	for status, want := range cases {
		if got := statusFailure(status); got != want {
			t.Errorf("status %d mapped to %q, want %q", status, got, want)
		}
	}
}

func TestResponsePayloadShapesAreParsed(t *testing.T) {
	chat := parseResponse(domain.ProtocolOpenAIChat, []byte(`{"choices":[{"message":{"content":[{"type":"text","text":"array"}]},"finish_reason":"stop"}]}`))
	encoded, _ := json.Marshal(chat)
	if chat.Text != "array" {
		t.Fatalf("chat array content not parsed: %s", encoded)
	}
	anthropic := parseResponse(domain.ProtocolAnthropic, []byte(`{"content":[{"type":"thinking","thinking":"x"},{"type":"text","text":"ok"}],"stop_reason":"max_tokens"}`))
	if anthropic.Failure != FailureTruncated {
		t.Fatalf("anthropic max_tokens must truncate: %#v", anthropic)
	}
}
