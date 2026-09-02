package usage

import (
	"database/sql"
	"encoding/base64"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func tempStore(t *testing.T) (*Store, string) {
	t.Helper()
	dir := t.TempDir()
	path := filepath.Join(dir, "test_usage.db")
	s, err := OpenStore(path)
	if err != nil {
		t.Fatalf("OpenStore failed: %v", err)
	}
	return s, path
}

func TestImporterAcceptsEasyCLIProxyAPIV3Database(t *testing.T) {
	destination, _ := tempStore(t)
	defer destination.Close()
	sourcePath := filepath.Join(t.TempDir(), "usage.db")
	db, err := sql.Open("sqlite", sourcePath)
	if err != nil {
		t.Fatal(err)
	}
	_, err = db.Exec(`CREATE TABLE usage_events (
		id INTEGER PRIMARY KEY, event_key TEXT, timestamp TEXT, timestamp_ms INTEGER,
		latency_ms INTEGER, ttft_ms INTEGER, provider TEXT, auth_index TEXT, model TEXT,
		alias TEXT, source TEXT, endpoint TEXT, api_key_hash TEXT, api_key_remark TEXT,
		failed INTEGER, canceled INTEGER, failure_status INTEGER, input_tokens INTEGER,
		output_tokens INTEGER, cache_read_tokens INTEGER, cache_creation_tokens INTEGER,
		reasoning_tokens INTEGER, total_tokens INTEGER, service_tier TEXT, created_at TEXT);
		INSERT INTO usage_events VALUES (1,'event-1','2026-08-31T12:00:00Z',1788177600000,
		120,20,'codex','account-1','gpt-5.6','', 'oauth','responses','hash-1','默认密钥',
		0,0,200,100,20,10,0,5,120,'default','2026-08-31T12:00:00Z')`)
	if err != nil {
		t.Fatal(err)
	}
	_ = db.Close()
	payload, _ := os.ReadFile(sourcePath)
	importer := NewImporter(destination, NewCalculator(destination))
	session, err := importer.BeginSession("usage.db", int64(len(payload)))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = importer.AppendChunk(session.ID, 0, base64.StdEncoding.EncodeToString(payload)); err != nil {
		t.Fatal(err)
	}
	preview, err := importer.PreviewSession(session.ID)
	if err != nil || !preview.Valid || preview.TotalRecords != 1 {
		t.Fatalf("preview = %+v, %v", preview, err)
	}
	if count, err := importer.CommitSession(session.ID); err != nil || count != 1 {
		t.Fatalf("commit = %d, %v", count, err)
	}
	events, _ := destination.GetEvents(Filter{Range: "all", Limit: 10})
	if len(events.Events) != 1 || events.Events[0].AccessKeyID != "hash-1" || events.Events[0].Model != "gpt-5.6" {
		t.Fatalf("imported events = %+v", events.Events)
	}
}

func TestPriceSyncUsesEasyCatalogAndPreservesManualOverride(t *testing.T) {
	store, _ := tempStore(t)
	defer store.Close()
	if err := store.UpsertPrice(&Price{ModelID: "gpt-test", InputPriceMicro: 9, Source: PriceSourceManual}); err != nil {
		t.Fatal(err)
	}
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		_, _ = io.WriteString(writer, `{"schemaVersion":1,"models":{"gpt-test":{"inputPer1M":1.5,"outputPer1M":2}}}`)
	}))
	defer server.Close()
	if count, err := SyncRemotePrices(store, server.URL); err != nil || count != 1 {
		t.Fatalf("sync = %d, %v", count, err)
	}
	price := NewCalculator(store).FindPrice("", "gpt-test")
	if price == nil || price.Source != PriceSourceManual || price.InputPriceMicro != 9 {
		t.Fatalf("manual price was overwritten: %+v", price)
	}
}

func TestStoreMigrationsAndInsert(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	now := time.Now().UTC()
	err := s.InsertEvent(&Event{
		ID:               "evt-1",
		RequestedAt:      now,
		LatencyMs:        120,
		TTFTMs:           30,
		Provider:         "openai",
		Model:            "gpt-4o",
		Result:           ResultSuccess,
		HTTPStatus:       200,
		InputTokens:      100,
		OutputTokens:     50,
		CacheReadTokens:  20,
		CacheWriteTokens: 0,
		ReasoningTokens:  0,
		TotalTokens:      150,
		CostMicro:        1500,
		ServiceTier:      "default",
	})
	if err != nil {
		t.Fatalf("InsertEvent failed: %v", err)
	}

	res, err := s.GetOverview(Filter{Range: "4h"})
	if err != nil {
		t.Fatalf("GetOverview failed: %v", err)
	}
	if res.Metrics.TotalRequests != 1 {
		t.Errorf("expected 1 total request, got %d", res.Metrics.TotalRequests)
	}
	if res.Metrics.TotalTokens != 150 {
		t.Errorf("expected 150 total tokens, got %d", res.Metrics.TotalTokens)
	}
	if res.Metrics.SuccessRate != 100.0 {
		t.Errorf("expected 100%% success rate, got %f", res.Metrics.SuccessRate)
	}
}

