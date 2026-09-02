package usage

import (
	"database/sql"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	_ "modernc.org/sqlite"
)

const (
	CurrentSchemaVersion = 1
	MaxEventsRetention   = 500000
	MaxDaysRetention     = 365
)

type Store struct {
	db   *sql.DB
	path string
	mu   sync.RWMutex
}

func DefaultDBPath() (string, error) {
	if override := os.Getenv("NATIVES_MODEL_HOST_CONFIG_DIR"); override != "" {
		if !filepath.IsAbs(override) {
			return "", errors.New("model host config override must be absolute")
		}
		return filepath.Join(override, "usage.db"), nil
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, ".natives", "usage.db"), nil
}

func DefaultUsageDBPath() string {
	p, _ := DefaultDBPath()
	if p == "" {
		return filepath.Join(".", ".natives", "usage.db")
	}
	return p
}

func NewStore(path string) (*Store, error) {
	return OpenStore(path)
}

func OpenStore(path string) (*Store, error) {
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		return nil, fmt.Errorf("create usage db dir: %w", err)
	}

	db, err := sql.Open("sqlite", path)
	if err != nil {
		return nil, fmt.Errorf("open sqlite db: %w", err)
	}

	db.SetMaxOpenConns(1)
	if _, err := db.Exec("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;"); err != nil {
		db.Close()
		return nil, fmt.Errorf("set pragma: %w", err)
	}

	store := &Store{db: db, path: path}
	if err := store.migrate(); err != nil {
		db.Close()
		return nil, fmt.Errorf("migrate usage db: %w", err)
	}

	if err := store.Prune(); err != nil {
		db.Close()
		return nil, fmt.Errorf("prune usage db: %w", err)
	}

	return store, nil
}

func (s *Store) DBPath() string {
	return s.path
}

func (s *Store) GetStatus() (int64, string, string, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()

	var count int64
	var minReq, maxReq sql.NullString
	err := s.db.QueryRow("SELECT COUNT(*), MIN(requested_at), MAX(requested_at) FROM usage_events").Scan(&count, &minReq, &maxReq)
	if err != nil {
		return 0, "", "", err
	}
	return count, minReq.String, maxReq.String, nil
}

func (s *Store) Close() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.db != nil {
		return s.db.Close()
	}
	return nil
}

func (s *Store) InsertEvent(e *Event) error {
	_, err := s.InsertEventIfAbsent(e)
	return err
}

func (s *Store) InsertEventIfAbsent(e *Event) (bool, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	if e.ID == "" {
		e.ID = fmt.Sprintf("evt_%d_%d", time.Now().UnixNano(), time.Now().Nanosecond()%1000)
	}
	if e.RequestedAt.IsZero() {
		e.RequestedAt = time.Now().UTC()
	}
	if e.CreatedAt.IsZero() {
		e.CreatedAt = time.Now().UTC()
	}

	query := `
	INSERT OR IGNORE INTO usage_events (
		id, requested_at, latency_ms, ttft_ms, provider, account_id,
		model, model_alias, source, endpoint, access_key_id, access_key_name,
		result, http_status, error_code, error_summary,
		input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
		reasoning_tokens, total_tokens, cost_micro, service_tier, created_at
	) VALUES (
		?, ?, ?, ?, ?, ?,
		?, ?, ?, ?, ?, ?,
		?, ?, ?, ?,
		?, ?, ?, ?,
		?, ?, ?, ?, ?
	)
	`
	result, err := s.db.Exec(
		query,
		e.ID, e.RequestedAt.Format(time.RFC3339Nano), e.LatencyMs, e.TTFTMs, e.Provider, e.AccountID,
		e.Model, e.ModelAlias, e.Source, e.Endpoint, e.AccessKeyID, e.AccessKeyName,
		string(e.Result), e.HTTPStatus, e.ErrorCode, e.ErrorSummary,
		e.InputTokens, e.OutputTokens, e.CacheReadTokens, e.CacheWriteTokens,
		e.ReasoningTokens, e.TotalTokens, e.CostMicro, e.ServiceTier, e.CreatedAt.Format(time.RFC3339Nano),
	)
	if err == nil {
		inserted, resultErr := result.RowsAffected()
		if resultErr != nil {
			return false, resultErr
		}
		if inserted > 0 {
			_, err = s.db.Exec("UPDATE usage_metadata SET usage_revision = usage_revision + 1 WHERE id = 1")
		}
		return inserted > 0, err
	}
	return false, err
}

