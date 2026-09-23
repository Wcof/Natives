package main

import (
	"encoding/json"
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"time"
)

var fallbackFundNames = map[string]string{
	"005827": "易方达蓝筹精选混合",
	"161725": "招商中证白酒指数",
	"110011": "易方达中小盘混合",
	"003003": "华夏中证500ETF联接A",
	"000001": "华夏成长混合",
	"163406": "兴全合润混合",
}

// FetchFundQuote 抓取场外公募基金的最新净值与增长率（东财官方 Mobile API，已在生产线验证）
func FetchFundQuote(fundCode string) (*NormalizedAssetItem, error) {
	code := strings.TrimSpace(fundCode)
	url := fmt.Sprintf("https://fundmobapi.eastmoney.com/FundMNewApi/FundMNHisNetList?FCODE=%s&deviceid=natives-fund&plat=Iphone&product=EFund&version=6.2.8&pageSize=30&type=0", code)

	req, err := http.NewRequest("GET", url, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("User-Agent", "Mozilla/5.0 (iPhone; CPU iPhone OS 16_0 like Mac OS X) eastmoney/6.2.8")

	resp, err := httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	var res struct {
		Datas []struct {
			FSRQ   string `json:"FSRQ"`   // 净值日期
			DWJZ   string `json:"DWJZ"`   // 单位净值
			LJJZ   string `json:"LJJZ"`   // 累计净值
			JZZZL  string `json:"JZZZL"`  // 日增长率 %
		} `json:"Datas"`
		ErrCode int `json:"ErrCode"`
	}

	if err := json.NewDecoder(resp.Body).Decode(&res); err != nil {
		return nil, err
	}

	if len(res.Datas) == 0 {
		return nil, fmt.Errorf("no fund data returned for %s", code)
	}

	latest := res.Datas[0]
	currentPrice, _ := strconv.ParseFloat(latest.DWJZ, 64)
	growthPct, _ := strconv.ParseFloat(latest.JZZZL, 64)

	prevPrice := currentPrice
	if len(res.Datas) > 1 {
		prevPrice, _ = strconv.ParseFloat(res.Datas[1].DWJZ, 64)
	}
	change := currentPrice - prevPrice

	// 历史走势 (升序)
	sparkline := make([]float64, 0, len(res.Datas))
	for i := len(res.Datas) - 1; i >= 0; i-- {
		if v, err := strconv.ParseFloat(res.Datas[i].DWJZ, 64); err == nil {
			sparkline = append(sparkline, v)
		}
	}

	name := fallbackFundNames[code]
	if name == "" {
		name = fmt.Sprintf("公募基金 (%s)", code)
	}

	return &NormalizedAssetItem{
		ID:            fmt.Sprintf("F:%s", code),
		Symbol:        code,
		Name:          name,
		AssetType:     "fund",
		Price:         currentPrice,
		PrevClose:     prevPrice,
		Change:        change,
		ChangePercent: growthPct,
		High:          currentPrice,
		Low:           currentPrice,
		Volume:        nil,
		Amount:        nil,
		Timestamp:     time.Now().UnixMilli(),
		Sparkline:     sparkline,
		Depth:         nil, // 场外基金无五档盘口
	}, nil
}
