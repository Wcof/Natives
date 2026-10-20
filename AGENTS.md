# Natives agent instructions

本文件是项目智能体执行入口。默认中文沟通，代码与文件命名沿用项目规范。
`CLAUDE.md` 仅引用本文件；开发规范正文只在 `docs/standards/` 维护。

## 执行流程（MUST）

1. **先核对任务与工作区**：读取本文件、`ROADMAP.md`、`docs/README.md`，检查
   `git status` 及任务相关 diff。使用现有方案的最新实施入口，不凭旧提示词、
   未跟踪文件或历史绿测试判断当前状态；保留其他人的改动。
2. **先读适用规范再编码**：读取 `docs/standards/README.md`，按下表读取涉及领域的
   规范正文与当前契约／ADR。列出路径、看到 `@` 引用或只读索引不等于已读正文；
   缺文件先查实际位置，不凭记忆编造要求。已读且本次未变化的内容不机械重复读取。
3. **给出可验证的实施说明**：首次修改前用简短说明交代目标、修改范围、数据／
   生命周期 owner、需要保持的接口及验证方式。跨模块、迁移或完整功能任务在
   现有方案中维护需求→实现→验收对应；简单修改无需新建模板文档。
   已明确授权的工作直接实施，说明计划不等于再次请求批准。
4. **按规范实现并检查**：先复用既有模块与测试，审计相关调用方；输入校验、
   事务／版本、取消／清理、错误状态、i18n 与 UI tokens 一起实现。
   非法输入、边界和失败路径纳入相关检查，不能只让主流程截图正常。
5. **验证后交付**：完成下文适用检查，核对实际 diff、生成资源和用户路径，
   同步受影响文档。报告区分已实现且已验证、已实现未验证、未实现及阻塞；
   未运行或失败的检查如实记录，不能宣称完整完成。

### 按任务读取规范

下表路径均相对 `docs/standards/`；跨领域修改读取所有涉及行，不以「只读三篇」
跳过安全、数据或生命周期要求。规范之外的契约入口使用 `docs/README.md`。

| 修改范围 | 必读正文 |
| --- | --- |
| 产品功能、页面结构、完成声明 | `product/01-positioning.md`、`product/02-feature-spec.md` |
| 跨域依赖、Native 协议、Host／Runtime | `technical/01-layering.md`、`technical/02-security.md`、`technical/08-module-boundaries-and-contracts.md` |
| SQLite、持久化、迁移、金额／状态权威 | `technical/03-data.md`、`technical/05-backend.md` |
| Extension／模块前端、异步状态、语言 | `frontend/01-structure.md`、`frontend/02-state-and-data.md`、`frontend/03-i18n.md` |
| UI、样式、图表、交互、键盘／焦点 | `ui-ux/01-design-tokens.md`、`ui-ux/02-interaction.md`、`ui-ux/03-feedback.md` |
| 内置模块、模块 UI、能力声明及打包 | `technical/06-built-in-modules.md`、`technical/07-built-in-app-development.md`、`technical/08-module-boundaries-and-contracts.md`，并读取上述安全／数据及前端规则 |
| 查询、timer、进程、资源及性能 | `technical/04-performance.md`；涉及端口再读 `technical/05-port-registry.md` |

本机全局约束入口是 `/Users/ldh/.codex/RTK.md` 与
`/Users/ldh/.claude/standards/README.md` 及其引用的术语／技术／前端／产品／UIUX
正文；实际读取适用内容，不依赖引用自动展开。项目技术决策仍以现行 ADR 和
`docs/standards/` 为准，不把本机偏好写成另一份项目架构。

## Source of truth

- Authority order: ADR-0031 (current highest architecture authority) >
  `docs/standards/` > other ADRs > `docs/architecture/` > root documentation.
  Accepted follow-up decisions apply within their explicit scope: ADR-0035
  retires the fixed App Runtime port; ADR-0036 defines investment-card ownership;
  ADR-0037 authorizes the generic module inference broker; ADR-0038 defines
  module Secret/view capabilities; ADR-0039 defines internal module boundaries
  without changing product or process boundaries. Do not treat these
  current decisions as historical merely because they are not ADR-0031.
  ADR-0042 adds the local Web business surface and shared product service
  within its explicit dual-entry scope.
  ADR-0043 permits only Host-validated PDF Blob previews to use Chrome's native
  reader without an iframe sandbox; HTML and business-module sandboxes remain unchanged.
  Superseded ADRs explain history; never restore their deleted production paths.
- A change that relaxes a MUST requires an ADR first. Do not silently work
  around it.
- Use the task map in `docs/README.md`; do not duplicate architecture or
  progress snapshots in new documents.

## Target architecture and current migration

