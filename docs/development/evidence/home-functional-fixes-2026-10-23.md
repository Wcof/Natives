# 首页功能实测修复（2026-10-23）

## 解锁后原生 Chrome 补验（2026-10-24 08:19）

通过正在使用的 Chrome 窗口操作本机 Web 投资页，补齐此前因锁屏中断的路径：

- 资讯列表点击“打开来源”进入详情，自动取得正文；实际显示从首段到“文章来源：新华社”的原文摘录，替换“未取得正文”状态。
- 详情点击“打开来源”后新标签打开 `https://finance.eastmoney.com/a/202610243907032176.html`，实际页面标题与资讯一致，正文加载；随后关闭本次来源标签并返回投资页。
- 首页选择“自选”，以键盘激活“查看全部”，资讯页保持“自选”选中，“全部”未选中；已检查真实页面渲染。
- 解锁后已核对目标编辑的历史名称、日期说明，未保存目标变更。

本次只补充功能验证记录，没有追加生产代码修改，也没有再次发送真实持仓给 AI。页面现有自动刷新仍提示 `em_stock_news(item_limit_reached)`，属于采集上限导致的部分返回，不能视为全量资讯已取得。未重复验证真实供应商完整输出、扩展入口和保存写路径；此前各项门禁结果按所属改动保留，不据此宣称全产品验收完成。

## 范围与实现

由真实 Chrome 首页操作发现问题后修复；保留现有账本、schema、推理授权及 Tokenusage owner，不自动提交交易或再次发送真实持仓。本轮补齐真实页面复验，并收紧估值状态与资讯去重语义。

| 问题 | 根因与修改 | 验证状态 |
| --- | --- | --- |
| 有完整基金估值，却显示已投资市值待核实 | `home.rs` 的 I 原要求未登记现金 C 已知；A 仅含已登记资产，因此无现金登记时 I 应为 A，C 仍未知；不完整 A 仍未知。财富详情与卡片共用同一快照状态：补齐 `missing_quote` 问题项，`stale` 不再降级为 `incomplete`，未知分类归分配复核而不混入估值完整状态。 | 单元回归覆盖无现金、缺报价、过期；真实 Chrome 卡片与详情均为 9,419.33，详情显示“已登记资产估值完整 / 待核实项 无” |
| Web 原文链接无反应 | 详情改为原生按钮，点击与 Enter 走同一路径；仅渲染 HTTP(S) 链接，打开期间禁用，Broker 失败显示本地化错误；Web 新窗口清除 opener，扩展继续 `chrome.tabs`，仍校验会话、能力及协议。 | 真实 Chrome 本地 Web：点击与 Enter 均打开真实东方财富原文 URL，无错误；扩展 tab 路径由既有 Broker 回归覆盖 |
| 港美股时间未知 | 腾讯字段 30 分别为斜线／横线／紧凑日期，原解析只认 14 位紧凑格式；兼容三种真实格式并验证日历与时分秒，不补抓取时间。卡片时间允许换行并保留完整 `title`，窄列不再以省略号截断。 | 真实 Chrome 三组：A 股 2026-10-23 16:15:00、港股 18:31:07、美股 2026-10-22 17:15:59；卡片时间无溢出、非法日期回归通过 |
| 目标旧字段文案自相矛盾 | 历史名称／期限合并为一行“历史信息，不参与当前金额目标计算”，不再逐项追加相同括注；质量 ready 转为用户可读说明。 | 真实 Chrome 编辑弹窗只剩一条历史说明，截止日期与名称同时可见 |
| 相同新闻占多个位置 | `information/store.rs` 在筛选后、分页前按“来源族＋规范标题＋北京时间日”折叠；同一新闻可来自不同来源且摘要长度不同，因此不再以完整摘要作为去重键；阅读／收藏状态不参与分组，避免标记后副本重现；公告与公开资讯分族，规范行优先强关系、个人状态、重要度、更丰富摘要与时间。 | 真实列表 50 条／50 条唯一标题；单测覆盖跨源不同摘要、阅读／收藏变化、公告不合并及分页 |

## 已执行检查

- `rtk env -u CARGO_TARGET_DIR cargo test -p fund-module --lib`：392 passed，0 failed，4 ignored。
- `rtk env -u CARGO_TARGET_DIR cargo test --workspace`：退出 0，28 个测试目标共 644 passed，0 failed；含既有 ignored 项。
- `rtk env -u CARGO_TARGET_DIR cargo fmt --check`：通过。
- `rtk npm run modules:ui:build`、`rtk npm run modules:ui:check`：通过。
- `rtk npm run perf:check`：退出 0，覆盖 Extension、Files、Apps、模块 UI、安全及边界检查；没有性能提升声明或新增长测。
- `rtk proxy node extension/module-inference-broker.test.mjs`：通过，含 Web 外链及弹窗拦截。
- 真实 Chrome / 本机 Web（隔离 Bootstrap、真实 Fund 数据库只读展示）：页面加载后核对卡片、详情、A/HK/US 三组行情、目标弹窗、资讯列表与详情外链；点击与 Enter 均打开真实原文 URL，`errors=[]`。未手工修改持仓、交易、现金或资讯个人状态。

