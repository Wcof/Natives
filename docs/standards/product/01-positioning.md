# 产品架构 01 · 定位与边界

> **版本**: 3.1.0 · **日期**: 2026-09-11
> **Apps 补充决策**：[ADR-0029](../../adr/0029-unified-suite-preinstalled-apps.md)，统一套件预装与分级验收
> **关联 ADR**: [ADR-0020](../../adr/0020-ai-native-personal-workspace-rearchitecture.md)（当前产品与架构冻结）、[ADR-0007](../../adr/0007-domain-wheel-reinvention-clarification.md)（复用原则）
> **取代**: ADR-0012 派生的三面/双轨与 Workshop 优先规则

## 一、产品身份

#### R-P0 · 产品身份唯一表述
- **等级**：MUST
- **分类**：命名、产品
- **规则**：产品身份**必须**表述为：

  > **AiNative = AI Native Personal Workspace。**
> 首页（Home）必须支持**多个 Workspace**：创建、打开、切换、关闭、重新打开、置顶、排序、重命名、复制、模板创建与删除；Workspace Tab 只代表**当前打开的 Workspace**，Grid/Canvas 是该 Workspace 的布局模式（MUST，ADR-0021）。

  产品是稳定、长期可维护的个人数字桌面，不是 Agent Runtime、AI Gateway、固定 Dashboard、低代码平台或插件市场。
- **为什么**：单一身份决定 IA、领域所有权与删除范围，避免历史能力继续反向定义产品。

#### R-P1 · 一级 IA 固定
- **等级**：MUST
- **分类**：产品、交互
- **规则**：一级入口**必须**为：首页、文件、应用、AI、数据与用量、设置。AI 下分 AI Resources、Local Proxy、AI Tool Integration。**禁止**新增 Workspace 一级菜单；Home 是唯一 Workspace 一级入口，其下可打开多个 Workspace（ADR-0021）。「应用」（应用中心）是内置功能模块的入口与偏好管理页，始终从「设置」可达；模块首次打开只做数据初始化/迁移，失败显示明确错误，不显示"准备中安装"语义。
  **托管扩展包内置模块交付（ADR-0027，2026-09-09 取代 ADR-0025/0026 Apps 目标；2026-09-12 收敛为整包内置模块）**：Natives 是唯一的用户产品，官方托管扩展包（如基金扩展包）是 Natives 的**内置功能模块**，代码随 Natives 完整安装包构建并进入安装包，不是独立桌面产品；应用中心只保留内置功能打开、导航显示/隐藏、偏好设置与数据管理，不存在"可添加/应用商店"、模块安装/下载/升级/卸载语义；入口与来源/托管分离——应用中心不包含任何单个模块的专用业务。新增模块只能通过新的完整 Natives 版本交付。
- **为什么**：这是用户任务组织方式，不以内部 runtime/技术名暴露产品结构。

#### R-P1.1 · 一次安装与内置模块即开即用（2026-09-12 收敛）
- **等级**：MUST
- **规则**：完整 Natives 安装包必须包含发布清单承诺的全部内置模块，首个完整候选包含 fund 的真实 UI/业务代码。应用中心对内置模块只提供打开/显示隐藏/设置/数据管理，不存在模块安装/下载/更新/卸载操作；首次打开只做数据初始化/迁移，不下载代码、不显示"安装基金"。不默认运行、联网刷新或静默更新；产品更新统一为用户触发的"更新 Natives"完整产品版本。模块显示/隐藏偏好、移除选择和用户数据在产品更新、重装、修复时保留。新增模块只能通过新的完整 Natives 版本交付，不得承诺在线添加任意新业务。
- **反例**：只安装 Files Host，却宣称基金开箱即用；用样例或空页面冒充基金；首用显示"正在安装基金"或从 Catalog/nap 下载模块代码；把模块首用称为安装；保留独立模块安装/更新/卸载入口。
- **检查方法**：断网首次打开内置基金（T04）、整包 N→N+1 升级（T18）、隐藏/重开入口（T10）、历史停用/移除迁移（T11），按 implementation-plan 的 local/production 两列记录证据。

## 二、Home 与 Widget

#### R-P2 · 首页就是 Personal Workspace Home
- **等级**：MUST
- **分类**：产品、状态
- **规则**：根路由 `/` **必须**是可配置 Home。Widget **必须**是内置 React renderer + config + versioned grid layout；**禁止** Widget Plugin Framework、Runtime、Event Bus、Worker、Marketplace 与 Infinite Canvas（多 Workspace 数据域合法，见 R-P0 与 ADR-0021）。
- **为什么**：Home 需要可组合，但不应演变成新的运行时平台。

