package quote

import (
	"testing"
	"time"
)

// Golden fixture: real qt.gtimg.cn response shape (GBK-decoded names).
const tencentFixture = `v_sh600519="1~贵州茅台~600519~1700.00~1680.00~1690.00~25000~12000~13000~1699.90~100~1699.80~200~1699.70~300~1699.60~400~1699.50~500~1700.10~600~1700.20~700~1700.30~800~1700.40~900~1700.50~1000~~20250917150003~20.00~1.19~1710.00~1675.00~1700.00~425000000~425000~1.20~1700.00~~1700.00~2.50~1.19~1698.75~1710.00~1675.00~14.50~170000000000~170000000000~1.5~1690.00~1700.00~1.19~20250917~150003~~";`

func TestParseTencent(t *testing.T) {
	now := time.Now()
	items, err := ParseTencent(tencentFixture, now)
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	if len(items) != 1 {
		t.Fatalf("want 1 item, got %d", len(items))
	}
	it := items[0]
	if it.ID != "A:sh600519" || it.Symbol != "600519" || it.Name != "贵州茅台" {
		t.Errorf("identity wrong: %+v", it)
	}
	if it.AssetType != "stock" {
		t.Errorf("600519 should be stock, got %s", it.AssetType)
	}
	if it.Price != 1700.00 || it.PrevClose != 1680.00 {
		t.Errorf("price wrong: %v / %v", it.Price, it.PrevClose)
	}
	if it.Change != 20.00 || it.ChangePercent != 1.19 {
		t.Errorf("change wrong: %v / %v", it.Change, it.ChangePercent)
	}
	if it.High == nil || *it.High != 1710.00 || it.Low == nil || *it.Low != 1675.00 {
		t.Errorf("high/low wrong: %v / %v", it.High, it.Low)
	}
	if it.Volume == nil || *it.Volume != 25000 {
		t.Errorf("volume wrong: %v", it.Volume)
	}
	if it.Amount == nil || *it.Amount != 425000000 {
		t.Errorf("amount wrong: %v", it.Amount)
	}
	if len(it.Depth.Bids) != 5 || len(it.Depth.Asks) != 5 {
		t.Fatalf("depth levels wrong: %d/%d", len(it.Depth.Bids), len(it.Depth.Asks))
	}
	if it.Depth.Bids[0][0] != 1699.90 || it.Depth.Bids[0][1] != 100 {
		t.Errorf("bid1 wrong: %v", it.Depth.Bids[0])
	}
	if it.Depth.Asks[4][0] != 1700.50 || it.Depth.Asks[4][1] != 1000 {
		t.Errorf("ask5 wrong: %v", it.Depth.Asks[4])
	}
	if it.Sparkline[0] != 1700.00 {
		t.Errorf("sparkline seed wrong: %v", it.Sparkline)
	}
}

func TestParseTencentETF(t *testing.T) {
	body := `v_sh510300="1~沪深300ETF~510300~4.000~3.950~3.960~100000~50000~50000~3.999~100~3.998~200~3.997~300~3.996~400~3.995~500~4.001~600~4.002~700~4.003~800~4.004~900~4.005~1000~~20250917150003~0.05~1.27~4.05~3.94~4.00~400000~400000~1.00~4.00~~4.00~2.00~1.27~3.98~4.05~3.94~3.00~~400000~1.5~3.96~4.00~1.27~20250917~150003~~";`
	items, err := ParseTencent(body, time.Now())
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	if items[0].AssetType != "etf" || items[0].ID != "A:sh510300" {
		t.Errorf("etf classification wrong: %+v", items[0])
	}
}

func TestCurrentPhase(t *testing.T) {
	cst := cstZone()
	cases := []struct {
		hhmm string
		want SessionPhase
	}{
		{"09:20", PhaseAuction},
		{"09:27", PhaseBreak},
		{"10:00", PhaseMorning},
		{"12:00", PhaseBreak},
		{"14:00", PhaseAfternoon},
		{"15:30", PhasePostMarket},
		{"23:00", PhaseClosed},
	}
	for _, c := range cases {
		tm, _ := time.ParseInLocation("2006-01-02 15:04", "2026-09-17 "+c.hhmm, cst) // Thursday
		if got := CurrentPhase(tm); got != c.want {
			t.Errorf("%s: want %v got %v", c.hhmm, c.want, got)
		}
	}
	// Weekend
	sat, _ := time.ParseInLocation("2006-01-02 15:04", "2026-09-19 10:00", cst)
	if got := CurrentPhase(sat); got != PhaseClosed {
		t.Errorf("saturday should be closed, got %v", got)
	}
}

func TestParseFundGzIntraday(t *testing.T) {
	body := `jsonpgz({"fundcode":"005827","name":"易方达蓝筹精选混合","jzrq":"2026-09-16","dwjz":"2.0000","gsz":"2.0470","gszzl":"2.35","gztime":"2026-09-17 14:30"});`
	item, err := ParseFundGz(body, false, time.Now())
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	if item.ID != "F:005827" || item.Symbol != "005827" || item.Name != "易方达蓝筹精选混合" {
		t.Errorf("identity wrong: %+v", item)
	}
	if item.AssetType != "fund" {
		t.Errorf("assetType wrong: %s", item.AssetType)
	}
	if item.Price != 2.0470 || item.PrevClose != 2.0000 {
		t.Errorf("price wrong: %v / %v", item.Price, item.PrevClose)
	}
	if item.ChangePercent != 2.35 {
		t.Errorf("pct wrong: %v", item.ChangePercent)
	}
	if item.Depth != nil || item.Volume != nil || item.High != nil {
		t.Errorf("fund optionals must be empty: %+v", item)
	}
}

func TestParseFundGzAfterHours(t *testing.T) {
	body := `jsonpgz({"fundcode":"005827","name":"易方达蓝筹精选混合","jzrq":"2026-09-16","dwjz":"2.0000","gsz":"2.0500","gszzl":"2.50","gztime":"2026-09-17 15:00"});`
	item, err := ParseFundGz(body, true, time.Now())
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	if item.Price != 2.0000 { // 盘后：正式净值替代估值
		t.Errorf("after-hours price should be dwjz, got %v", item.Price)
	}
}
