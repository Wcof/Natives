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
	return insertEventIfAbsent(e, s.db.Exec)
}

// 调用方持有 Store 锁；采集事务与普通事件写入共享同一入库路径。
func insertEventIfAbsent(e *Event, exec func(string, ...any) (sql.Result, error)) (bool, error) {
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
			reasoning_tokens, total_tokens, cost_micro, service_tier, created_at,
			billing_atom, session_id, tool_id, source_instance_id
		) VALUES (
			?, ?, ?, ?, ?, ?,
			?, ?, ?, ?, ?, ?,
			?, ?, ?, ?,
			?, ?, ?, ?,
			?, ?, ?, ?, ?,
			?, ?, ?, ?
		)
		`
	result, err := exec(
		query,
		e.ID, e.RequestedAt.Format(time.RFC3339Nano), e.LatencyMs, e.TTFTMs, e.Provider, e.AccountID,
		e.Model, e.ModelAlias, e.Source, e.Endpoint, e.AccessKeyID, e.AccessKeyName,
		string(e.Result), e.HTTPStatus, e.ErrorCode, e.ErrorSummary,
		e.InputTokens, e.OutputTokens, e.CacheReadTokens, e.CacheWriteTokens,
		e.ReasoningTokens, e.TotalTokens, e.CostMicro, e.ServiceTier, e.CreatedAt.Format(time.RFC3339Nano),
		e.BillingAtom, e.SessionID, e.ToolID, e.SourceInstanceID,
	)
	if err == nil {
		inserted, resultErr := result.RowsAffected()
		if resultErr != nil {
			return false, resultErr
		}
		if inserted > 0 {
			_, err = exec("UPDATE usage_metadata SET usage_revision = usage_revision + 1 WHERE id = 1")
		}
		return inserted > 0, err
	}
	return false, err
}

func (s *Store) buildFilterWhere(f Filter) (string, []interface{}) {
	return s.buildFilterWhereIn(f, time.UTC)
}

// buildFilterWhereIn 同 buildFilterWhere，但 today/时区相关边界按 loc 计算
// （整改 E2 §4.2：today 为用户本地零点；其余范围仍以 UTC 即时表达）。
func (s *Store) buildFilterWhereIn(f Filter, loc *time.Location) (string, []interface{}) {
	var clauses []string
	var args []interface{}
	if f.SessionKey != "" {
		parts := strings.SplitN(f.SessionKey, "|", 3)
		if len(parts) != 3 {
			clauses = append(clauses, "0")
		} else {
			clauses = append(clauses, "tool_id = ? AND source_instance_id = ? AND session_id = ?")
			for _, part := range parts {
				args = append(args, part)
			}
		}
	}

	now := time.Now().UTC()
	switch f.Range {
	case "4h":
		clauses = append(clauses, "requested_at >= ?")
		args = append(args, now.Add(-4*time.Hour).Format(time.RFC3339))
	case "24h":
		clauses = append(clauses, "requested_at >= ?")
		args = append(args, now.Add(-24*time.Hour).Format(time.RFC3339))
	case "72h":
		clauses = append(clauses, "requested_at >= ?")
		args = append(args, now.Add(-72*time.Hour).Format(time.RFC3339))
	case "today":
		todayStart := todayStartUTC(now, loc)
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
	loc, err := resolveTimezone(f.Timezone)
	if err != nil {
		return nil, err
	}
	s.mu.RLock()
	where, args := s.buildFilterWhereIn(f, loc)

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

	err = s.db.QueryRow(query, args...).Scan(
		&totalReqs, &totalTokens, &succCount,
		&inputToks, &outputToks, &readToks, &writeToks, &reasToks,
		&costMicro, &minReq, &maxReq,
	)
	if err != nil {
		s.mu.RUnlock()
		return nil, err
	}

	// 本地采集的用量 cost_micro=0（无计费），零成本部分按 (provider, model)
	// 聚合 token 分量，锁外按价格目录动态估算（FindPrice 内部再取读锁，
	// 非重入锁在持锁期间调用会死锁——与 GetPricingCatalog 同一纪律）。
	zeroRows, err := s.zeroCostRowsLocked("''", where, args)
	if err != nil {
		s.mu.RUnlock()
		return nil, err
	}

	var allCount int64
	if err := s.db.QueryRow("SELECT COUNT(*) FROM usage_events").Scan(&allCount); err != nil {
		s.mu.RUnlock()
		return nil, err
	}

	// 历史口径（T0）：按范围选桶，序列必须覆盖整个请求范围，不做 LIMIT 截断。
	// 短范围（4h/24h/today）用小时桶（≤24 点）；长范围（7d/30d/all/custom）
	// 用日桶（30d=30 点，all 有界为日粒度）。旧的固定小时桶 + LIMIT 400
	// 在满负载 30 天（720 小时）时会从第 400 小时处截断且总数对不上。
	// 整改 E2 §4.2：提供用户时区时，小时/日桶按该时区真实夏令时边界分桶。
	bucketExpr := trendBucketSQLExpr(nil, false)
	if f.Range == "7d" || f.Range == "30d" || f.Range == "all" || f.Range == "custom" || f.Range == "" {
		bucketExpr = trendBucketSQLExpr(s.tzSegmentsForFilter(f, loc), true)
	} else if loc != time.UTC {
		bucketExpr = trendBucketSQLExpr(s.tzSegmentsForFilter(f, loc), false)
	}
	trendQuery := fmt.Sprintf(`
		SELECT
			%s as bucket,
			COUNT(*),
			SUM(total_tokens),
			SUM(cost_micro)
		FROM usage_events %s
		GROUP BY bucket
		ORDER BY bucket ASC
	`, bucketExpr, where)

	rows, err := s.db.Query(trendQuery, args...)
	if err != nil {
		s.mu.RUnlock()
		return nil, err
	}
	var trend []TrendPoint
	if err == nil {
		for rows.Next() {
			var p TrendPoint
			var cMicro int64
			if err := rows.Scan(&p.Timestamp, &p.Requests, &p.Tokens, &cMicro); err != nil {
				rows.Close()
				s.mu.RUnlock()
				return nil, err
			}
			p.CostUSD = float64(cMicro) / 1000000.0
			trend = append(trend, p)
		}
		if err := rows.Err(); err != nil {
			rows.Close()
			s.mu.RUnlock()
			return nil, err
		}
		rows.Close()
	}
	s.mu.RUnlock()
	metricsPriced := true
	var zeroCostMicro int64
	estimates, err := estimateZeroCostByGroup(s, zeroRows)
	if err != nil {
		return nil, err
	}
	for _, v := range estimates {
		zeroCostMicro += v.Micro
		metricsPriced = metricsPriced && v.Priced
	}
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

	return &OverviewResult{
		Status:           status,
		TotalEventsCount: allCount,
		Metrics: MetricCards{
			TotalRequests:    totalReqs,
			TotalTokens:      totalTokens,
			SuccessRate:      successRate,
			TPS:              tps,
			CacheHitRate:     cacheHitRate,
			EstimatedCostUSD: float64(costMicro+zeroCostMicro) / 1000000.0,
			Priced:           metricsPriced,
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
	loc, err := resolveTimezone(f.Timezone)
	if err != nil {
		return nil, err
	}
	s.mu.RLock()
	where, args := s.buildFilterWhereIn(f, loc)

	var rankError error
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
			rankError = err
			return nil
		}
		defer rows.Close()

		var list []RankItem
		var sumReqs int64
		for rows.Next() {
			var it RankItem
			var cMicro int64
			if err := rows.Scan(&it.Key, &it.Name, &it.Requests, &it.Tokens, &cMicro); err != nil {
				rankError = err
				return nil
			} else {
				it.CostUSD = float64(cMicro) / 1000000.0
				sumReqs += it.Requests
				list = append(list, it)
			}
		}
		if err := rows.Err(); err != nil {
			rankError = err
			return nil
		}
		for i := range list {
			if sumReqs > 0 {
				list[i].Percent = float64(list[i].Requests) / float64(sumReqs) * 100.0
			}
		}
		return list
	}

	byModel, byModelZeroRows, err := s.analyticsByModelLocked(where, args)
	if err != nil {
		s.mu.RUnlock()
		return nil, err
	}
	byProvider := runRankQuery("provider", "provider")
	bySource := runRankQuery("source", "source")
	byAccessKey := runRankQuery("access_key_id", "COALESCE(NULLIF(access_key_name, ''), access_key_id)")
	byHour := runRankQuery("strftime('%H', requested_at)", "strftime('%H', requested_at) || ':00'")
	s.mu.RUnlock()
	if rankError != nil {
		return nil, rankError
	}

	// 锁外按价格目录估算零成本行（FindPrice 内部会再取读锁，非重入）
	byKeyCost, err := estimateZeroCostByGroup(s, byModelZeroRows)
	if err != nil {
		return nil, err
	}
	for i := range byModel {
		if est, ok := byKeyCost[byModel[i].Key]; ok {
			byModel[i].CostUSD += float64(est.Micro) / 1000000.0
			byModel[i].Priced = est.Priced
		}
	}

	return &AnalyticsResult{
		Status:      "ready",
		ByModel:     byModel,
		ByProvider:  byProvider,
		BySource:    bySource,
		ByAccessKey: byAccessKey,
		ByHour:      byHour,
	}, nil
}

// analyticsByModelLocked 为「使用情况」的模型聚合（读锁内调用）：显示名（别名
// 优先）分组、按 token 降序，并返回零成本行的 (provider, model) token 分量
// （Key=显示名）供调用方解锁后按价格目录估算金额。返回的两个切片均已完成扫描。
func (s *Store) analyticsByModelLocked(where string, args []any) ([]RankItem, []zeroCostRow, error) {
	q := fmt.Sprintf(`
		SELECT COALESCE(NULLIF(model_alias, ''), model) as k,
			COUNT(*) as reqs,
			COALESCE(SUM(total_tokens), 0) as toks,
			COALESCE(SUM(cost_micro), 0) as cost
		FROM usage_events %s
		GROUP BY k
		ORDER BY toks DESC
		LIMIT 100
	`, where)
	var list []RankItem
	rows, err := s.db.Query(q, args...)
	if err != nil {
		return nil, nil, err
	}
	if err == nil {
		for rows.Next() {
			var it RankItem
			var cMicro int64
			if err := rows.Scan(&it.Key, &it.Requests, &it.Tokens, &cMicro); err != nil {
				rows.Close()
				return nil, nil, err
			} else {
				it.Name = it.Key
				it.CostUSD = float64(cMicro) / 1000000.0
				it.Priced = true
				list = append(list, it)
			}
		}
		if err := rows.Err(); err != nil {
			rows.Close()
			return nil, nil, err
		}
		rows.Close()
	}

	zeroRows, err := s.zeroCostRowsLocked("COALESCE(NULLIF(model_alias, ''), model)", where, args)
	if err != nil {
		return nil, nil, err
	}

	return list, zeroRows, nil
}

func (s *Store) GetEvents(f Filter) (*EventsResult, error) {
	loc, err := resolveTimezone(f.Timezone)
	if err != nil {
		return nil, err
	}
	s.mu.RLock()
	defer s.mu.RUnlock()

	where, args := s.buildFilterWhereIn(f, loc)

	var total int64
	if err := s.db.QueryRow(fmt.Sprintf("SELECT COUNT(*) FROM usage_events %s", where), args...).Scan(&total); err != nil {
		return nil, err
	}

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
			reasoning_tokens, total_tokens, cost_micro, service_tier, created_at,
			session_id, tool_id, source_instance_id
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
			&e.SessionID, &e.ToolID, &e.SourceInstanceID,
		)
		if err != nil {
			return nil, err
		}
		e.RequestedAt, _ = time.Parse(time.RFC3339Nano, reqStr)
		e.CreatedAt, _ = time.Parse(time.RFC3339Nano, creStr)
		events = append(events, e)
	}
	if err := rows.Err(); err != nil {
		return nil, err
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

// GetSessions 返回真实会话聚合（R3；整改 E2 §4.1/§4.2）。
// distinct 键是三元 (tool_id, source_instance_id, session_id)：同 session ID
// 跨工具、跨 profile/安装实例不合并；无 session_id 的记录只计入
// Unattributed，不造会话；每日 distinct 独立去重，区间总数是 distinct 键数，
// 不等于各日相加。活跃天数与日桶按用户时区（真实夏令时边界）计算。
func (s *Store) GetSessions(f Filter) (*SessionsResult, error) {
	loc, err := resolveTimezone(f.Timezone)
	if err != nil {
		return nil, err
	}
	// 读锁只覆盖查询阶段；会话金额的动态计价（FindPrice 内部再取读锁，非重入）
	// 在解锁后进行。
	s.mu.RLock()

	where, args := s.buildFilterWhereIn(f, loc)
	sessKey := "tool_id || '|' || source_instance_id || '|' || session_id"

	var totalSessions, unattributed int64
	err = s.db.QueryRow(fmt.Sprintf(`
		SELECT
			COUNT(DISTINCT CASE WHEN session_id != '' THEN %s END),
			COALESCE(SUM(CASE WHEN session_id = '' THEN 1 ELSE 0 END), 0)
		FROM usage_events %s
	`, sessKey, where), args...).Scan(&totalSessions, &unattributed)
	if err != nil {
		s.mu.RUnlock()
		return nil, err
	}

	var allCount int64
	if err := s.db.QueryRow("SELECT COUNT(*) FROM usage_events").Scan(&allCount); err != nil {
		s.mu.RUnlock()
		return nil, err
	}
	status := "ready"
	if allCount == 0 {
		status = "empty"
	}

	// 子查询需要附加 session 条件；where 为空时不能拼出 "FROM x AND ..."。
	sessWhere := strings.TrimSpace(where)
	if sessWhere == "" {
		sessWhere = "WHERE session_id != ''"
	} else {
		sessWhere += " AND session_id != ''"
	}

	// 每日 distinct（用户时区日界，真实夏令时分段见 store_timezone.go）。
	dayExpr := localDaySQLExpr(s.tzSegmentsForFilter(f, loc))
	dayRows, err := s.db.Query(fmt.Sprintf(`
		SELECT day, COUNT(*) FROM (
			SELECT %s as day,
				%s as sk
			FROM usage_events %s
			GROUP BY day, sk
		) GROUP BY day ORDER BY day ASC
	`, dayExpr, sessKey, sessWhere), args...)
	if err != nil {
		s.mu.RUnlock()
		return nil, err
	}
	var byDay []DayCount
	activeDays := int64(0)
	if err == nil {
		defer dayRows.Close()
		for dayRows.Next() {
			var d DayCount
			if err := dayRows.Scan(&d.Date, &d.Sessions); err != nil {
				s.mu.RUnlock()
				return nil, err
			} else {
				byDay = append(byDay, d)
				activeDays++
			}
		}
		if err := dayRows.Err(); err != nil {
			s.mu.RUnlock()
			return nil, err
		}
		dayRows.Close()
	}

	// 按工具排名：每个工具的 distinct 三元会话数。
	srcRows, err := s.db.Query(fmt.Sprintf(`
		SELECT tool_id as k, tool_id as n, COUNT(DISTINCT %s) as sessions,
			COUNT(*) as reqs, COALESCE(SUM(total_tokens),0) as toks, COALESCE(SUM(cost_micro),0) as cost
		FROM usage_events %s
		GROUP BY tool_id ORDER BY sessions DESC LIMIT 100
	`, sessKey, sessWhere), args...)
	if err != nil {
		s.mu.RUnlock()
		return nil, err
	}
	var bySource []RankItem
	if err == nil {
		defer srcRows.Close()
		for srcRows.Next() {
			var it RankItem
			var cMicro int64
			var sessions int64
			// RankItem.Requests 在此承载 distinct 会话数；Tokens 承载该工具总 Token。
			if err := srcRows.Scan(&it.Key, &it.Name, &sessions, &it.Requests, &it.Tokens, &cMicro); err != nil {
				s.mu.RUnlock()
				return nil, err
			} else {
				it.Requests = sessions
				it.Name = it.Key
				it.CostUSD = float64(cMicro) / 1000000.0
				bySource = append(bySource, it)
			}
		}
		if err := srcRows.Err(); err != nil {
			s.mu.RUnlock()
			return nil, err
		}
		srcRows.Close()
	}

	// 会话明细（分页）：三元键逐一展开；标题取 usage_subjects（kind=session，
	// 由来源适配器显式注册），零成本行按 (会话, provider, model) 细分以供
	// 解锁后按价格目录动态估算金额。
	limit := f.Limit
	if limit <= 0 || limit > 100 {
		limit = 50
	}
	offset := f.Offset
	if offset < 0 {
		offset = 0
	}
	sessRows, err := s.db.Query(fmt.Sprintf(`
		SELECT s.sk, s.tool_id, s.source_instance_id, s.reqs, s.toks, s.cost,
			s.first_at, s.last_at, COALESCE(us.label, '')
		FROM (
			SELECT %s as sk, tool_id, source_instance_id, COUNT(*) as reqs,
				COALESCE(SUM(total_tokens),0) as toks, COALESCE(SUM(cost_micro),0) as cost,
				MIN(requested_at) as first_at, MAX(requested_at) as last_at, session_id
			FROM usage_events %s
			GROUP BY sk ORDER BY MAX(requested_at) DESC
			LIMIT ? OFFSET ?
		) s
		LEFT JOIN usage_subjects us ON us.kind = 'session' AND us.id = s.session_id
	`, sessKey, sessWhere), append(args, limit, offset)...)
	if err != nil {
		s.mu.RUnlock()
		return nil, err
	}
	var sessions []SessionStat
	if err == nil {
		for sessRows.Next() {
			var st SessionStat
			var cMicro int64
			var firstStr, lastStr string
			if err := sessRows.Scan(&st.Key, &st.ToolID, &st.SourceInstanceID, &st.Requests, &st.Tokens, &cMicro, &firstStr, &lastStr, &st.Title); err != nil {
				sessRows.Close()
				s.mu.RUnlock()
				return nil, err
			} else {
				st.CostUSD = float64(cMicro) / 1000000.0
				st.Priced = true
				st.FirstAt = firstStr
				st.LastAt = lastStr
				sessions = append(sessions, st)
			}
		}
		if err := sessRows.Err(); err != nil {
			sessRows.Close()
			s.mu.RUnlock()
			return nil, err
		}
		sessRows.Close()
	}

	// 零成本 token 分量按 (会话三元键, provider, model) 聚合，Key=sk
	zeroRows, err := s.zeroCostRowsLocked(sessKey, sessWhere, args)
	if err != nil {
		s.mu.RUnlock()
		return nil, err
	}

	s.mu.RUnlock()

	byKeyCost, err := estimateZeroCostByGroup(s, zeroRows)
	if err != nil {
		return nil, err
	}
	for i := range sessions {
		if est, ok := byKeyCost[sessions[i].Key]; ok {
			sessions[i].CostUSD += float64(est.Micro) / 1000000.0
			sessions[i].Priced = est.Priced
		}
	}

	return &SessionsResult{
		Status:        status,
		TotalSessions: totalSessions,
		ActiveDays:    activeDays,
		Unattributed:  unattributed,
		ByDay:         byDay,
		BySource:      bySource,
		Sessions:      sessions,
		Total:         totalSessions,
		Page:          (offset / limit) + 1,
		PageSize:      limit,
		HasMore:       int64(offset+len(sessions)) < totalSessions,
	}, nil
}

// tzSegmentsForFilter 计算 filter 时间范围内用户时区的夏令时分段；
// all/all 缺省范围用数据库极值兜底，避免区间外事件落进错误偏移分支。
func (s *Store) tzSegmentsForFilter(f Filter, loc *time.Location) []tzSegment {
	if loc == time.UTC {
		return nil
	}
	now := time.Now().UTC()
	start, end := filterTimeRange(f, now, loc)
	if f.Range == "all" || f.Range == "" {
		var minReq, maxReq sql.NullString
		if err := s.db.QueryRow("SELECT MIN(requested_at), MAX(requested_at) FROM usage_events").Scan(&minReq, &maxReq); err == nil {
			if t, perr := time.Parse(time.RFC3339Nano, minReq.String); perr == nil && !t.IsZero() {
				start = t.UTC()
			}
			if t, perr := time.Parse(time.RFC3339Nano, maxReq.String); perr == nil && !t.IsZero() {
				end = t.UTC()
			}
		}
	}
	if start.IsZero() || end.IsZero() || !start.Before(end) {
		return nil
	}
	return localDaySegments(loc, start, end)
}
