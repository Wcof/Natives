# 投资模块 V2 设计方案

> 版本：v1.0 · 日期：2026-11-02 · 状态：待评审
> 形态：Natives 内置模块插件（Chrome 扩展 / 本机 Web 双入口共用），不涉及独立 Web 端与移动端
> 上游约束：ADR-0031（内置模块）、ADR-0037/0040（模块推理）、ADR-0044（Model Host 直连）

## 一、背景与结论

### 1.1 现状

| 指标 | 现状 |
|---|---|
| 主导航入口 | 12 个平级（已收敛为 3 主 + 工具抽屉） |
| 视图 | 11 个 + settings |
| SQLite 表 | 59 张 |
| Rust | 170 文件 / 62,082 行 |
| 前端 JS | 15,822 行 |
| 用户数据 | **0 条**（`~/.natives/apps/fund/data/` 从未创建） |

### 1.2 诊断结论

按数据归属拆开，59 张表其实属于两个产品：

- **个人财产管理**（约 14 张）：`accounts`、`positions`、`transactions`、`funds`、`fund_nav`、`value_snapshots`、`opening_*`、`manual_assets`
- **投研情报终端**（约 45 张）：`information_*`、`opportunity_*`、`watch_*`、`strategy_*`、`rules`、`notify_*`、`analysis_reports`、`chart_drawings`、`group_plan*`、`otc_*`、`offline_*`、`corrections`

前者每周看一次，后者每天看多次。二者平级并列在同一层导航，是「太复杂」的直接来源。

**关键发现：现有数据层内核是正确的，不需要推倒重来。**

- `storage.rs:53` 注释明确 `positions` 是 `transactions` + 期初快照的只读回放投影，**不双权威** —— 「流水是唯一权威」这条原则已经落地
- `fixed.rs` 已实现十进制定点：份额/净值/单价 4 位小数、金额/费用 2 位小数、i64 存储、禁用浮点

所以本次重构的性质是 **瘦身 + 扩类**，不是重写：

| 动作 | 对象 |
|---|---|
| 保留并复用 | 定点精度、流水→投影回放、迁移框架、i18n、主题装配、app-runtime 集成 |
| 扩展 | 资产类别（股票 / 基金 / 数字货币）、A/C 类归组、海外市场与币种 |
| 归档 | 45 张投研情报表及其 UI |
| 新增 | 行情与汇率缓存、每日快照、AI 评估 |

### 1.3 定位

**我的钱现在值多少、变化从哪来、下一步该做什么。**

判定标准：任何功能若无法回答这句话，进入工具抽屉或不做，不进入主导航。

## 二、需求范围

### 2.1 本期做（Must）

| 编号 | 需求 | 成功标准 |
|---|---|---|
| FR-01 | 账户管理（渠道 + 币种） | 可增删改账户；渠道决定默认币种与可交易类别 |
| FR-02 | 标的维护（股票/基金/数字货币） | 支持 A/C 类归组；支持海外市场与币种 |
| FR-03 | 流水录入（买入/卖出/分红/拆分送转/费用） | 五类可录；改流水后持仓自动重算 |
| FR-04 | 持仓投影（数量、均价、浮盈） | 由流水推导，禁止手工改数量 |
| FR-05 | 每日快照与走势 | 缺失日期可自动补算；支持按类别/账户/标的聚合 |
| FR-06 | 总览页（净值 + 走势 + 分类） | 一屏内回答定位的三问 |
| FR-07 | AI 单标的评估 | 结构化五段输出，不给买卖指令 |
| FR-08 | AI 设置（模型 / 风格 / 总览展示） | 只选已配置模型，不接触 Key |

### 2.2 本期不做（Won't）

| 项 | 理由 |
|---|---|
| 实物资产（含格物） | 由格物独立承担；模型保留 `asset_class` 扩展位 |
| 实时行情推送 / 分时 / K 线 | 只做收盘后日频拉取 |
| 技术指标、缠论、画线 | 属投研能力，随 45 张表一并归档 |
| 资讯流、机会发现、策略配置 | 同上 |
| 多用户 / 权限 / 分享 | 单用户本地工具 |
| 独立 Web 端 / 移动端 | 本期只做模块形态 |

