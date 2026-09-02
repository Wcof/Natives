package usage

import (
	"crypto/rand"
	"crypto/sha256"
	"database/sql"
	"encoding/base64"
	"encoding/hex"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"time"
)

const (
	ImportChunkSize  = 524288 // 512 KiB
	MaxImportSize    = 2 << 30
	MaxImports       = 2
	ImportSessionTTL = 15 * time.Minute
)

type ImportSession struct {
	ID           string
	FileName     string
	FileSize     int64
	StagingPath  string
	CreatedAt    time.Time
	TotalBytes   int64
	ReceivedSize int64
	TotalChunks  int
	File         *os.File
}

type Importer struct {
	store      *Store
	calculator *Calculator
	sessions   map[string]*ImportSession
	mu         sync.Mutex
}

func NewImporter(store *Store, calculator ...*Calculator) *Importer {
	var calc *Calculator
	if len(calculator) > 0 {
		calc = calculator[0]
	}
	return &Importer{
		store:      store,
		calculator: calc,
		sessions:   make(map[string]*ImportSession),
	}
}

func (imp *Importer) BeginSession(fileName string, fileSize int64) (*ImportSession, error) {
	buffer := make([]byte, 16)
	if _, err := rand.Read(buffer); err != nil {
		return nil, fmt.Errorf("create import session: %w", err)
	}
	uploadID := "up_" + hex.EncodeToString(buffer)
	return imp.beginWithID(uploadID, fileName, fileSize)
}

func (imp *Importer) beginWithID(id, fileName string, fileSize int64) (*ImportSession, error) {
	imp.mu.Lock()
	defer imp.mu.Unlock()
	imp.expireSessionsLocked()
	if fileSize < 0 || fileSize > MaxImportSize {
		return nil, fmt.Errorf("import file exceeds 2 GiB limit")
	}
	if len(imp.sessions) >= MaxImports {
		return nil, fmt.Errorf("too many active import sessions")
	}

	stagingDir := filepath.Dir(imp.store.path)
	stagingPath := filepath.Join(stagingDir, fmt.Sprintf("staging_%s.db", id))

	file, err := os.OpenFile(stagingPath, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0600)
	if err != nil {
		return nil, fmt.Errorf("create staging file: %w", err)
	}

	session := &ImportSession{
		ID:          id,
		FileName:    fileName,
		FileSize:    fileSize,
		StagingPath: stagingPath,
		CreatedAt:   time.Now(),
		File:        file,
	}
	imp.sessions[id] = session
	return session, nil
}

func (imp *Importer) expireSessionsLocked() {
	cutoff := time.Now().Add(-ImportSessionTTL)
	for id, session := range imp.sessions {
		if session.CreatedAt.Before(cutoff) {
			if session.File != nil {
				_ = session.File.Close()
			}
			_ = os.Remove(session.StagingPath)
			delete(imp.sessions, id)
		}
	}
}

func (imp *Importer) AppendChunk(uploadID string, chunkIndex int, base64Data string) (*ImportSession, error) {
	imp.mu.Lock()
	defer imp.mu.Unlock()
	session, exists := imp.sessions[uploadID]

	if !exists || session.File == nil {
		return nil, fmt.Errorf("import session not found")
	}

	dataBytes, err := base64.StdEncoding.DecodeString(base64Data)
	if err != nil {
		return nil, fmt.Errorf("decode base64: %w", err)
	}
	if chunkIndex != session.TotalChunks || len(dataBytes) > ImportChunkSize {
		return nil, fmt.Errorf("import chunks must be sequential and at most %d bytes", ImportChunkSize)
	}
	if session.ReceivedSize+int64(len(dataBytes)) > session.FileSize {
		return nil, fmt.Errorf("import data exceeds declared file size")
	}

	offset := int64(chunkIndex) * ImportChunkSize
	if _, err := session.File.WriteAt(dataBytes, offset); err != nil {
		return nil, fmt.Errorf("write chunk: %w", err)
	}

	session.ReceivedSize += int64(len(dataBytes))
	session.TotalBytes += int64(len(dataBytes))
	session.TotalChunks++
	return session, nil
}

func (imp *Importer) PreviewSession(uploadID string) (*ImportPreviewResult, error) {
	return imp.Preview(uploadID)
}

