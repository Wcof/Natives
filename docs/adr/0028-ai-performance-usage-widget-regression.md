# ADR-0028: AI Performance 回归 — 复用 Model Host Usage 权威 + Widget 组合模型

- 状态: Accepted (2026-09-10)
- 取代: 无（新增能力域；不与 ADR-0020 §Usage 冲突，是其"复用现有采集、归一、聚合资产，不建设统一 Event Platform"的具体化）
- 关联: ADR-0020（Usage 编排权威）、ADR-0021/0022（Widget/主题权威）、`docs/standards/technical/03-data.md`、`docs/standards/technical/04-performance.md`、`docs/standards/ui-ux/01-design-tokens.md`

## 上下文

早期版本曾实现 AI 用量统计，现回归为 AI Performance 能力。需求（v1.0 方案）要点：

- 数据指标层（Metric）+ 可视化层（Visualization）+ 组件组合层（Widget），同一数据多视图。
- V1 指标：Token / Cost / Request / Session；来源：Claude Code / Codex（本地）。
- UI 必须继承现有设计系统，禁止新视觉体系；无常驻后台；数据聚合后进前端；图表生命周期完整。

现状盘点（2026-09-10 实测）：

1. **数据面已存在**：`model-host/internal/usage`（Go）持有 SQLite `usage_events`（字段含 source/provider/model/tokens/cost/result/latency 等，与 `ai_usage_event` 提案几乎一一对应）+ `model_prices` + 索引 + 保留期修剪。协议方法 `model_usage_status/overview/analysis/events/pricing/...` 已贯通，extension 侧 `model-usage-view.js` 已有总览/透视/明细/价格四页。
2. **约束**：`src/` 已删除，禁止重建；SW 必须无状态；页面无 SQLite/文件直访；Host 默认权威。

即：方案的"数据层 + Collector"在现有架构中**已经存在**，不需要新的 `ai_usage_event` 表、不需要新 Collector/daemon——缺的只是把现有 Usage 数据以 Metric→Chart→Widget 组合方式暴露到 Space 首页。

## 决策

### D1. 数据权威：复用 Model Host Usage，不新建 ai_usage_event

- `usage_events` 即方案中的 `ai_usage_event`；`source` 字段区分 claude_code/codex/atom_code 等来源。
- 经 Local Proxy（含 CLI 工具代理流量）的请求由现有 `NativesUsagePlugin` 自动记账；Claude Code / Codex 本地直连流量 V1 经现有 `UsageImporterWizard` 导入链路归一入同一库（复用 `model_usage_import_*` 分块协议），**不**新增常驻采集器。
- 不在 native-file-host / extension 侧复制任何用量数据；Metric Query 全部经 Model Host 协议方法。

### D2. 模块落点：extension/ 内独立目录，禁止 src/

```text
extension/
└── ai-performance/
    ├── metrics/        # Metric 定义（id/dimensions/aggregation），只做查询组装
    ├── charts/         # Chart Registry：number / line / bar / heatmap / timeline
    └── widget.js       # widget/aiPerformance Widget 插件（配置驱动）
```

- Widget 以现有 `widgetPlugins` 插件契约注册（`render` 返回 destroy 回调），加入 `WIDGET_CATEGORIES` 新分类 `ai`。
- Widget 只保存配置 `{ metric, chart, range, dimension }`；渲染流程：config → Metric Query（Model Host 协议）→ Chart Renderer → UI。
- 已有 `model-usage-view.js` 保持不变（设置页数据面）；ai-performance 只消费相同协议，不迁移不重写。

> **修订（2026-09-11，随[空间 AI 效能组件实施方案](../development/ai-efficiency-components-plan.md) §4/§8）：** V1 的单一 `widget/aiPerformance` key + view/metric 下拉不再作为目录产品形态。AI 效能分类改为七个独立注册项（`widget/aiCost`、`widget/aiTokens`、`widget/aiSessions`、`widget/aiRequests`、`widget/aiLimits`、`widget/aiAttention`、`widget/aiSavings`），各自独立添加/多实例/设置/删除，共享底层 renderer 与查询；旧 key 保留在兼容读取与安全白名单中但从新增目录隐藏，迁移规则见方案 §8.2。本条 D2 其余落点（extension/ai-performance/ 内注册、配置驱动渲染流程）继续有效。

