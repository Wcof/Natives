# Natives 现行规范、性能与内存回收一体化整改方案

> 审计日期：2026-09-13  
> 实施对象：当前生产架构，即 Chrome/Chromium Extension、`native-file-host`、
> `file-manager-core`、单用途 `model-host` 和随 Natives 整包交付的内置模块。  
> 规范基线：[`../standards/technical/04-performance.md`](../standards/technical/04-performance.md)。

本方案把规范收敛和性能整改作为同一次交付：先删除会误导实现的旧架构规则与旧生产链，
再按新规范修性能和内存回收。不得只完成其中一半。

## 1. 结论

当前应用**需要做定向优化和补齐验证**，但没有证据支持重写架构或引入新的性能框架。

已经确认正常的主链如下：

- `native-file-host` 随 Native Messaging stdin EOF 退出，Watch 与后台任务在退出时取消并等待回收；
- 内置模块 owner page 隐藏 60 秒后停止模块并断开 Host，`pagehide` 和显式销毁也会清理；
- AI Widget 共享一个 Model Host 连接，最后一个 Widget 销毁后释放连接；共享查询缓存上限为 32；
- Workspace 重绘和销毁会调用 Widget/背景 disposer，现有常见定时器都有对应 `clearInterval`；
- Model Host 未开启常驻时随最后连接退出；只有用户显式开启“后台常驻”后才保留 worker，现有生命周期测试通过。

需要整改的核心问题有五类：

1. Model Host 首次请求出现明显抖动，当前单次 debug 门禁不能给出可信结论；
2. 还没有真实 Chrome 下 30 分钟反复导航、开关模块和增删 AI Widget 的 RSS 增长证据，不能宣称“无内存泄漏”；
3. `extension/app.js` 重复注册了同一个主题监听器，部分时钟 Widget 在页面隐藏后仍保留自己的 interval；
4. Native Host 和 Extension 的性能脚本存在覆盖缺口，可能使用陈旧二进制，且没有真正验证全部产品路由和空闲 CPU。
5. 项目与用户级共享规范仍混有已删除架构的 MUST，正式门禁中也仍运行旧 Catalog/模块下载检查。

整改原则：先让门禁能稳定复现问题，再修根因；预算内且没有增长趋势的代码不做“预防性重构”。

## 2. 本次实测基线

以下数字来自同一台 Apple Silicon/macOS 开发机、当前工作树。它们是本地审计证据，
不等同于签名、公证后的 Release 用户验收。

| 对象 | 本次结果 | 预算/判断 | 状态 |
|---|---:|---:|---|
| Extension 分发体积估算 | 317,830 B | 368,640 B | PASS，余量 50,810 B |
| `native-file-host` Release 文件 | 1,913,472 B | 4 MiB 过渡预算 | PASS |
| `native-file-host` RSS | 8,256 KiB | 12 MiB | PASS |
| `native-file-host` EOF 退出 | 8 ms | 2 s | PASS |
| 10,000 项目录首 100 项查询 p95 | 43.98 ms | 50 ms | PASS，但余量较小 |
| Model Host 首次快照，第 1 次 | 2,712.67 ms | 1,500 ms | FAIL |
| Model Host 首次快照，紧接复测 | 197.98 ms | 1,500 ms | PASS，说明冷态抖动未被解释 |
| Model Host 空闲 RSS | 65,264–66,032 KiB | 当前脚本 256 MiB | PASS |
| Model Host 空闲 CPU | 0% | 当前脚本 5% | PASS |
| Model Host EOF 退出 | 2.79–3.14 ms | 1 s | PASS |
| Model Host 常驻开/复用/关 | 全部成功 | 显式开启才可常驻 | PASS |

进程快照还观察到一个 PPID 为 1、运行约一天的 debug Model Host worker。它符合当前
“用户显式开启后台常驻”后的设计，不能据此判定泄漏；最终验收必须分别覆盖常驻关闭和开启两种模式，
并确认设置页能准确显示当前状态。

本轮没有得到以下证据，因此状态保持 `unknown`：