### 2.3 范围外但需预留

- `asset_class` 增加 `physical` 时，模型不需要改结构（见 3.2 说明）
- 币种增加时不改结构，只增 `fx_rates` 记录

## 三、数据模型

### 3.1 表清单（12 张）

| # | 表 | 状态 | 职责 |
|---|---|---|---|
| 1 | `accounts` | 改造 | 账户：渠道 + 币种 |
| 2 | `instruments` | 改造（原 `funds`） | 标的：股票/基金/数字货币 |
| 3 | `transactions` | 改造 | 流水，唯一录入权威 |
| 4 | `positions` | 保留 | 持仓投影，只读，由流水回放 |
| 5 | `opening_snapshots` | 保留 | 期初建仓快照 |
| 6 | `quotes` | 新建（承 `fund_nav`） | 行情缓存，只存持仓标的 |
| 7 | `fx_rates` | 新建 | 汇率缓存，只存用到的币种 |
| 8 | `snapshots` | 改造（原 `value_snapshots`） | 每日总览快照 |
| 9 | `snapshot_items` | 新建 | 快照明细，按持仓粒度 |
| 10 | `ai_evaluations` | 新建 | AI 评估结果与输入快照 |
| 11 | `ai_settings` | 新建 | 模型选择与评估风格 |
| 12 | `app_meta` | 保留 | 迁移版本与元数据 |

### 3.2 核心表定义

```sql
-- 账户：渠道天然绑定市场与币种，不单独建 market 表
CREATE TABLE accounts (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    name          TEXT NOT NULL UNIQUE,
    channel       TEXT NOT NULL
                  CHECK (channel IN ('domestic_broker','fund_platform','overseas_broker','crypto_exchange')),
    currency      TEXT NOT NULL DEFAULT 'CNY',
    note          TEXT NOT NULL DEFAULT '',
    archived_at   TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

-- 标的：funds 的泛化。A/C 类靠 group_code 归组，两者独立记账、合并展示
CREATE TABLE instruments (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    code          TEXT NOT NULL,
    name          TEXT NOT NULL DEFAULT '',
    asset_class   TEXT NOT NULL CHECK (asset_class IN ('stock','fund','crypto')),
    market        TEXT NOT NULL DEFAULT 'CN',      -- CN / HK / US / CRYPTO
    currency      TEXT NOT NULL DEFAULT 'CNY',
    group_code    TEXT,                            -- A/C 类共享，NULL 表示不归组
    quote_symbol  TEXT,                            -- 行情源标识，与 code 分离
    latest_price_raw INTEGER,
    latest_price_at  TEXT,
    archived_at   TEXT,
    updated_at    TEXT NOT NULL,
    UNIQUE (code, market)
);
CREATE INDEX idx_instruments_group ON instruments(group_code) WHERE group_code IS NOT NULL;

-- 流水：五类，唯一录入面
CREATE TABLE transactions (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id    INTEGER NOT NULL REFERENCES accounts(id),
    instrument_id INTEGER NOT NULL REFERENCES instruments(id),
    type          TEXT NOT NULL CHECK (type IN ('BUY','SELL','DIVIDEND','SPLIT','FEE')),
    quantity_raw  INTEGER NOT NULL DEFAULT 0,   -- 份额/股数/枚，4 位小数
    price_raw     INTEGER NOT NULL DEFAULT 0,   -- 单价/净值，4 位小数
    amount_raw    INTEGER NOT NULL DEFAULT 0,   -- 成交金额，2 位小数
    fee_raw       INTEGER NOT NULL DEFAULT 0,   -- 费用，2 位小数
    ratio         REAL,                         -- 仅 SPLIT：拆股/送转比例
    trade_date    TEXT NOT NULL,
    note          TEXT NOT NULL DEFAULT '',
    request_id    TEXT UNIQUE,                  -- 手工录入幂等
    external_id   TEXT,                         -- 导入幂等
    created_at    TEXT NOT NULL
);
CREATE INDEX idx_tx_holding ON transactions(account_id, instrument_id, trade_date);
```

