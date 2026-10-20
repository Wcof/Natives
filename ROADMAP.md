# Natives 当前进度

架构与工程规则以 `docs/README.md` 指向的 ADR 和 Standards 为准；本文只记录当前实施状态。

## 进行中

- 2026-10-20 09:49：Chrome 扩展＋本机独立 Web 仍未整体验收。Files 已续修取消误提交、搜索／目录竞态、Tab 焦点与字号；Space 已续修便签丢字／peer 覆盖、快照重排失焦及空间表单。PDF 按明确授权落 ADR-0043 后限定修复，真实 E/W 内容、单多页、原生翻页／缩放、适宽、16 个布局、真实禁用及拒绝／释放路径通过；原生搜索人工核验受 Mac 锁屏阻塞。最终 perf 门禁通过。其他模块／全弹窗／视觉、安全与 Release 资源计数未齐；30 分钟最后核验，未重复长测、安装、提交、push 或修改真实用户库。见 [施工与证据](/Users/ldh/Downloads/project/plan/memory/dual-entry-migration/evidence/20261018-full-acceptance/execution.md) 、[PDF 分项结果](/Users/ldh/Downloads/project/plan/memory/dual-entry-migration/evidence/20261018-full-acceptance/pdf-acceptance.md) 。

- 2026-10-13 18:38：macOS 顶栏今日计量修复已按授权应用至真实开发环境。此前合成测试漏掉 Codex 两套不等累计计数混算；现改为新版稳定响应 ID＋单次 usage，旧版独立累计求差，解析模式跨批／重启持久化并恢复旧游标。真实库先完整备份，再仅校正今日 2 个会话的 396 条响应、移除 378 条重复累计：150,175,266,763→101,596,860 Token，其他事件及非游标表逐行不变、完整性通过。新版顶栏及 worker 重启补采后今日 113,579,341 Token；455 条 Codex 响应与原日志模型／缓存分桶／单次用量逐条一致，差异 0，再查没有重复增量；原生生产数据函数输出 113.6M · --（缺单价）。Go 219 项／14 packages、实测回放、升级／重启／归档专项与最终串行门禁通过；保留首轮快照超时。更早历史未重算，界面工具不识别无应用包的顶栏进程，屏幕显示未直接确认；未提交／推送／发布。见 [既有用量契约](docs/development/ai-efficiency-remediation-implementation-plan.md)、[Chrome 面板](scripts/statusbar-browser.test.mjs) 与 [原生 owner](scripts/statusbar-owner.test.m) 。

- 2026-10-12 20:13：投资整改本轮可修复问题已实施并完成本地验证；完整 P01～P18／T00～T08／A01～A32 的人工与平台项仍按分项记录保留，不恢复旧“全部验收”声明。真实季度两期、登记后个人收益、v28 资讯决定闭环、单账户股票记录分页、恢复入口与点击区域、组合状态保留、空间工具栏遮挡均已修复。用户授权将已有商汤模型设为全局默认，通过 Model Host 服务接口完成；原凭据不变。真实到账按约定为空，真实用户库及完整真实供应商 Chrome 链未验。见 [独立复核](docs/development/evidence/investment-remediation-2026-10-11.md) 与 [施工清单](goals/20261011-2254-investment-remediation.md) 。

- DES V2.1 可维护双主题与投资全页面整改：本轮生产 UI 改动及本地集成复验完成；T12 人工／平台验收待完成。单源 Archive／Volt、12 页模式、体检下钻、三类持仓与独立日／昨日收益、分组管理、研究／绘图／公开 AI、NAV 和设置均已实施；272 基础布局组合、完整 Native 隔离链、workspace 与最终 perf 门禁通过。A43 平台原生控件／人工读屏、A44 真实捕获／系统保存、全 F/A 人工验收与既有长测 CPU 未收口。入口：[核验记录](/Users/ldh/Downloads/project/plan/des/08-verification-records.md) 、[实施审阅](/Users/ldh/Downloads/project/demo/natives/investment-implementation-review.html) 。