- Chrome Renderer 的总 RSS、单页面内存和 30 分钟增长比例；
- 长时间增删 Widget 后监听器、interval、Native Port 的最终数量；
- 基金模块反复打开/隐藏/恢复/关闭后的进程与 loopback 端口归零；
- 50 万条 AI 用量事件下的查询 p95、峰值内存与取消后的释放；
- Windows 实机的进程、CPU、RSS 与安装后运行表现。

## 3. 旧规范与旧生产链审计

### 3.1 判断

当前规范体系尚未完成架构切换。ADR-0020、ADR-0027 和 ADR-0029 已经冻结了新架构，
但 `docs/standards/` 的 16 个 Markdown 文件里仍出现 Tauri、Next.js、React、WebView、
Daemon、Sidecar、Catalog、`.nap` 和旧源码路径。部分内容只是历史说明，部分却仍是 MUST。

更严重的是旧语义仍进入正式门禁：

- `extension:check` 仍执行 `app-download.test.mjs` 和 `catalog-client.test.mjs`；
- `apps:check` 仍执行 `apps:catalog:check`；
- Extension 中仍有 `app-download.js`、`catalog-client.js`、`app-catalog-policy.js` 和内置 Catalog 文件；
- `native-file-host` 仍保留 `apps:install_begin/chunk/finish/commit`、Suite Seed、`.nap` 和 Catalog v3 事务代码；
- `scripts/apps/` 仍有 seed、独立基金 Release、publish 和 demo package 脚本。

这些内容与“一个 Natives 完整安装包、内置模块不独立下载/安装/升级”的现行规则冲突。
规范整改不能只换词，必须同步删除旧生产调用、门禁和静默 fallback。

### 3.2 淘汰策略

旧规范不复制到 `docs/standards/archive/`。`docs/standards/` 内的每一段都会被 Agent 当成现行权威，
保留“历史 MUST”会继续产生错误实现。历史由以下三处保存：

1. Git 历史；
2. 已标记 superseded 的 ADR-0001–0019、ADR-0025、ADR-0026 等决策正文；
3. 更新后的 `docs/architecture/legacy-death-list.md`，只记录路径、替代物、删除证据和日期。

执行时按三类处置：

| 类别 | 条件 | 动作 |
|---|---|---|
| 现行原则 | 与 ADR-0020/0027/0029 和当前源码一致 | 改写为当前路径、当前进程和当前术语 |
| 历史原因 | 仍需解释为何禁止恢复，但不再约束实现细节 | 只在 ADR/legacy death list 保留，Standards 留一句禁止恢复 |
| 已删除实现细节 | Next/Tauri/Daemon/Plugin Runtime/独立模块分发的目录、API、预算和组件要求 | 从 Standards 直接删除 |

现行 MUST 的真正放宽仍须先补 ADR。本轮只是落实已经接受的 ADR；如果实施中发现某条现行安全、
数据或发布约束也需要放宽，必须停止该项并单独提交 ADR，不得借“清理旧规范”绕过。

### 3.3 逐文件目标