首次运行 `perf:check` 时 `extension/platform/recovery.test.mjs` 在动态加载 Web 会话模块后固定两次 `setImmediate` 内未建立首个事件流；该测试早于本轮改动。保持行为断言不变，将有界等待改为观察 `streams.length`（最多 50 次事件循环），复跑通过；未放宽流数量、隐藏抑制或 BFCache 断言。

## 本地真实路径与未完成项

已通过现有本地 Web 入口建立可信会话，并用真实 Chrome 操作首页与资讯详情；使用的是真实 Fund 数据库的只读展示路径，页面正常加载会按现有逻辑记录估值观察。扩展页面的原文 tab 路径本轮未再次人工打开，由 Broker 单元回归覆盖。

未提交、推送或生产发布，未手工修改用户持仓／交易／现金／资讯个人状态。不能据本轮门禁通过宣称扩展、平台安装或真实供应商完整验收。

## 首页全页复检（2026-10-23 20:20）

在真实 Chrome 本地 Web 上逐项操作首页并对照同一快照／数据库返回，补查主流程、数据一致性与窄窗／中英文。读数与快照一致：A＝I＝9,419.24（现金未建账，C 未知）、质量 ready 且问题项为空、基金持有收益 −1,404.92／成本 10,824.16／市值 9,419.24、今日估算 112.41 标记部分覆盖、分组当前值合计 9,419.24＋未归类 0.00、目标 1,000,000.00／覆盖 9,419.24／缺口 990,580.76、分类目标合计 100%、历史末点等于当前总值、资讯 50 条标题不重复。市场、区间（今日／本月／今年／全部／自定义）、目标／分类／现金／规则／资料／报告弹窗、资讯范围与详情、收藏往返、隐私三态、体检摘要、决策历史页、四档宽度（1024／768／560／390）与中英文切换通过；390 px 分类标签按既有横向滚动处理，无页面级横向溢出或金额截断。

复检发现并修复四处业务／文案缺陷：

| 问题 | 根因与修改 | 验证 |
| --- | --- | --- |
| 首页资讯默认显示“全部”而非持仓相关 | 契约要求首页默认持仓相关，API 在缺省 scope 时也默认 `holdings`；前端却显式发送 `scope=all`。将首页 `infoScope` 默认改为 `holdings`；资讯页仍默认全部。 | 新开真实页面默认选中“持仓”，列表为 A 股相关新闻；资讯页范围不受影响 |
| 日期型公告被显示为伪造的 00:00:00 | 后端已以 `timePrecision=day` 区分只有日期的公告，前端忽略该字段并按完整时刻格式化。`informationTimeLabel` 接受精度：`day` 只显示日期，`unknown` 显示上游时间未知，秒／分保持完整时间；列表、详情与关联资讯同源。 | 招商银行公告显示 `2026/10/24`，列表不再出现 `00:00:00`；详情同样只显示日期 |
| 北证指数显示 `bj899050`，英文回落为原始代码 | 指数代码清理只覆盖 sh／sz／hk／us，且缺少北证名称键。补充 `bj` 前缀清理、`899050 → fundHomeIndexBeijing50`，并在中英文 locale 增加“北证 50 / BSE 50”。 | 中文显示“北证 50 · 899050”，英文显示“BSE 50 · 899050”，无原始 i18n 键 |
| 单次采集达到 200 条上限时提示“部分来源失败” | 刷新结果以 `partial + item_limit_reached` 表示未完整覆盖，并非来源请求失败；原文案把所有 partial 都称为失败。保留部分完成状态与来源／错误码，改为“部分来源未完整返回”，不吞掉缺口。 | 真实刷新返回 18/18 身份覆盖但 `em_stock_news(item_limit_reached)`；提示准确说明未完整返回，不再称失败 |

门禁：`modules:ui:build`、`modules:ui:check`（含 locale 双侧一致与 dist 逐字节投影）、`perf:check`（含 Extension／Files／Apps／边界／安全／端口）、`git diff --check` 均退出 0；本轮未改 Rust，未重复运行 workspace。复检期间的写入仅为正常页面加载产生的估值观察、一次资讯采集缓存、收藏开关往返恢复及显示偏好循环后恢复；未保存目标、现金、分组或规则变更，未改持仓／交易／现金数据，未提交／推送／安装。扩展 tab 路径、真实 AI 推理、人工读屏与写操作保存路径本轮未复验。

## 五项回归复验（2026-10-23 22:05）