func (imp *Importer) Preview(uploadID string) (*ImportPreviewResult, error) {
	imp.mu.Lock()
	defer imp.mu.Unlock()
	session, exists := imp.sessions[uploadID]

	if !exists {
		return nil, fmt.Errorf("import session not found")
	}
	if session.FileSize > 0 && session.ReceivedSize != session.FileSize {
		return &ImportPreviewResult{Valid: false, Error: "upload is incomplete"}, nil
	}

	if session.File != nil {
		session.File.Close()
		session.File = nil
	}

	db, err := sql.Open("sqlite", session.StagingPath)
	if err != nil {
		return &ImportPreviewResult{Valid: false, Error: "invalid sqlite file"}, nil
	}
	defer db.Close()
	if _, err := db.Exec("PRAGMA query_only = ON"); err != nil {
		return &ImportPreviewResult{Valid: false, Error: "invalid sqlite file"}, nil
	}

	var totalRecords int64
	var minReq, maxReq sql.NullString
	columns, err := tableColumns(db, "usage_events")
	if err != nil {
		return &ImportPreviewResult{Valid: false, Error: "unsupported schema: missing usage_events table"}, nil
	}
	timeColumn := "requested_at"
	if columns["timestamp_ms"] {
		timeColumn = "timestamp"
	} else if !columns["requested_at"] {
		return &ImportPreviewResult{Valid: false, Error: "unsupported usage_events schema"}, nil
	}
	row := db.QueryRow("SELECT COUNT(*), MIN(" + timeColumn + "), MAX(" + timeColumn + ") FROM usage_events")
	if err := row.Scan(&totalRecords, &minReq, &maxReq); err != nil {
		return &ImportPreviewResult{Valid: false, Error: "unsupported schema: missing usage_events table"}, nil
	}

	var pricesCount int64
	if hasTable(db, "model_prices") {
		_ = db.QueryRow("SELECT COUNT(*) FROM model_prices").Scan(&pricesCount)
	}

	return &ImportPreviewResult{
		Valid:          true,
		TotalRecords:   totalRecords,
		EarliestRecord: minReq.String,
		LatestRecord:   maxReq.String,
		PricesCount:    pricesCount,
	}, nil
}

func (imp *Importer) CommitSession(uploadID string) (int64, error) {
	return imp.Commit(uploadID)
}

func (imp *Importer) Commit(uploadID string) (int64, error) {
	imp.mu.Lock()
	session, exists := imp.sessions[uploadID]
	delete(imp.sessions, uploadID)
	imp.mu.Unlock()

	if !exists {
		return 0, fmt.Errorf("import session not found")
	}

	if session.File != nil {
		session.File.Close()
		session.File = nil
	}
	defer os.Remove(session.StagingPath)

	srcDb, err := sql.Open("sqlite", session.StagingPath)
	if err != nil {
		return 0, fmt.Errorf("open staging db: %w", err)
	}
	defer srcDb.Close()
	if _, err := srcDb.Exec("PRAGMA query_only = ON"); err != nil {
		return 0, fmt.Errorf("open staging database read-only: %w", err)
	}
	columns, err := tableColumns(srcDb, "usage_events")
	if err != nil {
		return 0, fmt.Errorf("inspect staging events: %w", err)
	}
	if columns["timestamp_ms"] {
		return imp.commitEasyCLIProxy(srcDb)
	}

	rows, err := srcDb.Query(`
		SELECT
			id, requested_at, latency_ms, ttft_ms, provider, account_id,
			model, model_alias, source, endpoint, access_key_id, access_key_name,
			result, http_status, error_code, error_summary,
			input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
			reasoning_tokens, total_tokens, cost_micro, service_tier, created_at
		FROM usage_events
	`)
	if err != nil {
		return 0, fmt.Errorf("query staging events: %w", err)
	}
	defer rows.Close()

	imp.store.mu.Lock()
	defer imp.store.mu.Unlock()

	tx, err := imp.store.db.Begin()
	if err != nil {
		return 0, fmt.Errorf("begin transaction: %w", err)
	}
	defer tx.Rollback()

	stmt, err := tx.Prepare(`
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
	`)
	if err != nil {
		return 0, err
	}
	defer stmt.Close()

	var count int64
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
		if err != nil {
			return count, err
		}
		if e.ID == "" {
			h := sha256.Sum256([]byte(fmt.Sprintf("%s_%s_%s_%d", reqStr, e.Model, e.Provider, e.TotalTokens)))
			e.ID = fmt.Sprintf("imp_%x", h[:12])
		}
		result, err := stmt.Exec(
			e.ID, reqStr, e.LatencyMs, e.TTFTMs, e.Provider, e.AccountID,
			e.Model, e.ModelAlias, e.Source, e.Endpoint, e.AccessKeyID, e.AccessKeyName,
			string(e.Result), e.HTTPStatus, e.ErrorCode, e.ErrorSummary,
			e.InputTokens, e.OutputTokens, e.CacheReadTokens, e.CacheWriteTokens,
			e.ReasoningTokens, e.TotalTokens, e.CostMicro, e.ServiceTier, creStr,
		)
		if err != nil {
			return count, err
		}
		inserted, err := result.RowsAffected()
		if err != nil {
			return count, err
		}
		count += inserted
	}
	if err := rows.Err(); err != nil {
		return count, err
	}
	if count > 0 {
		if _, err := tx.Exec("UPDATE usage_metadata SET usage_revision = usage_revision + 1 WHERE id = 1"); err != nil {
			return count, err
		}
	}
	if err := tx.Commit(); err != nil {
		return 0, err
	}

	return count, nil
}

