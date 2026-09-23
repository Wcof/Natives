package main

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strconv"
	"strings"
	"time"
)

var httpClient = &http.Client{
	Timeout: 5 * time.Second,
}

// 常见标的代码到中文名称映射（兜底 GBK 解码）
var fallbackStockNames = map[string]string{
	"sh600519": "贵州茅台",
	"sz000001": "平安银行",
	"sz300750": "宁德时代",
	"sz002594": "比亚迪",
	"sh601318": "中国平安",
	"sh600036": "招商银行",
	"sh510300": "沪深300ETF",
	"sh510500": "中证500ETF",
	"sz159915": "创业板ETF",
	"sh588000": "科创50ETF",
	"sh512880": "证券ETF",
	"sh518880": "黄金ETF",
	"sz159937": "黄金基金ETF",
	"sh511010": "国债ETF",
	"sh511260": "十年国债ETF",
	"sh000001": "上证指数",
	"sz399001": "深证成指",
	"sz399006": "创业板指",
	"sh000688": "科创50",
}

// NormalizeSymbol 统一规范化股票/ETF代码
func NormalizeSymbol(raw string) string {
	s := strings.ToLower(strings.TrimSpace(raw))
	if strings.HasPrefix(s, "sh") || strings.HasPrefix(s, "sz") || strings.HasPrefix(s, "bj") {
		return s
	}
	if len(s) == 6 {
		if s == "000001" {
			return "sh000001"
		}
		if strings.HasPrefix(s, "6") || strings.HasPrefix(s, "5") {
			return "sh" + s
		}
		if strings.HasPrefix(s, "0") || strings.HasPrefix(s, "3") || strings.HasPrefix(s, "1") {
			return "sz" + s
		}
		if strings.HasPrefix(s, "8") || strings.HasPrefix(s, "4") || strings.HasPrefix(s, "9") {
			return "bj" + s
		}
	}
	return s
}

// FetchStockQuotes 批量抓取 A 股 / ETF 快照及五档盘口
func FetchStockQuotes(symbols []string) (map[string]*NormalizedAssetItem, error) {
	result := make(map[string]*NormalizedAssetItem)
	if len(symbols) == 0 {
		return result, nil
	}

	normSymbols := make([]string, len(symbols))
	for i, s := range symbols {
		normSymbols[i] = NormalizeSymbol(s)
	}

	url := fmt.Sprintf("https://qt.gtimg.cn/q=%s", strings.Join(normSymbols, ","))
	req, err := http.NewRequest("GET", url, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("User-Agent", "Natives-Invest/1.0")

	resp, err := httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	bodyBytes, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, err
	}

	lines := strings.Split(string(bodyBytes), "\n")
	for _, line := range lines {
		line = strings.TrimSpace(line)
		if !strings.Contains(line, "=\"") {
			continue
		}

		parts := strings.Split(line, "=")
		if len(parts) < 2 {
			continue
		}
		varCode := strings.TrimPrefix(parts[0], "v_")
		content := strings.Trim(parts[1], "\";")
		fields := strings.Split(content, "~")
		if len(fields) < 35 {
			continue
		}

		parseF := func(idx int) float64 {
			if idx < len(fields) {
				v, _ := strconv.ParseFloat(fields[idx], 64)
				return v
			}
			return 0
		}

		code := fields[2]
		price := parseF(3)
		prevClose := parseF(4)
		open := parseF(5)
		volume := parseF(6) // 手
		amountWan := parseF(37) // 万元
		amount := amountWan * 10000.0 // 转为元
		high := parseF(33)
		low := parseF(34)
		change := parseF(31)
		changePct := parseF(32)

		// 名称优先从映射获取，避免 GBK 乱码
		name := fallbackStockNames[varCode]
		if name == "" {
			name = fields[1]
			if name == "" {
				name = code
			}
		}

		assetType := "stock"
		if strings.HasPrefix(code, "51") || strings.HasPrefix(code, "15") || strings.HasPrefix(code, "16") || strings.HasPrefix(code, "58") || strings.HasPrefix(code, "56") {
			assetType = "etf"
		}

		// 五档买卖盘口
		bids := make([][2]float64, 0, 5)
		for i := 0; i < 5; i++ {
			p := parseF(9 + i*2)
			v := parseF(10 + i*2)
			if p > 0 {
				bids = append(bids, [2]float64{p, v})
			}
		}

		asks := make([][2]float64, 0, 5)
		for i := 0; i < 5; i++ {
			p := parseF(19 + i*2)
			v := parseF(20 + i*2)
			if p > 0 {
				asks = append(asks, [2]float64{p, v})
			}
		}

		depth := &MarketDepth{
			Bids: bids,
			Asks: asks,
		}

		nowTs := time.Now().UnixMilli()
		item := &NormalizedAssetItem{
			ID:            fmt.Sprintf("A:%s", varCode),
			Symbol:        code,
			Name:          name,
			AssetType:     assetType,
			Price:         price,
			PrevClose:     prevClose,
			Change:        change,
			ChangePercent: changePct,
			High:          high,
			Low:           low,
			Volume:        &volume,
			Amount:        &amount,
			Timestamp:     nowTs,
			Sparkline:     generateDefaultSparkline(price, prevClose, open, high, low),
			Depth:         depth,
		}

		result[varCode] = item
	}

	return result, nil
}

// FetchStockSparkline 获取标的高清分时微缩线 (30~60 点)
func FetchStockSparkline(symbol string) []float64 {
	norm := NormalizeSymbol(symbol)
	url := fmt.Sprintf("https://web.ifzq.gtimg.cn/appstock/app/minute/query?code=%s", norm)
	resp, err := httpClient.Get(url)
	if err != nil {
		return nil
	}
	defer resp.Body.Close()

	var data struct {
		Data map[string]struct {
			Data struct {
				Data []string `json:"data"`
			} `json:"data"`
		} `json:"data"`
	}

	if err := json.NewDecoder(resp.Body).Decode(&data); err != nil {
		return nil
	}

	rawPts := data.Data[norm].Data.Data
	if len(rawPts) == 0 {
		return nil
	}

	// 抽样出约 30~50 点
	step := len(rawPts) / 40
	if step < 1 {
		step = 1
	}

	pts := make([]float64, 0, 45)
	for i := 0; i < len(rawPts); i += step {
		parts := strings.Fields(rawPts[i])
		if len(parts) >= 2 {
			if p, err := strconv.ParseFloat(parts[1], 64); err == nil {
				pts = append(pts, p)
			}
		}
	}
	return pts
}

func generateDefaultSparkline(price, prevClose, open, high, low float64) []float64 {
	if price <= 0 {
		return []float64{}
	}
	return []float64{prevClose, open, low, (open + high) / 2, high, price}
}