用户复测仍见“已投资市值待核实、原文链接无反应、港美股时间未知、目标编辑文案冲突、持仓资讯重复”。逐项对照后确认：**五项症状全部命中 20:25 之前的构建**，源码修复未消失；运行中的 `native-file-host` 于 19:58 启动并持有旧 App Runtime 会话，页面因此继续显示旧后端与旧 UI。重建模块 UI、`natives-app-runtime` 与 `~/.natives-local/product-source`，并以 `apps:handshake` 重新密封 `activation.json`（`appRuntimeSha256` 同步为新载荷）后，五项在真实 Chrome 中全部通过。

| 复验项 | 实测结果 |
| --- | --- |
| 已投资市值 | 卡片 `9,352.74`；详情“管理资产 A 9,352.74 / 非现金市值 I 9,352.74 / 数据质量 已登记资产估值完整 / 待核实项 无”，两侧同源 |
| 原文链接 | 点击打开 `https://finance.eastmoney.com/a/202610233906215997.html`，标题真实加载；对另一条按 Enter 打开 `...202610233906209475.html` |
| 港美股时间 | 港股 `HSI · 2026-10-23 18:31:07`、`HSTECH · 2026-10-23 16:08:30`；美股 `NDX / DJI / INX · 2026-10-23 10:03:19` |
| 目标编辑 | 仅一行“历史信息，不参与当前金额目标计算：目标名称：终极目标 · 截止日期：2030-01-01”，无“未设置期限”与嵌套括号 |
| 资讯去重 | 持仓范围 50 条 / 50 个唯一标题，渲染列表无重复 |

本轮另修一处同区域真实缺陷：美股标普 500 的腾讯代码写成 `usSPX`，上游返回 `v_pv_none_match`，第三张美股卡片（价格与时间）永久缺失；改为 `usINX` 后恢复，并随指数卡片一并显示上游时间。该改动只涉及常量，新增注释说明代码来源。

门禁：`cargo test -p fund-module --lib` 392 passed / 0 failed / 4 ignored；`npm run modules:ui:check` 通过（含 dist 投影）；复验在隔离账本副本（真实 Fund 数据库快照）与本地 Web 入口完成，真实 Chrome、`errors=[]`。Branded Chrome 155 已禁用 `--load-extension`，扩展内路径本轮改由既有 Broker 单测覆盖，未在真实扩展中手工复验。


## 资产走势与 Demo 对齐（2026-10-23 22:50）

参考 Demo：`.tmp/home-insights-redesign-demo/index.html`（首页下半区重组，2026-10-21 确认稿）。Demo 的走势语义是：所有序列以区间首个可绘制点为 0%，总资产常显并叠加所选分类，共用同一百分比刻度；分类卡显示该序列的区间变化与迷你曲线；缺失日期不推算、不连线。

| 差异 | 生产实现（修复前） | Demo 语义 | 修复 |
| --- | --- | --- | --- |
| 纵轴 | 绝对金额，按区间 min/max 归一化到像素，无刻度 | 百分比变化，0% 为区间首日 | `home-overview.js` 改为按各自首点计算百分比，纵轴以 0% 为锚点生成“好看步长”刻度 |
| 序列 | 选中分类后**替换**总资产，只画一根线 | 总资产常显（面积），所选分类叠加 | 总资产与分类各成一组 `[data-series]`；分类最多一条叠加线 |
| 分类信息 | 纯文字标签，无变化值 | 分类卡带区间变化与走势 | 标签内增加区间变化（`fundHomeHistoryChange`），无两点时不显示；无历史时与主图同源改用当天观察 |
| 选择说明 | 无 | “本月 · 总资产 + 股票” | 新增 `#home-history-selection`，显示“总资产 + 分类”，隐藏模式只显示 `****` |
| 刻度可读性 | 无刻度；若写进 SVG，320 px 下会缩到 4 px | 固定像素侧栏 | 改为 44 px HTML 侧栏（320 px 下 40 px），绘图区 `preserveAspectRatio=none` 固定高度 |
| 单点状态 | 选中分类时显示分类当前值 | 总资产始终是主口径 | 单点/无历史一律展示总资产；分类没有两点不画线、不顶替总额 |

同时修正：图表 `aria-label` 原先写“归一化曲线”而实现是绝对金额，现已与实现一致；分类标签与主图在“仅今天有观察”时使用同一批点，避免“有曲线但没有变化值”。

验证：`home-history-browser.test.mjs`（重写分类断言为分序列计数）与 `home-insights-browser.test.mjs` 通过，含 Archive／Volt × 中英文 × 1280／320 共 8 组；真实账本副本（本地 Web、真实 Chrome）读到总资产 `-9.39%`、红利低波 `+0.22%`、黄金 `-4.95%`、科技主题 `-15.80%`，纵轴 `0.0% / -5.0% / -10.0%`，`errors=[]`。`npm run modules:ui:check` 通过。

## 投资模块逐项检查（2026-10-23）