| 文件 | 当前问题 | 新规范目标 |
|---|---|---|
| `standards/README.md` | 文件地图仍称官方应用独立交付；Legacy 仍像迁移中 | 明确 Extension + 三类 Native Host + 单一完整产品；任务地图改指内置模块标准 |
| `standards/00-glossary.md` | 进程仍定义为 Tauri/Renderer/Sidecar；App 称子应用 | 定义 Extension Page、Files Host、Model Host、Built-in Module Host、Launcher；App 仅保留内部技术身份 |
| `product/01-positioning.md` | Widget 写成 React；仍允许泛化 Sidecar | 改为构建期内置普通 JS renderer；只允许 ADR 明列的 Host，不给未来 Sidecar 留空白授权 |
| `product/02-feature-spec.md` | Home Gate 要求 packaged Tauri/WebKit；death proof 含 WebView/Daemon | 改为 packaged Chrome/Chromium + Native Messaging + 内置模块真实路径 |
| `technical/01-layering.md` | 主体仍保留 Tauri authority 表、Renderer/UDS/Sidecar 分层 | 从当前四条链重写：Extension→Files Host、Extension→Model Host、app.html→App Host、Launcher→Chrome/指南 |
| `technical/02-security.md` | Tauri supervisor、终端 Token、FOUC window、Child WebView、Catalog 下载规则混杂 | 保留 Keychain、Native Messaging、iframe handshake、CORS、路径和产品签名；删除旧窗口/终端/下载链 |
| `technical/03-data.md` | 关联 `src-tauri`，数据 authority 仍按旧 Host 描述 | 明确 natives.db/Files、workspace、Usage、各模块业务库的唯一 owner、迁移与用户数据保护 |
| `technical/04-performance.md` | 主窗口/WebView/Daemon 预算、child WebView LRU、`.nap` wire 预算仍生效 | 改为 Chrome Renderer、Files/Model/App Host、完整安装包；并吸收本文 M0–M5 的稳定预算 |
| `technical/05-backend.md` | 几乎整篇以 Tauri command、Daemon protocol、sidecar supervisor 为中心 | 重写为 Rust/Go Native Host 的帧协议、结构化错误、锁、阻塞 IO、子进程/EOF 和模块规模 |
| `technical/06-sub-apps.md` | 文件名和正文仍大量使用独立托管包、Catalog、安装事务 | `git mv` 为 `06-built-in-modules.md`，只保留整包交付、运行隔离、数据和产品级更新规则 |
| `frontend/01-structure.md` | 整篇是 Next App Router/React/src 目录 | 改为 `extension/` HTML 薄入口、页面 controller、domain client、build-time widget/background、测试同位规则 |
| `frontend/02-state-and-data.md` | ThemeContext/useState/Hook/tauri-adapter/invoke | 改为单页 controller 状态、`NativeClient`/领域 client、Chrome storage 派生、disposer 与事件去重 |
| `frontend/03-i18n.md` | 仍引用旧 `src/i18n` 路径 | 以 `extension/_locales/zh_CN` 和 `extension/_locales/en` 为唯一 UI 文案路径 |
| `ui-ux/01-design-tokens.md` | Tauri 透明窗口、Zod theme、liquid-glass-react、lucide-react | 以 Extension CSS/空间局部 token 为准；删除框架专属要求，SVG 图标不绑定不存在的依赖 |
| `ui-ux/02-interaction.md` | 无边框桌面拖动和 react-grid-layout 仍为 MUST | 删除 `-webkit-app-region`；保留当前 Grid/Canvas 交互语义并指向实际 `space-*` 实现 |
| `ui-ux/03-feedback.md` | design-tokens.ts、useFocusTrap、chime.ts 路径不存在 | 改为 CSS motion token、现有模态焦点工具和浏览器可访问性；删除不存在的提示音实现约束 |

用户级共享规范 `/Users/ldh/.claude/standards/` 也需要同步收敛。它目前虽然声明 Natives 项目规则以
仓库为准，正文仍复制了 Hub/Workshop、Tauri/Daemon、`tauri-adapter`、`lucide-react` 等旧规则。
处理方式不是再复制一次新版 Natives Standards，而是：

- `README.md` 保留通用规范入口和“项目规范优先”；
- `technical.md`、`frontend.md`、`product.md`、`uiux.md` 只保留跨项目通用原则；
- 所有 Natives 专属规则改成链接到仓库 `docs/standards/`，避免第三套权威；
- 删除“三面/双轨、Workshop、Daemon authority、Tauri IPC”等历史产品摘要。

### 3.4 规范收敛验收

新增一个使用 Node 标准库的 `scripts/check-standards-current.mjs`，接入现有
`scripts/perf/architecture-check.mjs` 或 `package.json` 的 `standards:check`，检查：

- `docs/standards/` 内不存在活动的 `src/`、`src-tauri/`、`tauri-adapter`、Next.js、React、
  react-grid-layout、liquid-glass-react、lucide-react、useFocusTrap、Child WebView、`.nap`、
  `install_chunk`、Suite Seed 或 Catalog v3 规则；
- Markdown 相对链接有效，关联源文件真实存在，规则 ID 在同一篇内唯一；
- `06-sub-apps.md` 不存在，所有活动引用都指向 `06-built-in-modules.md`；
- `legacy-death-list.md` 的每个删除项都有替代路径、全仓引用结果和验证命令；
- 全局共享规范不再复制 Natives 特定框架或产品结构。

