# 资产日内估值走势修复

实现依据：[首页资产走势契约](../../contracts/fund-holding-detail-contract.md#首页资产走势与市场合并2026-10-21)。本记录覆盖估值走势，不扩大为投资模块或整版产品验收。

## 问题与改动

旧查询按日期取 `MAX(id)`，当前估值替换当天记录；图表又只允许相邻日期连线。Chrome 红例以同一天相隔一分钟的两个分类估值断言应有一条线，实际为 0，复现了有数据但没有日内走势的问题。

- Fund v29 新增同库 `valuation_observations`，每分钟保存最新真实观察；历史日仅保留最后一次，保留最近 400 天。原 `value_snapshots`、现金流核对、收益、TWR、基准和归因不把观察值当确认基线。
- 可见刷新沿原调度器显式调用 `POST /api/home/valuation/observe`，只接收空对象；服务端生成金额、分类、时间并核对 revision。观察写入不推进账本 revision，但保留 writer 和恢复标记。GET、`readSnapshot` 和空间卡仍只读。
- 今天按时间绘制，两个相同金额画平线；缺值或超过 5 分钟的间隔不连线。日内最多 200 个真实样本，并按 96 KiB 点集预算减少样本，保留端点及缺口。旧快照只有可靠的当日创建时间才进入日内序列。
- 月／年仍按日；区间仅今天有数据时自动展开日内点。新分类缺少跨日历史时可消费同响应的 `todayPoints`，不改变全局收益区间，也不借用总资产或当前占比补历史。
- 首页快照 HTTP 适配收口至 `api/home_snapshot.rs`，旧公开读取入口通过再导出保留。UI 源码、生成资源、locale 与模块 schema 声明一并更新。

## 已完成的专项验证

| 对象 | 检查与结果 |
| --- | --- |
| 迁移 | v28→v29 重复执行；手动快照金额保留，新观察表为空；新库与重开可用 |
| 存储与事务 | 同分钟重复／多 Surface 幂等、较早响应不覆盖较新值、revision 冲突拒绝、重开保留、400 天保留与历史日压缩 |
| 读写边界 | 非空对象／数组及非法区间拒绝，GET observe 拒绝；GET 快照不记录；历史范围不记录当天金额；不产生手动收益快照 |
| Chrome 专项 | Archive／Volt × 中英文 × 1280／320 px；日内上升／平线／时间轴、分类历史值、未知／零、缺口、旧历史、隐私与键盘 |
| 页面生命周期 | 隐藏时中止进行中的观察；隐藏时不发起新请求；观察失败保留确认快照并显示错误；`readSnapshot` 使用 GET |
| 分类边界 | 总资产有旧日线而新分类没有跨日值时，直接显示该分类已积累的今日真实走势 |

专项入口：[Rust 观察与迁移](../../../modules/fund/src/valuation_history.rs)、[真实 API 边界](../../../modules/fund/tests/valuation_history_api_test.rs)、[Chrome 回归](../../../scripts/modules/home-history-browser.test.mjs)、[首页合并卡片回归](../../../scripts/modules/home-insights-browser.test.mjs)。

## 真实隔离 Native 链

最终源码报告：`.tmp/intraday-history/run-91okgz/report.json`。使用签名开发产品源、真实 Chrome、Core／Runtime 与隔离 Fund SQLite，未读取真实用户投资库或凭据。

通过原表单配置两个分类，以真实分钟 21:44→21:45 验证：总资产 10,000.00→11,000.00，第一分类 8,000.00→9,000.00 画上升线，另一分类 2,000.00→2,000.00 画平线。观察通过生产接口保存，无模拟时间或前端注入；刷新后保留，GET 读取不增观察、不变账本 revision，`value_snapshots` 仍为 0 条。8 组主题／语言／宽度及键盘／隐私通过，`pageerror=[]`。已实际检查 `real-rising-category.png`、`real-flat-category.png`、`volt-en-320.png`。

浏览器测试复用本机缓存的 `playwright-core`，由 `.tmp/history-series-removal/playwright-loader.mjs` 加载，未新增项目依赖。隔离 Native 入口为 `.tmp/intraday-history/native.mjs`，其构建复用现有 `refresh-dev-product-source.mjs`，不创建第二安装链。

## 原始失败与边界

- 首轮隔离 Native 在 iframe 等待处超时，测试夹具复用了旧 schema 28 的 Core；依据编译描述与签名清单严格相等的契约重建 Core 后通过，未跳过版本或验签。
- 既有完整 `home-browser.test.mjs` 的首次失败是旧 `.home-extra-research` 入口不存在；对齐常驻卡片入口后，又在已移除的 `.home-opportunities-region` 断言失败。本次不恢复退役功能或删断言掩盖，完整旧脚本不记为通过；本任务的走势、错误、只读和隐藏回归由上列专项及真实 Native 单独给出证据。
- 未替换正在运行的真实开发源、触发真实用户库迁移、发送真实持仓或安装／发布。更新后的正常 `npm start` 会构建配套 Host 与模块；已有可靠时间戳的记录可以读取，缺失的过去观察不能补造。
- 未运行 Windows、人工读屏或新一轮 30 分钟长测。复用原刷新／可见性／进程 owner，没有新增 timer、线程、后台采集或常驻；本次以隐藏取消及有界记录检查验证直接影响，不把专项当作历史资源问题收口。

2026-10-22 21:49：当前源码的 `cargo test --workspace` 已通过：28 套件，633 passed／0 failed／13 ignored；原有网络／历史夹具忽略项保持。`cargo fmt --check`、`modules:ui:build`、`modules:ui:check`、standards 与 diff 检查通过。最终 `perf:check`（包含 Extension、Files、Apps、模块 UI、契约、边界与安全）退出 0，目录 p95 45.125 ms，未放宽预算。全仓与集成日志为 `.tmp/intraday-history/workspace-final.log`、`.tmp/intraday-history/perf-final.log`。打印过的首轮全仓通过后，因新增分类日内边界和有界投影复跑了当前源码，未以旧结果替代新代码。
