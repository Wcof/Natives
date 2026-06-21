# 个人主页 Token 统计迁移至 ccusage 重构方案

> 本文档为个人主页（Dashboard）的 Token 使用量及成本统计能力全面迁移至 `ccusage` 插件的重构设计方案。该方案旨在通过 `ccusage` 统一管理多编码代理（Claude Code, Codex, Copilot CLI 等）的 Token 用量与成本，简化后端自定义解析，并保证前端渲染的 100% 兼容性。

---

## 一、背景与重构必要性 (Why ccusage)

1. **多代理支持 (Multi-Agent Support)**：
   当前后端 `usage.rs` 通过分别读取 `~/.claude/stats-cache.json` 和 `~/.codex/sessions` 来获取 Claude Code 和 Codex 的用量。这种方式难以扩展到其他工具（如 Copilot CLI, Gemini CLI）。而 `ccusage` 作为专业的 CLI 用量分析工具，原生支持了 15+ 种不同编码代理的用量扫描。
2. **精准的离线成本计算 (Accurate Offline Costing)**：
   `ccusage` 内置了完整的模型价格字典，支持通过 `--offline` 标志进行离线成本换算与模型细分统计，极大降低了后端自行维护计费规则的复杂度。
3. **后端代码瘦身与高鲁棒性**：
   通过调用 `ccusage` 统一获取 JSON 数据，后端可以物理删除近 600 行用于手动解析各代理日志与统计缓存的文件读取和正则解析逻辑，避免因第三方工具文件格式更新导致解析崩溃。

---

## 二、后端重构设计 (Backend Design)

### 1. 核心接口与命令调用

在后端 [usage.rs](file:///Users/ldh/Downloads/project/AiNative/Natives2/src-tauri/src/commands/usage.rs) 中，替换传统的文件读取流程，改为调用命令行执行 `ccusage` 获取实时统计：

- **调用命令**：
  ```bash
  ccusage daily -j -O
  ```
  *(说明：`-j` 或 `--json` 输出结构化 JSON 格式，`-O` 或 `--offline` 开启离线估算防止由于网络断开导致命令卡死悬挂)*
- **执行环境**：
  通过 `/bin/zsh -lc` 唤起以继承用户的 PATH 环境变量，优先调用系统 `which ccusage`；若未全局安装，则 fallback 至 `npx --no-install ccusage daily -j -O` 检查本地 npx 缓存。

### 2. ccusage JSON 数据模型定义 (Rust Structs)

在 `usage.rs` 中使用 `serde` 声明 ccusage 返回的 JSON 结构体：

```rust
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CcusageResponse {
    pub history: Vec<CcusageHistoryPoint>,
    pub totals: CcusageTotals,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CcusageHistoryPoint {
    pub period: String,              // 例如 "2026-06-21"
    pub inputTokens: u64,
    pub outputTokens: u64,
    pub cacheCreationTokens: u64,
    pub cacheReadTokens: u64,
    pub totalTokens: u64,
    pub totalCost: f64,
    pub modelBreakdowns: Vec<CcusageModelBreakdown>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CcusageModelBreakdown {
    pub modelName: String,
    pub inputTokens: u64,
    pub outputTokens: u64,
    pub cacheCreationTokens: u64,
    pub cacheReadTokens: u64,
    pub cost: f64,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CcusageTotals {
    pub inputTokens: u64,
    pub outputTokens: u64,
    pub cacheCreationTokens: u64,
    pub cacheReadTokens: u64,
    pub totalTokens: u64,
    pub totalCost: f64,
}
```

### 3. 数据映射层 (Data Mapping)

为了实现**前端零修改（Zero Frontend Changes）**，我们在 Rust 后端将 `CcusageResponse` 直接映射到现有的 `UsageResponse` 接口模型上：

#### 1) Totals 映射（渲染个人主页看板 TokenHero）
将 `totals` 里的汇总数据映射到 `claude` (作为主展示数据)：
- `ClaudeUsage.local_tokens.total` = `totals.totalTokens`
- `ClaudeUsage.local_tokens.input` = `totals.inputTokens`
- `ClaudeUsage.local_tokens.output` = `totals.outputTokens`
- `ClaudeUsage.local_tokens.cacheCreation` = `totals.cacheCreationTokens`
- `ClaudeUsage.local_tokens.cacheRead` = `totals.cacheReadTokens`
- `ClaudeUsage.total_cost` = `Some(totals.totalCost)`
- `ClaudeUsage.total_requests` = 历史天数内命令请求的总和（或置 0）

#### 2) History 趋势图映射（渲染 TokenTrendChart）
遍历 `history` 并映射为 `UsageHistoryPoint`：
- `UsageHistoryPoint.date` = `CcusageHistoryPoint.period`
- `UsageHistoryPoint.input` = `CcusageHistoryPoint.inputTokens`
- `UsageHistoryPoint.output` = `CcusageHistoryPoint.outputTokens`
- `UsageHistoryPoint.cacheWrite` = `CcusageHistoryPoint.cacheCreationTokens`
- `UsageHistoryPoint.cacheRead` = `CcusageHistoryPoint.cacheReadTokens`

#### 3) Model Stats 映射（渲染模型分配比例表格 ModelStatsTable）
遍历 `history` 里的 `modelBreakdowns` 进行汇总聚合，累计得到每种模型的总用量和比例，映射为 `ModelStatUsage` 列表。

#### 4) 数据源面包屑规避“假数据红线”
将 `source_breadcrumbs` 设置为 `["ccusage (via daily -j -O)"]`，如果 ccusage 命令执行成功，则 `source_configured` 为 `true`。

---

## 三、前端兼容性与修改 (Frontend Actions)

由于后端在 `UsageResponse` 数据结构层面对 `ccusage` 的数据进行了完美的对齐和映射：
- **`src/app/page.tsx`**：无需做任何代码修改。它将继续像以前一样调用 `nativesAPI.usage.refresh()`，获取相同格式的 JSON。
- **`TokenHero.tsx`** 和 **`TokenTrendChart.tsx`**：完全无需修改。其对 `localTokens` 字段及趋势图坐标轴字段的解构依旧能完全吻合。
- **RTK 节约量卡片**：由于 `ccusage` 本身只统计消费（Usage）而不统计节约量（Savings），RTK 节约量卡片（Token Savings）可继续保留现有逻辑，或者根据实际需要展示。

---

## 四、实施与验证计划

### 1. 开发实施步骤
1. **修改 `UsageResponse` 构造逻辑**：
   在 `usage.rs` 中保留原接口声明，清空 `read_claude_usage_from_files` 和 `read_codex_usage_from_files` 的调用。
2. **实现命令行调用**：
   新增后台执行 `ccusage daily -j -O` 命令的辅助函数，捕获并解析 JSON 结果。
3. **完成数据映射**：
   在 `usage_refresh` command 里，将 `CcusageResponse` 各数据项填入 `UsageResponse`，并写入本地数据库缓存。

### 2. 验证与检查命令
- **Rust 后端编译**：
  ```bash
  cd src-tauri && cargo check
  ```
- **前端 TypeScript 检查**：
  ```bash
  npm run typecheck
  ```
- **功能性验收**：
  打开应用首页仪表盘，确认 Token 使用总量、日用量趋势图折线、模型分布比例表格等数据正常回显，且数据准确无误。