允许在 ADR、legacy death list 和 Git 历史中保留上述词语；门禁只扫描当前权威 Standards 和
生产入口。这样既能追溯旧决策，也不会让 Agent 把旧规则当作实施要求。

## 4. 已确认性能问题与处置优先级

| 优先级 | 问题 | 证据 | 处置 |
|---|---|---|---|
| P0 | Model Host 冷启动门禁不稳定 | 同一工作树 2.71 s → 198 ms | 改成 Release、多样本、分阶段测量；定位后只延迟非快照必需初始化 |
| P0 | 缺真实 Chrome 长循环内存证据 | 当前脚本只测独立 Host | 建立现有浏览器 Harness 上的 30 分钟生命周期场景 |
| P0 | Native Host 门禁可能复用陈旧 Release 二进制 | 仅比较 `main.rs` 与二进制 mtime | 始终执行增量 `cargo build --release`，由 Cargo 判断是否重建 |
| P0 | Native Host 空闲 CPU 标为 unsupported，但总门禁仍可 PASS | 报告中未参与 `ok` | macOS/Linux 加入 60 秒 CPU 采样；Windows 单独实现或明确阻断平台门禁 |
| P1 | `app.js` 同一主题变化监听注册两次 | 源码存在两段相同 `onChanged.addListener` | 删除重复注册，并加一个监听次数回归检查 |
| P1 | 时钟类 Widget 隐藏后仍保留独立 interval | time/binary/countdown/since/work-hours | 隐藏时暂停、恢复时立即刷新；优先复用一个页面级时钟，不增加依赖 |
| P1 | Model Host 初始化同步打开 Usage DB 并读取 Keychain | `NewEngine` 在快照前完成全部 usage 初始化 | 先用阶段计时确认占比，再将 Usage/Key 解析延迟到首次相关调用或代理启动 |
| P1 | Extension 门禁测的是整包估算，不是每个入口的首屏依赖 | 当前结果只有一个 aggregate 数字 | 输出 newtab/files/apps/app/model-settings 各入口 gzip 与共享依赖 |
| P2 | AI 共享查询订阅保持页面级监听 | 订阅仅注册一次、缓存上限 32 | 当前有界，不改；只有 soak 证明页面存活期持续增长才增加 owner 计数 |
| P2 | 10k 目录 p95 距预算约 6 ms | 五个样本 p95 43.98 ms | 先扩大样本确认；未超过 50 ms 不改查询实现 |

## 5. 目标生命周期

完成后必须满足下面的资源关系：

```text
打开 newtab/空间
  └─ 只为可见 Widget 保留渲染工作
     ├─ 页面隐藏：时钟、轮播、刷新暂停
     └─ 最后一个 AI Widget 销毁：Model Native Port 断开

打开文件页
  └─ 页面持有唯一 Files Native Port
     ├─ 隐藏且无任务 60 秒：断开
     └─ pagehide/关闭：断开 → Host EOF → 2 秒内退出

打开内置模块
  └─ owner page → app Host → loopback iframe
     ├─ 隐藏 60 秒：stop + iframe remove + port disconnect
     └─ pagehide/关闭：进程、端口、timer 全部归零

打开模型/AI 用量能力
  └─ Model Host
     ├─ resident=false：最后连接关闭后退出
     └─ resident=true：只保留一个 worker；关闭设置后退出
```

禁止为了性能增加后台 daemon、Service Worker Native Port、轮询 keepalive、第二套缓存或新的应用运行链。

## 6. 实施工作包

### S0：冻结规范权威和旧链清单

1. 以 ADR-0020、ADR-0027、ADR-0029、AGENTS.md 与当前生产入口建立一张唯一映射表。
2. 对每条待删除 MUST 记录“来源 ADR、为何已被取代、替代规则”；未找到取代依据的不得删除。
3. 冻结全仓旧链引用清单，覆盖文档、Extension、Rust、脚本、package scripts、fixture 和 Release 流程。
4. 把 `legacy-death-list.md` 从 2026-08-20 的迁移中快照更新为当前实际状态。

