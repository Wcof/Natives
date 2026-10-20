package host

import (
	"context"
	"encoding/json"
	"net"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"

	"github.com/ldh/natives/model-host/internal/domain"
)

func TestInferenceModelsRequireUniqueEnabledRoute(t *testing.T) {
	s := domain.NewSnapshot()
	s.Providers = []domain.Provider{
		{ID: "a", Name: "A", Enabled: true, Models: []domain.Model{{ID: "one", Alias: "shared", Enabled: true}, {ID: "ok", Alias: "unique", Enabled: true}}},
		{ID: "b", Name: "B", Enabled: true, Models: []domain.Model{{ID: "two", Alias: "shared", Enabled: true}}},
		{ID: "c", Name: "C", Enabled: false, Models: []domain.Model{{ID: "off", Alias: "other", Enabled: true}}},
	}
	models := inferenceModels(s)
	if len(models) != 1 || models[0]["modelId"] != "ok" || models[0]["route"] != "unique" {
		t.Fatalf("ambiguous/disabled routes exposed: %#v", models)
	}
}

func TestInferenceRequestUsesConfiguredLoopbackAndOwner(t *testing.T) {
	expectedBudget := 1600
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/v1/chat/completions" || r.Header.Get("Authorization") != "Bearer local-test-key" {
			t.Errorf("unexpected request path or credential: %s", r.URL.Path)
		}
		var payload struct {
			Model     string `json:"model"`
			MaxTokens int    `json:"max_tokens"`
			Messages  []struct {
				Content string `json:"content"`
			} `json:"messages"`
		}
		if err := json.NewDecoder(r.Body).Decode(&payload); err != nil || payload.Model != "unique" || payload.MaxTokens != expectedBudget || len(payload.Messages) != 2 || payload.Messages[1].Content != "synthetic evidence" {
			t.Errorf("unexpected inference payload: %#v, %v", payload, err)
		}
		_, _ = w.Write([]byte(`{"choices":[{"message":{"content":"facts and limits"}}]}`))
	}))
	defer server.Close()
	engine, _, _ := newAIQueryTestHarness(t)
	engine.secrets.Set("gateway:access-key", "local-test-key")
	_, portText, err := net.SplitHostPort(strings.TrimPrefix(server.URL, "http://"))
	if err != nil {
		t.Fatal(err)
	}
	port, _ := strconv.Atoi(portText)
	_, err = engine.repo.Update(nil, func(s *domain.Snapshot) error {
		s.Gateway.State = "running"
		s.Gateway.Port = port
		s.Providers = []domain.Provider{{ID: "a", Name: "A", Enabled: true, Models: []domain.Model{{ID: "one", Alias: "unique", Enabled: true}}}}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	text, failure := engine.runInference(context.Background(), "unique", "synthetic evidence")
	if failure != "" || text != "facts and limits" {
		t.Fatalf("inference: text=%q failure=%q", text, failure)
	}
	expectedBudget = 4096
	text, failure = engine.runInferenceWithProgress(context.Background(), "unique", "synthetic evidence", expectedBudget, nil)
	if failure != "" || text != "facts and limits" {
		t.Fatalf("configured inference: text=%q failure=%q", text, failure)
	}
	owner := strings.Repeat("o", 32)
	if _, err := engine.startInference(context.Background(), json.RawMessage(`{"owner":"short","providerId":"a","modelId":"one","text":"synthetic"}`)); err == nil {
		t.Fatal("short owner accepted")
	}
	if _, err := engine.inferenceStatus(json.RawMessage(`{"owner":"`+owner+`","id":"foreign"}`), false); err == nil {
		t.Fatal("foreign job visible")
	}
}

func TestInferenceTokenBudgetRejectsInvalidInputs(t *testing.T) {
	for _, budget := range []string{"0", "511", "8193", "-1", "512.5", `"4096"`} {
		_, err := parseInference(json.RawMessage(`{"owner":"` + strings.Repeat("o", 32) + `","maxTokens":` + budget + `}`))
		if err == nil {
			t.Fatalf("invalid token budget accepted: %s", budget)
		}
	}
	for _, budget := range []string{"512", "4096", "8192"} {
		if _, err := parseInference(json.RawMessage(`{"owner":"` + strings.Repeat("o", 32) + `","maxTokens":` + budget + `}`)); err != nil {
			t.Fatal(err)
		}
	}
}
