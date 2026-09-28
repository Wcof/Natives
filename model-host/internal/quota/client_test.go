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

func pct(v float64) *float64 { return &v }

func TestAntigravitySuspiciousFreshFingerprint(t *testing.T) {
	now := time.Date(2026, 9, 27, 10, 0, 0, 0, time.UTC)
	real := QuotaWindow{
		Name:             "Weekly Limit Remaining",
		RemainingPercent: pct(32.6),
		ResetTime:        time.Date(2026, 10, 1, 12, 11, 40, 0, time.UTC).Format(time.RFC3339),
	}
	if antigravitySuspiciousFresh(real, now) {
		t.Fatal("锚定 reset 的真实窗口不应判为假视图")
	}
	flaky := QuotaWindow{
		Name:             "Weekly Limit Remaining",
		RemainingPercent: pct(100),
		ResetTime:        now.Add(7 * 24 * time.Hour).Format(time.RFC3339),
	}
	if !antigravitySuspiciousFresh(flaky, now) {
		t.Fatal("reset=请求时刻+7天 的 100% 窗口应判为假视图")
	}
	legitFresh := QuotaWindow{
		Name:             "Five Hour Limit Remaining",
		RemainingPercent: pct(100),
		ResetTime:        now.Add(5 * time.Hour).Format(time.RFC3339),
	}
	if !antigravitySuspiciousFresh(legitFresh, now) {
		t.Fatal("5小时假视图指纹应能识别")
	}
	partial := QuotaWindow{
		Name:             "Weekly Limit Remaining",
		RemainingPercent: pct(80),
		ResetTime:        now.Add(7 * 24 * time.Hour).Format(time.RFC3339),
	}
	if antigravitySuspiciousFresh(partial, now) {
		t.Fatal("余量<100% 的窗口不应判为假视图")
	}
}

func TestMergeAntigravityMinWindowsConvergesToRealView(t *testing.T) {
	now := time.Date(2026, 9, 27, 10, 0, 0, 0, time.UTC)
	flakyReset := now.Add(7 * 24 * time.Hour).Format(time.RFC3339)
	realReset := time.Date(2026, 10, 1, 12, 11, 40, 0, time.UTC).Format(time.RFC3339)
	// 第一次命中假视图（100% 全新），第二次返回真实视图（32.6% 锚定）
	first := []QuotaWindow{
		{Name: "Weekly Limit Remaining", Group: "Gemini Models", RemainingPercent: pct(100), ResetTime: flakyReset},
		{Name: "Weekly Limit Remaining", Group: "Claude and GPT models", RemainingPercent: pct(34.23), ResetTime: realReset},
	}
	second := []QuotaWindow{
		{Name: "Weekly Limit Remaining", Group: "Gemini Models", RemainingPercent: pct(32.6), ResetTime: realReset},
		{Name: "Weekly Limit Remaining", Group: "Claude and GPT models", RemainingPercent: pct(34.23), ResetTime: realReset},
	}
	merged := mergeAntigravityMinWindows(first, second)
	if len(merged) != 2 {
		t.Fatalf("合并后窗口数 = %d, want 2", len(merged))
	}
	if got := *merged[0].RemainingPercent; got != 32.6 {
		t.Fatalf("Gemini 周窗口余量 = %v, want 32.6", got)
	}
	if merged[0].ResetTime != realReset {
		t.Fatalf("Gemini 周窗口 reset = %q, want 锚定值 %q", merged[0].ResetTime, realReset)
	}
	if got := *merged[1].RemainingPercent; got != 34.23 {
		t.Fatalf("Claude 周窗口余量 = %v, want 34.23", got)
	}
}

func TestMergeAntigravityMinWindowsKeepsStableResult(t *testing.T) {
	reset := time.Now().Add(-24 * time.Hour).Add(7 * 24 * time.Hour).Format(time.RFC3339)
	both := []QuotaWindow{
		{Name: "Weekly Limit Remaining", Group: "Gemini Models", RemainingPercent: pct(32.6), ResetTime: reset},
	}
	merged := mergeAntigravityMinWindows(both, both)
	if len(merged) != 1 || *merged[0].RemainingPercent != 32.6 {
		t.Fatalf("两次一致时结果不应改变: %+v", merged)
	}
}
