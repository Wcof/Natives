package usage

// v4 只增加一条非敏感语义列：供应商是否实际返回了 usage。
// 旧记录均来自带 usage 的代理 Record，默认 reported；迁移幂等。
func (s *Store) migrateV4() error {
	if _, err := s.db.Exec(`ALTER TABLE usage_events ADD COLUMN usage_status TEXT NOT NULL DEFAULT 'reported'`); err != nil {
		if !isDuplicateColumnErr(err) {
			return err
		}
	}
	_, err := s.db.Exec(`UPDATE usage_metadata SET schema_version = 4 WHERE id = 1 AND schema_version < 4`)
	return err
}
