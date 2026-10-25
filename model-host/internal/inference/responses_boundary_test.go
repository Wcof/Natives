package inference

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/ldh/natives/model-host/internal/domain"
)

func TestResponsesTerminalOutputLimit(t *testing.T) {
	for _, size := range []int{MaxOutputRunes, MaxOutputRunes + 1} {
		t.Run(fmt.Sprint(size), func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				w.Header().Set("Content-Type", "text/event-stream")
				event, _ := json.Marshal(map[string]any{"type": "response.completed", "response": map[string]any{
					"status": "completed", "output_text": strings.Repeat("文", size),
					"usage": map[string]int{"input_tokens": 10, "output_tokens": 20, "total_tokens": 30},
				}})
				fmt.Fprintf(w, "data: %s\n\n", event)
			}))
			defer server.Close()
			got := Run(context.Background(), Request{Protocol: domain.ProtocolOpenAIResponses, BaseURL: server.URL, ModelID: "fixture", Text: "public fixture"})
			want := ""
			if size > MaxOutputRunes {
				want = FailureLength
			}
			if got.Failure != want || len([]rune(got.Text)) > MaxOutputRunes || got.Usage == nil || got.Usage.TotalTokens != 30 {
				t.Fatalf("failure=%q text runes=%d usage=%+v", got.Failure, len([]rune(got.Text)), got.Usage)
			}
		})
	}
}

func TestResponsesIncompleteBodyIsNotSuccess(t *testing.T) {
	for _, status := range []string{"in_progress", "incomplete", "failed"} {
		t.Run(status, func(t *testing.T) {
			payload := fmt.Sprintf(`{"status":%q,"output_text":"partial","usage":{"input_tokens":10,"output_tokens":2,"total_tokens":12}}`, status)
			result := parseResponse(domain.ProtocolOpenAIResponses, []byte(payload))
			if result.Failure == "" || result.Usage == nil || result.Usage.TotalTokens != 12 {
				t.Fatalf("non-terminal response accepted or usage lost: %+v", result)
			}
			stream := fmt.Sprintf("data: {\"type\":\"response.incomplete\",\"response\":%s}\n\n", payload)
			result = parseStream(domain.ProtocolOpenAIResponses, strings.NewReader(stream), nil)
			if result.Failure == "" || result.Usage == nil || result.Usage.TotalTokens != 12 {
				t.Fatalf("incomplete stream accepted or usage lost: %+v", result)
			}
		})
	}
}
