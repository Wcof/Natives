package main

// NormalizedAssetItem 对齐 token-monitor 数据契约
type NormalizedAssetItem struct {
	ID            string       `json:"id"`
	Symbol        string       `json:"symbol"`
	Name          string       `json:"name"`
	AssetType     string       `json:"assetType"` // "stock" | "etf" | "fund"
	Price         float64      `json:"price"`
	PrevClose     float64      `json:"prevClose"`
	Change        float64      `json:"change"`
	ChangePercent float64      `json:"changePercent"`
	High          float64      `json:"high"`
	Low           float64      `json:"low"`
	Volume        *float64     `json:"volume,omitempty"` // 手
	Amount        *float64     `json:"amount,omitempty"` // 元
	Timestamp     int64        `json:"timestamp"`
	Sparkline     []float64    `json:"sparkline"`
	Depth         *MarketDepth `json:"depth,omitempty"`
}

type MarketDepth struct {
	Bids [][2]float64 `json:"bids"` // 买一到买五: [[price, volume], ...]
	Asks [][2]float64 `json:"asks"` // 卖一到卖五
}

// ClientMessage 前端发送的订阅消息
type ClientMessage struct {
	Action  string   `json:"action"` // "subscribe" | "unsubscribe" | "snapshot"
	Symbols []string `json:"symbols"`
}

// ServerMessage Host 发送给前端的消息
type ServerMessage struct {
	Type         string                 `json:"type"` // "snapshot" | "tick" | "market_status"
	Data         interface{}            `json:"data,omitempty"`
	MarketStatus string                 `json:"marketStatus,omitempty"`
	Timestamp    int64                  `json:"timestamp,omitempty"`
}
