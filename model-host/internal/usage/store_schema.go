package usage

import "time"

func (s *Store) migrate() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	_, err := s.db.Exec(`
	CREATE TABLE IF NOT EXISTS usage_metadata (
		id INTEGER PRIMARY KEY CHECK (id = 1), schema_version INTEGER NOT NULL DEFAULT 1,
		usage_revision INTEGER NOT NULL DEFAULT 1, price_catalog_version TEXT NOT NULL DEFAULT '', last_pruned_at TEXT NOT NULL DEFAULT '');
	INSERT OR IGNORE INTO usage_metadata (id, schema_version, usage_revision) VALUES (1, 1, 1);
	CREATE TABLE IF NOT EXISTS usage_events (
		id TEXT PRIMARY KEY, requested_at TEXT NOT NULL, latency_ms INTEGER NOT NULL, ttft_ms INTEGER NOT NULL,
		provider TEXT NOT NULL, account_id TEXT NOT NULL DEFAULT '', model TEXT NOT NULL, model_alias TEXT NOT NULL DEFAULT '',
		source TEXT NOT NULL DEFAULT '', endpoint TEXT NOT NULL DEFAULT '', access_key_id TEXT NOT NULL DEFAULT '', access_key_name TEXT NOT NULL DEFAULT '',
		result TEXT NOT NULL, http_status INTEGER NOT NULL, error_code TEXT NOT NULL DEFAULT '', error_summary TEXT NOT NULL DEFAULT '',
		input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0, cache_read_tokens INTEGER NOT NULL DEFAULT 0,
		cache_write_tokens INTEGER NOT NULL DEFAULT 0, reasoning_tokens INTEGER NOT NULL DEFAULT 0, total_tokens INTEGER NOT NULL DEFAULT 0,
		cost_micro INTEGER NOT NULL DEFAULT 0, service_tier TEXT NOT NULL DEFAULT 'default', created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')));
	CREATE INDEX IF NOT EXISTS idx_usage_events_req_at ON usage_events(requested_at);
	CREATE INDEX IF NOT EXISTS idx_usage_events_model ON usage_events(model);
	CREATE INDEX IF NOT EXISTS idx_usage_events_provider ON usage_events(provider);
	CREATE INDEX IF NOT EXISTS idx_usage_events_key ON usage_events(access_key_id);
	CREATE INDEX IF NOT EXISTS idx_usage_events_result ON usage_events(result);
	CREATE TABLE IF NOT EXISTS model_prices (
		id TEXT PRIMARY KEY, provider_id TEXT NOT NULL DEFAULT '', model_id TEXT NOT NULL,
		input_price_micro INTEGER NOT NULL DEFAULT 0, output_price_micro INTEGER NOT NULL DEFAULT 0,
		cache_read_price_micro INTEGER NOT NULL DEFAULT 0, cache_write_price_micro INTEGER NOT NULL DEFAULT 0,
		source TEXT NOT NULL, updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')));
	CREATE UNIQUE INDEX IF NOT EXISTS idx_model_prices_unique ON model_prices(provider_id, model_id);`)
	return err
}

func (s *Store) Prune() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	cutoff := time.Now().UTC().AddDate(0, 0, -MaxDaysRetention).Format(time.RFC3339)
	if _, err := s.db.Exec("DELETE FROM usage_events WHERE requested_at < ?", cutoff); err != nil {
		return err
	}
	var count int64
	if err := s.db.QueryRow("SELECT COUNT(*) FROM usage_events").Scan(&count); err != nil {
		return err
	}
	if count > MaxEventsRetention {
		if _, err := s.db.Exec("DELETE FROM usage_events WHERE id IN (SELECT id FROM usage_events ORDER BY requested_at ASC LIMIT ?)", count-MaxEventsRetention); err != nil {
			return err
		}
	}
	_, err := s.db.Exec("UPDATE usage_metadata SET last_pruned_at = ? WHERE id = 1", time.Now().UTC().Format(time.RFC3339))
	return err
}