func (s *Store) buildFilterWhere(f Filter) (string, []interface{}) {
	var clauses []string
	var args []interface{}

	now := time.Now().UTC()
	switch f.Range {
	case "4h":
		clauses = append(clauses, "requested_at >= ?")
		args = append(args, now.Add(-4*time.Hour).Format(time.RFC3339))
	case "24h":
		clauses = append(clauses, "requested_at >= ?")
		args = append(args, now.Add(-24*time.Hour).Format(time.RFC3339))
	case "today":
		todayStart := time.Date(now.Year(), now.Month(), now.Day(), 0, 0, 0, 0, time.UTC)
		clauses = append(clauses, "requested_at >= ?")
		args = append(args, todayStart.Format(time.RFC3339))
	case "7d":
		clauses = append(clauses, "requested_at >= ?")
		args = append(args, now.AddDate(0, 0, -7).Format(time.RFC3339))
	case "30d":
		clauses = append(clauses, "requested_at >= ?")
		args = append(args, now.AddDate(0, 0, -30).Format(time.RFC3339))
	case "custom":
		if f.StartTime != nil {
			clauses = append(clauses, "requested_at >= ?")
			args = append(args, f.StartTime.Format(time.RFC3339))
		}
		if f.EndTime != nil {
			clauses = append(clauses, "requested_at <= ?")
			args = append(args, f.EndTime.Format(time.RFC3339))
		}
	}

	if len(f.Models) > 0 {
		placeholders := strings.Repeat("?,", len(f.Models))
		placeholders = placeholders[:len(placeholders)-1]
		clauses = append(clauses, fmt.Sprintf("model IN (%s)", placeholders))
		for _, m := range f.Models {
			args = append(args, m)
		}
	}
	if len(f.Providers) > 0 {
		placeholders := strings.Repeat("?,", len(f.Providers))
		placeholders = placeholders[:len(placeholders)-1]
		clauses = append(clauses, fmt.Sprintf("provider IN (%s)", placeholders))
		for _, p := range f.Providers {
			args = append(args, p)
		}
	}
	if len(f.Sources) > 0 {
		placeholders := strings.Repeat("?,", len(f.Sources))
		placeholders = placeholders[:len(placeholders)-1]
		clauses = append(clauses, fmt.Sprintf("source IN (%s)", placeholders))
		for _, src := range f.Sources {
			args = append(args, src)
		}
	}
	if len(f.AccessKeyIDs) > 0 {
		placeholders := strings.Repeat("?,", len(f.AccessKeyIDs))
		placeholders = placeholders[:len(placeholders)-1]
		clauses = append(clauses, fmt.Sprintf("access_key_id IN (%s)", placeholders))
		for _, k := range f.AccessKeyIDs {
			args = append(args, k)
		}
	}
	if f.Result != nil {
		clauses = append(clauses, "result = ?")
		args = append(args, string(*f.Result))
	}

	if len(clauses) == 0 {
		return "", args
	}
	return "WHERE " + strings.Join(clauses, " AND "), args
}