### D3. Metric 模型（V1）

| Metric id | 数据来源（现有协议字段） | 维度 | 聚合 |
|---|---|---|---|
| `token_usage` | `model_usage_analysis`/`overview` tokens | time / source / model | sum(input/output/total) |
| `ai_cost` | costMicro + `model_usage_pricing` | model / provider / time | sum |
| `request_count` | MetricCards.totalRequests / successRate | time / source / model / result | count |
| `session_duration` | latencyMs / ttftMs（V1 以请求侧近似会话活跃度） | time / source | count / avg |

注：方案原文的"Session 持续时间"在现有数据面无独立 session 实体，V1 以请求活跃度近似并在 UI 标注口径；真正 session 聚合留待有本地 session 数据来源时扩展。

> **修订（2026-09-11，随[空间 AI 效能组件实施方案](../development/ai-efficiency-components-plan.md) §4.1.1/§7）：** `session_duration` 的请求近似口径退役为"旧调用活跃"兼容配置，归属调用统计（`widget/aiRequests`）。会话活跃改为真实指标：区间内 distinct `(toolId, sourceInstanceId, nativeSessionId)`，与请求次数严格分开；每日会话数为 distinct，不得相加为区间去重数；无 session ID 的记录进入"未归属用量"。聚合由 Model Host 新增 session 聚合查询承担，不借 `totalRequests`。旧 key/旧配置按方案 §8.2 迁移，不静默改写语义。

### D4. Chart Registry（V1：number / line / bar / heatmap）

- `charts/` 内统一注册表 `{ id, render(container, series, { t }), destroy }`；同一 Metric 任意组合。
- heatmap 复用 `plugins/widgets/activity-calendar.js` 的渲染范式（day/value 网格），样式仅用 Theme Token。
- timeline 列入 Registry 但 V1 可 `unsupported`（诚实状态，不假数据）。

### D5. UI 与主题

- 全部样式消费现有 CSS custom properties（`--surface` / `--border` / `--radius` / `--accent` 等）；主题持久权威遵循 ADR-0022 `settings:theme`（词表 `dark | light`）；禁止新颜色/圆角/阴影/字体/间距。
- i18n：`_locales/zh_CN` 与 `en` 同步；文案走 `t()` + messages.json，无硬编码。

> **修订（2026-09-11，随[空间 AI 效能组件实施方案](../development/ai-efficiency-components-plan.md) §5）：** 画布内 Widget 的最终外观在空间层解析（已授权卡片外观覆盖 → 空间当前外观/Surface Policy → 空间默认值），不得直接消费全局 Files/模型设置页的 `--text`/`--accent`/`--display-font` 盖过空间外观。数据卡统一消费空间解析后的局部语义角色（拟新增 `--space-widget-*` 解析结果别名：text/text-secondary/surface/chart-line/grid/volume-0…8/warning/danger 等，无独立主题配置）；图表在当前卡片根取色（`currentColor`/局部 CSS 变量），禁止公共 shadowRoot 探针与第一次渲染颜色快照；AI 图表不继承 `.Widgets svg` 装饰性 drop-shadow。全局主题持久权威仍按 ADR-0022，不新增空间主题持久库。

### D6. 性能红线

- 无 daemon/常驻后台；页面仅在 Widget mount 时发起查询，range 默认 30d，展开历史时增量拉取（`model_usage_events` 分页 + `range` 聚合）。
- 所有图表消费聚合结果（overview/analysis 已聚合），禁止原始事件整表进前端。
- destroy 必须取消未决请求、清理定时器并 `replaceChildren()`；无 per-widget 轮询定时器。

## 后果

- 正面：零新增存储/进程/协议栈；早期功能以组合方式回归；Metric/Chart 扩展只增注册项。
- 负面/边界：Claude Code / Codex 直连流量的"自动采集"退化为"手动导入"，与方案原文有差距——是有意取舍（避免 Collector/daemon 违反无常驻约束）；若未来要自动采集，须另立 ADR 决定采集时机（Host 被动记账 vs 用户触发扫描）。
- 验收对齐：不破坏现有 Space/Widget ✅（纯新增插件）、新模块独立 ✅、继承设计系统 ✅、无常驻 ✅、聚合 ✅、生命周期 ✅、可配置生成 Dashboard ✅。
