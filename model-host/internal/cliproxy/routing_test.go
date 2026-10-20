package cliproxy

import (
	"bytes"
	"context"
	"io"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/ldh/natives/model-host/internal/domain"
	"github.com/ldh/natives/model-host/internal/secrets"
)

func TestRuntimeModelPrefixPreservesUserContent(t *testing.T) {
	t.Setenv("HOME", t.TempDir())
	seen := make(chan string, 1)
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		seen <- string(body)
		w.Header().Set("Content-Type", "application/json")
		io.WriteString(w, `{"id":"test","object":"chat.completion","model":"route-model","choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}]}`)
	}))
	defer upstream.Close()
	snapshot := domain.NewSnapshot()
	snapshot.Providers = []domain.Provider{{ID: "route", Kind: "custom", Enabled: true, BaseURL: upstream.URL + "/v1", Protocol: domain.ProtocolOpenAIChat, SecretRef: "provider", Models: []domain.Model{{ID: "route-model", Enabled: true}}}}
	store := secrets.NewMemoryStore()
	store.Values["gateway:access-key"] = "test-gateway"
	store.Values["provider"] = "test-provider"
	runtime := &Runtime{}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	port, err := runtime.Start(ctx, snapshot, store, NewAuthStore(store, nil, nil), filepath.Join(t.TempDir(), "runtime.yaml"))
	if err != nil {
		t.Fatal(err)
	}
	defer runtime.Stop(ctx)
	r, _ := http.NewRequestWithContext(ctx, "POST", "http://127.0.0.1:"+strconv.Itoa(port)+"/chat/completions", bytes.NewBufferString(`{"model":"natives-gateway/route-model","messages":[{"role":"user","content":"natives-gateway/keep-this-text"}]}`))
	r.Header.Set("Authorization", "Bearer test-gateway")
	r.Header.Set("Content-Type", "application/json")
	response, err := http.DefaultClient.Do(r)
	if err != nil {
		t.Fatal(err)
	}
	defer response.Body.Close()
	if response.StatusCode != 200 {
		t.Fatalf("status = %d", response.StatusCode)
	}
	select {
	case body := <-seen:
		if !strings.Contains(body, `"model":"route-model"`) || !strings.Contains(body, "natives-gateway/keep-this-text") {
			t.Fatalf("routing changed user content: %s", body)
		}
	case <-ctx.Done():
		t.Fatal(ctx.Err())
	}
}

func TestGatewayRejectsInvalidAndOversizedJSONBeforeForwarding(t *testing.T) {
	for _, fixture := range []struct {
		body   string
		status int
	}{{`{"model":`, 400}, {strings.Repeat(" ", (32<<20)+1), 413}} {
		called := false
		handler := gatewayHandler(http.HandlerFunc(func(http.ResponseWriter, *http.Request) { called = true }))
		request := httptest.NewRequest("POST", "/v1/chat/completions", strings.NewReader(fixture.body))
		request.Header.Set("Content-Type", "application/json")
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if called || response.Code != fixture.status {
			t.Fatalf("status=%d forwarded=%v", response.Code, called)
		}
	}
}

func TestGatewayKeysFailClosed(t *testing.T) {
	for _, fixture := range []struct {
		name        string
		keys        []domain.GatewayAccessKey
		unavailable bool
	}{
		{"missing", nil, false},
		{"locked", nil, true},
		{"disabled", []domain.GatewayAccessKey{{SecretRef: "disabled", Enabled: false}}, false},
		{"missing-enabled", []domain.GatewayAccessKey{{SecretRef: "missing", Enabled: true}}, false},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			snapshot := domain.NewSnapshot()
			snapshot.Gateway.AccessKeys = fixture.keys
			store := secrets.NewMemoryStore()
			store.Unavailable = fixture.unavailable
			if fixture.keys != nil {
				store.Values["gateway:access-key"] = "legacy-must-not-bypass"
			}
			if keys, err := enabledGatewayKeys(snapshot, store); err == nil || len(keys) != 0 {
				t.Fatalf("keys=%v error=%v", keys, err)
			}
		})
	}
}
