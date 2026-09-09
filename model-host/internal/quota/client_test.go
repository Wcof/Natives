package quota

import (
	"testing"
	"time"
)

func TestParseWindowKeepsEpochResetTime(t *testing.T) {
	window := parseWindow(map[string]any{"used_percent": 100.0, "reset_at": 1788855120.0}, "5-hour", nil)
	want := time.Unix(1788855120, 0).UTC().Format(time.RFC3339)
	if window.ResetTime != want {
		t.Fatalf("ResetTime = %q, want %q", window.ResetTime, want)
	}
}

func TestParseWindowConvertsRelativeResetTime(t *testing.T) {
	window := parseWindow(map[string]any{"reset_after_seconds": 6180.0}, "5-hour", nil)
	reset, err := time.Parse(time.RFC3339, window.ResetTime)
	seconds := time.Until(reset).Seconds()
	if err != nil || seconds < 6178 || seconds > 6181 {
		t.Fatalf("ResetTime = %q, want about 1 hour 43 minutes from now", window.ResetTime)
	}
}
