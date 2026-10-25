# 已登记基金历史走势修复（2026-10-23）

## 根因与复现

用户已有多天持仓却仍没有资产走势。只读检查开发库的计数／日期：13 条期初登记，均自 2026-09-30 起；663 条 NAV（2026-08-05 至 2026-10-22）；手动估值 0 条；自动观察只有 2026-10-22 的 1 条。不读取或输出标的名称、账户金额、凭据，不修改真实库。

此前首页日序列只取 `value_snapshots` 和新启用的 `valuation_observations`，没有使用既有登记和 NAV。隔离 API 回归 `registered_fund_with_nav_history_has_trend_without_saved_valuations` 先失败：已有 4 天份额／净值，预期 4 个点，实际 1 个。命令：`rtk env -u CARGO_TARGET_DIR cargo test -p fund-module --test valuation_history_api_test registered_fund_with_nav_history_has_trend_without_saved_valuations -- --exact`。

## 实现与边界

- `holding_income.rs` 从原 API 内纯计算文件提升为领域模块，原持仓收益适配器通过同一导入继续使用；新增历史市值操作复用期初、确认买卖与校准数量规则，未知成本不阻止有价市值。没有复制第二份份额算法。
- `holding_history.rs` 只读回放登记后的份额 × 当时已发布本地 NAV，保留价格日期；最近价格最长沿用 7 天，缺价、未知币种、非场外基金或数值异常保留缺口。当前分类按当前绑定回看，明确参考口径。人工／现金／线下历史未覆盖时，总额不能退化为基金小计。
- 首页组装在同一读快照中接入回放。保存的核对快照／自动观察优先，旧分类记录不改写；回放不写快照、不产生 TWR、不改 schema。今天仍走原日内观察；自定义单历史日也可查询。
- 回放最多 200 个账户／标的身份、合计 10000 条操作、最近 400 天；点集超过 96 KiB 时截取最近连续段并显示截断。没有新增网络、计时器、进程或持久化缓存。
- `home-overview.js` 将连续历史参考点连成虚线，缺价断开；不会把当天回放分类值替换成其他当前报价。中英文说明区分参考估值与已核对收益，源码生成 dist。

## 验收对应

| 用户路径／边界 | 证据 |
| --- | --- |
| 多天已登记基金，没有手动估值也有走势 | 真实 API 红→绿回归，4 天数据；GET 不推进 revision、不写估值／观察 |
| 买入、卖出、校准、未知成本 | 共享 `IncomeBook` 回归覆盖历史份额、同日校准终态、未知成本与真正清仓零值 |
| 净值缺口／币种／混合资产／历史范围 | 缺价不补零，未知币种不计算；混合资产不冒充完整总额；单历史日、登记前、400 天与字节限制 |
| 分类及旧记录 | 分类值随历史份额／净值变化，当前绑定明确标注；已存记录金额与旧分类不被回填覆盖 |
| 实际网页与持久化 | `.tmp/holding-history/run-rz7XPL/report.json`：真实签名 Core → Runtime → SQLite → Chrome，实际 API 登记两项基金及 4 日 NAV，手动估值为 0，总额 30→33，上涨分类 10→13，平线分类 20；刷新保持、只读 GET、键盘／隐私、Archive／Volt × 中英文 × 1280／320 共 8 组，`errors=[]` |
| UI 缺口及失败状态 | `home-history-browser.test.mjs`：回放连线、当日分类历史金额、缺 NAV 断线；保留日内、隐藏取消、刷新失败保留数据、隐私与 8 布局断言 |

截图已实际查看：`real-total-history.png`、`volt-en-320.png`。`ui/src`、`ui/dist` 与密封源 `home-overview.js` SHA 相同（`72dbace821b11926…`）；密封 Runtime SHA 与签名产品清单一致，schema 仍为 29。

## 检查记录

- `rtk env -u CARGO_TARGET_DIR cargo test --workspace --no-run`：通过。
- `rtk env -u CARGO_TARGET_DIR cargo test --workspace`：28 套件，638 passed、0 failed、13 ignored；原始日志 `.tmp/holding-history/workspace.log`。50 次生命周期正常完成约 82.62 秒。
- `rtk env -u CARGO_TARGET_DIR cargo fmt --check`：通过。
- `rtk npm run modules:ui:build`、`rtk npm run modules:ui:check`：通过，当前 Fund UI tree SHA 前缀 `d1dd2c96e9080f91`。
- `home-history-browser.test.mjs`、`home-insights-browser.test.mjs`：真实 Chrome 各 8 组通过。复用本机已有 Playwright，通过 `.tmp/history-series-removal/playwright-loader.mjs` 解析，未新增依赖。
- 最终 `rtk npm run perf:check`：退出 0，包含 Extension／Files／Apps 与模块 UI、边界及安全检查；Files 目录 p95 39.359 ms，日志 `.tmp/holding-history/perf.log`。

文档收尾 `rtk npm run standards:check` 与 `rtk git diff --check` 通过（ROADMAP 截取旧记录后新增的末尾空行已纠正）。

开发中保留的失败：领域文件移动后原测试越过私有 API 的相对路径失效，改为测试实际公开 HTTP dispatch；新分类夹具缺少必填说明，按真实校验补全；新增响应字节上限后原夹具恰好 400 点的假设不再成立，改为同时验证 2～400 点、96 KiB 和最新终点，未放宽生产预算。

本轮不修改真实用户投资数据，不发送持仓到外网，不提交／推送或生产发布。旧全首页脚本对已退役机会区域的断言问题沿用上一轮记录，不计入本次通过项。未运行人工读屏、完整产品安装或新的 30 分钟长测；本次为有界只读计算与既有 renderer 的修复，没有新持续任务，保留项目既有长测未完成状态。