`SPLIT` 用 `ratio` 表达数量变化，不动 `amount_raw`；`DIVIDEND` 走 `amount_raw` 计入回收，不增数量。

### 3.3 精度约定（复用 `fixed.rs`，不改）

| 对象 | scale | 说明 |
|---|---|---|
| 份额 / 股数 / 币量 | 4 | `quantity_raw` |
| 净值 / 单价 | 4 | `price_raw` |
| 金额 / 费用 | 2 | `amount_raw` / `fee_raw` |
| 汇率 | 6 | 换算中间值，最终金额仍回 2 位 |

禁止浮点累计。账本运算 ROUND_DOWN，最终估值 half-up。

### 3.4 表数量变化

```
59 张 → 12 张（净减 47）
其中：保留 3、改造 4、新建 5、归档 45
```

## 四、核心算法

### 4.1 持仓投影

```
数量 = 期初 + Σ(BUY.quantity) − Σ(SELL.quantity) + Σ(SPLIT 调整)
成本 = 期初成本 + Σ(BUY.amount + BUY.fee) − Σ(SELL 成本摊销)
均价 = 成本 ÷ 数量
```

**采用移动加权平均**（已确认）：卖出时按当时均价摊销成本，不追踪买入批次，不需要 lot tracking。

### 4.2 日均收益（格物指标的金融化改造）

格物原式（实物）：
```
日用成本 = (购买总价 + 后续成本 − 回收) ÷ 持有天数
```

投资改造：
```
净投入   = 买入总额 + 费用 − 卖出回收 − 现金分红
日均收益 = (当前市值 − 净投入) ÷ 持有天数
```

同一公式，符号反转。正数表示「这笔钱每天给我赚多少」，负数表示「每天在消耗」。

已实现盈亏与浮动盈亏分开记，不混入同一数字。

### 4.3 快照与补算

格物靠服务端 cron 每日写快照。Natives 模块无常驻进程（ADR 要求 Worker 关闭 2 秒内释放），改用 **lazy snapshot**：

```
模块打开
  → 读 snapshots 最后一条日期 D
  → 若 D < 今天：对 D+1 … 今天逐日补算
  → 补算用当日可得的收盘价与汇率，缺则沿用前值并标记 stale
```

好处：关机一周后再打开会一次性补齐，不会断层。代价：首次打开可能慢（N 天 × M 持仓），需在 UI 显示补算进度。

## 五、页面结构

```
主层（3）
  概览   净资产 · 走势 · 分类占比 · 日均收益 · AI 结论
  持仓   按类别分组，含均价、浮盈、持有天数
  资产   按账户分组，含币种、渠道

工具抽屉
  流水录入 · 标的维护 · 账户维护 · 行情状态 · 设置

详情页（下钻）
  持仓详情（流水 + 走势 + AI 看看）
  标的历史（跨账户合并）
```

对应格物的 8 页面结构，不引入额外层级。

## 六、AI 设计

### 6.1 可复用格物的三点

1. **算好的指标作为入参**，不让模型做算术
2. **结构化输出 + schema 校验 + 失败重试 1 次**
3. **存 `snapshot_json`**，可回溯当时的判断依据

### 6.2 不可复用：Key 存储

格物在设置页填 `baseUrl` + `apiKey` 并明文存库。本项目受 ADR-0044/0037/0038 约束：

- Key 只存 Keychain，由 Model Host 注入
- 模块只持有 `providerId` + `modelId` 安全引用
- 设置页**不出现** 地址与 Key 输入框

### 6.3 输入契约

