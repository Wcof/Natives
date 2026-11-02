# 投资模块信息架构简化方案（方案 B：分家不删除）

> 日期：2026-11-01 · 范围：`modules/fund/ui` 导航与首页 · 不涉及 Rust 后端与数据迁移

## 1. 一句话定位

**我的钱现在值多少、变化从哪来、下一步该做什么。**

判定标准：任何新增功能若无法回答这句话，进入工具抽屉，不进入主导航。这条是本次简化的唯一取舍依据。

## 2. 现状诊断

| 指标 | 现状 |
|---|---|
| 主导航入口 | 8 个 tab + 4 个工具项 = 12 个平级入口 |
| 视图 | 11 个 + settings |
| SQLite 表 | 59 张 |
| Rust | 170 文件 / 62,082 行 |
| 前端 JS | 15,822 行 |

问题不在功能数量，在于**入口平级**：12 个入口没有主次，用户每次打开都要重新做一次"我该点哪个"的决策。

按数据归属拆开看，59 张表实际属于两个产品：

- **个人财产管理**（约 14 张）：`accounts`、`positions`、`transactions`、`fund_orders`、`fund_nav`、`funds`、`value_snapshots`、`valuation_*`、`cash_*`
- **投研情报终端**（约 45 张）：`information_*`、`opportunity_*`、`watch_*`、`strategy_*`、`rules`、`notify_*`、`analysis_reports`、`chart_drawings`、`group_plan*`、`otc_*`、`offline_*`

前者每周看一次，后者每天看多次。把它们放在同一层导航里，是"太复杂"的直接来源。

## 3. 三层模型

```text
主层（常驻 3 个）
  概览  净资产 · 四笔钱 · 本期变化 · 目标进度
  持仓  明细、成本、盈亏
  资产  账户与账户维度视图

工具层（抽屉，默认收起）
  自选 · 趋势 · 行情 · 资讯 · AI 决策 · 决策历史 · 交易记录 · 批量导入 · 公募净值

设置
```

### 3.1 视图映射

全部复用现有视图，不新建 DOM，不改 `home.js` 绑定：

| 现有视图 | 新位置 | 理由 |
|---|---|---|
| `home` | 主层「概览」 | 定位载体 |
| `positions` | 主层「持仓」 | 高频核对 |
| `assets` | 主层「资产」 | 账户维度 |
| `watchlist` | 工具 | 研究行为 |
| `trends` | 工具 | 研究行为 |
| `market` | 工具 | 研究行为 |
| `news` | 工具 | 信息消费 |
| `history` | 工具 | 低频回顾 |
| `tx` | 工具 | 记录动作 |
| `import` | 工具 | 一次性动作 |
| `nav` | 工具 | 数据核对 |
| `tab-ai` | 工具（快捷跳首页 AI 区） | 不是独立视图 |

### 3.2 兼容性保证

`app.js` 的 `setTabSelected` 已按角色分流：

```js
if (el.getAttribute("role") === "tab") el.setAttribute("aria-selected", selected ? "true" : "false");
else el.toggleAttribute("data-active", selected);
```

因此把入口从 `role="tab"` 改为 `role="menuitem"` 并移入 `#tools-menu`，**选择态、快捷键、视图切换全部无需改动**。`tabs` 数组仅调整顺序，不增删元素。

## 4. 首页信息架构

参考 Cointrail 卡片语言，重排为：

```text
┌─────────────────────────────────────────────┬──────────────────┐
│ Hero：净资产 + 本期变化 + 时间范围 pill      │ 目标进度          │
│      走势图（面积 + 成交量柱）                │ 四笔钱分布        │
├─────────────────────────────────────────────┤                  │
│ 收益归因（股票 / 基金 / 离线 / 合计）          │ 今日概览          │
│ 资产配置（实际 vs 目标偏离）                   │ 集中度            │
├─────────────────────────────────────────────┴──────────────────┤
│ 资讯（2/3）                          │ AI 决策（1/3）          │
└──────────────────────────────────────┴─────────────────────────┘
```

### 4.1 与参考图的对应关系

| 参考图元素 | 本项目落法 |
|---|---|
| Macro pulse 大卡 + 时间 pill | Hero 净资产卡 + `hc-range` 区间切换（已有） |
| 三个并列 KPI | `home-ledger-strip` 四格收敛为三列 |
| BTC fund flows 多标签 + 红绿柱 | 收益归因卡（已有 `home-income-grid`） |
| Market calendar 右栏 | 目标进度 + 四笔钱 |
| Signal feed 右栏 | 今日概览 + 集中度 |
| 顶部一级导航 + 二级 tab | 主层 3 tab + 工具抽屉 |

### 4.2 不能照抄的地方

参考图是加密产品，**绿涨红跌**。本项目按仓库既有约定使用 `--color-up` / `--color-down`，由 `data-color-mode` 控制，默认 `cn`（红涨绿跌）。禁止在组件层写死涨跌色值。

