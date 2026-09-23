package quote

import (
	"encoding/json"
	"fmt"
	"net/url"
	"strconv"
	"strings"
	"time"

	"natives/market-host/internal/model"
)

// FundEstimateURL builds the EastMoney realtime estimate URL (JSONP).
func FundEstimateURL(codes []string) string {
	// fundgz 只支持单只请求，调用方并发逐只拉取；此处返回单只模板。
	return "http://fundgz.1234567.com.cn/js/" + url.PathEscape(codes[0]) + ".js"
}

// fundGz mirrors the JSONP payload fields (subset used by the contract).
type fundGz struct {
	FundCode string  `json:"fundcode"`
	Name     string  `json:"name"`
	DWJZ     string  `json:"dwjz"` // 单位净值（上一交易日披露值）
	GSZ      string  `json:"gsz"`  // 实时估算净值（盘中）
	GSZZL    string  `json:"gszzl"` // 估算涨跌幅（已 ×100）
	GZTime   string  `json:"gztime"` // yyyy-MM-dd HH:mm
	JZRQ     string  `json:"jzrq"`  // 净值日期 yyyy-MM-dd
}

// ParseFundGz strips the JSONP wrapper and projects one fund estimate.
// afterHours: when true the estimate is ignored and dwjz becomes price.
func ParseFundGz(body string, afterHours bool, now time.Time) (model.NormalizedAssetItem, error) {
	body = strings.TrimSpace(body)
	l, r := strings.Index(body, "("), strings.LastIndex(body, ")")
	if l < 0 || r <= l {
		return model.NormalizedAssetItem{}, fmt.Errorf("fundgz: not a jsonp payload")
	}
	var gz fundGz
	if err := json.Unmarshal([]byte(body[l+1:r]), &gz); err != nil {
		return model.NormalizedAssetItem{}, fmt.Errorf("fundgz: %w", err)
	}
	if gz.FundCode == "" {
		return model.NormalizedAssetItem{}, fmt.Errorf("fundgz: empty fundcode")
	}
	fnum := func(s string) float64 {
		v, _ := strconv.ParseFloat(strings.TrimSpace(s), 64)
		return v
	}
	prev := fnum(gz.DWJZ)
	price := prev
	if !afterHours {
		if gsz := fnum(gz.GSZ); gsz > 0 {
			price = gsz
		}
	}
	if prev <= 0 || price <= 0 {
		return model.NormalizedAssetItem{}, fmt.Errorf("fundgz: non-positive nav for %s", gz.FundCode)
	}
	change := price - prev
	pct := fnum(gz.GSZZL)
	if pct == 0 && change != 0 {
		pct = change / prev * 100
	}
	var ts int64
	if !afterHours && gz.GZTime != "" {
		if t, err := time.ParseInLocation("2006-01-02 15:04", gz.GZTime, cstZone()); err == nil {
			ts = t.UnixMilli()
		}
	} else if gz.JZRQ != "" {
		if t, err := time.ParseInLocation("2006-01-02 15:04", gz.JZRQ+" 15:00", cstZone()); err == nil {
			ts = t.UnixMilli()
		}
	}
	if ts == 0 {
		ts = now.UnixMilli()
	}
	return model.NormalizedAssetItem{
		ID:            model.ID(gz.FundCode, model.TypeFund),
		Symbol:        gz.FundCode,
		Name:          gz.Name,
		AssetType:     model.TypeFund,
		Price:         price,
		PrevClose:     prev,
		Change:        change,
		ChangePercent: pct,
		High:          nil, Low: nil, Volume: nil, Amount: nil, // 契约：基金为空
		Timestamp: ts,
		Sparkline: []float64{price},
		Depth:     nil,
	}, nil
}

// cstZone returns Asia/Shanghai; falls back to a fixed +08:00 zone.
func cstZone() *time.Location {
	if z, err := time.LoadLocation("Asia/Shanghai"); err == nil {
		return z
	}
	return time.FixedZone("CST", 8*3600)
}