```json
{
  "holding": { "name": "", "code": "", "asset_class": "", "market": "" },
  "account": { "channel": "" },
  "position": { "quantity": "", "avg_cost": "", "price": "", "market_value": "", "unrealized": "" },
  "metrics": { "held_days": 0, "daily_gain": "", "realized": "" },
  "flows":  [{ "type": "", "date": "", "amount": "" }],
  "context": { "class_weight": "", "benchmark_return": "" }
}
```

### 6.4 输出契约

```json
{
  "score": 0,
  "tone": "steady | watch | alert",
  "label": "≤8 字定调",
  "summary": "≤80 字",
  "concerns": ["≤3 条关注点，每条 ≤20 字"]
}
```

**行为约束三层**：

| 层 | 规则 |
|---|---|
| Always | 只基于输入数据陈述；指出数据缺口 |
| Ask first | 涉及「是否加减仓」只给关注点，不落到具体操作 |
| Never | 不预测涨跌、不给买卖指令、不承诺收益 |

### 6.5 触发与降级

- 手动触发：持仓详情「AI 看看」
- 批量触发：总览「全局体检」，每周最多提醒一次
- 降级路径：模型不可用 → 保留上次结论并标注时间；解析失败重试 1 次 → 仍失败则显示「评估不可用」并附原始错误码
- 不阻塞：AI 失败不影响净值、持仓、走势任何数字

### 6.6 设置项（三项）

| 项 | 取值 |
|---|---|
| 默认模型 | 从 Model Host 已配置列表选择 |
| 评估风格 | 保守 / 中性 / 进取（影响 prompt 措辞，不影响数据） |
| 总览展示 | 是否在概览页显示最近一次评估结论 |

## 七、实施计划

| 阶段 | 内容 | 触碰数据 | 可回滚 |
|---|---|---|---|
| P1 | 表结构重建（12 张）+ 迁移框架升版到 v30 | 是（新库） | 删除 `fund.db` 重建 |
| P2 | 账户 / 标的基础维护 + 流水录入 | 是 | 同上 |
| P3 | 持仓投影 + 均价 + 日均收益 | 否 | 代码回退 |
| P4 | 快照与 lazy 补算 + 总览走势 | 是（写快照） | 清空 `snapshots` |
| P5 | 归档 45 张表及其 UI，收敛导航 | 是（DROP） | 需先备份 |
| P6 | AI 评估（broker 接入 + 评估页） | 是（写评估） | 删 `ai_evaluations` |

**P5 是唯一不可逆步骤**，执行前必须完成一次 `fund.db` 全量备份，且备份留存到验收通过。

### 7.1 迁移策略

数据库从未初始化，`user_version = 0`。两条路径：

| 场景 | 做法 |
|---|---|
| 全新用户（当前） | 直接建 v30 全量表，不经历阶梯迁移 |
| 已有数据（若存在） | 走 `accounts → transactions → positions` 映射，45 张投研表 DROP 前先导出 JSON 存档 |

若确认无历史数据，跳过映射，直接 `rm -rf ~/.natives/apps/fund/data` 重建。

## 八、验收标准

- [ ] 录入一笔买入后，持仓数量、均价、市值、浮盈四项自动正确，无需手工修正
- [ ] 修改任意一笔流水，所有下游数字（持仓、快照、走势）同步更新
- [ ] 删除一笔流水后，持仓回到录入前状态，无残留
- [ ] 拆股（1 拆 2）后数量翻倍、均价减半、市值不变
- [ ] 现金分红后净投入减少、数量不变
- [ ] 关机模拟 7 天后再打开，快照自动补齐 7 天，走势不断层
- [ ] 美股持仓按汇率折算后计入总净值，切换汇率后总额随之变化
- [ ] A/C 类基金分开持仓、合并展示，合并数字等于两者之和
- [ ] AI 评估失败时，净值/持仓/走势不受影响，且显示明确降级原因
- [ ] 设置页无任何 Key 输入框
- [ ] `modules:ui:check` 与 `cargo test -p fund` 通过
- [ ] archive / volt 双主题下布局一致

## 九、风险登记

