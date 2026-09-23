package usage

// 用户时区支持（整改 E2，方案 §4.2）：
//   - 事件仍以 UTC 保存；
//   - 查询 Filter 携带 IANA 时区，不识别的时区返回错误（不静默退回 UTC）；
//   - 活跃天数、日热力图与趋势日桶在 SQL 中按真实夏令时边界分桶；
//   - "today" 范围按用户本地零点换算为 UTC 即时。

import (
	"fmt"
	"strings"
	"time"
)

// maxTZSegments 为单次查询允许的夏令时段数上限（100 年 × 每年 2 次仍有余量）。
const maxTZSegments = 256

// resolveTimezone 校验并解析 IANA 时区；空串为 UTC（向后兼容旧调用方）。
func resolveTimezone(tz string) (*time.Location, error) {
	tz = strings.TrimSpace(tz)
	if tz == "" {
		return time.UTC, nil
	}
	loc, err := time.LoadLocation(tz)
	if err != nil {
		return nil, fmt.Errorf("invalid timezone %q", tz)
	}
	return loc, nil
}

// ResolveTimezone 为跨包校验入口（host handler 白名单核验用）。
func ResolveTimezone(tz string) (*time.Location, error) {
	return resolveTimezone(tz)
}

// tzSegment 为一个夏令时段：boundary 是该段结束的 UTC 即时（秒精度，
// RFC3339 去 Z 以便与 substr(requested_at,1,19) 字典序比较）；
// offsetSeconds 为该段内本地相对 UTC 的偏移（东正西负）。
type tzSegment struct {
	boundaryUTC   string
	offsetSeconds int
}

// localDaySegments 返回覆盖 (start, end] 的时区分段（用 Location 的真实
// ZoneBounds，不做固定偏移近似）。UTC 或无分段时返回 nil。
func localDaySegments(loc *time.Location, start, end time.Time) []tzSegment {
	if loc == nil || loc == time.UTC || !start.Before(end) {
		return nil
	}
	var segs []tzSegment
	cur := start.UTC()
	for cur.Before(end) && len(segs) < maxTZSegments {
		_, off := cur.In(loc).Zone()
		_, zoneEnd := cur.In(loc).ZoneBounds()
		if zoneEnd.IsZero() || zoneEnd.After(end) {
			segs = append(segs, tzSegment{offsetSeconds: off})
			break
		}
		segs = append(segs, tzSegment{
			boundaryUTC:   zoneEnd.UTC().Format("2006-01-02T15:04:05"),
			offsetSeconds: off,
		})
		cur = zoneEnd
	}
	return segs
}

// sqliteOffsetModifier 把偏移秒数格式化为 SQLite 修饰符（"±NNN seconds"）。
func sqliteOffsetModifier(offsetSeconds int) string {
	if offsetSeconds < 0 {
		return fmt.Sprintf("-%d seconds", -offsetSeconds)
	}
	return fmt.Sprintf("%d seconds", offsetSeconds)
}

// hasBoundary 判断分段是否含真实切换点（固定偏移时区无切换）。
func hasBoundary(segs []tzSegment) bool {
	for _, seg := range segs {
		if seg.boundaryUTC != "" {
			return true
		}
	}
	return false
}

// localDaySQLExpr 构造把 requested_at 映射为本地日历日的 SQL 表达式；
// segs 为空时退回 UTC 日界（与既有趋势桶一致）。
func localDaySQLExpr(segs []tzSegment) string {
	if len(segs) == 0 {
		return "strftime('%Y-%m-%d', requested_at)"
	}
	if !hasBoundary(segs) {
		// 固定偏移（无夏令时）：单一偏移，无需 CASE（CASE 无 WHEN 非法）。
		return fmt.Sprintf("strftime('%%Y-%%m-%%d', requested_at, '%s')",
			sqliteOffsetModifier(segs[len(segs)-1].offsetSeconds))
	}
	var b strings.Builder
	b.WriteString("CASE ")
	for _, seg := range segs {
		if seg.boundaryUTC == "" {
			continue
		}
		fmt.Fprintf(&b, "WHEN substr(requested_at,1,19) < '%s' THEN strftime('%%Y-%%m-%%d', requested_at, '%s') ",
			seg.boundaryUTC, sqliteOffsetModifier(seg.offsetSeconds))
	}
	fmt.Fprintf(&b, "ELSE strftime('%%Y-%%m-%%d', requested_at, '%s') END",
		sqliteOffsetModifier(segs[len(segs)-1].offsetSeconds))
	return b.String()
}

// trendBucketSQLExpr 构造趋势分桶表达式；timezone 生效时小时桶/日桶都
// 按本地钟点输出（键仍是 ISO 文本，前端按文本展示）。
func trendBucketSQLExpr(segs []tzSegment, daily bool) string {
	grain := "%Y-%m-%dT%H:00:00Z"
	if daily {
		grain = "%Y-%m-%dT00:00:00Z"
	}
	if len(segs) == 0 {
		return fmt.Sprintf("strftime('%s', requested_at)", grain)
	}
	if !hasBoundary(segs) {
		// 固定偏移（无夏令时）：单一偏移，无需 CASE。
		return fmt.Sprintf("strftime('%s', requested_at, '%s')",
			grain, sqliteOffsetModifier(segs[len(segs)-1].offsetSeconds))
	}
	// 跨夏令时切换：用 CASE 精确处理每段偏移。
	var b strings.Builder
	b.WriteString("CASE ")
	for _, seg := range segs {
		if seg.boundaryUTC == "" {
			continue
		}
		fmt.Fprintf(&b, "WHEN substr(requested_at,1,19) < '%s' THEN strftime('%s', requested_at, '%s') ",
			seg.boundaryUTC, grain, sqliteOffsetModifier(seg.offsetSeconds))
	}
	fmt.Fprintf(&b, "ELSE strftime('%s', requested_at, '%s') END", grain, sqliteOffsetModifier(segs[len(segs)-1].offsetSeconds))
	return b.String()
}

// filterTimeRange 返回 filter 的 (start, end) UTC 即时（用于时区分段计算）。
// 无界范围（all / 自定义缺一边）由调用方先用数据库极值补齐。
func filterTimeRange(f Filter, now time.Time, loc *time.Location) (time.Time, time.Time) {
	now = now.UTC()
	switch f.Range {
	case "4h":
		return now.Add(-4 * time.Hour), now
	case "24h":
		return now.Add(-24 * time.Hour), now
	case "72h":
		return now.Add(-72 * time.Hour), now
	case "7d":
		return now.AddDate(0, 0, -7), now
	case "30d":
		return now.AddDate(0, 0, -30), now
	case "today":
		return todayStartUTC(now, loc), now
	case "custom":
		var start, end time.Time
		if f.StartTime != nil {
			start = f.StartTime.UTC()
		}
		if f.EndTime != nil {
			end = f.EndTime.UTC()
		}
		if start.IsZero() || end.IsZero() {
			// 有一边缺失：给出宽松界（供分段计算，不影响过滤本身）。
			if start.IsZero() {
				start = end.AddDate(-1, 0, 0)
			}
			if end.IsZero() {
				end = start.AddDate(1, 0, 0)
			}
		}
		return start, end
	default: // all
		return now.AddDate(-1, 0, 0), now
	}
}

// todayStartUTC 返回 loc 时区"今天"零点对应的 UTC 即时。
func todayStartUTC(nowUTC time.Time, loc *time.Location) time.Time {
	y, m, d := nowUTC.In(loc).Date()
	return time.Date(y, m, d, 0, 0, 0, 0, loc).UTC()
}