| 环节 | 检查内容 | 结论 |
| --- | --- | --- |
| 财富四量 | A／I／C 口径、质量词表、待核实项 | 通过；`I=A−C` 与人工现金计入 A 的事实一致，无现金登记时 `I=A` |
| 资产估值 | 完整值/已知值、缺价、币种、重复归属、过期 | 通过；`complete_value_minor` 仅在无缺口时给出，`stale` 不降级为 `incomplete` |
| 期间收益 | 基线取前一日、期末当前估值、外部流水符号、回填检测 | 通过；`netReturn = end − begin − external`，流水日期越界报 `unexplained_adjustment` |
| 历史回放 | 份额×NAV、7 天沿用、缺价断线、分类缺成员、400 天/96 KiB 上限 | 通过；分类任一成员缺价时整组未知，不用小计冒充 |
| 分组分配 | 双绑定拒绝、离线/现金/人工/基金四类引用、未归类 = A − 已归类 | 通过；同一现金不能两种引用重复归组 |
| 目标 | 空期限不等于过期、A/B/specific 范围、版本冲突 | 通过；空 `target_date` 不判过期 |
| 资讯 | 去重键（来源族＋标题＋北京时间日）、分页、缓存键含筛选条件、阅读状态 | 通过；50 条列表 50 个唯一标题 |
| 行情 | 腾讯三种时间格式、港美股指数代码、缺失报价 | 通过；`usSPX` 已改 `usINX`，港美股时间正常 |
| 持仓 | 当日/昨日事实选择、估值未知不回退、已实现收益汇总 | 通过；估算仅在确认值为空且同日时替代 |
| 静态检查 | `cargo clippy -p fund-module --lib` | 仅风格类告警（复杂类型、冗余闭包），无未使用分支或恒真条件 |

已知测试卫生问题（非本轮回归）：`scripts/modules/home-browser.test.mjs` 仍在断言已退役的 `.home-opportunities-region` 与 `#home-finance-details`，该脚本不在 `modules:ui:check`／`perf:check` 门禁内；本轮未改写，避免用删断言掩盖旧覆盖缺口。

## 走势卡两级按钮与六个周期（2026-10-25）

用户反馈：走势卡的按钮分“类型”和“类型下的明细”两级，类型全选时应按类型画线，未全选时按所有资产明细画线；周期要能选当日／本周／本月／半年／一年／全部。

修复后的行为：

| 场景 | 之前 | 现在 |
| --- | --- | --- |
| 类型全选 | 多选 pill + 总资产 pill 混在一行，总资产也能被取消 | 一级只列“全部 + 各分类”，总资产恒为基准线，不参与勾选 |
| 点某个类型 | 只有单选时 drill，选两个就退回汇总，中途出现“未选择资产序列”空图 | 全选时点某类型＝只看该类型并下钻到明细；未全选时是多选开关；取消最后一个类型恢复全部 |
| 二级明细 | 无 | `#home-holding-controls`：全部明细 + 各资产 ✓ pill（类型名/代码/账户），删除身份后自动清理选择 |
| 周期 | 当日／本月／今年／全部／自定义 | 当日／本周／本月／半年／一年／全部／自定义；本周从周一起算，半年／一年按自然月回退且目标月无该日取月末（`year1` 取代语义含糊的 `year`，旧 `year` 仍兼容为“今年”） |

真实账本副本复核又暴露一个边界，一并修掉：分类只建了分组、没绑资产时当前值为 0，之前会画出一条贴底的 0 线（压扁纵轴），按钮仍可点，点下去只剩基准线，观感就是“按钮坏了”。现在可绘制判定＝至少两个可用估值点且不全为零；不满足的类型按钮置灰、带原因提示、不参与选择与绘制，非总资产序列也要求至少两个点才成线。

验证：

- `cargo test -p fund-module --lib` 393 passed（含 `rolling_ranges_use_monday_and_calendar_month_ends`）。
- `home-history-browser.test.mjs`、`home-insights-browser.test.mjs`、`asset-allocation-browser.test.mjs` 通过；新增两组断言：空类型按钮 `disabled` + 标题提示、给两个日估值后按钮恢复可点。
- `npm run modules:ui:check` 通过（含 i18n 投影与中英文键一致性）。
- 真实账本副本（隔离 8899、真实 Chrome、`app.html?app=fund`）：全选只画 4 条类型线（8 个分类中 4 个无绑定资产）、空类型按钮点不动、明细下钻后出现资产 pill 且取消后该线消失、复位回类型层；周期 `today` 单日 46 点／`week` 2026-10-19 起 7 点／`month` 10-01 起 25 点／`half` 起点 2026-04-25／`year1` 起点 2025-10-25／`all` 不设起点；360px 无横向溢出（pill 行按设计横向滚动），`errors=[]`。

产品源：`refresh-dev-product-source` + `apps:handshake` 重新密封（generation 115，runtime sha `f665e334…`），本地 8771 服务重启后加载新 UI。

## 明细行下移到基准提示之后（2026-10-25）

