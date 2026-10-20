package host

import (
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"testing"
	"time"

	"github.com/ldh/natives/model-host/internal/usage"
)

func TestUsageSessionDetailFiltersIdentityAndPaginates(t *testing.T) {
	engine, _, base := newAIQueryTestHarness(t)
	for i := 0; i < 57; i++ {
		tool, instance := "codex", "profile-a"
		if i == 55 {
			instance = "profile-b"
		} else if i == 56 {
			tool = "pi"
		}
		err := engine.usageStore.InsertEvent(&usage.Event{ID: fmt.Sprintf("detail-%d", i),
			RequestedAt: time.Now().UTC(), CreatedAt: time.Now().UTC(),
			ToolID: tool, SourceInstanceID: instance, SessionID: "same-session",
			Provider: "unpriced", Model: "unpriced", Result: usage.ResultSuccess, TotalTokens: 1})
		if err != nil {
			t.Fatal(err)
		}
	}
	key := "codex|profile-a|same-session"
	for _, offset := range []int{0, 50} {
		resp, body := aiQueryGet(t, base, fmt.Sprintf("/ai/usage/events?range=all&sessionKey=%s&offset=%d&limit=50", url.QueryEscape(key), offset))
		var result usage.EventsResult
		if resp.StatusCode != http.StatusOK || json.Unmarshal(body, &result) != nil {
			t.Fatalf("session query failed: %d", resp.StatusCode)
		}
		want := 50
		if offset > 0 {
			want = 5
		}
		if result.Total != 55 || len(result.Events) != want || result.HasMore != (offset == 0) {
			t.Fatalf("offset=%d total=%d count=%d more=%v", offset, result.Total, len(result.Events), result.HasMore)
		}
		for _, event := range result.Events {
			if event.ToolID != "codex" || event.SourceInstanceID != "profile-a" || event.CostStatus != "unpriced" {
				t.Fatal("session identity or unknown cost leaked across the projection")
			}
		}
	}
	result, err := engine.getUsageEvents(json.RawMessage(`{"sessionKey":"codex|missing|same-session"}`))
	if err != nil || len(result.Events) != 0 {
		t.Fatal("unknown key must return empty instead of all events")
	}
	resp, _ := aiQueryGet(t, base, "/ai/usage/events?sessionKey=invalid")
	if resp.StatusCode != http.StatusBadRequest {
		t.Fatal("malformed key must not silently become an unrestricted query")
	}
}
