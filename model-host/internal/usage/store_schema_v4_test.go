package usage

import (
	"database/sql"
	"path/filepath"
	"testing"
	"time"
)

// TestSchemaV4PreservesLegacyUsageOnRepeatedUpgrade T2/T4：以真实 v3 表形
// （无 usage_status）为隔离夹具，升级后历史事件保留、缺失列按 reported
// 回填，重复打开不新增或改写事件。
func TestSchemaV4PreservesLegacyUsageOnRepeatedUpgrade(t *testing.T) {
	path := filepath.Join(t.TempDir(), "usage.db")
	store, err := OpenStore(path)
	if err != nil {
		t.Fatal(err)
	}
	event := &Event{
		ID: "legacy-event-1", RequestedAt: time.Now().UTC().Add(-time.Hour),
		Provider: "legacy-provider", Model: "legacy-model", Source: "claude-code",
		Result: ResultSuccess, HTTPStatus: 200, InputTokens: 10, OutputTokens: 5, TotalTokens: 15,
		CostMicro: 42, BillingAtom: "legacy:event-1",
	}
	if err := store.InsertEvent(event); err != nil {
		t.Fatal(err)
	}
	if err := store.Close(); err != nil {
		t.Fatal(err)
	}

	raw, err := sql.Open("sqlite", path)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := raw.Exec(`ALTER TABLE usage_events DROP COLUMN usage_status`); err != nil {
		t.Fatal(err)
	}
	if _, err := raw.Exec(`UPDATE usage_metadata SET schema_version = 3 WHERE id = 1`); err != nil {
		t.Fatal(err)
	}
	if err := raw.Close(); err != nil {
		t.Fatal(err)
	}

	for attempt := 0; attempt < 2; attempt++ {
		store, err = OpenStore(path)
		if err != nil {
			t.Fatalf("reopen %d: %v", attempt, err)
		}
		events, err := store.GetEvents(Filter{Range: "all", Limit: 10})
		if err != nil {
			t.Fatal(err)
		}
		if events.Total != 1 || len(events.Events) != 1 {
			t.Fatalf("reopen %d: history changed: total=%d events=%d", attempt, events.Total, len(events.Events))
		}
		got := events.Events[0]
		if got.ID != event.ID || got.InputTokens != 10 || got.OutputTokens != 5 || got.TotalTokens != 15 || got.CostMicro != 42 {
			t.Fatalf("reopen %d: historical usage not preserved: %#v", attempt, got)
		}
		if got.UsageStatus != "reported" {
			t.Fatalf("reopen %d: legacy event must default to reported, got %q", attempt, got.UsageStatus)
		}
		var version int
		if err := store.db.QueryRow(`SELECT schema_version FROM usage_metadata WHERE id = 1`).Scan(&version); err != nil {
			t.Fatal(err)
		}
		if version != CurrentSchemaVersion {
			t.Fatalf("reopen %d: schema_version=%d, want %d", attempt, version, CurrentSchemaVersion)
		}
		if err := store.Close(); err != nil {
			t.Fatal(err)
		}
	}
}
