# 产品架构 01 · 定位与边界

> **版本**: 3.0.0 · **日期**: 2026-08-19
> **关联 ADR**: [ADR-0020](../../adr/0020-ai-native-personal-workspace-rearchitecture.md)（当前产品与架构冻结）、[ADR-0007](../../adr/0007-domain-wheel-reinvention-clarification.md)（复用原则）
> **取代**: ADR-0012 派生的三面/双轨与 Workshop 优先规则

## 一、产品身份

#### R-P0 · 产品身份唯一表述
- **等级**：MUST
- **分类**：命名、产品
- **规则**：产品身份**必须**表述为：

  > **AiNative = AI Native Personal Workspace。**

  产品是稳定、长期可维护的个人数字桌面，不是 Agent Runtime、AI Gateway、固定 Dashboard、低代码平台或插件市场。
- **为什么**：单一身份决定 IA、领域所有权与删除范围，避免历史能力继续反向定义产品。

#### R-P1 · 一级 IA 固定
- **等级**：MUST
- **分类**：产品、交互
- **规则**：一级入口**必须**为：首页、文件、应用、AI、数据与用量、设置。AI 下分 AI Resources、Local Proxy、AI Tool Integration。**禁止**新增 Workspace 一级菜单；V1 只有一个 Home。
- **为什么**：这是用户任务组织方式，不以内部 runtime/技术名暴露产品结构。

## 二、Home 与 Widget

#### R-P2 · 首页就是 Personal Workspace Home
- **等级**：MUST
- **分类**：产品、状态
- **规则**：根路由 `/` **必须**是可配置 Home。Widget **必须**是内置 React renderer + config + versioned grid layout；**禁止** Widget Plugin Framework、Runtime、Event Bus、Worker、Marketplace、Infinite Canvas 与多 Workspace 数据域。
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
  - Apps 使用 App / RuntimeSpec / RuntimeInstance / Surface；App 不是 Widget，运行不等于呈现。
  - Provider = 厂商；Connection = 真实 upstream；Credential = 独立可轮换凭证；API protocol 不是 Provider。
  - Proxy = 个人本地轻量代理；**禁止**建设企业 AI Gateway、租户计费或通用控制/数据平面。
  - Claude Code、Codex、Gemini CLI、OpenCode = AI Tool Integration。
  - Usage/Analytics 复用现有采集与聚合；**禁止**建设统一 Event Platform。
- **为什么**：稳定术语让数据模型、界面和代码所有权保持一致。

#### R-P5 · Tauri Host 是默认 Native Backend
- **等级**：MUST
- **分类**：分层、进程
- **规则**：本机领域能力默认归 Tauri Rust Host。只有真实独立生命周期或隔离需求才允许 Sidecar，并必须由 Host 监督。**禁止**新建 Agent Runtime、Harness、Planner、Subagent Runtime、Capability Gateway、Jobs 自主任务系统或 Plugin Runtime。
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