func tableColumns(db *sql.DB, table string) (map[string]bool, error) {
	rows, err := db.Query("SELECT name FROM pragma_table_info(?)", table)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	columns := make(map[string]bool)
	for rows.Next() {
		var name string
		if err := rows.Scan(&name); err != nil {
			return nil, err
		}
		columns[name] = true
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	if len(columns) == 0 {
		return nil, fmt.Errorf("table %s not found", table)
	}
	return columns, nil
}

func hasTable(db *sql.DB, table string) bool {
	var exists bool
	_ = db.QueryRow("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?)", table).Scan(&exists)
	return exists
}

func (imp *Importer) commitEasyCLIProxy(src *sql.DB) (int64, error) {
	rows, err := src.Query(`SELECT id, event_key, timestamp, latency_ms, COALESCE(ttft_ms, 0),
		provider, auth_index, model, alias, source, endpoint, api_key_hash, api_key_remark,
		failed, canceled, failure_status, input_tokens, output_tokens, cache_read_tokens,
		cache_creation_tokens, reasoning_tokens, total_tokens, service_tier, created_at
		FROM usage_events ORDER BY id`)
	if err != nil {
		return 0, fmt.Errorf("query EasyCLIProxyAPI usage events: %w", err)
	}
	defer rows.Close()
	var imported int64
	for rows.Next() {
		var sourceID int64
		var eventKey, requestedAt, createdAt string
		var event Event
		var failed, canceled bool
		if err := rows.Scan(&sourceID, &eventKey, &requestedAt, &event.LatencyMs, &event.TTFTMs,
			&event.Provider, &event.AccountID, &event.Model, &event.ModelAlias, &event.Source,
			&event.Endpoint, &event.AccessKeyID, &event.AccessKeyName, &failed, &canceled,
			&event.HTTPStatus, &event.InputTokens, &event.OutputTokens, &event.CacheReadTokens,
			&event.CacheWriteTokens, &event.ReasoningTokens, &event.TotalTokens,
			&event.ServiceTier, &createdAt); err != nil {
			return imported, err
		}
		event.ID = importedEventID(eventKey, requestedAt, sourceID)
		event.RequestedAt, _ = time.Parse(time.RFC3339Nano, requestedAt)
		event.CreatedAt, _ = time.Parse(time.RFC3339Nano, createdAt)
		event.Result = ResultSuccess
		if canceled {
			event.Result = ResultCancelled
		} else if failed {
			event.Result = ResultFailed
		}
		if event.HTTPStatus > 0 {
			event.ErrorSummary = fmt.Sprintf("HTTP %d", event.HTTPStatus)
		}
		if imp.calculator != nil {
			event.CostMicro = imp.calculator.CalculateCost(event.Provider, event.Model, event.InputTokens, event.OutputTokens, event.CacheReadTokens, event.CacheWriteTokens)
		}
		inserted, err := imp.store.InsertEventIfAbsent(&event)
		if err != nil {
			return imported, err
		}
		if inserted {
			imported++
		}
	}
	return imported, rows.Err()
}

func importedEventID(eventKey, requestedAt string, sourceID int64) string {
	digest := sha256.Sum256([]byte(fmt.Sprintf("easy:%s:%s:%d", eventKey, requestedAt, sourceID)))
	return fmt.Sprintf("imp_%x", digest[:12])
}

func (imp *Importer) CancelSession(uploadID string) {
	imp.Cancel(uploadID)
}

func (imp *Importer) Cancel(uploadID string) {
	imp.mu.Lock()
	session, exists := imp.sessions[uploadID]
	delete(imp.sessions, uploadID)
	imp.mu.Unlock()

	if exists {
		if session.File != nil {
			session.File.Close()
		}
		os.Remove(session.StagingPath)
	}
}