| 编号 | 风险 | 概率 | 影响 | 等级 | 应对 |
|---|---|---|---|---|---|
| RK-01 | P5 归档 45 张表不可逆，误删有用数据 | 2 | 5 | 10 中 | 执行前全量备份 + 导出 JSON，验收通过后才清理 |
| RK-02 | lazy 补算首次打开耗时过长（持仓多、缺失天数多） | 3 | 3 | 9 中 | 分批补算 + 进度提示；超过 90 天只补月度末值 |
| RK-03 | 行情源不稳定导致市值虚高/虚低 | 4 | 4 | 16 高 | 缺行情时沿用前值并标 stale；总览显著提示数据新鲜度 |
| RK-04 | 汇率换算口径与券商对不上，净值有差异 | 3 | 3 | 9 中 | 汇率来源与时间固定写入口径说明；差异 >0.5% 时提示 |
| RK-05 | AI 输出被误读为投资建议 | 3 | 4 | 12 中 | prompt 三层约束 + UI 固定免责声明 + 不给买卖指令 |
| RK-06 | 移动加权均价与券商 App 显示不一致，用户困惑 | 4 | 2 | 8 中 | 持仓详情标注「本系统口径：移动加权」并给计算公式 |

## 十、实施记录

### P1 数据层（2026-11-02 完成）

| 项 | 结果 |
|---|---|
| `SCHEMA_VERSION` | 29 → 30 |
| 新增表 | 9 张（instruments / flows / holdings / quotes / fx_rates / snapshots / snapshot_items / ai_evaluations / ai_settings） |
| 改造表 | `accounts` 加 `channel`、`currency`（ALTER，不重建） |
| 旧表改动 | **零**。45 张投研表原样保留，留待 P5 |
| `module.json` | `dataSchemaVersion` 29 → 30 |

### 验证证据

```
cargo test -p fund-module
  398 passed（lib） + 3 + 2 + 12 + 1 + 3 + 6 + 2 passed（集成）
  0 failed
```

新增两个针对性测试：

- `v30_asset_model_upgrade_preserves_v29_data_and_is_idempotent`
  空库→v29→写入旧数据→升到 v30：旧数据逐项完好、accounts 取到默认渠道、9 张新表可读、重复执行幂等、FK 零违规；并单独验证**新库路径**（`Store::open`）同样含 9 张表。
- `v30_flow_and_evaluation_constraints_reject_invalid_rows`
  未知事件类型、缺比例的拆股、越界评分三类非法写入必须被拒绝。

### 实施中修复的三个问题

1. **CHECK 约束对 NULL 放行**：`split_from > 0` 在 NULL 时求值为 NULL，而 SQL CHECK 遇 NULL 视为通过，导致缺比例的拆股被接受。改为显式 `IS NOT NULL` 判空。
2. **新库与升级是两条独立路径**：`Store::open` 走「`SCHEMA_V1` + 逐版 `execute_batch`」，不走迁移阶梯。v30 必须同时挂到 `open` 与 `open_in_memory` 两处，否则全新用户缺表。该缺陷由既有测试 `fresh_init_and_migration_ladder_produce_identical_tables` 捕获。
3. **契约版本一致性**：`module.json` 的 `dataSchemaVersion` 必须等于迁移目标，否则 `test_fund_module_contract_lifecycle` 失败。

### 与「方案 B」P4 的关系

方案 B 曾计划 `accounts.bucket`（四笔钱：活钱/稳健/长期/保障）。V2 改为按**资产类别**组织，`bucket` 不再是核心维度，由 `channel`（渠道）取代——渠道天然绑定市场与币种。**原 P4 的 bucket 列不再执行。**

### P2–P6 业务层与界面（2026-11-02 完成）