- Current production code is limited to the Chrome/Chromium extension and
  ADR-0042 local Web surface,
  `crates/native-file-host`, `crates/file-manager-core`, the single-purpose
  `model-host` authorized by ADR-0020, and the unified product app runtime
  `crates/app-runtime` / `crates/app-runtime-core` (producing `natives-app-runtime`
  authorized by ADR-0031) with official built-in modules in `modules/` (such as
  `modules/fund`). `src/`, `src-tauri/`, `src-agent-daemon/`, Agent/Harness/Capability
  crates, Jobs, Assistant, and Plugin Runtime have been deleted; do not recreate them.
- **Unified built-in app runtime and monorepo modules (ADR-0031, 2026-09-14; supersedes
  per-app executable and per-app Native Host decisions in ADR-0027/0029)**: Natives is
  the single user-facing product. Fund and other official features are **built-in modules**
  whose source code lives in `modules/<appId>` within the Natives Monorepo and compiles
  statically into the single product-level `natives-app-runtime` binary. There is
  **no per-app executable (no fund-host)** and **no per-app Native Messaging Host registration
  (no com.natives.app.a<hash>)**; the OS Native Messaging manifest registers solely
  `com.natives.app_runtime` (or `com.natives.local.app_runtime` for local development).
  ADR-0042 requires one product-owned `natives-app-runtime` process per active module
  with independent Surface sessions for extension and local Web pages. The former
  direct per-page Native Port path is a migration gap, not the target architecture.
  Unselected modules consume zero dedicated memory (≈0 dedicated runtime memory). A
  Surface closing releases only its own session; after the last owner releases and no
  protected task remains, ADR-0042 grants a 60-second grace period. Worker shutdown or
  its private stdin EOF then exits completely in $\le 2$ seconds.
  Internal modules (portfolio, ledger, nav, import, storage, migration) belong to the module
  and do not have separate install records, card projections, or product identities.
  The Core App Store (`crates/native-file-host`) registers and verifies product manifest v2
  at product install/update time. User application roots (`~/.natives/apps/<appId>/`) strictly
  contain `activation.json`, `data/` (`fund.db`), `imports/`, `cache/`, `logs/` — no executables
  are placed in user directories.
  The generic `extension/app.html` uses Core Relay / authenticated Web RPC to select the
  module via App Runtime Protocol v2 (`app:handshake`), and hosts its UI in a restricted sandbox
  iframe over loopback 127.0.0.1. App Runtime binds `127.0.0.1:0`; the actual port
  is reported by `app:start`. Fixed port 8765 is retired under ADR-0035; do not
  restore fixed-port-first fallback or unauthenticated business APIs. See
  `docs/standards/technical/05-port-registry.md`.
  Modules must not write to `/Applications` or create
  independent `.app`, Dock, or LaunchServices entries.
- **Unified suite delivery (ADR-0029, 2026-09-11; converged 2026-09-12 to
  single-product built-in modules)**: the Natives installer is one complete
  product containing the thin launcher entry, the Chrome extension component,
  main Hosts, and all built-in modules (fund in the first complete candidate).
  First use of a module does data initialization only, as the current OS user;
  there is no offline seed/seed-reconciliation chain and no second download.
  The package installer never writes user DBs/activation or runs app
  migrations as root. A root-owned macOS system source under
  `/Library/Application Support/Natives/` holds main Hosts, the fixed unpacked
  Chrome extension, and built-in module files, not user data or a second App
  Registry. **Dev product-source revision (2026-09-17, ADR-0029):** the
  root-owned source is a release/installer-candidate requirement only. Everyday
  local iteration (module UI changes, rebuilding `natives-app-runtime`, payload
  SHA re-sealing, dev manifest re-signing) must stay in the user-writable
  `~/.natives-local/` namespace (product source defaults to
  `~/.natives-local/product-source/`, overridable via environment variable)
  and must not require sudo; dev tooling must not make sudo a prerequisite of
  routine iteration. Build the root-owned source only for installer-candidate
  verification (A-Local/B-Local installer acceptance). Verification rules
  (signature, payload hash/摘要) are unchanged in either layout.
  **Visible launcher revision (2026-09-13, explicit user decision):**
  the same installer must provide `/Applications/Natives.app` with a normal
  name, icon, and system application registration. Double-click opens Chrome;
  first use shows extension loading guidance and locates the bundled folder;
  an actual current-launch handshake hands off to the extension. This sole
  main-product `.app` and its brief native setup/diagnostic window are the
  precise ADR-0020/0029 exception. No module `.app`, Workbench, desktop business
  UI, forced Dock pinning, persistent launcher, or second install engine.
  Local development uses an isolated launcher name/id/path. Do not claim
  silent local extension installation in ordinary Chrome; show the required
  browser steps when automatic installation is unavailable. The launcher
  bundles an offline HTML guide that opens in Chrome before extension setup;
  the installer conclusion shows a static summary generated from the same
  content. The guide needs no installed extension, Native Bridge, business
  data, or local web server; it cannot report authenticated connection state.
  The launcher exits after handoff or cancellation. Reinstall/update/repair must
  preserve module display/hide preferences, disabled/removed choices, user
  data, and Keychain; implicit downgrades are rejected. Module business stays
  built with the product; internal modules are never separately installed.
