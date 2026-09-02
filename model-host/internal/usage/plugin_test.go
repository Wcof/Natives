package usage

import (
	"context"
	"testing"
	"time"

	cliproxyusage "github.com/router-for-me/CLIProxyAPI/v7/sdk/cliproxy/usage"
)

func TestUsagePluginAttributesKeyAndDoesNotPersistUpstreamBody(t *testing.T) {
	store, _ := tempStore(t)
	defer store.Close()
	plugin := NewNativesUsagePlugin(store, NewCalculator(store), func(key string) (string, string) {
		if key == "client-secret" {
			return "key-1", "测试密钥"
		}
		return "", ""
	}, nil)
	plugin.HandleUsage(context.Background(), cliproxyusage.Record{
		Provider: "openai", ExecutorType: "openai", Model: "gpt-4o", APIKey: "client-secret",
		RequestedAt: time.Now().UTC(), Failed: true,
		Fail:   cliproxyusage.Failure{StatusCode: 401, Body: "token=must-not-be-persisted"},
		Detail: cliproxyusage.Detail{InputTokens: 10, OutputTokens: 2, TotalTokens: 12},
	})
	plugin.Close()
	events, err := store.GetEvents(Filter{Range: "all", Limit: 10})
	if err != nil || len(events.Events) != 1 {
		t.Fatalf("events = %+v, %v", events, err)
	}
	event := events.Events[0]
	if event.AccessKeyID != "key-1" || event.AccessKeyName != "测试密钥" || event.ErrorSummary != "Unauthorized" {
		t.Fatalf("stored event = %+v", event)
	}
}