按用户指定：`#home-holding-controls` 从类型按钮正下方移到 `<p id="home-benchmark-status">` 之后，DOM 顺序变为 `home-history-categories → home-benchmark-status → home-holding-controls → home-benchmark-chart → home-attribution-body`。样式沿用虚线分隔，并在上方补 2px 间距。

同时修掉一个由此放大的既视问题：没有基准序列时 `#home-benchmark-chart` 仍占 100px（`.home-insights .home-history-chart` 的 min-height），明细行与「收益贡献明细」之间出现一大块空白。新增 `.home-insights #home-benchmark-chart:empty { min-height: 0; padding: 0; }`，空图不再占位（`renderBenchmark` 在非 ready 状态本来就会 `replaceChildren()`，`:empty` 判定可靠）。

验证：真实账本页面（8771 + 真实 Chrome）读得 `previousElementSibling=home-benchmark-status`、`nextElementSibling=home-benchmark-chart`，纵向位置 提示 1717 → 明细行 1767 → 基准图 1818 → 归属说明 1818（空图 0 高）；明细行渲染「全部明细 ✓易方达深证100ETF联接A 110019 · 支付宝」，`errors=[]`。`home-history-browser`、`home-insights-browser`、`asset-allocation-browser` 与 `npm run modules:ui:check` 全部通过。

## 市场分区样式对齐（2026-10-25）

问题：市场分区自带一套尺寸——名称/时间 9px、涨跌 10px、分组按钮 7px 圆角、28px 命中区，而同一张卡片里的区间按钮是 3px 圆角 32px，分区标题也不统一（15/14/14）。R-U5 要求正文不小于 12px、间距 8px 基线、按钮命中区 32px，市场分区三项都不达标。

改动（`modules/fund/ui/src/styles/home.css`）：

| 位置 | 之前 | 现在 |
| --- | --- | --- |
| 市场外壳 | `padding:12px 14px`、`margin-bottom:16px` | `var(--natives-space-2/3)`、`var(--natives-space-3)` |
| 分组按钮 | 28px / 7px 圆角 / 透明描边 | 32px / 3px 圆角 / 保留 1px 描边（与 `.hc-range` 同几何） |
| 选中态 | `color-mix(...)` 现算描边 | accent 描边 + selected 底色 |
| 行情卡名称 | 12px / 400 | 12px / 500 |
| 行情卡主值 | 14px | `--natives-fs-table-amount` + 700 |
| 涨跌百分比 | 10px | 12px + tabular-nums |
| 代码与时间 | 9px、可断行 | 12px、最多两行、完整值进 `title` |
| 分区标题 | 15px（走势）／14px（市场） | 统一 14px / 650 |
| 分类涨跌、分类区提示、胶囊内涨跌 | 11px／10px／10px | 统一 12px |

同时删掉三段死规则：`.home-market #home-market-body`、`.home-market .market-row`、`.home-market .market-meta` 以及两处 `#home-market-body` 的媒体查询列数——页面实际渲染的是 `.home-market-grid`／`.home-market-card`，旧规则只会让后来的人以为还有第二套网格。

验证：`modules:ui:check` 通过；`home-insights-browser`、`home-history-browser`、`asset-allocation-browser` 通过；真实账本页面（8771 + 真实 Chrome）实测 3 个分组按钮 32px/3px、5 张行情卡无溢出、代码与时间 12px 两行完整显示、文档无横向溢出、`errors=[]`；archive／volt × 1280／390 四组截图核对（`.tmp/market-style/`）。

## 分类入口合并成一套卡（2026-10-25）

反馈：同一张卡片里出现了两处“资产分类”——顶部类型胶囊 + 底部分类卡；要求只保留一套，并在下面显示所选分类的资产。

改动：

| 位置 | 之前 | 现在 |
| --- | --- | --- |
| 顶部分类 | `#home-history-series` 胶囊（全部 + 各分类，多选） | `#home-history-categories` 卡片行：全部资产 + 各分类，单选；再点一次回到全部 |
| 底部明细 | `#home-holding-controls` 胶囊（全部明细 + 各资产） | `#home-holding-cards` 卡片行：标题写明范围（全部持仓 / 某分类 · 具体持仓），列出该范围内的资产卡 |
| 卡片内容 | 名称 + 数值·变化 + 右侧 64px 迷你走势 | 名称整行（可省略号）+ 数值·变化 + 通栏迷你走势（名字不再被压成“红利低波基”） |
| 排布 | 横向滚动，卡片被容器切一半 | 172px 自适应列宽换行，不截断 |
| 交互 | 全选→类型线；部分选→明细线；明细行另有一套勾选 | 全部资产→分类线；选一个分类→它的资产线；点资产卡只切换该线显隐 |
| 计数 | 已选 N / M 个类型 | 全部资产：共 N 项资产；分类内：已选 n / m 项明细 |

