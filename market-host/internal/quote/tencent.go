// Package quote implements the adapter layer: raw domestic sources
// (Tencent/Sina quotes, EastMoney fund estimates) → model.NormalizedAssetItem.
package quote

import (
	"fmt"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"

	"golang.org/x/text/encoding/simplifiedchinese"

	"natives/market-host/internal/model"
)

// TencentQuoteURL builds the batch quote URL for A-share/ETF symbols.
// Symbols are `[a-z]{2}\d{6}` — URL-safe by construction; the comma must
// stay raw because qt.gtimg.cn does not decode %2C in the q parameter.
func TencentQuoteURL(symbols []string) string {
	return "http://qt.gtimg.cn/q=" + strings.Join(symbols, ",")
}

// gbkFix decodes GBK bytes when the payload is not valid UTF-8.
func gbkFix(s string) string {
	if utf8.ValidString(s) {
		return s
	}
	if out, err := simplifiedchinese.GBK.NewDecoder().String(s); err == nil {
		return out
	}
	return s
}

// ParseTencent parses a full qt.gtimg.cn response into normalized items.
// Field indices per contract review (阶段一 §1.1); defensive on missing data.
func ParseTencent(body string, now time.Time) ([]model.NormalizedAssetItem, error) {
	var items []model.NormalizedAssetItem
	for _, line := range strings.Split(body, ";") {
		line = strings.TrimSpace(line)
		if line == "" || !strings.Contains(line, "=\"") {
			continue
		}
		eq := strings.Index(line, "=\"")
		key := strings.TrimPrefix(line[:eq], "v_")
		if key == "" || len(key) < 8 {
			continue
		}
		raw := strings.Trim(line[eq+2:], "\"")
		fields := strings.Split(raw, "~")
		if len(fields) < 40 {
			continue
		}
		f := func(i int) string {
			if i < len(fields) {
				return fields[i]
			}
			return ""
		}
		num := func(i int) float64 {
			v, _ := strconv.ParseFloat(strings.TrimSpace(f(i)), 64)
			return v
		}
		na := func(i int) *float64 {
			s := strings.TrimSpace(f(i))
			if s == "" || s == "0.00" && (i == 33 || i == 34) {
				if s == "" {
					return nil
				}
			}
			v, err := strconv.ParseFloat(s, 64)
			if err != nil {
				return nil
			}
			return &v
		}
		ts := func() int64 {
			t, err := time.Parse("20060102150405", strings.TrimSpace(f(30)))
			if err != nil {
				return now.UnixMilli()
			}
			return t.UnixMilli()
		}()
		symbol := key // e.g. sh600519
		price := num(3)
		prev := num(4)
		change := num(31)
		if price > 0 && prev > 0 {
			if calc := price - prev; abs(calc-change) > 0.0001 && calc != 0 {
				change = calc // trust the computed delta on source drift
			}
		}
		pct := num(32)
		if pct == 0 && prev > 0 && price > 0 {
			pct = change / prev * 100
		}
		depth := &model.Depth{Bids: [][2]float64{}, Asks: [][2]float64{}}
		for i := 0; i < 5; i++ {
			bp, bv := num(9+i*2), num(10+i*2)
			if bp > 0 {
				depth.Bids = append(depth.Bids, [2]float64{bp, bv})
			}
			ap, av := num(19+i*2), num(20+i*2)
			if ap > 0 {
				depth.Asks = append(depth.Asks, [2]float64{ap, av})
			}
		}
		// 字段 36 为成交额（元）；37 为万元冗余位。
		amount := na(36)
		items = append(items, model.NormalizedAssetItem{
			ID:            model.ID(symbol, model.Classify(symbol)),
			Symbol:        symbol[2:],
			Name:          gbkFix(f(1)),
			AssetType:     model.Classify(symbol),
			Price:         price,
			PrevClose:     prev,
			Change:        change,
			ChangePercent: pct,
			High:          na(33),
			Low:           na(34),
			Volume:        na(6), // 手
			Amount:        amount,
			Timestamp:     ts,
			Sparkline:     []float64{price},
			Depth:         depth,
		})
	}
	if len(items) == 0 {
		return nil, fmt.Errorf("tencent: no parsable records")
	}
	return items, nil
}

func abs(v float64) float64 {
	if v < 0 {
		return -v
	}
	return v
}
