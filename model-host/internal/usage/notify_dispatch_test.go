package usage

// notify_dispatch.go 测试：投递决策集成层（§6.5）。
// 覆盖：全局事件 ID 去重（ErrAlreadyDispatched）、queued 占位防并发重复、
// submitted/failed 如实回写、失败不自动重试、pending 批量投递跳过已投递条目。

import (
	"strings"
	"testing"
)

// ingestWaitingAlert 造一条等待许可事件入箱，返回 alert ID。
func ingestWaitingAlert(t *testing.T, s *Store, eventID, sessionID string) string {
	t.Helper()
	e := &ToolEvent{
		EventID:    eventID,
		ToolID:     "claude-code",
		SessionID:  sessionID,
		Kind:       "waiting_permission",
		Detail:     "Bash command approval",
		OccurredAt: "2026-09-12T10:00:00Z",
	}
	inserted, err := s.IngestToolEvent(e)
	if err != nil || !inserted {
		t.Fatalf("ingest: inserted=%v err=%v", inserted, err)
	}
	return "evt-" + eventID
}

func TestDispatchNotificationDedupesByGlobalEventID(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()
	alertID := ingestWaitingAlert(t, s, "ev-1", "sess-1")

	calls := 0
	send := func(title, body string) NotificationResult {
		calls++
		return NotificationSubmitted
	}

	// 首次投递：submitted 且回写。
	result, err := s.DispatchNotification(alertID, send)
	if err != nil || result != NotificationSubmitted {
		t.Fatalf("first dispatch: result=%v err=%v", result, err)
	}
	if calls != 1 {
		t.Fatalf("send calls = %d, want 1", calls)
	}
	var state string
	_ = s.db.QueryRow("SELECT delivery_state FROM usage_alerts WHERE id = ?", alertID).Scan(&state)
	if state != "submitted" {
		t.Errorf("delivery_state = %q, want submitted", state)
	}

	// 同一事件再投递：ErrAlreadyDispatched，不重复弹窗（§9.2 多页场景）。
	result2, err := s.DispatchNotification(alertID, send)
	if result2 != NotificationSubmitted || err != ErrAlreadyDispatched {
		t.Fatalf("second dispatch: result=%v err=%v, want submitted + ErrAlreadyDispatched", result2, err)
	}
	if calls != 1 {
		t.Errorf("send calls after re-dispatch = %d, want 1 (deduped)", calls)
	}
}

func TestDispatchNotificationFailedStateRecordedNoAutoRetry(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()
	alertID := ingestWaitingAlert(t, s, "ev-2", "sess-2")

	calls := 0
	send := func(title, body string) NotificationResult {
		calls++
		return NotificationFailed
	}
	result, err := s.DispatchNotification(alertID, send)
	if err != nil || result != NotificationFailed {
		t.Fatalf("dispatch: result=%v err=%v", result, err)
	}
	var state string
	_ = s.db.QueryRow("SELECT delivery_state FROM usage_alerts WHERE id = ?", alertID).Scan(&state)
	if state != "failed" {
		t.Errorf("delivery_state = %q, want failed", state)
	}

	// failed 不自动重试：再次调用被去重拒绝（显式用户动作才可重发）。
	if _, err := s.DispatchNotification(alertID, send); err != ErrAlreadyDispatched {
		t.Errorf("failed state must not auto-retry, got err=%v", err)
	}
	if calls != 1 {
		t.Errorf("send calls = %d, want 1", calls)
	}
}

func TestDispatchPendingAttentionSubmitsNewSkipsDispatched(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()
	new1 := ingestWaitingAlert(t, s, "ev-3", "sess-3")
	new2 := ingestWaitingAlert(t, s, "ev-4", "sess-4")
	dispatched := ingestWaitingAlert(t, s, "ev-5", "sess-5")

	// 预置一条已 submitted 的条目。
	if _, err := s.DispatchNotification(dispatched, func(_, _ string) NotificationResult {
		return NotificationSubmitted
	}); err != nil {
		t.Fatalf("pre-dispatch: %v", err)
	}

	var sent []string
	submitted, failed := s.DispatchPendingAttention(func(title, body string) NotificationResult {
		sent = append(sent, title+"|"+body)
		return NotificationSubmitted
	})
	if submitted != 2 || failed != 0 {
		t.Fatalf("pending dispatch: submitted=%d failed=%d, want 2/0", submitted, failed)
	}
	// 已投递条目不重复出现在本次投递内容里。
	joined := strings.Join(sent, ";")
	if strings.Contains(joined, "sess-5") {
		t.Errorf("already-dispatched alert re-sent: %q", joined)
	}
	_ = new1
	_ = new2
}

func TestDispatchNotificationMissingAlert(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()
	if _, err := s.DispatchNotification("evt-nope", func(_, _ string) NotificationResult {
		return NotificationSubmitted
	}); err == nil || !strings.Contains(err.Error(), "no such table") && !strings.Contains(err.Error(), "not found") && !strings.Contains(err.Error(), "no rows") {
		// 允许任意底层错误文案，但必须非 nil 且指向查询失败。
		if err == nil {
			t.Fatalf("missing alert must error, got nil")
		}
	}
}