顺带清理：删除不再渲染的 `.home-history-series`／`.history-series-*` 全部样式与 7 个废弃文案键（`fundHomeHistoryAllSeries`、`fundHomeHistoryHoldings`、`fundHomeHistoryAllDetails`、`fundHomeHistoryTypesSelected`、`fundHomeHistoryTypesCount`、`fundHomeHistorySeries`、`fundHomeHistoryAllTypes`）；新增 `fundHomeHistoryAllAssets`、`fundHomeHistoryAssetsTotal`、`fundHomeHistoryAllHoldingsTitle`、`fundHomeHistoryAllHoldingsHint`，并把三处操作提示改成与实际交互一致的文案。

验证：`modules:ui:check` 通过；`home-history-browser`（改写成卡片选择：全部→选 g1→隐藏 fund-1-1→再点 g1 回到全部→选 g3→回全部）、`home-insights-browser`（隐私模式下卡片显示 `****` 且不泄露金额百分比）、`asset-allocation-browser` 通过；真实账本页面顶部 5 张卡 1 行、底部 13 张资产卡 3 行、选中分类后底部 1 张、`errors=[]`、文档无横向溢出。

## 卡行超过一行折叠（2026-10-25）

反馈：两行还是太满，超过一行就折叠，提示里带“还剩多少项”。

- `#home-history-categories`（分类）与 `#home-holding-cards`（资产）共用 `applyCardFold`：先量出第一行能放几张，超过一行就只显示第一行，并在行下方给出 `点击展开（还有 N 项）`；展开后按钮文字变为 `收起`，位置不变。
- 折叠状态存在 `historySelection.cardsOpen`（按行分别记录），切区间、刷新快照、切换分类后不会自己弹回。
- 分类行额外带 `minVisible: 3` 保底：窄屏一行只放得下一两张卡时，折叠会把分类选择器藏起来，所以至少保留 3 张；资产行不设保底。
- 卡片的 `hidden` 需要显式 `display:none`（`.history-cat-card` 是 grid，会覆盖原生 `[hidden]`）。

验证：真实账本 13 项资产——折叠时可见 5 张 1 行、提示 `点击展开（还有 8 项）`；展开后 13 张 3 行、提示 `收起`；再收起回到 5 张，`aria-expanded` 随之切换，`errors=[]`。`home-history-browser` 增加折叠用例（12 项资产：折叠可见数 < 总数、展开恢复、再收起），并通过；`home-insights-browser`、`asset-allocation-browser`、`modules:ui:check` 通过。

## 首页卡片去重与精简（2026-10-25）

按“聚合信息、减少重复”调整合并卡：

| 区块 | 处理 | 理由 |
| --- | --- | --- |
| `#home-trend-summary`（最新估值／区间变化／区间起点） | 删除 | 顶部资产格已有管理资产与今日收益，收益卡已有区间收益，三指标重复 |
| `#hc-cumulative`（累计投资收益） | 删除 | 与收益卡的持有收益／已实现收益重复 |
| `#hc-invested`（已投资市值） | 删除 | 与基金／股票收益卡的市值字段重复 |
| `#home-trend-inspection`（检查条 + 滑块） | 改为浮层 | 常驻一条占高度；改为指针跟随的 `.history-tip`，聚焦图表后左右键移动、Esc 收起 |
| `.history-cat-spark`（分类卡迷你走势） | 删除 | 主图已经表达走势，卡片只留名称与当前值·区间变化 |

同时清掉对应 CSS（`.home-trend-summary*`、`.home-ledger-extra`、`.home-trend-inspection`、`.history-inspect-*`、`.history-cat-spark`，19 条）与 `home.js` 里对已删节点的写入（`hc-invested` 的 `setCard` 与副标题），否则渲染时会 `Cannot set properties of null`。

验证：`modules:ui:check` 通过；`home-history-browser`、`home-insights-browser`（改为断言四个重复区块数量为 0、收益卡数值节点仍在）、`asset-allocation-browser` 通过；真实账本页面：四区块查询结果为 0、分类卡高 67px（原约 91px）、13 项资产的持仓行折叠为 1 行、悬停图表出现浮层「2026/10/14 · 总资产 ¥9,805.21 · 各分类金额」，移出后浮层隐藏，`errors=[]`。

## 截图资产维护与清仓校准（2026-10-25 19:20）

用户明确授权维护其截图中的真实数据，确认日期为当日，随后补充基金代码／份额、明确未出现的旧基金已清仓，并确认黄金沿用原产品。通过现有 Native Core Relay、Protocol v2 握手及受鉴权 Fund API 执行，未直接写 SQLite、读取凭据或发送真实持仓给 AI。原始截图、提取明细、操作前记录、稳定请求回执和最终读回资料只存模块私有 imports，不将真实财务资料放进仓库。

