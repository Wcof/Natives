// Package model defines the single wire contract shared with the extension
// front-end: NormalizedAssetItem (see /goal 阶段一 §2). All provider parsers
// must project their raw source into these structs; no source-specific fields
// leak past this boundary.
package model

// AssetType discriminates the three domestic instrument classes.
type AssetType string

const (
	TypeStock AssetType = "stock"
	TypeETF   AssetType = "etf"
	TypeFund  AssetType = "fund"
)

// Depth is the five-level order book; empty for OTC funds.
type Depth struct {
	Bids [][2]float64 `json:"bids"` // [price, volume] ×5
	Asks [][2]float64 `json:"asks"` // [price, volume] ×5
}

// NormalizedAssetItem is the only shape pushed over the WebSocket.
type NormalizedAssetItem struct {
	ID            string    `json:"id"`
	Symbol        string    `json:"symbol"`
	Name          string    `json:"name"`
	AssetType     AssetType `json:"assetType"`
	Price         float64   `json:"price"`
	PrevClose     float64   `json:"prevClose"`
	Change        float64   `json:"change"`
	ChangePercent float64   `json:"changePercent"`
	High          *float64  `json:"high"`
	Low           *float64  `json:"low"`
	Volume        *float64  `json:"volume"` // 手
	Amount        *float64  `json:"amount"` // 元
	Timestamp     int64     `json:"timestamp"`
	Sparkline     []float64 `json:"sparkline"`
	Depth         *Depth    `json:"depth,omitempty"`
}

// Snapshot / Tick envelopes (host → front-end).
type Envelope struct {
	Type  string                `json:"type"` // "snapshot" | "tick"
	Items []NormalizedAssetItem `json:"items"`
}

// ID builds the contract id: "A:sh600519" or "F:005827".
func ID(symbol string, t AssetType) string {
	if t == TypeFund {
		return "F:" + symbol
	}
	return "A:" + symbol
}

// Classify maps a normalized CN symbol to its asset type by exchange/code rules.
func Classify(symbol string) AssetType {
	if len(symbol) < 3 {
		return TypeStock
	}
	mkt := symbol[:2]
	code := symbol[2:]
	switch mkt {
	case "sh":
		if len(code) == 6 && (code[:2] == "51" || code[:2] == "58" || code[:2] == "56") {
			return TypeETF
		}
	case "sz":
		if len(code) == 6 && (code[:2] == "15" || code[:2] == "16") {
			return TypeETF
		}
	}
	return TypeStock
}
