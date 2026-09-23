package main

import (
	"encoding/json"
	"testing"
)

func TestStockAdapter(t *testing.T) {
	symbols := []string{"sh600519", "sz000001", "sh510300"}
	quotes, err := FetchStockQuotes(symbols)
	if err != nil {
		t.Fatalf("FetchStockQuotes failed: %v", err)
	}

	for _, s := range []string{"sh600519", "sz000001"} {
		item, ok := quotes[s]
		if !ok {
			t.Fatalf("Expected quote for %s not found", s)
		}
		if item.Price <= 0 {
			t.Errorf("Invalid price for %s: %f", s, item.Price)
		}
		if item.PrevClose <= 0 {
			t.Errorf("Invalid prevClose for %s: %f", s, item.PrevClose)
		}
		if item.Depth == nil {
			t.Errorf("Expected depth for %s, got nil", s)
		}
		if len(item.Depth.Bids) == 0 || len(item.Depth.Asks) == 0 {
			t.Errorf("Expected 5-level bids/asks for %s, got bids: %d, asks: %d", s, len(item.Depth.Bids), len(item.Depth.Asks))
		}

		// 验证 JSON 序列化符合 NormalizedAssetItem 契约
		data, err := json.Marshal(item)
		if err != nil {
			t.Fatalf("JSON marshal failed: %v", err)
		}
		if len(data) == 0 {
			t.Errorf("Empty JSON output")
		}
	}
}

func TestFundAdapter(t *testing.T) {
	fundCode := "005827" // 易方达蓝筹精选
	item, err := FetchFundQuote(fundCode)
	if err != nil {
		t.Fatalf("FetchFundQuote failed: %v", err)
	}

	if item.Symbol != fundCode {
		t.Errorf("Expected symbol %s, got %s", fundCode, item.Symbol)
	}
	if item.Price <= 0 {
		t.Errorf("Invalid fund price/gsz: %f", item.Price)
	}
	if item.AssetType != "fund" {
		t.Errorf("Expected assetType fund, got %s", item.AssetType)
	}
	if item.Depth != nil {
		t.Errorf("Expected nil depth for fund, got %v", item.Depth)
	}

	data, err := json.Marshal(item)
	if err != nil {
		t.Fatalf("JSON marshal failed: %v", err)
	}
	if len(data) == 0 {
		t.Errorf("Empty JSON output")
	}
}