| 阶段 | 交付 | 位置 |
|---|---|---|
| P2 | 账户 / 标的 CRUD，流水录入（五类事件） | `src/invest/crud.rs`、`flows.rs` |
| P3 | 持仓回放：移动加权均价、日均收益、持有天数 | `src/invest/holdings.rs` |
| P4 | 总览聚合、每日快照、汇率折算 | `src/invest/overview.rs` |
| P5 | 导航收敛：1 主入口 + 工具抽屉 | `partials/header.html`、`app.js` |
| P6 | AI 三段式（组装输入 / 外部推理 / 回存结果） | `src/invest/ai.rs` |
| UI | 账本页：概览 / 持仓 / 流水 三段式 | `partials/view-ledger.html`、`ledger-v2.js`、`ledger.css` |

**API**（全部挂在 `/api/v2/`，与旧路由隔离）：

```
GET/POST/PATCH/DELETE  /accounts
GET/POST/PATCH/DELETE  /instruments
GET/POST/DELETE        /flows
GET                    /holdings
GET                    /overview        （顺带沉淀当天快照）
GET                    /snapshots
GET/PUT                /ai/settings
GET                    /ai/context       （模块组装输入，不含推理）
GET/POST               /ai/evaluations
```

### 回放算法的四条不变量（已被测试锁定）

1. **移动加权**：卖出按当时均价摊销，不追踪批次
2. **成本精确归零**：卖光时剩余成本为 0，不留整数除法的分位残渣
3. **超卖拒绝**：录入当时就回放，任何时点为负直接拒绝，而不是展示时才发现
4. **删除保护**：删掉某笔会让后续卖出失去依据时，拒绝删除

### 缺数据时的行为（刻意设计）

| 情况 | 行为 |
|---|---|
| 持仓无行情 | 不计入净值，`incomplete=true`，UI 显示「缺行情」 |
| 外币无汇率 | 按 1:1 计入但标记未折算，不伪造精确数字 |
| 无快照历史 | 走势区显示空态文案，不画假曲线 |

### P5 的实际做法与理由

原计划「DROP 45 张投研表」。实际改为 **UI 层下线，表结构保留**：

- 旧的概览 / 持仓 / 资产 / 自选 / 趋势 / 市场 / 资讯 / 交易 / 导入 / 净值 / 历史 全部收进工具抽屉的「经典视图」等分组
- 45 张表不再有任何主界面入口，但表仍在，可随时回退
- 理由：DROP 不可逆，而收益仅是磁盘空间；在界面已经简化的前提下，保留表结构等于免费买到回滚能力

### 验证证据

```
cargo test -p fund-module
  413 passed（lib） + 3 + 2 + 12 + 1 + 3 + 6 + 2 passed（集成）  0 failed

npm run modules:ui:check
  6 + 6 + 6 + 1 全过

静态渲染：概览 / 持仓 / 工具抽屉，archive 主题截图核对
```

新增 16 个投资账本测试，覆盖：回放算错（人工核对过加权均价与已实现盈亏）、卖光归零、拆股、
分红、超卖拦截、幂等、删除保护、缺行情降级、缺汇率降级、账户归档保留流水、API 路由装配与错误码映射。

### P7 行情、补算与 AI 串联（2026-11-02 完成）

| 能力 | 实现 | 关键取舍 |
|---|---|---|
| 行情自动拉取 | `src/invest/quotes.rs`：只拉**当前持仓**标的，复用 `market::fetch_tencent_quotes` | 拉取失败**不覆盖旧价**，只把 `price_stale=1`；数字货币无数据源直接跳过，不编价 |
| 历史快照补算 | `src/invest/backfill.rs`：按日期截断回放 + 不复权历史价 | 历史某天的持仓数量 = 只回放到那天，而不是拿今天的持仓套历史价 |
| AI 前端串联 | `ui/src/ledger-ai.js`：`options → start → 轮询 status → 存结果` | 复用 `Fund.home.requestCapability`（已有 challenge/超时/取消）；模块仍不碰 Key |

**代码映射**（`quotes.rs`）：

```
CN → normalize_symbol 补 sh/sz/bj
US → us + 代码大写
HK → hk + 5 位数字
CRYPTO → None（数据源不支持，跳过而非报错）
```

**为什么用不复权价**：复权价会改写历史价格，用它算历史市值会与当时真值偏离。不复权价在除权日有跳空，那正是真实市值变化。