### S1：重写仓库 Standards v4

按第 3.3 节逐篇重写全部 16 个 Standards 文件；不是搜索替换框架名称。先写产品与术语，
再写进程/安全/数据/性能/后端，最后写前端和 UI，确保下游规则只引用已经确定的上游概念。

`technical/06-sub-apps.md` 改名为 `technical/06-built-in-modules.md`；更新 README、ADR 链接、
契约链接、脚本注释和实施文档引用。Superseded ADR 正文保留历史结论，只修断链和顶部状态说明。

### S2：同步用户级共享规范

以仓库 Standards 为 Natives 唯一权威，将 `/Users/ldh/.claude/standards/` 收敛为通用工程规则和
项目链接。实施前保存可回滚 diff；同步后分别从 Claude Code 与 Codex 加载入口，确认不会再注入
Tauri/Daemon/Workshop/Next/React 专属要求。

### S3：删除旧模块分发生产链

删除或改写前必须先确认所有 caller。目标结果：

- 删除 Extension 的 app download/catalog client/policy、Catalog 文件和对应测试/i18n；
- 从 Core protocol/dispatch 删除模块 install begin/chunk/finish/commit/abort 的生产可调用入口；
- 删除 Suite Seed、seed reconciliation、独立 `.nap`、独立 fund release/publish 路径；
- 产品级完整组合清单继续复用签名、hash、原子激活和恢复原语，但改成 product/suite 语义；
- App Center 只查询内置模块真实状态并提供打开、显示隐藏、偏好和数据管理；
- 独立 sample 只保留为 `app-host-support` 的低层安全/协议 fixture，不进入产品 Catalog 或安装体验。

不得只让旧 IPC 返回 unsupported 后长期保留全部旧实现。完成 product-level 替代并迁移现有状态后，
删除旧 handler、types、schema 字段、测试和脚本；确需读取一次的旧数据只留有版本边界的迁移器。

### S4：建立规范和架构防回归门禁

实现第 3.4 节 `standards:check`，并将它放入 `extension:check` 之前或统一 CI 的最早阶段。
同时更新 `architecture-check.mjs` 的登记例外，移除 `app_store/install.rs`、旧 catalog 测试等已删除路径。

S0–S4 退出条件：当前 Standards 只描述新架构，旧分发生产入口为 0，历史仍可从 ADR/Git/death list 追溯。

### M0：先修性能门禁

修改现有脚本，不新增测试框架：

- `scripts/perf/check-model-host.mjs`
  - 使用 Release 二进制；
  - 至少采集 5 个隔离冷启动样本，报告 p50/p75/p95，按 p75 ≤ 2.5 s 判定；
  - 单列进程出现、Engine 初始化、首帧解析、EOF、RSS 和 60 秒空闲 CPU；
  - 保留 resident on → relay 复用 → resident off 的现有测试；
  - 每个样本用独立临时配置并保证所有测试 worker 在 `finally` 中回收。
- `scripts/perf/check-native-host.mjs`
  - 每次先调用 `rtk env -u CARGO_TARGET_DIR cargo build -p native-file-host --release`；
  - 将 stdout 改为一个帧解析器和 pending map，响应完成后删除 pending，不为每个请求叠加 `data` listener；
  - 对 10k 目录采集至少 20 个计时样本并报告 p50/p95；
  - 加入 60 秒空闲 CPU 采样，不能再以 `unsupported` 通过目标平台门禁。
- Extension 体积门禁
  - 从 manifest/HTML/module import 图生成各产品入口的首屏依赖集合；
  - 分别报告 gzip 后 JS/CSS，并继续保留整包上限；
  - 入口至少包含 `newtab.html`、`files.html`、`apps.html`、`app.html` 和模型设置入口。

M0 退出条件：同一 HEAD 连续执行得到可解释的稳定结果；失败报告能指出是启动、初始化、查询、
空闲或退出阶段，不再只有一个总耗时。

### M1：修 Model Host 冷启动根因

涉及：`model-host/internal/host/engine.go`、Usage Store 初始化和相应定向测试。

