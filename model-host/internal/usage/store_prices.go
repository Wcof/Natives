package usage

import (
	"fmt"
	"time"
)

func (s *Store) GetPrices() ([]Price, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	rows, err := s.db.Query(`SELECT id, provider_id, model_id, input_price_micro, output_price_micro,
		cache_read_price_micro, cache_write_price_micro, source, updated_at FROM model_prices ORDER BY provider_id, model_id`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var prices []Price
	for rows.Next() {
		var price Price
		if err := rows.Scan(&price.ID, &price.ProviderID, &price.ModelID, &price.InputPriceMicro, &price.OutputPriceMicro,
			&price.CacheReadPriceMicro, &price.CacheWritePriceMicro, &price.Source, &price.UpdatedAt); err != nil {
			return nil, err
		}
		prices = append(prices, price)
	}
	return prices, rows.Err()
}

func (s *Store) UpsertPrice(price *Price) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if price.ID == "" {
		price.ID = fmt.Sprintf("prc_%d", time.Now().UnixNano())
	}
	price.UpdatedAt = time.Now().UTC().Format(time.RFC3339)
	_, err := s.db.Exec(`INSERT INTO model_prices (id, provider_id, model_id, input_price_micro, output_price_micro,
		cache_read_price_micro, cache_write_price_micro, source, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
		ON CONFLICT(provider_id, model_id) DO UPDATE SET input_price_micro=excluded.input_price_micro,
		output_price_micro=excluded.output_price_micro, cache_read_price_micro=excluded.cache_read_price_micro,
		cache_write_price_micro=excluded.cache_write_price_micro, source=excluded.source, updated_at=excluded.updated_at
		WHERE excluded.source NOT IN ('synced','imported') OR model_prices.source NOT IN ('manual','manual_global')`,
		price.ID, price.ProviderID, price.ModelID, price.InputPriceMicro, price.OutputPriceMicro,
		price.CacheReadPriceMicro, price.CacheWritePriceMicro, string(price.Source), price.UpdatedAt)
	if err == nil {
		_, err = s.db.Exec("UPDATE usage_metadata SET usage_revision=usage_revision+1 WHERE id=1")
	}
	return err
}

func (s *Store) UpsertCustomPrice(price Price) error { return s.UpsertPrice(&price) }
func (s *Store) DeleteCustomPrice(provider, model string) error {
	return s.deletePrice("provider_id=? AND model_id=?", provider, model)
}
func (s *Store) DeletePrice(id string) error { return s.deletePrice("id=?", id) }

func (s *Store) deletePrice(where string, values ...any) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if _, err := s.db.Exec("DELETE FROM model_prices WHERE "+where, values...); err != nil {
		return err
	}
	_, err := s.db.Exec("UPDATE usage_metadata SET usage_revision=usage_revision+1 WHERE id=1")
	return err
}
