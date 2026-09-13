package usage

// R5 计价验收（方案 §7.2/§9.1）：缓存计费示例与非重叠桶。
// 教学价格为方案指定的测试价格，非真实模型报价。

import (
	"testing"
)

// TestCalculateCostCacheBuckets 方案 §7.2 验收示例：
// 普通输入 $1/百万，缓存读 $0.1/百万；输入总量 100 万（其中缓存读 80 万，
// 无输出）→ 正确费用 $0.28（0.2M×$1 + 0.8M×$0.1），
// 不能把缓存读再按普通输入价算成 $1.08。
func TestCalculateCostCacheBuckets(t *testing.T) {
	store, _ := tempStore(t)
	defer store.Close()
	calc := NewCalculator(store)

	const testModel = "test-cache-model"
	builtinPricesLock.Lock()
	builtinPrices[testModel] = Price{
		ModelID:             testModel,
		InputPriceMicro:     1_000_000, // $1.00 / 1M tokens
		CacheReadPriceMicro: 100_000,   // $0.10 / 1M tokens
	}
	builtinPricesLock.Unlock()
	defer func() {
		builtinPricesLock.Lock()
		delete(builtinPrices, testModel)
		builtinPricesLock.Unlock()
	}()

	// 非重叠桶：input_uncached = 200_000，cache_read = 800_000。
	got := calc.CalculateCost("openai", testModel, 200_000, 0, 800_000, 0)
	// 期望 280_000 micro = $0.28；若缓存读被重复按输入价计则为 $1.08。
	if got != 280_000 {
		t.Errorf("cache pricing = %d micro ($%.4f), want 280_000 ($0.28)", got, float64(got)/1e6)
	}
}

// TestCalculateCostMissingPriceIsNotFree 缺价不冒充免费：目录缺该模型时
// CalculateCost 返回 0 仅是"无估算金额"，读侧由 costStatus=unpriced 标注
// （usage_handlers.go 读时判定），金额与状态不得混同（§9.1）。
func TestCalculateCostMissingPriceIsNotFree(t *testing.T) {
	store, _ := tempStore(t)
	defer store.Close()
	calc := NewCalculator(store)

	if p := calc.FindPrice("openai", "definitely-not-a-model"); p != nil {
		t.Fatalf("unexpected price for unknown model: %+v", p)
	}
	// 金额为 0 且 FindPrice 为 nil —— 上层据此注解 unpriced，与合法零价（有价、金额 0）区分。
	cost := calc.CalculateCost("openai", "definitely-not-a-model", 1000, 0, 0, 0)
	if cost != 0 {
		t.Errorf("missing price cost = %d, want 0 (amount unknown, status unpriced)", cost)
	}
}