**补算边界**：单次最多 90 天（`MAX_BACKFILL_DAYS`），只写缺失日期，**绝不覆盖已有快照**——已有的是实时价落的，更准。

**新增接口**：

```
POST /api/v2/quotes/refresh        拉当前持仓行情
POST /api/v2/snapshots/backfill    补算缺失的历史快照
```

**AI 三段式的落地形态**：`ledger-ai.js` 里的 `evaluate()` 依次做——取 `/ai/context`、`broker('options')` 校验默认模型、`broker('start')`、轮询 `broker('status')` 直到 done、解析 JSON、`POST /ai/evaluations` 回存。prompt 版本固定 `v1` 并随结果落库，可追溯。

### 最终验证证据

```
cargo test -p fund-module
  420 passed（lib） + 3 + 2 + 12 + 1 + 3 + 6 + 2 passed（集成）  0 failed
  6 ignored（含 2 个需联网的 live_ 测试）

cargo test -p fund-module live_ -- --ignored
  live_quote_fetch_returns_price   ok   （腾讯实时行情）
  live_kline_fetch_returns_bars    ok   （腾讯日 K 不复权）

npm run modules:ui:check  6+6+6+1，fail 0
```

静态渲染核对：概览（hero + 分类）、持仓（10 列含 AI 列、分组、涨红跌绿）、工具抽屉。

### P8 三个边界的修复（2026-11-02 完成）

| 边界 | 实现 | 关键取舍 |
|---|---|---|
| 汇率 | `invest/fx.rs`：新浪外汇（实时）+ er-api（兜底） | 双源都失败就**不写库**，让外币持仓继续标「未折算」，不拿 1:1 冒充精确值 |
| 交易时段 | `invest/trading.rs`：按「最近交易日」判定行情是否过期 | 不含法定节假日，宁可多标一次 stale，也不假装最新 |
| AI 历史对比 | 评估结果里展示上一次评分与语气 | 首次评估单独提示，不显示空对比 |

**汇率链路的三个细节**：

1. **只拉用到的币种**：从 `instruments.currency` 与 `accounts.currency` 取并集，排除 CNY。没有外币持仓就不发请求。
2. **USDT 没有直接报价**：按 USD 近似，`fx_rates.source` 记成 `sina-usd-proxy` 或 `er-api-usd-proxy`，事后可追溯这是近似值。
3. **写入用 UPSERT**：同一天重复刷新只更新，不累积行。

**交易时段的判定逻辑**：

```
最近交易日 = 
  今天若是工作日 且 已过开盘时间 → 今天
  否则 → 往前找最近的工作日
行情过期 = 行情日期 < 最近交易日
```

这样周末拉到周五收盘价**不算过期**，而周一开盘后仍显示周五价**才**算出问题。

**修掉的 bug**：`trading::evaluate` 最初把「Unix 天数」直接喂给 `ordinal_to_ymd`，而它要的是儒略日，差了 2440588 天，算出来是公元前 4713 年。测试断言一条条失败才发现——如果只写代码不写测试，交易时段会全错，而 UI 上只会表现为「stale 标记莫名其妙」。

### 最终验证证据

```
cargo test -p fund-module
  429 passed（lib） + 3 + 2 + 12 + 1 + 3 + 6 + 2 passed（集成）  0 failed
  8 ignored（需联网的 live_ 测试）

cargo test -p fund-module live_ -- --ignored
  live_quote_fetch_returns_price          ok  腾讯实时行情
  live_kline_fetch_returns_bars           ok  腾讯日 K 不复权
  live_fx_sources_return_positive_rates   ok  新浪 + er-api 双源
  live_e2e_refresh_fills_market_value_and_fx  ok  端到端闭环

npm run modules:ui:check  6+6+6+1，fail 0
```