func TestStoreAnalyticsAndEventsPagination(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	now := time.Now().UTC()
	for i := 0; i < 5; i++ {
		s.InsertEvent(&Event{
			ID:          string(rune('a' + i)),
			RequestedAt: now.Add(time.Duration(i) * time.Minute),
			Provider:    "anthropic",
			Model:       "claude-3-5-sonnet",
			Result:      ResultSuccess,
			HTTPStatus:  200,
			TotalTokens: int64(100 * (i + 1)),
		})
	}

	analytics, err := s.GetAnalytics(Filter{Range: "24h"})
	if err != nil {
		t.Fatalf("GetAnalytics failed: %v", err)
	}
	if len(analytics.ByModel) == 0 || analytics.ByModel[0].Requests != 5 {
		t.Errorf("expected 5 requests in ByModel, got %+v", analytics.ByModel)
	}

	events, err := s.GetEvents(Filter{Range: "24h", Limit: 2, Offset: 0})
	if err != nil {
		t.Fatalf("GetEvents failed: %v", err)
	}
	if len(events.Events) != 2 {
		t.Errorf("expected 2 events, got %d", len(events.Events))
	}
	if events.Total != 5 {
		t.Errorf("expected total 5, got %d", events.Total)
	}
	if !events.HasMore {
		t.Errorf("expected hasMore to be true")
	}
}

func TestPricingAndCostCalculation(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	calc := NewCalculator(s)
	// gpt-4o builtin: 2.50 input / 10.00 output per 1M
	cost := calc.CalculateCost("openai", "gpt-4o", 1000000, 1000000, 0, 0)
	// 2500000 + 10000000 = 12500000 micro-usd ($12.50)
	if cost != 12500000 {
		t.Errorf("expected cost 12500000 micro, got %d", cost)
	}

	// Custom manual price
	err := s.UpsertPrice(&Price{
		ProviderID:       "custom-prov",
		ModelID:          "my-model",
		InputPriceMicro:  1000000,
		OutputPriceMicro: 2000000,
		Source:           PriceSourceManual,
	})
	if err != nil {
		t.Fatalf("UpsertPrice failed: %v", err)
	}

	costCustom := calc.CalculateCost("custom-prov", "my-model", 1000000, 1000000, 0, 0)
	if costCustom != 3000000 {
		t.Errorf("expected custom cost 3000000 micro, got %d", costCustom)
	}
}

func TestImporterChunkAndCommit(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	// Create dummy staging database
	stagingDir := t.TempDir()
	stagingPath := filepath.Join(stagingDir, "staging.db")
	srcStore, _ := OpenStore(stagingPath)
	srcStore.InsertEvent(&Event{
		ID:          "imp-1",
		RequestedAt: time.Now().UTC(),
		Provider:    "openai",
		Model:       "gpt-4o",
		Result:      ResultSuccess,
		HTTPStatus:  200,
		TotalTokens: 50,
	})
	srcStore.Close()

	imp := NewImporter(s)
	data, _ := os.ReadFile(stagingPath)
	session, err := imp.BeginSession("staging.db", int64(len(data)))
	if err != nil {
		t.Fatalf("Begin failed: %v", err)
	}

	// Copy dummy database file into staging
	session.File.Write(data)
	session.ReceivedSize = int64(len(data))
	session.File.Close()
	session.File = nil

	preview, err := imp.Preview(session.ID)
	if err != nil {
		t.Fatalf("Preview failed: %v", err)
	}
	if !preview.Valid || preview.TotalRecords != 1 {
		t.Errorf("expected valid preview with 1 record, got %+v", preview)
	}

	committed, err := imp.Commit(session.ID)
	if err != nil {
		t.Fatalf("Commit failed: %v", err)
	}
	if committed != 1 {
		t.Errorf("expected 1 committed event, got %d", committed)
	}

	overview, _ := s.GetOverview(Filter{Range: "all"})
	if overview.TotalEventsCount != 1 {
		t.Errorf("expected 1 event in store, got %d", overview.TotalEventsCount)
	}
}
