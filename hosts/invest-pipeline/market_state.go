package main

import (
	"time"
)

type MarketStatus string

const (
	StatusPreMarket MarketStatus = "pre_market" // 09:15 - 09:25
	StatusTrading   MarketStatus = "trading"    // 09:30 - 11:30, 13:00 - 15:00
	StatusBreak     MarketStatus = "break"      // 11:30 - 13:00
	StatusClosed    MarketStatus = "closed"     // 盘后及周末
)

// GetCurrentMarketStatus 获取 A 股当前时段状态与推荐抓取间隔
func GetCurrentMarketStatus() (MarketStatus, time.Duration) {
	cst := time.FixedZone("CST", 8*3600)
	now := time.Now().In(cst)

	weekday := now.Weekday()
	if weekday == time.Saturday || weekday == time.Sunday {
		return StatusClosed, 30 * time.Second
	}

	hour := now.Hour()
	min := now.Minute()
	currentMinutes := hour*60 + min

	// 09:15 - 09:25 (555 - 565)
	if currentMinutes >= 9*60+15 && currentMinutes < 9*60+25 {
		return StatusPreMarket, 3 * time.Second
	}

	// 09:30 - 11:30 (570 - 690)
	if currentMinutes >= 9*60+30 && currentMinutes < 11*60+30 {
		return StatusTrading, 2 * time.Second
	}

	// 11:30 - 13:00 (690 - 780)
	if currentMinutes >= 11*60+30 && currentMinutes < 13*60 {
		return StatusBreak, 15 * time.Second
	}

	// 13:00 - 15:00 (780 - 900)
	if currentMinutes >= 13*60 && currentMinutes < 15*60 {
		return StatusTrading, 2 * time.Second
	}

	// 盘后
	return StatusClosed, 30 * time.Second
}