1. 给 `NewEngine` 的现有步骤加测试态阶段计时，确认 Repository、Keychain、SQLite、Usage plugin 各自占比；完成定位后不保留高频详细日志。
2. `model_snapshot` 只依赖配置快照，不应等待 Usage DB、价格表、导入器和全部 gateway secret 预热。
3. 将 Usage Store/Calculator/Importer 延迟到第一次 usage 查询、采集或代理启动；并发首次访问用 Go `sync.Once` 或现有互斥模式保证只初始化一次。
4. gateway key 映射在第一次需要鉴权时加载；密钥新增、轮换、删除后沿用现有显式刷新，Secret 仍只在 Keychain 和 Host 内存中。
5. 代理启动前必须完成 usage plugin 注册，不能以漏计 Token/成本换取启动速度。
6. 初始化失败返回现有明确错误，不允许把 Usage 不可用伪装成空数据。

M1 验收：Release 冷启动 5 次 p75 ≤ 2.5 s；热连接首个快照 p95 ≤300 ms；
resident 两种模式、Usage 查询、Gateway 鉴权和 Keychain 安全测试全部通过。

### M2：清理 Extension 监听器和隐藏态工作

涉及：`extension/app.js`、时钟类 Widget、`space-dashboard.js` 及现有测试。

1. 删除 `app.js` 重复的主题监听注册；监听在 document 生命周期内只存在一份。
2. time、binary-time、countdown、since、work-hours 在 `document.hidden` 时停止 interval，恢复可见时先立即刷新，再恢复计时。
3. 多个可见时钟 Widget 复用一个页面 tick 源；不同精度只在回调中判断 1 秒/10 秒，不创建 N 个永久 interval。
4. 保持现有 disposer 调用顺序：旧 Widget disposer → 清容器 → 新 render；页面 destroy 后 tick subscriber、背景 timer、AI event subscriber 均为 0。
5. 不给用户启动的番茄钟偷偷丢状态：隐藏时记录剩余目标时间，恢复时按墙钟重算；销毁仍清理 timer。
6. AI 请求取消先不增加 Host 协议。现有查询去重、32 项缓存和 30 秒超时保持；只有 M4 soak 证明未决请求堆积时，才增加同一 Native Port 内的 cancel method。

M2 验收：反复新增/删除同类 Widget 100 次后，页面级 visibility/theme/tick 监听数回到基线；
页面隐藏 60 秒无 Widget interval 唤醒；恢复后的时间、倒计时和工作时长正确。

### M3：补齐 Host 与内置模块回收

涉及：现有 Files Host、`extension/app.js`、`app-host-support` 和真实基金模块，不新增运行时。

- Files：覆盖正常关闭、隐藏超时、浏览器崩溃/断开、Watch 活跃、导入取消五条路径；最终都由 stdin EOF 或显式取消进入同一 shutdown。
- 模块：循环执行打开 → 握手 → 隐藏 60 秒 → 恢复 → 再打开 → 关闭；每轮核对 iframe、Host 子进程、loopback 监听端口和 session token 清零。
- Model：resident=false 时关闭最后一个设置页/AI Widget 后进程退出；resident=true 时只允许一个 worker，关闭常驻后 2 秒内退出。
- 所有退出检查按 PID 和启动身份核对，不能误杀同名进程；测试失败时也必须回收自己创建的子进程。

M3 验收：三类 Host 的非驻留路径退出 ≤2 秒；停止后的进程、端口、timer 为 0；重复 100 次不产生僵尸进程。

### M4：验证大数据与内存增长

复用已有 Usage fixture、Files 临时目录和浏览器测试能力：

- AI 用量：1 万、10 万、50 万事件；测 overview、analysis、sessions、events、billing、ledger；记录 p50/p95、Host 峰值 RSS、查询完成 60 秒后的 RSS。
- 文件：1 万和 10 万目录项；只渲染可视窗口，测首屏、滚动、搜索、Watch 风暴和关闭释放。
- 空间：首页放置 20 个 Widget，其中至少 7 个 AI 效能卡片和 5 个时钟类卡片；循环切换空间、修改筛选、增删卡片。
- 模块：基金使用代表性真实数据，循环打开/隐藏/关闭。

