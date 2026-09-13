package usage

import (
	"fmt"
	"path/filepath"
	"testing"
	"time"
)

func TestInsightsEngine(t *testing.T) {
	dir := t.TempDir()
	store, err := NewStore(filepath.Join(dir, "usage.db"))
	if err != nil {
		t.Fatalf("NewStore failed: %v", err)
	}
	defer store.Close()

	now := time.Now().UTC()

	// 1. 空数据库时返回空 insights。
	res, err := store.GetInsights()
	if err != nil {
		t.Fatalf("GetInsights failed: %v", err)
	}
	if len(res.Insights) != 0 {
		t.Errorf("expected 0 insights on empty store, got %d", len(res.Insights))
	}

	// 2. 模拟长上下文膨胀：同 session 10 次请求，前 5 次 1000 tokens，后 5 次 3000 tokens。
	for i := 0; i < 10; i++ {
		tokens := int64(1000)
		if i >= 5 {
			tokens = 3000
		}
		evt := &Event{
			ID:          fmt.Sprintf("evt-ctx-%d", i),
			SessionID:   "session-long-ctx",
			Provider:    "anthropic",
			Model:       "claude-3-7-sonnet",
			InputTokens: tokens,
			TotalTokens: tokens + 100,
			CostMicro:   1000,
			Result:      ResultSuccess,
			RequestedAt: now.Add(time.Duration(i) * time.Minute),
		}
		if err := store.InsertEvent(evt); err != nil {
			t.Fatalf("InsertEvent failed: %v", err)
		}
	}

	res, err = store.GetInsights()
	if err != nil {
		t.Fatalf("GetInsights failed: %v", err)
	}
	var foundCtx bool
	for _, ins := range res.Insights {
		if ins.RuleKey == "long_context_cost_spike" {
			foundCtx = true
			if ins.Severity != "info" {
				t.Errorf("expected info severity, got %s", ins.Severity)
			}
		}
	}
	if !foundCtx {
		t.Errorf("expected long_context_cost_spike insight, got %+v", res.Insights)
	}

	// 3. 模拟高费用集中：追加 4 个小费用 session 和 1 个高费用 session。
	for sIdx := 1; sIdx <= 4; sIdx++ {
		evt := &Event{
			ID:          fmt.Sprintf("evt-small-%d", sIdx),
			SessionID:   fmt.Sprintf("session-small-%d", sIdx),
			Provider:    "openai",
			Model:       "gpt-4o",
			InputTokens: 500,
			TotalTokens: 600,
			CostMicro:   1000, // 4 * 1000 = 4000 micro
			Result:      ResultSuccess,
			RequestedAt: now.Add(time.Duration(20+sIdx) * time.Minute),
		}
		if err := store.InsertEvent(evt); err != nil {
			t.Fatalf("InsertEvent failed: %v", err)
		}
	}
	// 高费用 session
	evtBig := &Event{
		ID:          "evt-big-1",
		SessionID:   "session-big",
		Provider:    "openai",
		Model:       "gpt-4o",
		InputTokens: 50000,
		TotalTokens: 60000,
		CostMicro:   100000, // 远超 30%
		Result:      ResultSuccess,
		RequestedAt: now.Add(30 * time.Minute),
	}
	if err := store.InsertEvent(evtBig); err != nil {
		t.Fatalf("InsertEvent failed: %v", err)
	}

	res, err = store.GetInsights()
	if err != nil {
		t.Fatalf("GetInsights failed: %v", err)
	}
	var foundHighCost bool
	for _, ins := range res.Insights {
		if ins.RuleKey == "high_cost_session_concentration" {
			foundHighCost = true
			if ins.Severity != "warn" {
				t.Errorf("expected warn severity, got %s", ins.Severity)
			}
		}
	}
	if !foundHighCost {
		t.Errorf("expected high_cost_session_concentration insight, got %+v", res.Insights)
	}
}
