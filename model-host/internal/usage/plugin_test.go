package usage

import (
	"context"
	"sync"
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

func TestUsagePluginProcessesRecordEvenWithCancelledContext(t *testing.T) {
	store, _ := tempStore(t)
	defer store.Close()
	plugin := NewNativesUsagePlugin(store, NewCalculator(store), nil, nil)
	cancelledCtx, cancel := context.WithCancel(context.Background())
	cancel() // Already cancelled

	plugin.HandleUsage(cancelledCtx, cliproxyusage.Record{
		Provider: "openai", ExecutorType: "openai", Model: "gpt-4o",
		RequestedAt: time.Now().UTC(),
		Detail:      cliproxyusage.Detail{InputTokens: 5, OutputTokens: 15, TotalTokens: 20},
	})
	plugin.Close()

	events, err := store.GetEvents(Filter{Range: "all", Limit: 10})
	if err != nil || len(events.Events) != 1 {
		t.Fatalf("expected 1 event from cancelled context call, got %d (err: %v)", len(events.Events), err)
	}
	if events.Events[0].TotalTokens != 20 {
		t.Fatalf("stored tokens = %d, want 20", events.Events[0].TotalTokens)
	}
}

func TestUsageManagerRestartPreservesPublish(t *testing.T) {
	mgr := cliproxyusage.NewManager(10)
	var records []cliproxyusage.Record
	var mu sync.Mutex

	testPlugin := testRecordPlugin(func(r cliproxyusage.Record) {
		mu.Lock()
		records = append(records, r)
		mu.Unlock()
	})
	mgr.Register(testPlugin)

	// Cycle 1
	mgr.Start(context.Background())
	mgr.Publish(context.Background(), cliproxyusage.Record{Model: "cycle-1"})
	time.Sleep(50 * time.Millisecond)
	mgr.Stop()

	// Cycle 2: must restart cleanly and accept new records
	mgr.Start(context.Background())
	mgr.Publish(context.Background(), cliproxyusage.Record{Model: "cycle-2"})
	time.Sleep(50 * time.Millisecond)
	mgr.Stop()

	mu.Lock()
	defer mu.Unlock()
	if len(records) != 2 {
		t.Fatalf("expected 2 records across restart cycles, got %d: %+v", len(records), records)
	}
	if records[0].Model != "cycle-1" || records[1].Model != "cycle-2" {
		t.Fatalf("unexpected record order: %+v", records)
	}
}

type testRecordPlugin func(cliproxyusage.Record)

func (f testRecordPlugin) HandleUsage(ctx context.Context, record cliproxyusage.Record) {
	f(record)
}