Chrome 真实生命周期场景持续 30 分钟，至少每 10 秒采集：Renderer/Extension/各 Host RSS、CPU、
进程数、Native Port 数和可观察 timer/listener 数。先预热 5 分钟，以预热后稳定值为基准：

- 30 分钟循环结束并空闲 60 秒后，总 RSS 增长 ≤15%；
- 空闲 60 秒 CPU ≤2%；
- 单次交互 p95 ≤100 ms，缓存页面切换 ≤300 ms；
- 主线程任务 ≤50 ms；可见动效 ≥55 FPS；
- Files Host RSS ≤12 MiB，10k 目录 p95 ≤50 ms，EOF ≤2 s；
- Managed App 冷启动 p75 ≤2.5 s，停止后资源归零；
- Model Host 使用 M1 的预算，并分别报告 resident=false/true。

如 RSS 在 GC 后形成稳定平台且增长 ≤15%，判定通过；仅因系统缓存或一次峰值上涨不得直接判为泄漏。
超过预算时，先用增长对象/进程/句柄证据定位 owner，再修改代码。

### M5：整体验收与回归

开发阶段只跑覆盖改动的最小检查。集成完成后，在同一 HEAD、同一设备、Release 构建执行一次：

```sh
rtk npm run extension:check
rtk npm run perf:check
rtk npm run perf:files
rtk env -u CARGO_TARGET_DIR cargo fmt --check
rtk env -u CARGO_TARGET_DIR cargo test --workspace
```

另执行 M4 的 30 分钟真实 Chrome soak，并把机器型号、系统/Chrome 版本、构建类型、数据集、
命令、原始样本、p50/p75/p95、前后 RSS 和失败项写回本文。Windows 的平台结果单列，不能用 macOS 结果代替。

## 7. 统一交付顺序

Agent 必须在一个整改分支中按以下顺序实施：

1. S0 冻结当前权威、旧规则和旧代码引用基线；
2. S1 重写仓库 Standards，完成 `06-built-in-modules` 改名；
3. S2 收敛用户级共享规范；
4. S3 删除 Catalog/`.nap`/Suite Seed/独立发布生产链；
5. S4 建立规范防回归门禁；
6. M0 修性能门禁并冻结可比较的 before 数据；
7. M1 修 Model Host 冷启动；
8. M2 修 Extension 监听器和隐藏态工作；
9. M3 验证 Files/Model/内置模块生命周期；
10. M4 执行大数据和 30 分钟真实 Chrome soak；
11. 只对超预算且已经定位的路径继续优化；
12. M5 在同一 HEAD 执行完整门禁并回写证据。

不得先改阈值、关闭 Gatekeeper、取消安全校验或把 resident 默认打开来掩盖性能问题。

## 8. 完成定义

只有同时满足以下条件，才可以声明“规范、性能和内存回收符合预期”：

- S0–S4 全部完成，`standards:check` 通过；
- 仓库与全局共享规范不再包含会驱动旧架构实现的活动 MUST；
- Catalog/`.nap`/Suite Seed/模块独立发布的生产 caller、门禁和静默 fallback 为 0；
- 历史决策在 superseded ADR、Git 与更新后的 death list 中可追溯；
- M0–M5 全部完成；
- 所有适用预算在 Release 构建下通过；
- 30 分钟真实 Chrome 循环增长 ≤15%，且空闲后非驻留资源归零；
- Model Host 冷启动抖动已由分阶段数据解释并解决；
- 常驻开关、Files EOF、基金模块隐藏/关闭三条真实路径均有 PID/端口证据；
- 没有新增运行时、轮询 keepalive、性能依赖或第二套缓存；
- 报告明确区分 PASS、FAIL、BLOCKED、UNSUPPORTED，不把缺失证据写成完成。

## 9. 历史说明

本文此前记录的 Tauri/Next/通用 Daemon 性能数据对应已删除的旧生产架构，仅有历史参考价值，
不再作为当前 Natives 的验收依据。当前结论只以本文件第 2 节基线和 M0–M5 新证据为准。
