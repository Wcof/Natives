package usage

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"
)

func TestToolEventValidation(t *testing.T) {
	cases := []struct {
		name    string
		ev      ToolEvent
		wantErr bool
	}{
		{"valid", ToolEvent{EventID: "e1", ToolID: "claude-code", SessionID: "s1", Kind: "waiting_permission"}, false},
		{"missing id", ToolEvent{ToolID: "claude-code", SessionID: "s1", Kind: "ended"}, true},
		{"bad kind", ToolEvent{EventID: "e1", ToolID: "claude-code", SessionID: "s1", Kind: "approve_me"}, true},
		{"bad time", ToolEvent{EventID: "e1", ToolID: "claude-code", SessionID: "s1", Kind: "ended", OccurredAt: "yesterday"}, true},
		{"oversize", ToolEvent{EventID: "e1", ToolID: "claude-code", SessionID: "s1", Kind: "ended",
			Detail: strings.Repeat("x", 17<<10)}, true},
	}
	for _, tc := range cases {
		err := tc.ev.Validate()
		if tc.wantErr && err == nil {
			t.Errorf("%s: expected error", tc.name)
		}
		if !tc.wantErr && err != nil {
			t.Errorf("%s: unexpected error: %v", tc.name, err)
		}
	}
}

func TestIngestToolEventIdempotent(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	ev := &ToolEvent{EventID: "evt-1", ToolID: "claude-code", SessionID: "sess-1",
		Kind: "waiting_permission", OccurredAt: "2026-09-11T09:00:00Z"}
	first, err := s.IngestToolEvent(ev)
	if err != nil || !first {
		t.Fatalf("first ingest: new=%v err=%v", first, err)
	}
	// 同一事件重发：不重复入箱（方案 §9.2）。
	again, err := s.IngestToolEvent(ev)
	if err != nil {
		t.Fatalf("re-ingest error: %v", err)
	}
	if again {
		t.Errorf("duplicate event must not re-enter inbox")
	}
	items, err := s.ListAttention(10)
	if err != nil {
		t.Fatalf("list: %v", err)
	}
	if len(items) != 1 {
		t.Fatalf("expected exactly 1 attention item, got %d", len(items))
	}
	// 等待许可排在最前（提醒顺序）。
	if items[0].Kind != "waiting_permission" {
		t.Errorf("top item kind = %q", items[0].Kind)
	}
}

func TestIngestToolEventEndedNotInInbox(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	// ended 只更新会话状态，默认不入箱（用户显式开启才提醒，T8）。
	if _, err := s.IngestToolEvent(&ToolEvent{EventID: "e-end", ToolID: "codex",
		SessionID: "s-end", Kind: "ended"}); err != nil {
		t.Fatalf("ended ingest: %v", err)
	}
	items, _ := s.ListAttention(10)
	for _, it := range items {
		if it.Kind == "ended" {
			t.Errorf("ended must not enter default inbox")
		}
	}
}

func TestSessionStateNoRegression(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	if _, err := s.IngestToolEvent(&ToolEvent{EventID: "a", ToolID: "claude-code",
		SessionID: "sx", Kind: "waiting_permission", OccurredAt: "2026-09-11T10:00:00Z"}); err != nil {
		t.Fatal(err)
	}
	// 乱序送达：更早时间的 waiting_input 不得把状态/时间倒退覆盖。
	if _, err := s.IngestToolEvent(&ToolEvent{EventID: "b", ToolID: "claude-code",
		SessionID: "sx", Kind: "waiting_input", OccurredAt: "2026-09-11T09:00:00Z"}); err != nil {
		t.Fatal(err)
	}
	s.mu.Lock()
	var state, observed string
	err := s.db.QueryRow("SELECT last_state, last_observed_at FROM usage_sessions WHERE id='sx'").Scan(&state, &observed)
	s.mu.Unlock()
	if err != nil {
		t.Fatal(err)
	}
	if state != "waiting_permission" {
		t.Errorf("stale event must not regress state, got %q", state)
	}
	if observed != "2026-09-11T10:00:00Z" {
		t.Errorf("stale event must not regress observed time, got %q", observed)
	}
}

func TestAcknowledgeAttention(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	if _, err := s.IngestToolEvent(&ToolEvent{EventID: "ack-1", ToolID: "codex",
		SessionID: "s-ack", Kind: "error"}); err != nil {
		t.Fatal(err)
	}
	items, _ := s.ListAttention(10)
	if len(items) != 1 {
		t.Fatalf("expected 1 item, got %d", len(items))
	}
	if err := s.AcknowledgeAttention(items[0].ID); err != nil {
		t.Fatalf("ack: %v", err)
	}
	after, _ := s.ListAttention(10)
	if len(after) != 0 {
		t.Errorf("acknowledged item must leave default inbox, got %d", len(after))
	}
	// 不存在的 ID 显式报错。
	if err := s.AcknowledgeAttention("nope"); err == nil {
		t.Errorf("ack of unknown id must error")
	}
}

func TestSendNotificationStates(t *testing.T) {
	// 成功 → submitted（不宣称“已看到”）。
	got := sendNotificationWith(context.Background(), func(ctx context.Context, title, body string) error {
		return nil
	}, "Natives", "test")
	if got != NotificationSubmitted {
		t.Errorf("success must be submitted, got %q", got)
	}
	// 失败 → failed（收件箱仍保留记录，由调用方决定）。
	got = sendNotificationWith(context.Background(), func(ctx context.Context, title, body string) error {
		return errors.New("denied")
	}, "Natives", "test")
	if got != NotificationFailed {
		t.Errorf("failure must be failed, got %q", got)
	}
	// 3 秒超时有界：慢 notifier 被取消。
	slow := sendNotificationWith(context.Background(), func(ctx context.Context, title, body string) error {
		<-ctx.Done()
		return ctx.Err()
	}, "Natives", "test")
	if slow != NotificationFailed {
		t.Errorf("timeout must be failed, got %q", slow)
	}
}

func TestAttentionOrderPriority(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	// 乱序注入：error 先、permission 后；收件箱按 permission > input > error 排序。
	seq := []ToolEvent{
		{EventID: "o1", ToolID: "codex", SessionID: "so", Kind: "error"},
		{EventID: "o2", ToolID: "codex", SessionID: "so", Kind: "waiting_input"},
		{EventID: "o3", ToolID: "codex", SessionID: "so", Kind: "waiting_permission"},
	}
	for i := range seq {
		if _, err := s.IngestToolEvent(&seq[i]); err != nil {
			t.Fatal(err)
		}
		time.Sleep(2 * time.Millisecond) // created_at 时间戳可区分
	}
	items, _ := s.ListAttention(10)
	if len(items) != 3 {
		t.Fatalf("expected 3 items, got %d", len(items))
	}
	if items[0].Kind != "waiting_permission" || items[1].Kind != "waiting_input" || items[2].Kind != "error" {
		t.Errorf("priority order wrong: %s, %s, %s", items[0].Kind, items[1].Kind, items[2].Kind)
	}
}