- **Local and release gates**: A-Local must pass with the real built-in fund
  inside the complete product candidate before B0—B3; B-Local must pass before
  calling the local suite usable. Existing independent samples serve only as
  low-level test fixtures for shared logic. Local builds may use ad-hoc/development signatures only under the explicit,
  isolated non-production policy in the managed-app contract. Release rejects
  development keys/fixtures and still requires full A/B production evidence,
  platform signatures/notarization and separate publication authorization.
  Never call A-Local/B-Local full A-Gate/B-Gate, weaken sandbox/data controls,
  remove quarantine or disable Gatekeeper. Reuse the existing dev entry and
  single installation engine, not another production chain.
- The extension's `newtab.html` is static, while `files.html` directly owns its
  Native Messaging Port. The Host owns filesystem access and never exposes
  arbitrary paths, processes, SQLite, or Secret plaintext to the page.
- Service Worker code must remain stateless: no Native Port, polling, keepalive,
  or local service. Host cleanup is driven by Native Messaging stdin EOF.
- The extension page and authenticated ADR-0042 local Web page are the business
  surfaces; the main launcher may show only the setup/diagnostic window
  authorized above. Business-module iframes are only allowed for
  the ADR-0027 app sandbox exception (no `allow-same-origin`, no downloaded
  business JS in extension context); do not reintroduce WebView, React
  workspace, plugin, or runtime surfaces. Files PDF previews follow the narrowly
  scoped native-reader exception in ADR-0043; it does not apply to HTML previews.
- `model-host` is an AI-domain Native Messaging Host, not a general daemon. It
  may remain resident only after the user explicitly enables that setting; it
  must bind loopback, remain single-instance, keep Secrets in OS Keychain, and
  must not own Files capabilities or expose CLIProxyAPI management endpoints.
- Built-in module inference uses the generic Extension session broker and Model
  Host under ADR-0037. Module iframes never receive provider credentials.

## Change discipline

- Use `rtk` for shell commands.
- Run Cargo from the repository root with the ambient target override removed:
  `rtk env -u CARGO_TARGET_DIR cargo ...`. The checked-in `.cargo/config.toml`
  then keeps every agent on the current checkout's single `target/`.
- Never create or select another Cargo target directory (`.cargo-target-*`, a
  worktree-local target, or a temporary target). Never run `cargo clean` on the
  shared target unless the user explicitly requests it.
- Do not run Cargo in secondary worktrees; run their Rust verification once
  from the main checkout after integration so artifacts are reused.
- Activation integration fixtures hash and launch the same immutable test
  executable snapshot (`crates/app-runtime/tests/support`); do not hash the
  shared target binary and then launch a potentially rebuilt file. This is a
  fixture copy, not a second Cargo target or an exception to signature checks.
- Reuse existing domain modules, adapters, types, tokens, and test patterns
  before adding abstractions or dependencies.
- 修改模块 UI 的 `ui/src`，通过 `modules:ui:build` 生成 `ui/dist`；不能只改
  dist。核对 `ui.rs` 的编译期资源、HTML／脚本入口与签名产品清单的一致性。
- 正式 HTML 不复制静态 Demo 的内联业务脚本或模拟数据。Demo 必须与生产链
  分开，明确数据／推理为示例，不能作为真实数据、真实 AI 或发布验收证据。
- 新增动态文案、错误和 aria 文案使用现有 i18n；消费当前 Surface 的语义
  tokens、受控 SVG 及原生语义控件。主题、语言、空／错／缺数据状态与焦点
  随功能一起验证，不留到「功能做完再补」。
- 不通过提高预算、扩大例外名单、关闭规则、吞掉错误、固定日期夹具或修改
  断言来掩盖缺陷。修测试夹具先核对真实契约；必要例外按现有规范审查，不能
  只为变绿登记理由。规模检查入口是 `scripts/perf/architecture-check.mjs`。
- Keep user-visible data real, error states explicit, and extension locales in
  `extension/_locales/zh_CN` / `extension/_locales/en` synchronized.
- Preserve the applicable security defenses in
  `docs/standards/technical/02-security.md`; persistent Secret ownership is OS
  Keychain per ADR-0020/R-S12.
- Performance work MUST follow `docs/standards/technical/04-performance.md`
  and include comparable before/after evidence.
- Do not mix unrelated formatting, renames, generated output, or historical
  cleanup into a focused change.
