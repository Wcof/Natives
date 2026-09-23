//! pricing: 常见 AI 模型定价表与微美元费用计算。
//! 费用以微美元（1 USD = 1,000,000 micros）定点整数存储与计算。

pub struct ModelPrice {
    /// 每百万输入 Token 价格（微美元）
    pub input_per_million: i64,
    /// 每百万输出 Token 价格（微美元）
    pub output_per_million: i64,
    /// 每百万缓存读取 Token 价格（微美元）
    pub cache_read_per_million: i64,
    /// 每百万缓存写入 Token 价格（微美元）
    pub cache_write_per_million: i64,
}

impl ModelPrice {
    pub const fn new(
        input_usd: f64,
        output_usd: f64,
        cache_read_usd: f64,
        cache_write_usd: f64,
    ) -> Self {
        Self {
            input_per_million: (input_usd * 1_000_000.0) as i64,
            output_per_million: (output_usd * 1_000_000.0) as i64,
            cache_read_per_million: (cache_read_usd * 1_000_000.0) as i64,
            cache_write_per_million: (cache_write_usd * 1_000_000.0) as i64,
        }
    }
}

pub fn get_model_price(model: &str) -> ModelPrice {
    let m = model.to_lowercase();
    if m.contains("claude-3-7-sonnet") || m.contains("claude-3.7-sonnet") {
        ModelPrice::new(3.0, 15.0, 0.30, 3.75)
    } else if m.contains("claude-3-5-sonnet") || m.contains("claude-3.5-sonnet") {
        ModelPrice::new(3.0, 15.0, 0.30, 3.75)
    } else if m.contains("claude-3-5-haiku") || m.contains("claude-3.5-haiku") {
        ModelPrice::new(0.80, 4.0, 0.08, 1.0)
    } else if m.contains("claude-3-opus") {
        ModelPrice::new(15.0, 75.0, 1.5, 18.75)
    } else if m.contains("gpt-4o-mini") {
        ModelPrice::new(0.15, 0.60, 0.075, 0.15)
    } else if m.contains("gpt-4o") {
        ModelPrice::new(2.50, 10.0, 1.25, 2.50)
    } else if m.contains("o1-mini") {
        ModelPrice::new(1.10, 4.40, 0.55, 1.10)
    } else if m.contains("o1") {
        ModelPrice::new(15.0, 60.0, 7.50, 15.0)
    } else if m.contains("o3-mini") {
        ModelPrice::new(1.10, 4.40, 0.55, 1.10)
    } else if m.contains("deepseek-reasoner") || m.contains("deepseek-r1") {
        ModelPrice::new(0.55, 2.19, 0.14, 0.55)
    } else if m.contains("deepseek-chat") || m.contains("deepseek-v3") {
        ModelPrice::new(0.14, 0.28, 0.014, 0.14)
    } else if m.contains("gemini-2.0-flash") {
        ModelPrice::new(0.10, 0.40, 0.025, 0.10)
    } else if m.contains("gemini-2.0-pro") {
        ModelPrice::new(1.25, 5.0, 0.3125, 1.25)
    } else if m.contains("qwen") {
        ModelPrice::new(0.20, 0.60, 0.05, 0.20)
    } else {
        // 默认通用基线估算
        ModelPrice::new(1.0, 3.0, 0.1, 1.0)
    }
}

pub fn calculate_cost_micros(
    model: &str,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    reasoning_tokens: i64,
) -> i64 {
    let price = get_model_price(model);
    let uncached_input = (input_tokens - cache_read_tokens - cache_write_tokens).max(0);
    let total_output = output_tokens + reasoning_tokens;

    let input_cost = (uncached_input * price.input_per_million) / 1_000_000;
    let cache_read_cost = (cache_read_tokens * price.cache_read_per_million) / 1_000_000;
    let cache_write_cost = (cache_write_tokens * price.cache_write_per_million) / 1_000_000;
    let output_cost = (total_output * price.output_per_million) / 1_000_000;

    input_cost + cache_read_cost + cache_write_cost + output_cost
}
