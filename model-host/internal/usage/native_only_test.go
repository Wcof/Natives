package usage

// §9.0.1 第 3 行硬验收："关闭 Proxy 转发，仅导入已授权的原生日志/DB"。
// 验证点：零 Proxy 事件的前提下，原生 parser（kimi-session/1 fixture）
// → InsertEventIfAbsent（collector 同一入库路径）后，
// Token、可计价成本、会话活跃三列均可出现，且来源不含 proxy。

import (
	"testing"
	"time"
)

func TestNativeOnlyCollectionWithoutProxy(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()

	now := time.Date(2026, 9, 12, 10, 0, 0, 0, time.UTC)

	// 成本在事件入库时按价格快照写入聚合字段：先插测试价格再导入
	// （非真实报价，§7.2 验收示例口径）。
	if err := s.UpsertPrice(&Price{
		ProviderID:           "moonshot",
		ModelID:              "kimi-k2",
		InputPriceMicro:      1_000_000, // $1/百万 tokens（测试价格）
		OutputPriceMicro:     1_000_000, // $1/百万 tokens（测试价格）
		CacheReadPriceMicro:  100_000,   // $0.1/百万（测试价格）
		CacheWritePriceMicro: 1_250_000, // $1.25/百万（测试价格）
		Source:               "manual",
	}); err != nil {
		t.Fatalf("upsert test price: %v", err)
	}

	// 仅原生路径：kimi-session/1 parser（无任何 Proxy/plugin 记账）。
	events, parsed, skipped := ParseKimiSessionJSONL([]byte(kimiFixtureV1), "kimi-sess-native", now)
	if parsed == 0 || skipped == 0 {
		t.Fatalf("fixture must produce parsed=%d skipped=%d, got %d/%d", parsed, skipped, parsed, skipped)
	}
	// 入库走采集层同一成本路径：事件导入前用产品 Calculator 按价格快照计价
	// （与 importer.go 同一 CalculateCost 调用，§7.2）。
	calc := NewCalculator(s)
	var imported int
	for _, ne := range events {
		ev := nativeEventToEvent(ne)
		ev.Source = "Kimi Code"
		ev.CostMicro = calc.CalculateCost(ev.Provider, ev.Model, ev.InputTokens, ev.OutputTokens, ev.CacheReadTokens, ev.CacheWriteTokens)
		if ok, _ := s.InsertEventIfAbsent(ev); ok {
			imported++
		}
	}
	// 入库数 = 唯一计费原子数（重复 messageId 行共享 atom，去重后不重复计量）；
	// parser 产出 3 个事件，去重后应入库 2 条。
	if imported != parsed-1 {
		t.Fatalf("imported = %d, want %d (unique billing atoms only; duplicate msg-aaa deduped)", imported, parsed-1)
	}

	// 三列均可出现：Token > 0、成本已计价、会话 distinct ≥1。
	f := Filter{Range: "7d"}
	overview, err := s.GetOverview(f)
	if err != nil {
		t.Fatalf("GetOverview: %v", err)
	}
	if overview.Metrics.TotalTokens <= 0 {
		t.Errorf("native-only tokens = %d, want > 0", overview.Metrics.TotalTokens)
	}
	if overview.Metrics.EstimatedCostUSD <= 0 {
		t.Errorf("native-only estimated cost = %f, want > 0 (kimi fixture has model kimi-k2)", overview.Metrics.EstimatedCostUSD)
	}
	sessions, err := s.GetSessions(f)
	if err != nil {
		t.Fatalf("GetSessions: %v", err)
	}
	if sessions.TotalSessions < 1 {
		t.Errorf("native-only distinct sessions = %d, want >= 1", sessions.TotalSessions)
	}

	// 来源诚实性：全部事件来自原生日志，无 proxy collector 记录。
	var proxyCount int
	if err := s.db.QueryRow(`SELECT COUNT(*) FROM usage_events WHERE collector_kind='proxy'`).Scan(&proxyCount); err != nil {
		t.Fatalf("count proxy: %v", err)
	}
	if proxyCount != 0 {
		t.Errorf("proxy events = %d, want 0 (native-only scenario)", proxyCount)
	}

	// 重复导入同一原生文件：计量不增加（§9.1 幂等）。
	for _, ne := range events {
		ev := nativeEventToEvent(ne)
		ev.Source = "Kimi Code"
		if ok, _ := s.InsertEventIfAbsent(ev); ok {
			t.Errorf("re-import must not double-count atom %q", ne.BillingAtom)
		}
	}
	overview2, _ := s.GetOverview(f)
	if overview2.Metrics.TotalTokens != overview.Metrics.TotalTokens {
		t.Errorf("tokens changed on re-import: %d -> %d", overview.Metrics.TotalTokens, overview2.Metrics.TotalTokens)
	}
}
