package usage

import "fmt"

// 调用方持有 Store 读锁；三种面板投影共用零计费记录的完整读取。
func (s *Store) zeroCostRowsLocked(key, where string, args []any) ([]zeroCostRow, error) {
	if where == "" {
		where = "WHERE cost_micro = 0 AND total_tokens > 0"
	} else {
		where += " AND cost_micro = 0 AND total_tokens > 0"
	}
	rows, err := s.db.Query(fmt.Sprintf(`SELECT %s AS k, provider, model,
		COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
		COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0)
		FROM usage_events %s GROUP BY k, provider, model`, key, where), args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var result []zeroCostRow
	for rows.Next() {
		var row zeroCostRow
		if err := rows.Scan(&row.Key, &row.Provider, &row.Model, &row.In, &row.Out, &row.CR, &row.CW); err != nil {
			return nil, err
		}
		result = append(result, row)
	}
	return result, rows.Err()
}