#### R-P3 · Widget 只做轻量投影
- **等级**：MUST
- **分类**：分层、性能
- **规则**：Widget **必须**消费 Files、Apps、AI、Usage 等领域 query/facade；**禁止**直接访问 SQLite、扫描文件、解析工具日志、查询进程、访问 Provider、读取 Secret 或创建重复 timer/poll。
- **为什么**：数据权威与资源生命周期必须留在领域 module；Widget 数量不能线性放大 IPC/SQL/Timer。

## 三、领域边界

#### R-P4 · 领域语义不可混用
- **等级**：MUST
- **分类**：命名、分层
- **规则**：
  - Files 的资源 CRUD/Trash/Watch/Search 与内容编辑能力分离。
  - Apps 使用 App / RuntimeSpec / RuntimeInstance / Surface / PackageSpec（ADR-0025 D1）；只有一个 Apps Registry；托管扩展包是 Natives 的组成部分而非独立桌面应用，包内模块（持仓、账本、NAV、导入等）不分别拥有独立安装或产品身份；App 不是 Widget，运行不等于呈现；托管扩展包由受控本机实现承载，UI 随扩展包交付于隔离沙箱呈现，业务代码禁止在扩展上下文中执行（ADR-0027 取代 ADR-0025 D2/ADR-0026 构建期内置 UI 限制）。
  - Provider = 厂商；Connection = 真实 upstream；Credential = 独立可轮换凭证；API protocol 不是 Provider。
  - Proxy = 个人本地轻量代理；**禁止**建设企业 AI Gateway、租户计费或通用控制/数据平面。
  - Claude Code、Codex、Gemini CLI、OpenCode = AI Tool Integration。
  - Usage/Analytics 复用现有采集与聚合；**禁止**建设统一 Event Platform。
- **为什么**：稳定术语让数据模型、界面和代码所有权保持一致。

#### R-P5 · Native Backend 按当前产品范围划分
- **等级**：MUST
- **分类**：分层、进程
- **规则**：当前 Chrome/Chromium 产品遵循 technical/01 的 Files / Model / 官方 App Host 分工；Tauri 仅为历史架构，不得恢复。业务所属子应用保持独立，不因统一套件而并入 Core。只有真实独立生命周期或隔离需求才允许额外 Sidecar，并必须有监督与退出契约。**禁止**新建 Agent Runtime、Harness、Planner、Subagent Runtime、Capability Gateway、Jobs 自主任务系统或 Plugin Runtime。
- **为什么**：个人桌面产品不需要为内部逻辑增加进程、协议和第二数据 authority。

## 四、复用与诚实能力

#### R-P6 · 复用优先且只有一个 Source of Truth
- **等级**：MUST
- **分类**：分层、数据
- **规则**：实现顺序**必须**为复用 > adapter/wrapper > 职责拆分 > 小改 > 新建。成熟 Files CRUD、App lifecycle、Provider codec、Usage parser、UI tokens **必须**先审计再决定迁移。**禁止**长期保留新旧双 production path 或复制 Source of Truth。
- **为什么**：重构目标是降低总复杂度，不是把旧系统换名复制。

#### R-P7 · 不得伪造完成度
- **等级**：MUST
- **分类**：无假数据、错误处理
- **规则**：未通过协议、迁移、生命周期或平台 Gate 的能力**必须**标记为 partial/pending/unsupported；**禁止**用静态模型、假延迟、默认零值、伪完成事件或浏览器 Spike 冒充生产可用。
- **为什么**：个人 Workspace 管理真实文件、进程、凭证与成本，错误状态比缺功能更危险。

## 五、Legacy 迁移纪律

#### R-P8 · 旧系统只允许迁移与删除工作
- **等级**：MUST
- **分类**：版本、分层
- **规则**：Assistant、Agent、Jobs、Capabilities、Daemon、Plugin Runtime 旧路径只允许安全修复、兼容迁移、引用清理和删除阻断修复。生产切换**必须**有 parity、rollback 与 death proof；迁移完成后删除旧入口、旧调用与旧测试，不保留静默 fallback。
- **为什么**：继续给 legacy 加功能会让最终删除无限后移。

## 六、合规自检

- [ ] 新入口属于 Home / Files / Apps / AI / Data & Usage / Settings。
- [ ] 没有新增 Workspace、Widget runtime、Agent/Capability/Job runtime。
- [ ] Provider / Connection / Credential / Proxy / AI Tool 术语使用正确。
- [ ] Host 是默认 owner；Sidecar 有真实生命周期理由且被监督。
- [ ] 用户可见数据有真实来源，未用 partial 冒充 complete。
- [ ] 新旧生产路径没有双执行、双写或静默 fallback。