## 5. 设计约束（不可违反）

来自 `docs/standards/ui-ux/01-design-tokens.md`：

- **字号闭集**：11 / 12 / 13 / 14 / 17 / 20 / 22 / 28
- **间距闭集**：0 / 2 / 4 / 8 / 12 / 16 / 20 / 24 / 32
- **圆角闭集**：0 / 4 / 6 / 8 / 12 / 999
- **字重闭集**：400 / 600 / 650（700 仅 hero 数字）
- **层级只来自派生角色**：`--natives-surface-{raised,chrome,well}`、`--natives-edge`、`--natives-elev-*`
- 组件层禁止 `[data-theme=...]` 选择器与 `color-mix()` 现算层级
- hero 数值每屏最多 1 处，20–28px，≤760px 降到 22–24px

## 6. 分阶段实施

| 阶段 | 内容 | 触碰数据 | 回滚方式 |
|---|---|---|---|
| P1 | 导航收敛为 3 主 + 工具抽屉 | 否 | 还原 `header.html` + `layout.css` |
| P2 | 首页容器重排 + 卡片样式 | 否 | 还原 `view-home.html` + `home.css` |
| P3 | 其他视图套用卡片语言 | 否 | 按视图逐个还原 |
| P4（独立） | `accounts.bucket` 四笔钱列 | **是** | migration 回滚 |

P1、P2 全部为 UI 层改动，不产生 schema 变更、不写用户数据，可整段 `git revert`。

P4 是唯一的数据变更，独立提交、独立验证：

```sql
ALTER TABLE accounts ADD COLUMN bucket TEXT;  -- 活钱 | 稳健 | 长期 | 保障，默认 NULL
```

## 7. 验收

- [x] 主导航可见入口 ≤ 3，工具抽屉可键盘打开、Esc 关闭、有可见选中态（真机实测：Esc 后回焦按钮、外部点击收起、菜单项激活后自动收起并带 `data-active`）
- [x] 12 个原有入口全部可达，无功能丢失（3 tab + 9 菜单项逐一切换；快捷键 1-9 重排后仍可用，0 回概览）
- [x] `home.js` 未修改（仅 1 行 i18n 键名跟随改名），所有 `getElementById` 目标仍存在
- [x] `npm run modules:ui:check` 通过（单源主题角色未重复声明）
- [x] 涨跌色仍走 `--color-up` / `--color-down`
- [x] 字号、间距、圆角均在闭集内（卡片改 `--fund-card-radius` + 20/24 内距、标题 14px/600）
- [x] archive 与 volt 两套主题下布局一致、可读（真机截图 `.tmp/invest-ia-cointrail/home-{volt,archive}.png`）

## 8. 实施与验证状态（2026-11-01 23:13）

- **P1 导航收敛：已实施并真机验证。** 3 主入口（概览/持仓/资产）＋分组工具抽屉（研究：自选/趋势/市场/资讯；记录与数据：交易流水/批量导入/公募净值/决策历史；AI 决策置底）。`tabs` 数组重排（1-9 快捷键优先级，0 回概览），`fundNav*` locale 键入列 `fund-locales.mjs` 前缀白名单。真机 20/20 断言通过（volt/archive 双主题：3 tab、抽屉 9 项分组、Esc 关闭回焦、外部点击收起、菜单项切换＋选中态＋自动收起、12 入口可达、快捷键 2/0、双栏 840px+360px、无页面错误）。
- **P2 首页重排：已实施并真机验证。** `.home-layout` 双栏（主列 840px + 侧栏 360px，≤1180px 收单列），侧栏 = 目标水位＋今日概览（含健康提示与集中度）；主列 = 收益归因/资产配置/资讯/AI 决策原序；全宽 KPI 条对齐 Cointrail hero 行。卡片改 `--fund-card-radius`、20/24 内距、标题 600；走势图 300→260px。窄窗 1100/760px 实测单列堆叠、无横向溢出。
- **P3 其他视图套卡片语言：未实施**（positions/assets/工具内视图逐个套用，无数据风险）。
- **P4 `accounts.bucket` 四笔钱列：未实施**（唯一数据变更，独立提交独立验证）。
- 验证命令与结果：`modules:ui:build`/`modules:ui:check` 绿；`home-insights-browser.test.mjs` 绿（真实 Chrome 合成夹具）；`architecture-check`、`standards:check`、`git diff --check` 绿。`home-browser.test.mjs` 未跑——其 `.home-opportunities-region` 退役断言失败为 2026-10-29 已记录的既有测试卫生问题，本轮未触碰该区域。
- 已知运维坑（本轮踩中）：改 `ui/src` 后必须 `modules:ui:build` → `cargo build -p app-runtime` → `refresh-dev-product-source` → 杀掉旧 `natives-app-runtime` 进程，否则模块页仍由旧二进制吐旧 dist。