func (s *Store) GetOverview(f Filter) (*OverviewResult, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()

	where, args := s.buildFilterWhere(f)

	var totalReqs, totalTokens, succCount, inputToks, outputToks, readToks, writeToks, reasToks, costMicro int64
	var minReq, maxReq sql.NullString

	query := fmt.Sprintf(`
		SELECT
			COUNT(*),
			COALESCE(SUM(total_tokens), 0),
			COALESCE(SUM(CASE WHEN result = 'success' THEN 1 ELSE 0 END), 0),
			COALESCE(SUM(input_tokens), 0),
			COALESCE(SUM(output_tokens), 0),
			COALESCE(SUM(cache_read_tokens), 0),
			COALESCE(SUM(cache_write_tokens), 0),
			COALESCE(SUM(reasoning_tokens), 0),
			COALESCE(SUM(cost_micro), 0),
			MIN(requested_at),
			MAX(requested_at)
		FROM usage_events %s
	`, where)

	err := s.db.QueryRow(query, args...).Scan(
		&totalReqs, &totalTokens, &succCount,
		&inputToks, &outputToks, &readToks, &writeToks, &reasToks,
		&costMicro, &minReq, &maxReq,
	)
	if err != nil {
		return nil, err
	}

	var allCount int64
	s.db.QueryRow("SELECT COUNT(*) FROM usage_events").Scan(&allCount)

	status := "ready"
	if allCount == 0 {
		status = "empty"
	}

	successRate := 0.0
	if totalReqs > 0 {
		successRate = float64(succCount) / float64(totalReqs) * 100.0
	}

	cacheHitRate := 0.0
	if inputToks+readToks > 0 {
		cacheHitRate = float64(readToks) / float64(inputToks+readToks) * 100.0
	}

	tps := 0.0
	if totalReqs > 0 && minReq.Valid && maxReq.Valid {
		t1, _ := time.Parse(time.RFC3339, minReq.String)
		t2, _ := time.Parse(time.RFC3339, maxReq.String)
		diffSec := t2.Sub(t1).Seconds()
		if diffSec > 1 {
			tps = float64(totalTokens) / diffSec
		} else {
			tps = float64(totalTokens)
		}
	}

	trendQuery := fmt.Sprintf(`
		SELECT
			strftime('%%Y-%%m-%%dT%%H:00:00Z', requested_at) as bucket,
			COUNT(*),
			SUM(total_tokens),
			SUM(cost_micro)
		FROM usage_events %s
		GROUP BY bucket
		ORDER BY bucket ASC
		LIMIT 400
	`, where)

	rows, err := s.db.Query(trendQuery, args...)
	var trend []TrendPoint
	if err == nil {
		defer rows.Close()
		for rows.Next() {
			var p TrendPoint
			var cMicro int64
			if err := rows.Scan(&p.Timestamp, &p.Requests, &p.Tokens, &cMicro); err == nil {
				p.CostUSD = float64(cMicro) / 1000000.0
				trend = append(trend, p)
			}
		}
	}

	return &OverviewResult{
		Status:           status,
		TotalEventsCount: allCount,
		Metrics: MetricCards{
			TotalRequests:    totalReqs,
			TotalTokens:      totalTokens,
			SuccessRate:      successRate,
			TPS:              tps,
			CacheHitRate:     cacheHitRate,
			EstimatedCostUSD: float64(costMicro) / 1000000.0,
		},
		Trend: trend,
		Tokens: TokenComposition{
			Input:      inputToks,
			Output:     outputToks,
			CacheRead:  readToks,
			CacheWrite: writeToks,
			Reasoning:  reasToks,
		},
	}, nil
}