数据逐项核验完成：十个基金身份的实际份额、当前成本和净值日期读回匹配；九笔旧基金保留历史并添加当日零校准；四笔在途款通过现有其他资产入口独立登记，不提前增加份额或现金。原申请日期、手续费及成交份额未知，未伪造 OTC 订单。待实际确认／到账后需核对这些其他资产，避免与新增份额或现金重复计入。新登记基金未擅自归入用户资金分组或修改目标比例，可能显示待归类。黄金采用用户最后提供的具体持仓详情，早先不同汇总原值保留。现行 `ROUND_DOWN` 与渠道估值的分位差异在私有核对资料逐项列出，未修改真实份额／净值以凑金额；首页只读快照总额与当前账本各资产求和一致。累计收益只保留来源资料，未伪造历史交易／分红。

`otc_holding_edit.rs` 的 calibration 仅在份额、金额、当前收益同时为零时允许清仓锚点；新增和其他更正仍要求正份额。内存库用例覆盖非法组合原子拒绝、重复回执不推进 revision、原期初及历史估值保留、没有新增成交和现金。源码已沿原开发产品源脚本构建、密封并通过 Core 现有配置链启用；不涉及正式安装发布。

验证结果：`cargo test -p fund-module zero_calibration`、最终 `cargo test --workspace`、`modules:ui:build/check`、`extension:check`、`apps:check`、`standards:check`、`git diff --check` 通过。`cargo fmt --check` 仍报告工作区既有区间改动 `home.rs`／`home_range_tests.rs` 的格式差异，未整理其他人的改动。`perf:check` 在 Native 目录查询 p95 251.79 ms（预算 50 ms）处失败；当时存在并发构建，停止该负载后只复测 Native 单项，p95 81.45 ms 仍超预算，保留失败，不继续复跑或修改预算。后续单项 `perf:model-host` 和 `apps:check` 通过，不能将这些分项改记为完整 perf 门禁通过。本轮没有再做真实 Chrome 页面读屏／完整走势交互验收，没有新增长期任务或外部交易。

## 估值舍入与分类对账（2026-10-25 19:44）

根因已用隔离用例复现：四位份额乘四位净值后直接截断人民币分，导致渠道四舍五入金额与项目估值相差一分；首页、持仓、收益端点和历史回放存在各自计算。`fixed::valuation_minor` 统一非负估值的最终分位 half-up，覆盖 `assets`、`valuation`、`holding_income`、`positions_detail`、`securities_income`、`nav_estimate`、`portfolio`，原本同规则的 OTC 金额换算复用它。通用 Fixed 运算、成交／费用／成本分摊规则不变，溢出及非法负值仍显式缺失，旧保存估值及观察不重写。

需求与验证对应：分位上下界、真实零、负值和 i64 溢出由定点用例覆盖；每项舍入后求和及正负日收益端点由 N04 估值用例覆盖；合成基金＋现金＋其他资产由持仓 API 集成用例对照首页、持仓、分类、历史和账本只读性。首轮金额用例稳定红后转绿；历史用例首轮因仅当天数据会按契约切换为日内观察而失败，补齐前一日真实夹具身份／净值后通过，未改生产分支或放宽断言。最终 `cargo test --workspace`、`cargo fmt --check`、`perf:check`（含 Extension、UI check、Apps）、`standards:check` 和 `git diff --check` 退出 0。相关格式调整仅格式化当前任务触及的区间文件，保留语义；此前 19:20 的 fmt／Native p95 失败仍按原记录保留，未修改预算。

已沿既有开发产品源流程构建／密封。旧 Worker 绑定前一代产品，按安全规则拒绝复用；重开本轮启动的本地开发 Core 后正常握手，未绕过代际校验或修改其他进程。通过正式 Fund API 逐项对照用户来源，十只当前基金的份额、市值、成本与原有身份全部匹配，已清仓项不再重复计入，总额与来源求和一致。用户另行确认分类后更新现有分组绑定，原分组身份／顺序／描述／目标比例保持，失效绑定为零，只有未知品类的一项保留待归类；四项在途跟随所属基金分类。当前估值观察已按正式入口记录，末点与当前资产／分类一致，原始旧点不改写。来源与核对结果只保存在本机模块私有 imports，不将真实财务数字加入仓库。

真实 Chrome 核验尚未完成：旧浏览器页在开发 Core 更换后显示连接说明，界面操作又遇用户正在操作 Chrome；已请求方便切换窗口的时间，在回复前继续完成独立数据核对，未反复抢占窗口。尚不能确认整页渲染、完整走势交互、主题／语言／窄窗／键盘均正确，API／隔离测试不替代这些验收。已登记在途款待真实确认或到账后需同步核对，避免再与份额或现金重复计入；没有创建真实交易或调用 AI。

## 资产详情与数据状态整改（2026-10-26 21:57）

根因：详情按钮只在长页面底部展开内容，没有滚动或焦点转移；现金预留未设置时，前端隐藏了已记录余额；多个不同缺失状态共用“待核实”。先由 Chrome 回归复现详情不在视口／没有焦点，再修复正式源文件。