- 模块边界与商业化工程整改：T00～T10 已有部分实现，2026-10-05 独立复核确认仍需整改，不能沿用旧“全部收口”声明。本轮已修复通知取消／启动与关闭竞态、超时 worker 句柄丢失、动态导入／Rust 反向依赖漏检、删除 API 的 CORS 声明缺失，并补充真实绘图／三入口设置验证和连续 Chrome 资源采样。当前源码整改、75 步完整 Native 功能回归及串行最终门禁已通过；长时资源验收未通过：完整 1,068 轮／245 条采样的空闲总 CPU 为 2.248538%＞2%，复跑在 568 轮后遇设置按钮稳定性等待超时。保留原始失败，根因未确认，不声明方案全部完成；原“127 轮／2,540 次开关”仅为短进程协议循环，不能证明同一 Renderer 的长期资源稳定性。A34 原始 before 基线不可恢复；正式 Release Files Host 拒绝开发信任根，开发 Chrome 链与独立 Release 探针分列。平台安装／签名／公证及真实供应商／通知外发未验。入口：[mode Plan](/Users/ldh/Downloads/project/plan/mode/README.md) 、[实施与证据索引](/Users/ldh/Downloads/project/plan/mode/09-verification-records.md) 。

- Host 进程生命周期整改：T0～T5 业务修复已实施并通过本地门禁（2026-10-04）；T6 余项为 Release 对比、30 分钟真实 Chrome 循环、Windows 实机与 macOS Intel。已完成：F1～F8 全部修复（SDK 用量队列有界化＋补丁重放一致、clientHub 有界广播与慢客户端隔离、全部启动器 reap、Engine 任务登记/owner 取消/stopping 原子门、采集 ctx 贯穿与遍历预算、locate 有界化、App accept 唤醒与看门狗去重、dev 会话隔离与冲突检测、Windows 管道命名空间派生、顶栏就绪等待与退出清理）；model:host:check 203 项＋定向 race、SDK 34 项、workspace 25 套件、fmt、perf:check（p95 46.07 ms）、standards、diff、顶栏 clang 编译通过；三 Host stdin EOF 实测中位数 1.3～3.9 ms（契约 1s/2s/2s）。T6／L16 尚未验收：本轮 mode 已完成一轮真实 Chrome 连续窗口，但 CPU 超预算，复跑又发生动作稳定性等待超时；Release 比较／完整签名链、Windows 实机仍未验收。入口：[实施记录](docs/development/evidence/host-lifecycle/implementation-2026-10-04.md) 、[生命周期整改与交接](docs/architecture/application-performance-remediation.md#进程生命周期整改与交接2026-10-04) 。

- N04 统一资产估值与收益聚合：专项已实施并通过 A64～A67 隔离验收（2026-10-04），契约见 [Plan 执行入口](/Users/ldh/Downloads/project/plan/08-agent-execution-prompt.md) ，证据见 [N04 验证记录](/Users/ldh/Downloads/project/plan/09-implementation-verification.md) 。整版生产验收另行宣称。

- PanWatch R2 整版修缮：持仓最多 4 位／移除 pos-watch、双入口财务设置、S06 HTTPS 代理、v25 报告反馈、安全模板适配、九渠道与会话通知均已有实现与分项证据，不再按旧 R4／R5 未实施起点重做。当前进行 R6 最终集成与 R7 全量审计；D15 分享卡导出、A61 全流程真实精度证据及外部／正式 Release／平台验收仍未齐。本轮默认 workspace 已通过（含 50 次生命周期，83.06 s）；此前并发失败未独立解释，不能把本次通过当成旧问题根因结论或整版生产完成。详见 [当前状态与证据](/Users/ldh/Downloads/project/plan/09-implementation-verification.md) 。

- 待在真实 macOS Apple Silicon、macOS Intel 与 Windows 设备完成安装、升级、签名、公证及多模块进程验收；本地源码检查不能替代发布验收。

## 最近完成

- 2026-10-18 14:23：双入口本轮缺陷修复已完成相关回归：事件／配对恢复、基金表单保留、SSE 权限、大文件导入、跨入口持仓刷新、订单重试幂等、定投状态及只读快照冲突；同步候选构建顺序和原始证据。整体迁移仍在进行，见 [收尾核验](/Users/ldh/Downloads/project/plan/memory/dual-entry-migration/evidence/20261018-closeout/acceptance.md) 。

- 2026-10-18 09:19：按用户要求统一长测频率：现行技术 04、AGENTS／文档入口及双入口施工包明确「一次有效验收、后续按影响复用」，普通功能／样式／文档、重新构建或摘要变化不自动触发；必要补验先定位和短测，禁止盲目循环至绿。5 个 Release 性能样本不代表 5 次长测；历史失败和现有未验状态保持。本次仅改文档，未启动长测。见 [现行规则](docs/standards/technical/04-performance.md#长测频率与结果复用) 。


- 2026-10-15 09:41：Model Host 路由内核由 CLIProxyAPI v7.3.17 迁移至上游 v8.0.28（commit `0d18d3b365be000eeaee7de4615f9da596b2059d`）；同步 v8 SDK 导入、依赖、编译版本和可重放的 Natives 补丁，保留有界用量、Antigravity 地域重试、静态模型及 JSON→SSE 兼容。开发更新器现支持同主版本 v8 更新并继续阻止未来跨主版本直接换装；未改 schema、真实配置、Keychain、用户数据或供应商凭据。见 [ADR-0041](docs/adr/0041-cliproxyapi-v8-kernel-migration.md) 与 [内核说明](third_party/cliproxyapi/NATIVES_FORK.md) 。

- 2026-10-14 22:56：首页资讯按生成视觉稿重排，四范围与来源／标题／摘要／操作分层；AI 修复重复快照取消证据准备、改为有界流式阶段反馈与明确失败分类。SDK 补齐忽略 stream 的 JSON→SSE，无重复请求；内核跨主版本检查提前提示迁移，不误显示可直接更新。ADR-0040、双语与开发源同步，未修改测试代码。见 [证据](/Users/ldh/Downloads/project/plan/memory/evidence/20261014-news-ai-redesign/README.md) 。

- 2026-10-14 22:32：首页资产分布／组合设置固定为同高 480 px，尺寸通过 Fund 主题变量维护；仅正文纵向滚动，标题／编辑动作常驻；可键盘聚焦滚动，刷新保留正文滚动位置。测试及验证脚本未修改。

- 2026-10-14 18:36：首页／资讯／AI 选材统一可读来源、摘要／完整／截断提示及北京时间；资讯详情不再把截断正文标作完整摘录。基金详情按当日日期识别正式收益，过期收益不阻止当天估算，正式当日净值不再被标作估算。仅改生产 UI 与生成资源，测试／验证脚本未修改。

- 2026-10-14 12:27：资讯恢复 Demo 阅读层次及默认收起的高级筛选；全部整合公开财经与持仓／自选，新增财经范围。接入东方财富四类财经栏目，有界采集、主题排序、来源开关与配置导入一致，无持仓也有真实资讯。未修改真实用户库。见 [实现与证据](/Users/ldh/Downloads/project/plan/memory/evidence/20261014-information-layout/README.md) 。

- 2026-10-13 21:29：持仓「账户管理与持仓汇总」常驻展示；首页原待到账按用户确认改为证券今日收益，与持仓顶部／卡片同日正式及基金估算口径对齐，正负／未知／覆盖状态分开。基金卡片和详情新增真实公开主题／跟踪指数与来源；拒绝热门推荐冒充归属，独立资料 owner 有界、取消、缓存、重试及焦点／隐私齐备。生成 UI、Runtime 与开发源一致，未读写真实账本或提交／推送。见 [专项证据](/Users/ldh/Downloads/project/plan/memory/evidence/20261013-holdings-daily-themes/README.md) 。

- 2026-10-13 20:46：持仓管理文案、首次切页取消不恢复及行情竞争账本读取修复；已加载卡片保留。基金当日公布净值／涨幅优先，待公布阶段沿现有可见页面 owner 每 60 秒检查；最近季度公开境内股票与同日报价生成部分贡献参考收益／净值，标明覆盖率及未覆盖不变假设，不写入确认收益、历史与目标。源码、生成 UI、Runtime 与开发源一致，未操作真实投资库或提交／推送。见 [专项证据](/Users/ldh/Downloads/project/plan/memory/evidence/20261013-positions-current-data/README.md) 。

- 2026-10-13 18:35：投资资讯空数据修复：独立页原先只读缓存、没有采集入口，现默认全部并首次获取持仓／自选；启用来源未采集或尝试超过 15 分钟才自动取一次，手动刷新／取消／下一批复用现有任务。结果／错误在当前页显示，语言重绘、失败保留、隐藏／离开取消和游标范围绑定齐备；保留来源禁用，不自动 AI。源码、生成 UI、Runtime 与本地开发源同步，未读写真实账本、未提交／推送。见 [资讯证据](/Users/ldh/Downloads/project/plan/memory/evidence/20261013-information-data/README.md) 。

- 2026-10-13 16:49：投资全部 14 个生产视图及维护弹窗按确认 Demo 继续还原：资产卡片、持仓次级管理、自选分组、真实市场候选、股票四卡摘要与记录／资讯／研究、基金切换、独立资讯／历史、设置概览、流水检索、导入步骤和 NAV。修复机会重绘丢失筛选按钮焦点、资讯详情抓取错误在当前弹窗不可见；保留真实数据 owner、精确金额及隐私。UI／Runtime／本地开发源摘要一致，未提交或推送。见 [全页面核验](docs/development/evidence/investment-remediation-2026-10-11.md)、[对照预览](/Users/ldh/Downloads/project/demo/natives/investment-all-pages-review.html) 。

- 2026-10-12 22:54：投资视觉与 UE 复刻更正：按确认 Demo 恢复居中容器／导航、共同字体与卡片角色、首页顺序／等高 AI、水位标题比例／固定刻度、持仓卡牌与基金详情；额外能力折叠保留，日期浮层离开触发点安全收起。生成 UI／隔离候选／开发源同步，79 个文件摘要一致，旧页面需重载重开。见 [核验](docs/development/evidence/investment-remediation-2026-10-11.md)。

- 2026-10-12 20:13：投资独立整改本地交付：基金四部分详情与历史真实数量／费用／校准、两期公开披露比较及正确证券身份、v28 资讯决定绑定报告与历史回看、默认统一 AI／预览／取消／保存幂等、股票分页与身份、组合展开状态／说明、水位、期初恢复、主题按钮与空间安全区。当前源码／生成 UI 与隔离受验签候选一致；未更新真实安装源或真实投资库、未提交／推送。分项与剩余人工范围见 [证据](docs/development/evidence/investment-remediation-2026-10-11.md) 。

- 2026-10-10 21:25：自选单目标移动更正：单项／批量只选一次目标，打开实时读取完整分组，来源组不列为目标；全部统一归目标，具体组仅移当前归属；已有目标合并保留元信息，原子回滚／409 复核／失败重试／取消与语言保留。复用行同步身份及详情回调，清理旧来源选择文案；源码／dist／本地开发源同步，领域／全仓／真实 Native 与最终门禁通过，未触真实用户库。入口：[核验](/Users/ldh/Downloads/project/plan/des/evidence/20261010-watchlist-one-target/checks.json) 。

- 2026-10-10 20:57：趋势分析按用户更正保留老版外观，仅修搜索／报价分行、工具分组和正常图表比例：宽窗保留工具与两栏，图表独立滚动；窄窗上下，容器 resize 同实例／缩放并重投影绘图锚点，pagehide／BFCache 清理恢复。主题高度角色／源码／dist／本地开发源同步；真实扩展和合成各 16 布局组合、专注／绘图／NAV 与最终门禁通过，无行情／指标算法或真实用户数据变更。入口：[核验](/Users/ldh/Downloads/project/plan/des/evidence/20261010-trends-layout/checks.json) 。

- 2026-10-10 20:18：首页收益拆为股票／基金（场内＋场外）／线下投资／全资产四卡：分类账本、清仓已实现及独立缺口；线下累计账面收益／净回收／待到账，未知成本或退出历史不完整标待核实；全资产今日／所选期间／TWR／仓位保留。四／两／单列、双语和隐私通过，源码／生成 UI／本地开发源同步；无 schema 或真实用户数据变更。入口：[核验](/Users/ldh/Downloads/project/plan/des/evidence/20261010-home-income-cards/checks.json) 。

- 2026-10-10 20:18：自选单条／批量移动本地门禁收口：前轮单项／批量／真实 Chrome／原子回滚验证保持，最新 workspace 与串行 perf:check（Files p95 38.701041 ms＜50 ms）通过，本地源同步。保留原 65.712209／50.870209 ms 失败，不宣称其根因修复；仅本地验收。入口：[原专项及后续复验](/Users/ldh/Downloads/project/plan/des/evidence/20261010-watchlist-moves/checks.json) 。

- 2026-10-10 18:54：自选默认全部及分组栏修复：跨组 symbol 去重、实际组写入与增删调用统一；分组导航／工具两行、8px token 间距、36px 控件与窄窗换行，复用按钮避免刷新失焦。生成 UI／本地开发源同步；真实 Native 隔离账本八组矩阵和最终门禁通过，未触碰真实用户库。入口：[核验](/Users/ldh/Downloads/project/plan/des/evidence/20261010-watchlist-groups/checks.json) 。

- 2026-10-10 18:42：资产走势空白修复：单个真实点从边缘移至可读估值卡，显示金额／日期；历史为空时区分当前已估值部分、纯历史区间与隐私，不补造曲线或自动保存。提供记录今日估值入口、聚焦既有核对表单、默认不勾核对，取消零写入。Home UI／双语／生成资源、本地开发源同步；最终门禁与真实 Native 隔离账本通过，未碰真实数据。入口：[核验](/Users/ldh/Downloads/project/plan/des/evidence/20261010-home-history/checks.json) 。

- 2026-10-09 20:37：首页全资产净收益与区间修复：用户指定全资产为主，真实基线／净投入／当前只读参考与部分覆盖，不伪造核对快照；今天／本月／今年／以来／自定义联动收益、历史、归因和配置基准，移除固定 60 日及误绑定。单个原生双月日期 Popover，选完自动应用、窄窗／键盘／焦点与失败保留；开市消费配置刷新周期及行情更新。最终门禁与真实 Native 隔离账本通过，本地源码／Runtime／产品源同步，未碰真实账本。入口：[证据](/Users/ldh/Downloads/project/plan/des/evidence/20261009-home-ranges/checks.json) 。

## 最近验证

- 2026-10-20 09:49：PDF 冻结源码后 perf:check 退出 0，包含 Extension／Apps／规模／主题／双语及安全门禁；新增 controller 回归通过。双入口当前 Blob 原生成功渲染、单多页、翻页／125% 缩放、适宽、关闭释放、Host 拒绝坏 magic／超限、真实禁用 reader、HTML 脚本隔离及 16 布局通过。Files 导入／移动／副本／隔离回收站／PNG／WAV 相关闭环复跑通过；搜索栏人工结果未验，不以按键或外框冒充成功。见 [PDF 证据](/Users/ldh/Downloads/project/plan/memory/dual-entry-migration/evidence/20261018-full-acceptance/pdf-acceptance.md) 。

- 2026-10-20 00:26：Files／Space 本批最终 perf:check（含 Extension／Apps／边界与规模）、standards、diff 均通过；真实 E/W 文件弹窗 16 组合及原生搜索键盘、工作区增改删切换、待办／计数器／便签连续输入与 peer 更新复验通过。PDF 实际错误页保留，未将 wrapper 或门禁当全迁移通过；未运行 30 分钟。见 [本批证据](/Users/ldh/Downloads/project/plan/memory/dual-entry-migration/evidence/20261018-full-acceptance/execution.md) 。

- 2026-10-18 14:23：当前 Rust workspace／fmt、模块 UI build/check、Go 219 tests／14 packages、最终 perf:check 全部退出 0。真实隔离 Chrome 两入口大文件逐字节一致、持仓共享自动同步、申购实际提交后丢响应重试只一笔、多定投、同文档／跨 Tab 配对及基金草稿保留通过，errors=[]；Files 16 组合无横向溢出，Model 七页／所覆盖子页实际导航。一次约 36 秒短测只作诊断，未冒充完整资源、全页面、真实供应商或安装验收。见 [原始记录](/Users/ldh/Downloads/project/plan/memory/dual-entry-migration/evidence/20261018-closeout/acceptance.md) 。

- 2026-10-18 09:20：长测频率文档核验：`standards:check`、`git diff --check` 通过；核对技术 04 规则、AGENTS／docs 入口与施工包 7 份文件一致，复用规则链接存在。仅调整执行频率与证据复用，未降低指标、未修改历史结果、未运行长测。

- 2026-10-18 01:20：双入口续修复验：`perf:check` 通过（bundle 359598B／drift 20426；native 39.86ms；Model 776.91ms）；本地安装候选已组装并审计 payload；30 分钟真实标签页运行中止（486 轮/173 样本）；9 轮 smoke E +13.64%／W +6.50%；无有效最终长测；200 文件批处理进度/取消/解压取消通过；Web BFCache persisted 恢复通过，Extension 未进入 BFCache；入口 gzip E 159888/98785、W 164207/103104B；SQL Core 10/10/10、Fund 2/0/0；UI 64.7／63.6ms、缓存 85.8／141.2ms、FPS 60.97／60.51。本地候选已组装；真实安装、全业务弹窗、SQL 时长、真实 AI 仍未验。见 [证据](/Users/ldh/Downloads/project/plan/memory/dual-entry-migration/evidence/20261017-final-continuation/acceptance.md) 。

- 2026-10-17 16:12：双入口关键修复分项复验：Rust workspace／fmt、Extension、模块 UI／契约／边界／鉴权与最终 perf 门禁通过；保留契约文档／旧鉴权脚本及 pending Surface 关闭竞态的首次失败，后者红→绿并覆盖重开竞态。真实隔离 Chrome E/W 共享 Fund、关闭一页另一页继续；224 张投资主视图截图布局溢出 0、错误 0，差异对照仍未整体验收。真实协议观察 Fund 1 个 Worker、加 Tokenusage 后 2 个，所有 owner 释放约 60.81 秒后 0 个；第三卡和私有任务／令牌边界通过。Release Core 空闲 60 秒前后各 5 样本 CPU max 0.7499%→0.3832%、after RSS max 6752 KiB；串行 RPC p95 2.94→6.89 ms，10k 目录 p95 49.43 ms，未放宽预算。完整候选 handoff、Core 偏好／事件／其他模块闭环、Chrome 30 分钟和查询／连接计数未完成；没有真实用户库、提交／push／系统安装。见 [本轮证据](/Users/ldh/Downloads/project/plan/memory/dual-entry-migration/evidence/20261017-critical-fixes-1559/acceptance.md) 。

- 2026-10-16 19:51：macOS accepted socket 断连先在真实 Chrome 复现；修正后隔离 Web 配对、Space 四项 Core RPC、双网页续租／关闭、70 秒可见停留以及中英文 App Center／投资 iframe／文件页真实链通过。首次最终规模门禁发现 715 行服务路由违规，按 Surface 职责拆分后复跑架构、Rust workspace／fmt、Extension、perf:check、standards 和 diff 通过。产品源静态页面烟测通过；未做扩展与 Web 同模块 Worker、投资全页、完整安装候选及 30 分钟长测，不计整体验收。见 [本轮证据](/Users/ldh/Downloads/project/plan/memory/dual-entry-migration/evidence/20261016-followup-1933/acceptance.md) 。

- 2026-10-16 14:58：双入口后续复核：全仓 Rust、fmt、Extension、模块 UI build/check、Model Host、最终 perf:check 和 diff 通过；真实无扩展 Chrome 隔离页面验证 Web 配对、Space RPC、双主题／双语／1440 和 320 px 与键盘焦点。Release 目录性能首轮及复跑分别 134.98／140.89 ms 超 50 ms，隔离修复后 43.13／43.21 ms；单服务 58 秒空闲 CPU 0.74%。完整双入口同开、投资 14 视图、资源 30 分钟与 5 个 Release 样本未验证，不计通过。见 [证据](/Users/ldh/Downloads/project/plan/memory/dual-entry-migration/evidence/20261016-followup-1431/acceptance.md) 。

- 2026-10-16 03:50：双入口独立复核：standards、architecture、UI、Apps、Extension、Rust fmt、当前服务编译及服务／会话各 6 个现有单测通过；5 项独立安全／SSE／多 Surface 契约探针全部失败，原始输出保留。真实无扩展 Chrome 经当前生产 HTTP／资源处理器、内存鉴权及无数据库 owner 的诊断链复现 Model 不支持、SSE 404、配对语言和焦点缺陷；不冒充真实账本／完整产品链。旧 CSV、图片摘要与测试动作逐项核对，发现直接填 PASS、复制截图及未测性能；原集成脚本会打开默认用户 Core 库，未直接执行。Release 长测／全页矩阵／完整候选及其他平台未测，不计通过。见 [证据](/Users/ldh/Downloads/project/plan/memory/dual-entry-migration/evidence/20261016-independent-audit-0347/acceptance.md) 。

- 2026-10-15 09:41：v8.0.28 补丁从干净上游重放后与生产 `internal/`、`sdk/` 逐文件一致；`model:host:check` 218 项／14 包、SDK usage 与 OpenAICompat／Antigravity 定向测试、Model Host 构建及 `perf:check` 通过（首快照 809.12 ms、RSS 121296 KiB、空闲 CPU 0%、EOF 11.06 ms、Files p95 39.209 ms）。隔离 Native 报告 `kernelVersion=v8.0.28`；复用既有 Keychain 引用的真实 SenseNova 路由以公开合成提示完成 waiting→reasoning→writing，未发送持仓或修改配置。完整 executor 套件仍有上游两项 `thinkingLevel`／`thinkingBudget` 断言失败，已确认干净 v8.0.28 同样复现，不冒充全量上游测试通过；现有测试只迁移 v8 导入与版本夹具，未弱化断言。

- 2026-10-14 22:56：Go 218 项／14 包、SDK OpenAICompat 与补丁重放、首页 Chrome、真实隔离 Native AI 完整保存／决定／回看及 12 布局组合通过；真实商汤合成公开证据 7 秒返回 JSON，未发送持仓。最终 UI build／perf:check／源摘要一致；保留超时、JSON 兼容、准备竞态和三轮资源／规模失败。v8 主版本迁移未做，旧 worker 需重启开发会话。见 [记录](/Users/ldh/Downloads/project/plan/memory/evidence/20261014-news-ai-redesign/README.md) 。

- 2026-10-14 22:32：Chrome 实际加载生产 HTML／CSS，Archive／Volt×1280／320 四组合验证卡高 480 px、正文溢出与 End 键滚动、标题可见及无横向溢出；首次即时取 scrollTop 早于浏览器滚动完成，等待实际滚动后通过。UI build 与 perf:check（包含 UI／Extension／Apps）通过，正式开发源已刷新。隔离长列表仅用于布局验证，未操作真实投资库。

- 2026-10-14 18:36：现有资讯、基金详情、首页 Chrome 检查通过；原有 Native --inline-ai 完成预览→显式推理→保存→决定→历史及 12 主题／语言／宽度组合，errors=[]（隔离账本、合成供应商）。UI build、perf:check（包含 UI／Extension／Apps）、standards、diff 通过，开发源 UI／清单一致。脚本内容摘要逐一确认无改动；未测试真实供应商与真实用户库，本轮未改 Rust 业务。

- 2026-10-14 12:27：资讯专项、首页／112 布局回归、真实隔离 Native 8 组合通过；真实财经 30 条、关联新闻 13 条。最终 fmt／workspace、UI build/check、perf:check（p95 43.098417 ms）、开发源摘要一致性通过。保留初次配置往返失败并修复，网络取消／409 分列；真实 AI／用户库／人工平台项未验。见 [记录](/Users/ldh/Downloads/project/plan/memory/evidence/20261014-information-layout/README.md) 。

- 2026-10-13 21:29：汇总折叠及首页读线下应收红→绿；主题身份／推荐排除与 Chrome 空／错／重试／焦点／隐私通过。真实隔离 Native 验证首页／持仓 -4.30 对齐、场内主题与场外主题／公开红利低波指数及初次切页恢复。持仓／首页／基金／112 布局、fmt／workspace、UI build/check、Extension 与串行 perf 通过（Files p95 42.730292 ms＜50 ms），开发源摘要一致。保留只读快照／旧视觉夹具及未指定测试浏览器路径的失败事实；没有真实用户库／开市估算／人工平台验收。见 [范围与结果](/Users/ldh/Downloads/project/plan/memory/evidence/20261013-holdings-daily-themes/README.md) 。

- 2026-10-13 20:46：文案归属、切页／行情竞争及净值重试红→绿；季度估算 2 项、持仓／基金／首页 Chrome、112 布局组合、真实隔离 Native 切页与公开当日 NAV、fmt／workspace、UI build/check、串行 perf 全通过（Files p95 40.703333 ms＜50 ms），开发源摘要一致。正式源样本已公布净值，开市现场估算未观察；首次空 Core 初始化锁失败保留，测试预初始化不代表生产并发修复。真实用户页面因工具 policy 失败未检查，未读取实际账本。见 [证据与边界](/Users/ldh/Downloads/project/plan/memory/evidence/20261013-positions-current-data/README.md) 。

- 2026-10-13 18:38：真实顶栏采集链：准确匹配开发版顶栏／worker 后正常停止，完整 SQLite backup 保存 ~/.natives/backups/usage-20261013-174202-before-codex-today-repair.db（权限 0600、完整性通过），仅今日 2 个获授权会话事务修复；其他事件／非游标表与备份逐行一致，integrity_check=ok。新 Go／clang 二进制构建并重启；实际 overview（Asia/Shanghai）113,579,341 Token／458 条请求，455 条 Codex 响应与来源 usage、模型、缓存分桶逐条一致（0 差异），再次读取增量 0；生产原生数据函数输出 113.6M · --。Cua 不识别无应用包的 natives-statusbar，未直接读屏；较早历史未修复、缺单价金额仍 unknown，未做上游／发布或其他用户库操作。

- 2026-10-13 18:35：资讯首次采集回归从 0≠1 红到绿；缓存／手动刷新／取消／游标范围／失败保留／禁用／隐私检查通过。真实 Chrome→Native→公开适配器隔离链获取 12 条近期文章，8 个主题／语言／宽窄组合、键盘刷新及隐私通过，errors=[]。首页回归、14 页 112 组合、UI build/check、最终 perf:check（Files p95 42.335167 ms＜50 ms）、standards／diff 通过；本轮未修改 Rust／schema，未重跑全仓或真实用户库／AI。英语状态残留及测试中缺少首页动态按钮的失败已保留并修正；来源可用性以采集结果为准。见 [证据](/Users/ldh/Downloads/project/plan/memory/evidence/20261013-information-data/README.md) 。

- 2026-10-13 17:16：105B 真实计量整改：实测脱敏 JSONL 回放红（150,126,366,521）→绿（75,765,501），最小不等双计量器／单响应／升级前缀恢复／重启／归档去重通过；Go 全套 219 项／14 packages（隔离 config dir、GOFLAGS=-p=1）、gofmt、standards、diff 通过。最终串行 perf:check（含 extension:check／perf:files／apps:check）退出 0，Files p95 44.586ms，Model snapshot 762.97ms；保留首轮并行 1507.49ms＞1500ms 失败与独立 746.64ms 通过，不宣称性能改善。真实用量库只读备份副本试修约 150.17B→98.74M，其他工具逐行一致，SQLite integrity_check=ok；真实修改／重启待授权，较早历史未修复。

- 2026-10-13 16:51：顶栏复查：模型／续读／部分费用／自动刷新红→绿；用量 99 项，全量 Go 217 项／14 包（GOFLAGS=-p=1、隔离 config root）通过；Objective-C 数据／原生真实 NSStatusItem owner／Chrome 生产面板、gofmt、clang 调试构建、任务 diff、standards 通过。原生覆盖单飞、旧面板／旧 worker 回调、缺额度、英文窗口分类与跳转白名单；Chrome 覆盖空周期、未知金额、估算标记、额度先到、趋势顺序、刷新通知及键盘。串行最终 perf:check 全通过，Files p95 41.659 ms、Model 首快照 822.29 ms、idle CPU 0%、RSS 70928 KiB、EOF 47.14 ms。保留首次并行运行首快照 1563.44 ms＞1500 ms 的失败；独立复验 739.73 ms 与最终串行通过不证明波动根因或性能提升。Namespace／加号时区 fixture 修正后通过；未弱化规则。真实安装／历史库／上游与多屏人工验收未做；投资文件未由本任务改动。
