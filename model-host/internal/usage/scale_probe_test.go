package usage

// 一次性规模基准（整改 §12）：10 万事件下的查询与增量扫描时延。
// 用临时库执行；不作为常规 CI 用例（-run ScaleProbe 手动触发）。

import (
	"fmt"
	"testing"
	"time"
)

func TestScaleProbe(t *testing.T) {
	if testing.Short() {
		t.Skip("scale probe is manual")
	}
	s, _ := tempStore(t)
	defer s.Close()

	base := time.Now().UTC().AddDate(0, 0, -60)
	start := time.Now()
	tx, err := s.db.Begin()
	if err != nil {
		t.Fatal(err)
	}
	stmt, _ := tx.Prepare(`INSERT INTO usage_events (id, requested_at, latency_ms, ttft_ms, provider, model, result, http_status, source, tool_id, source_instance_id, session_id, billing_atom, total_tokens, cost_micro)
		VALUES (?, ?, 100, 50, 'openai', 'gpt-4o', 'success', 200, 'Codex', 'codex', ?, ?, ?, 500, 1000)`)
	const N = 100000
	for i := 0; i < N; i++ {
		at := base.Add(time.Duration(i%60*24) * time.Hour).Add(time.Duration(i%86400) * time.Second)
		sess := fmt.Sprintf("sess-%d", i%3000)
		inst := fmt.Sprintf("inst-%d", i%4)
		if _, err := stmt.Exec(fmt.Sprintf("evt-%d", i), at.Format(time.RFC3339Nano), inst, sess,
			fmt.Sprintf("codex:%s:%s:%d", inst, sess, i)); err != nil {
			t.Fatal(err)
		}
	}
	if err := tx.Commit(); err != nil {
		t.Fatal(err)
	}
	t.Logf("seed 100k events: %s", time.Since(start).Round(time.Millisecond))

	bench := func(name string, f func()) {
		t0 := time.Now()
		f()
		t.Logf("%s: %s", name, time.Since(t0).Round(time.Millisecond))
	}
	bench("GetOverview(30d)", func() {
		if _, err := s.GetOverview(Filter{Range: "30d"}); err != nil {
			t.Fatal(err)
		}
	})
	bench("GetOverview(30d, tz)", func() {
		if _, err := s.GetOverview(Filter{Range: "30d", Timezone: "Asia/Shanghai"}); err != nil {
			t.Fatal(err)
		}
	})
	bench("GetSessions(30d)", func() {
		if _, err := s.GetSessions(Filter{Range: "30d"}); err != nil {
			t.Fatal(err)
		}
	})
	bench("GetSessions(30d, tz DST)", func() {
		if _, err := s.GetSessions(Filter{Range: "30d", Timezone: "America/New_York"}); err != nil {
			t.Fatal(err)
		}
	})
	bench("GetEvents(50)", func() {
		if _, err := s.GetEvents(Filter{Range: "30d", Limit: 50}); err != nil {
			t.Fatal(err)
		}
	})
}