func (s *Store) GetAnalytics(f Filter) (*AnalyticsResult, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()

	where, args := s.buildFilterWhere(f)

	runRankQuery := func(column, aliasCol string) []RankItem {
		q := fmt.Sprintf(`
			SELECT
				%s as k,
				%s as n,
				COUNT(*) as reqs,
				SUM(total_tokens) as toks,
				SUM(cost_micro) as cost
			FROM usage_events %s
			GROUP BY k
			ORDER BY reqs DESC
			LIMIT 100
		`, column, aliasCol, where)

		rows, err := s.db.Query(q, args...)
		if err != nil {
			return []RankItem{}
		}
		defer rows.Close()

		var list []RankItem
		var sumReqs int64
		for rows.Next() {
			var it RankItem
			var cMicro int64
			if err := rows.Scan(&it.Key, &it.Name, &it.Requests, &it.Tokens, &cMicro); err == nil {
				it.CostUSD = float64(cMicro) / 1000000.0
				sumReqs += it.Requests
				list = append(list, it)
			}
		}
		for i := range list {
			if sumReqs > 0 {
				list[i].Percent = float64(list[i].Requests) / float64(sumReqs) * 100.0
			}
		}
		return list
	}

	return &AnalyticsResult{
		Status:      "ready",
		ByModel:     runRankQuery("model", "COALESCE(NULLIF(model_alias, ''), model)"),
		ByProvider:  runRankQuery("provider", "provider"),
		BySource:    runRankQuery("source", "source"),
		ByAccessKey: runRankQuery("access_key_id", "COALESCE(NULLIF(access_key_name, ''), access_key_id)"),
		ByHour:      runRankQuery("strftime('%H', requested_at)", "strftime('%H', requested_at) || ':00'"),
	}, nil
}

func (s *Store) GetEvents(f Filter) (*EventsResult, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()

	where, args := s.buildFilterWhere(f)

	var total int64
	s.db.QueryRow(fmt.Sprintf("SELECT COUNT(*) FROM usage_events %s", where), args...).Scan(&total)

	limit := f.Limit
	if limit <= 0 || limit > 100 {
		limit = 50
	}
	offset := f.Offset
	if offset < 0 {
		offset = 0
	}

	sortBy := "requested_at"
	switch f.SortBy {
	case "latency":
		sortBy = "latency_ms"
	case "tokens":
		sortBy = "total_tokens"
	case "cost":
		sortBy = "cost_micro"
	}

	sortDir := "DESC"
	if strings.EqualFold(f.SortDir, "asc") {
		sortDir = "ASC"
	}

	qArgs := append(args, limit, offset)
	q := fmt.Sprintf(`
		SELECT
			id, requested_at, latency_ms, ttft_ms, provider, account_id,
			model, model_alias, source, endpoint, access_key_id, access_key_name,
			result, http_status, error_code, error_summary,
			input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
			reasoning_tokens, total_tokens, cost_micro, service_tier, created_at
		FROM usage_events %s
		ORDER BY %s %s
		LIMIT ? OFFSET ?
	`, where, sortBy, sortDir)

	rows, err := s.db.Query(q, qArgs...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var events []Event
	for rows.Next() {
		var e Event
		var reqStr, creStr string
		err := rows.Scan(
			&e.ID, &reqStr, &e.LatencyMs, &e.TTFTMs, &e.Provider, &e.AccountID,
			&e.Model, &e.ModelAlias, &e.Source, &e.Endpoint, &e.AccessKeyID, &e.AccessKeyName,
			&e.Result, &e.HTTPStatus, &e.ErrorCode, &e.ErrorSummary,
			&e.InputTokens, &e.OutputTokens, &e.CacheReadTokens, &e.CacheWriteTokens,
			&e.ReasoningTokens, &e.TotalTokens, &e.CostMicro, &e.ServiceTier, &creStr,
		)
		if err == nil {
			e.RequestedAt, _ = time.Parse(time.RFC3339Nano, reqStr)
			e.CreatedAt, _ = time.Parse(time.RFC3339Nano, creStr)
			events = append(events, e)
		}
	}

	page := (offset / limit) + 1
	hasMore := int64(offset+len(events)) < total

	return &EventsResult{
		Events:   events,
		Total:    total,
		Page:     page,
		PageSize: limit,
		HasMore:  hasMore,
	}, nil
}
