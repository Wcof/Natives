package usage

import (
	"testing"
	"time"
)

func TestMissingUsageDoesNotBecomeFreeConsumption(t *testing.T) {
	s, _ := tempStore(t)
	defer s.Close()
	r := NewRecorder(s, NewCalculator(s))
	_, err := r.Record(Record{ID: "missing", RequestedAt: time.Now(), Provider: "fixture", Model: "missing", Result: ResultSuccess})
	if err != nil {
		t.Fatal(err)
	}
	overview, err := s.GetOverview(Filter{Range: "all"})
	if err != nil {
		t.Fatal(err)
	}
	if overview.Metrics.Priced {
		t.Fatal("missing usage was reported as a complete zero cost")
	}
}

func TestUsageCoverageAcrossAggregates(t *testing.T) {
	for _, tc := range []struct {
		name        string
		missing     bool
		knownTokens int64
		cost        int64
	}{
		{"reported_zero", false, 0, 0},
		{"missing", true, 0, 0},
		{"partial", true, 10, 250000},
	} {
		t.Run(tc.name, func(t *testing.T) {
			s, _ := tempStore(t)
			defer s.Close()
			at := time.Now().UTC()
			if !tc.missing || tc.knownTokens > 0 {
				insertPanelCostEvent(t, s, "codex", "fixture", "fixture", "model", tc.knownTokens, 0, 0, 0, tc.cost, at)
			}
			if tc.missing {
				if err := s.InsertEvent(&Event{RequestedAt: at.Add(time.Second), Provider: "fixture", Model: "model", Source: "Test", ToolID: "codex", SessionID: "fixture", UsageStatus: "missing", Result: ResultFailed}); err != nil {
					t.Fatal(err)
				}
			}
			missing := int64(0)
			if tc.missing {
				missing = 1
			}
			f := Filter{Range: "all"}
			o, err := s.GetOverview(f)
			if err != nil {
				t.Fatal(err)
			}
			if o.Metrics.Priced == tc.missing || o.Metrics.MissingUsageRequests != missing || o.Metrics.TotalTokens != tc.knownTokens || o.Metrics.EstimatedCostUSD != float64(tc.cost)/1e6 {
				t.Fatalf("overview lost coverage or known subtotal: %+v", o.Metrics)
			}
			if len(o.Trend) != 1 || o.Trend[0].MissingUsageRequests != missing {
				t.Fatalf("trend: %+v", o.Trend)
			}
			a, err := s.GetAnalytics(f)
			if err != nil {
				t.Fatal(err)
			}
			for _, rows := range [][]RankItem{a.ByModel, a.ByProvider, a.BySource, a.ByAccessKey, a.ByHour} {
				if len(rows) != 1 || rows[0].MissingUsageRequests != missing || rows[0].Priced == tc.missing || rows[0].Tokens != tc.knownTokens || rows[0].CostUSD != float64(tc.cost)/1e6 {
					t.Fatalf("rank lost coverage or subtotal: %+v", rows)
				}
			}
			ss, err := s.GetSessions(f)
			if err != nil {
				t.Fatal(err)
			}
			if len(ss.Sessions) != 1 || ss.Sessions[0].MissingUsageRequests != missing || ss.Sessions[0].Priced == tc.missing || ss.Sessions[0].Tokens != tc.knownTokens {
				t.Fatalf("sessions: %+v", ss)
			}
			f.Models = []string{"other-model"}
			empty, err := s.GetOverview(f)
			if err != nil || empty.Metrics.MissingUsageRequests != 0 || !empty.Metrics.Priced {
				t.Fatalf("filter: %+v %v", empty, err)
			}
		})
	}
}