**端到端那条最有说服力**：建一个美股账户买 10 股 AAPL →
刷新前 `incomplete=true`、净值 0 → 刷行情 + 刷汇率 →
`incomplete=false`、净值非零、分类汇总出现。这是「行情 + 汇率 + 总览」三者真正打通的证据。

### P9 三个局限的修复（2026-11-02 完成）

| 原局限 | 修法 | 关键点 |
|---|---|---|
| 法定节假日 | `invest/calendar.rs`：**用指数日 K 反推交易日历**，写入 `trading_days` 表 | 上证指数有行情的日期就是 A 股交易日，恒生对应港股，标普 500 对应美股。调休、临时休市自动包含，**零维护**，不用维护几百行节假日常量 |
| 美股夏令时 | 改用数据源返回的 `qt.market` 字段（形如 `US_open_交易中`） | 夏令时由上游处理，比自己算「3 月第二个周日」可靠；60 秒内存缓存，一次刷新只多一个请求 |
| AI 只有最近两次 | `GET /api/v2/ai/trend` + 对话框内折线图 | 升序序列直接画，能看到评分随时间的走向 |

**为什么用指数反推而不是维护节假日表**：

一年的 A 股休市安排约 20 条，十年就是 200 条常量，还得每年更新、处理临时休市（如台风、纪念日）。而指数 K 线本身就是「这一天市场开过门」的权威记录——数据源在哪里维护，我们就不必重复维护。

**表设计的一个取舍**：`trading_days` 只存**有行情的日期**，不存「休市」记录。空白即休市，避免维护两份互斥的真值。

**又修掉一个真 bug**：`calendar::last_trading_day` 最初带了一套「排除周末」的兜底，但它不知道各市场的开盘时间——周一 08:00 时它返回「今天」，而正确答案是上周五。后来把兜底权收归 `trading`（它手上有 `open_at`），`calendar` 只负责查表，为空就返回 `None`。

**职责边界因此变清晰**：

```
calendar  → 哪天开过市（查表，不知道开盘时间）
trading   → 现在算不算开市 + 最近交易日该是哪天（手上有开盘时间）
```

### 最终验证证据

```
cargo test -p fund-module
  435 passed（lib） + 3 + 2 + 12 + 1 + 3 + 6 + 2 passed（集成）  0 failed
  10 ignored（需联网的 live_ 测试）

cargo test -p fund-module live_ -- --ignored
  live_quote_fetch_returns_price                  ok  腾讯实时行情
  live_kline_fetch_returns_bars                   ok  腾讯日 K 不复权
  live_fx_sources_return_positive_rates           ok  新浪 + er-api 双源
  live_calendar_fetch_returns_dates_and_status    ok  三个市场的日历与状态
  live_calendar_populates_and_drives_last_trading_day  ok  日历真实落库
  live_e2e_refresh_fills_market_value_and_fx      ok  端到端闭环

npm run modules:ui:check  6+6+6+1，fail 0
```

### 仍然存在的局限

| 项 | 说明 | 为什么不修 |
|---|---|---|
| 日历首次为空 | 全新安装、尚未刷新过时，`trading_days` 为空，回落星期规则（可能把节假日当交易日） | 刷新一次即可自愈；首次使用本来就要联网拉行情 |
| 美股盘前盘后 | `US_open` 只表示常规交易时段，不区分盘前盘后 | 记账用途下取常规收盘价足够 |
| 汇率精度 | 新浪取买卖价均值，er-api 取日频中间价 | 差异在万分之几，`source` 字段可追溯 |
| AI 趋势无告警 | 只画曲线，不做「评分骤降」提醒 | 属于通知能力，和模块现有的 notify 通道是两件事 |

## 十一、遗留问题

| 项 | 状态 |
|---|---|
| 实物资产（格物）如何并入 | 本期不做，`asset_class` 已留扩展位 |
| 海外股票交易时间与结算差异 | 只影响行情刷新时点，不影响记账 |
| 是否支持多币种现金账户 | 待定，本期账户币种单一 |

---

本方案为产品设计参考，不构成投资建议；AI 输出仅供自我复盘，具体决策请自行判断。