- 详情自动定位并聚焦标题，关闭／Escape 回到触发按钮。管理资产展示同一快照的分类、成员与未分类金额，并可进入真实持仓／资产页面。
- 现金按已有账户展示。未设置预留时标题为“现金余额”，余额仍可见，可投资额度保持未知；无账户时不显示空详情入口。窄窗口改为账户明细，金额不拆行。
- 未持仓、缺少成本／估值、缺少期初历史分别解释，不再使用泛化的“待核实”。复用现有行情／净值同步 owner，不新增轮询、供应商或收益推算。
- 实际对仍缺当日净值的基金调用正式同步入口：东方财富移动源更新 40 条历史记录，最新仍为 2026-10-23。未将滞后的公开净值改成今日，也未补造个人历史成本、期初估值或确认交易。具体基金及真实账户回执保存在模块私有 `imports/2026-10-26-home-verification/`。

验证：`home-wealth-browser.test.mjs` 的 Archive／Volt、中英文、1280／390 px 共 8 组通过，含金额、缺失／空态、隐私、键盘与无横向溢出；相关 `home-insights-browser.test.mjs` 8 组通过。`home-wealth-web.test.mjs` 经真实 Chrome → Cookie／CSRF Core → 签名 Runtime → 隔离 SQLite 验证现金未知预留及两个页面入口，`errors=[]`。本机用户开发源重新编译密封后，真实账本页面确认账户合计等于显示金额、分类成员显示、焦点定位及无泛化核实文案，当前估值质量 ready／issues 为空。余额与较早快照不同，验收按同一当前快照求和，没有把旧金额写回。

`modules:ui:build`、`perf:check`（含 Extension／UI check／Apps／安全／规模）、`git diff --check` 通过。扩展隔离测试先因 Chrome for Testing 不存在失败，指定普通 Chrome 后被 `ERR_BLOCKED_BY_CLIENT` 拒绝，保留失败事实；真实 Web 通过不冒充扩展通过。未改 Rust 业务，本轮不重复 Rust 全仓或 30 分钟长测；未调用真实 AI、补录交易、提交或发布。

## 分类走势排除全资产总额（2026-10-28 14:51）

用户确认问题是“选分类后不应再显示总资产线”。只读核对本机开发库的估值观察，约 30 万确为已保存的全资产总额，而非硬编码的分类值；未修改账本。旧 renderer 无论选择哪个分类都插入 `total`，使其同时进入折线、纵轴范围与查看点。另一个独立错误是金额轴超过 10 万后只保留整万，将 29.8 万、30 万、30.2 万都显示为“30 万”。

按现有契约先修订分类展示口径，再修改 `home-overview.js` 的单选状态及绘制序列：总资产只在“全部资产”视图参与绘制；选分类只绘制其明细，没有明细时读取分类自身历史。缺历史／全部隐藏不回退总资产；单个分类也与“全部资产”明确区分，删除已选分类时恢复全局视图。`home-trend-chart.js` 使用当前语言的紧凑金额格式并保留精度，金额轴侧栏按实际标签宽度留白。中英文副标题、生成 locale／dist 和编译包含资源同步。

验证与边界：

- `rtk proxy node scripts/modules/home-insights-browser.test.mjs`：新增分类断言先稳定复现 `total` 数量为 1（应为 0），修复后分类线、悬停金额与 8 组布局通过。
- `rtk proxy node scripts/modules/home-history-browser.test.mjs`：刻度红例实际得到三个“30 万”，修复后为 30.2 万／30 万／29.8 万。以 30 万总额与数百元持仓构造隔离快照，核验选分类后两条持仓曲线、金额轴及悬停框均排除总额；覆盖单分类、删除分类、全部隐藏、缺值／缺日、日内／日序列、金额／百分比、键盘及隐私，Archive／Volt × 中英文 × 1280／320 px 共 8 组通过。新增窄窗测试首轮因第二张持仓卡折叠而超时，改用真实“展开”入口后通过；未强制点击隐藏控件。
- `rtk npm run modules:ui:build`、`rtk env -u CARGO_TARGET_DIR cargo build -p app-runtime --bin natives-app-runtime`、最终 `rtk npm run perf:check` 通过。最终门禁包含 Extension、Files、模块 UI、Apps、安全与边界检查；日志位于 `.tmp/asset-trend-axis-20261028/perf-final.log`。截图在 `.tmp/home-history/axis-*` 与 `selected-*`，已复核。

本轮 Chrome 使用生产 renderer 与隔离快照，不能冒充真实 Native／Web 全链或真实账本页面验收；浏览器连接返回空白页，当前运行页面未完成复验。仅重建工作区 Runtime，未换装用户开发产品源、重启用户进程、写真实数据、提交或发布。未改 Rust 业务，不重复 workspace／30 分钟长测；原长测未收口状态不变。