- 任务临时文件只放 `.tmp/<task-slug>/`，正式交付物放约定目录；生产链不能
  依赖临时文件。默认不执行 add／commit／push、安装／发布或改真实用户数据。
  这些操作按用户明确授权范围执行，不能把修改代码扩展成外部发布授权。

## 文档与实施状态

- `AGENTS.md` 保存稳定执行约束和文档导航，不复制业务方案、schema 版本或
  开发进度；实际功能／待办状态唯一汇总在 `ROADMAP.md`。
- 需求／契约发生改变时，先更新相应现有方案／契约，再实施；放宽 MUST 先 ADR。
  临时 HTML、外部交接提示词和旧报告不能覆盖现行工程规范。
- 完成影响项目状态的开发或规范修改后更新 `ROADMAP.md`。项目 `README.md`
  仅在实际用法／事实变化时更新，不为凑三份文档制造无关改动。
- ROADMAP 的完成／验证记录使用 `rtk env TZ=Asia/Shanghai date '+%Y-%m-%d %H:%M'`
  获取北京时间；各最多 20 条，倒序保留。未验证事项不能移入最近完成冒充通过。

## Rust lock and test-hang discipline

- Never call a method that can lock the same non-reentrant `Mutex` while its
  guard is still in scope. After a transaction commits, explicitly `drop` the
  connection guard before calling public snapshot/query methods, or use an
  existing helper that accepts the held connection.
- When fixing a lock bug, audit every caller and every sibling
  `commit -> snapshot/session/query` path; fix the shared pattern once rather
  than patching only the reported test.
- A Rust test that stops producing output is a suspected hang, not a slow
  build. Time-bound one exact test, inspect its stack, and check for stale
  matching Cargo/test processes before starting another run. Do not stack
  repeated attempts.
- Do not pipe an active test run to `tail`: `tail` cannot emit the final lines
  until the producer exits and therefore hides hangs. Run it directly while
  diagnosing; add output truncation only after the command is known to finish.

## 验证与交付门禁（MUST）

开发中运行覆盖变更的最小相关检查；Rust 优先精确用例／受影响 package，
诊断时先 `cargo test --no-run` 区分编译和执行。相关代码未变化不重复通过检查。
**完成生产代码实施并交付前执行适用最终门禁，不等到用户要求提交或发布才执行。**

| 修改范围 | 最终检查 |
| --- | --- |
| 仅规范／文档 | `rtk npm run standards:check`、`rtk git diff --check`，核对文档链接／命令及与实际架构的一致性；不为文档修改运行 Cargo 全量 |
| Rust | `rtk env -u CARGO_TARGET_DIR cargo fmt --check`、`rtk env -u CARGO_TARGET_DIR cargo test --workspace` |
| Go Model Host | `rtk npm run model:host:check`，并检查修改的 Go 文件格式 |
| Extension | `rtk npm run extension:check`、`rtk npm run perf:files` |
| 模块 UI | `rtk npm run modules:ui:build`、`rtk npm run modules:ui:check`，并核对编译包含资源和实际页面 |
| 生产代码最终集成 | `rtk npm run extension:check`、`rtk npm run perf:check`，另加上面受影响语言／模块检查 |

先查 `package.json` 的实际 scripts；`perf:check` 当前包含 `perf:files`、
`extension:check` 和 `apps:check`，同次通过结果可覆盖重复门禁，不机械重跑。
失败保留原始事实，定位是本次改动、已有缺陷还是环境阻塞；不擅改无关内容，
不将失败重命名为不适用。修复相关代码后重跑受影响检查。

完整 30 分钟长测不作为每轮修改／交付的默认门禁。先按
`docs/standards/technical/04-performance.md` 的「长测频率与结果复用」核验并复用
一次有效结果；普通功能／样式／文档修改或摘要变化本身不触发。确需重跑时先记录
具体资源影响或证据缺陷、短测结果及目标，禁止盲目重复长测。

UI／用户流程改动还需真实 Chrome 验证对应路径、失败状态、Archive／Volt、
中英文、窄窗与键盘／焦点。复用已有浏览器脚本；打开 HTML、截图、静态 Demo、
API 替身、无障碍树、合成供应商均不能分别冒充完整流程、真实账本、人工读屏或
真实供应商验证。无需读真实用户投资库或发送真实持仓才能完成隔离测试。

多步骤功能按现有方案验收逐项给证据；最终报告写明修改文件、检查命令及实际
结果、真实路径的覆盖范围、失败／未验证／阻塞。性能提升必须有可比 Release
样本；人工或平台项未做就标未做。门禁通过不是全部功能已完成的替代证明。

这些脚本能检查部分可测规则，不能证明智能体已阅读或理解全部规范。
本文件不声称已安装自动执行钩子；执行流程与验证证据仍由实施智能体负责。
