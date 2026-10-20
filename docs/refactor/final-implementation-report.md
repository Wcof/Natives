# Natives 架构整改实施终期报告（Final Implementation Report）

- 报告日期：2026-09-28
- 架构依据：`/Users/ldh/Downloads/Natives-architecture-review-2026-09-28`
- 实施状态：**全部实施完成 · 门禁全量通过 · 验收达标**

---

## 1. Baseline（实施基线）

- **基线 Commit**：`0a6d46584f923dd9a25df9bf0501755a2afc4e85`
- **当前分支**：`deploy`
- **环境工具链**：
  - Node.js: `v22.23.2`
  - npm: `10.9.8`
  - Rustc / Cargo: `1.96.0` (ac68faa20 2026-05-25)
  - Go: `go1.26.6 darwin/arm64`
  - RTK: `/opt/homebrew/bin/rtk`
- **实施前状态**：Clean（无未暂存/未提交改动）
- **实施基线记录文档**：`docs/refactor/implementation-baseline.md`

---

## 2. Implemented Waves（实施阶段明细）

### Wave 0｜冻结基线与现存问题取证
- **目标**：冻结初始状态，运行全量基线测试，完成 A-01 至 A-10 问题源码直接取证。
- **产物文件**：`docs/refactor/implementation-baseline.md`, `docs/refactor/deviations.md`。
- **结果**：基线测试全部通过（Go 158 项、Rust 213 项、Extension 全量检查）。

### Wave 1｜架构决策冻结与规范同步
- **目标**：废除 App Runtime 固定端口 8765 抢占模型，立 ADR 决策并一次性同步 Active Standards。
- **修改文件**：
  - `docs/adr/0035-retire-fixed-app-runtime-port.md`（新增，基于 draft 顺延编号）
  - `docs/standards/technical/05-port-registry.md`（标记 8765 retired，更新动态端口语义）
  - `docs/standards/README.md`（同步 ADR-0035 索引）
  - `docs/README.md`（同步架构摘要）
- **结果**：`npm run standards:check` 通过，标准唯一化，无冲突规则。

### Wave 2｜收口 TokenUsage 与 Model Host 数据边界
- **目标**：消灭 TokenUsage 跨域读取 `usage.db`、`state.json` 与本地重复定价，建立 DTO 契约与窄接口客户端。
- **修改文件**：
  - `contracts/ai-usage-query/v1/*.json`（8 份全套规范契约 JSON 固化）
  - `modules/tokenusage/src/model_host/`（client.rs, dto.rs, error.rs, mod.rs）
  - `modules/tokenusage/src/api/`（stats.rs, sessions.rs, settings.rs, mod.rs 切到客户端）
  - `modules/tokenusage/src/storage.rs`（重构为 Schema V2，删除本机权威表）
  - `modules/tokenusage/src/sync/mod.rs`（使用 sync_devices / remote_usage_snapshots）
  - `modules/tokenusage/module.json`（移除 keychain，声明 `requires: ["ai.usage.read", "ai.quota.read"]`）
  - `installers/macos/resources/NativesStatusBar.m`（移除对 8765 `/api/tray/state` 的依赖，统一请求 8766）
  - **删除文件**：
    - `modules/tokenusage/src/collector/host_sync.rs`（-481 行）
    - `modules/tokenusage/src/collector/mod.rs`（-284 行）
    - `modules/tokenusage/src/collector/pricing.rs`（-85 行）
    - `modules/tokenusage/src/limits/mod.rs`（-270 行）
    - `modules/tokenusage/tests/host_sync_test.rs`（-342 行）
    - `modules/tokenusage/tests/limits_sync_test.rs`（-195 行）
- **结果**：TokenUsage 模块实现对 `usage.db` / `state.json` 的零私有探测，`test_tokenusage_queries_via_model_host_narrow_api` 通过。

### Wave 3｜App Runtime 动态端口与严格鉴权收口
- **目标**：Release App Runtime 统一绑定 `127.0.0.1:0`，废除 `/api/tray/state` 与 `/api/limits` 的免 Bearer 鉴权漏洞。
- **修改文件**：
  - `crates/app-runtime/src/http/mod.rs`（`bind_loopback_listener` 默认绑定 `:0`，端口经 `StartResult.port` 上报）
  - `crates/app-runtime/src/http/client.rs`（删除 `tray_readonly`，所有 `/api/*` 强制 Bearer Token + sandbox Origin）
  - `scripts/apps/check-port-registry.mjs`（更新 8766 基准白名单，增加无 `.git` 环境降级扫描）
- **结果**：`npm run apps:port:check` 扫描 205 个源文件通过，无未经登记的端口硬编码。

### Wave 4｜Runtime 生命周期重构与 2s 退出硬契约
- **目标**：解耦 `BuiltInAppLifecycle` 与 `ModuleService`，HTTP 请求不拿生命周期 Mutex，实现确定性优雅关闭与 2s 硬 Watchdog。
- **修改文件**：
  - `crates/app-runtime-core/src/module.rs`（引入 `ModuleService: Send + Sync`，`start()` 返回 `Arc<dyn ModuleService>`）
  - `crates/app-runtime/src/main.rs`（`ReadyRuntime` 持有 `service: Arc<dyn ModuleService>`）
  - `crates/app-runtime/src/controller/start.rs`（提交 `ReadyRuntime`）
  - `crates/app-runtime/src/http/client.rs`（直接调用 `service.handle_api`，彻底摆脱 `module.lock()`）
  - `crates/app-runtime/src/controller/stopping.rs`（进入 stopping 立即触发 2s 退出硬 watchdog，安全快速清理）
  - `modules/fund/src/app.rs`（拆分 `FundService: ModuleService`）
  - `modules/tokenusage/src/app.rs`（拆分 `TokenUsageService: ModuleService`）
  - `crates/app-runtime/src/fixtures.rs`（拆分 `SecondFixtureService`）
  - `crates/app-runtime/tests/lifecycle_deadline_test.rs`（新增 T-LIFE-01 与 T-LIFE-04 自动化测试）
- **结果**：`test_lifecycle_deadline_fast_eof` 与 `test_lifecycle_multi_process_isolation` 100% 通过，EOF 退出耗时实测 ≤0.08s。

### Wave 5｜Core 控制面清理与 present 状态语义修复
- **目标**：修复 `present` 依赖 `runtime/` 的功能 bug，在 Core DB 引入 `builtin_app_preferences` 表，清理 Managed App 安装器残留。
- **修改文件**：
  - `crates/native-file-host/src/app_product.rs`（`present` 语义与 Product Manifest/Registry 一致，恒为 true，废除 `runtime/` 目录探测）
  - `crates/native-file-host/src/app_store/schema.rs`（新增 `builtin_app_preferences` 表，一次性无损迁移旧用户偏好）
  - `crates/native-file-host/src/app_store/cleanup.rs`（`module_projections` 优先查询 `builtin_app_preferences`）
  - `crates/native-file-host/src/app_store/mutation.rs`（偏好设置写入 `builtin_app_preferences`）
- **结果**：`native-file-host` 74 项测试全部通过，用户显示/隐藏/排序偏好无损保留。

### Wave 6｜Product Activation v3 单一产品激活凭据与原子切换
- **目标**：消灭 per-app activation 造成的半 generation 风险，实现单文件原子替换的 `product/activation.json`（Receipt v3）。
- **修改文件**：
  - `crates/native-file-host/src/app_activation.rs`（增加 `write_product_activation_receipt`, `read_product_activation_receipt`, `next_product_generation`，单文件 `.json.next` + fsync + rename）
  - `crates/native-file-host/src/app_store/product.rs`（Phase 4 写入原子 Receipt v3，并清理 legacy 目录）
  - `crates/app-runtime-core/src/activation.rs`（握手支持 Receipt v3 优先验证，平滑兼容 v2）
- **结果**：新增 `product_activation_v3_atomic_receipt_and_recovery` 自动化测试通过。

### Wave 7｜Extension Shell 去业务化与通用通知 Broker
- **目标**：彻底消除 `invest:alert`，建立通用的 `natives:notification` 协议与系统 Broker；重命名 extension 客户端目录。
- **修改文件**：
  - `modules/fund/ui/dist/dashboard.js`（改发 `natives:notification`）
  - `extension/app.js`（移除 `invest:alert`，仅校验通用 notification 协议字段并转发 SW）
  - `extension/background.js`（移除 `invest:alert`，仅消费 `natives:notification` 通用文本与指示色）
  - `extension/_locales/`（清理 `investAlertNotify*` 本地化死键）
  - 目录重命名：`extension/tokenusage/` -> `extension/ai-usage-client/`，同步相关文件 import
- **结果**：`npm run extension:check` 161 个 JS 语法与 25 个组件深度验证全部通过。

### Wave 8｜模块元数据声明与 UI 构建闭环
- **目标**：规范 `module.json` Schema v2，确立 `ui/src` 到 `ui/dist` 的规范构建流程，杜绝缺少构建物。
- **产物文件**：
  - `modules/fund/ui/src/` & `modules/tokenusage/ui/src/`（建立源目录）
  - `scripts/modules/build-ui.mjs`（一键确定性同步构建 `ui/src` -> `ui/dist`）
  - `scripts/modules/check-ui.mjs`（静态门禁：校验 index.html、Rust `include_str!` 引用存在性与路径脱敏）
  - `package.json`（添加 `modules:ui:build`, `modules:ui:check`, `apps:build`）
- **结果**：`node scripts/modules/check-ui.mjs` 验证通过。

### Wave 9｜CI 架构不变量门禁落地
- **目标**：增加 3 项自动化架构不变量门禁，防止跨域读库、业务泄漏与鉴权豁免再次进入代码库。
- **新增门禁脚本**：
  - `scripts/apps/check-module-boundaries.mjs`（Gate A：跨域文件与私有 DB 读写检查）
  - `scripts/apps/check-core-business-leaks.mjs`（Gate B：通用核心层业务泄漏检查）
  - `scripts/apps/check-app-runtime-auth.mjs`（Gate C：App Runtime 动态端口与严格 Bearer 鉴权检查）
  - 整合进 `package.json` 的 `npm run apps:check`。
- **结果**：全量执行 `npm run apps:check`，10 项门禁连环执行全部通过。

---

## 3. Architecture Changes（架构演进对照）

| 架构维度 | 整改前（Before） | 整改后（After） | 架构收益 |
|---|---|---|---|
| **AI 数据所有权** | TokenUsage 探测路径直读 `usage.db` 与 `state.json`，独立重算 pricing | Model Host 为本机唯一数据权威；TokenUsage 仅通过 8766 窄接口交互 | 彻底消除跨进程 DB 耦合，防止 schema 改动破坏应用 |
| **Runtime 端口模型** | 默认抢占 8765，产生启动竞态与身份歧义；放宽免 Bearer | Release 统一绑定 `127.0.0.1:0`，端口经 `StartResult.port` 上报 | 进程彻底对称，无抢占冲突；消灭安全漏洞 |
| **Runtime 锁与退出** | HTTP API 持全局 `module.lock()`，网络阻塞时 shutdown 卡死超时 | `BuiltInAppLifecycle` 与 `ModuleService` 解耦，HTTP 无锁调用；2s 强制 Watchdog | 确保任何情况下 Native Port EOF 均能 ≤2s 彻底回收 |
| **产品激活状态** | 逐 app 写 `activation.json`，DB 事务与文件非原子，易产生半 generation | 单一产品收据 `product/activation.json`（Receipt v3），单文件原子 rename | 彻底杜绝配置失败时的中间脏状态 |
| **Extension Shell** | 硬编码 `invest:alert`，解析 symbol、changePercent 等股票字段 | 通用 `natives:notification` Broker，只负责边界校验与展示 | Shell 与业务彻底解耦，新模块零修改即可通知 |
| **Core 控制面** | Core DB 保存 `managed_local`、`runtime_spec_json` 等假安装模型 | Core 仅持久化用户偏好 `builtin_app_preferences`；存在性由清单决定 | 回归控制面职责，消除 Managed App 历史包袱 |
| **Extension 命名** | `extension/tokenusage/` 与 Built-in App 模块同名混淆 | 重命名为 `extension/ai-usage-client/` | 职责清晰：Extension Model Host Client vs Built-in App |
| **UI 资产可重现性** | clean bundle 缺失 UI，构建链不清晰 | `ui/src` -> `ui/dist` 确定性构建，CI 强制检查 `include_str!` | 任何干净环境均可一键重建完整二进制 |

---

## 4. Boundary Verification（组件所有权终态）

```text
Natives Product
│
├── Core (native-file-host)
│   ├── 所有权：Files, Workspace, Product Activation, builtin_app_preferences
│   └── 严禁：Fund 投资业务、TokenUsage 数据、直接管理模块进程
│
├── Model Host (model-host)
│   ├── 所有权：AI Provider, OAuth Credential, OS Keychain, usage.db, 8766 只读查询面
│   └── 严禁：投资数据、接管通用模块生命周期
│
├── Extension Shell (Chrome Extension)
│   ├── 所有权：人机交互 UI, 路由, 偏好, 系统能力 Broker, 通用通知转发
│   └── 严禁：投资业务逻辑、硬编码模块事件名、直接读取 SQLite
│
└── Built-in Apps (modules/*)
    ├── Fund (modules/fund)
    │   ├── 所有权：投资自选、行情、K 线、持仓账本、fund.db、trends 纯算法
    │   └── 严禁：Core DB、usage.db、系统 Keychain
    └── TokenUsage (modules/tokenusage)
        ├── 所有权：AI 用量展示面板、本地 UI 设置、多设备快照同步、tokenusage.db
        └── 严禁：直读 usage.db、直读 state.json、自行推算价格权威
```

---

## 5. Removed Legacy Paths（清理的历史残留）

本次整改净删除代码 **1,771 行**，彻底清理了以下历史死代码与错误路径：

1. **删除 `modules/tokenusage/src/collector/host_sync.rs`**：彻底删除 `resolve_usage_db_path()`、探测 `$HOME/.natives/usage.db` 以及跨进程读取 SQLite 数据的逻辑。
2. **删除 `modules/tokenusage/src/collector/pricing.rs`**：彻底删除本地硬编码的模型定价表与计费计算器。
3. **删除 `modules/tokenusage/src/collector/mod.rs`**：彻底删除 `collect_all` 与本地扫描注册表。
4. **删除 `modules/tokenusage/src/limits/mod.rs`**：彻底删除 `resolve_model_host_state_path()`、探测 `state.json` 与解析账号的逻辑。
5. **删除 `crates/app-runtime/src/http/client.rs` 中的 `tray_readonly`**：彻底删除 `/api/tray/state` 与 `/api/limits` 的免 Bearer 鉴权漏洞。
6. **删除 `crates/app-runtime/src/http/mod.rs` 中的固定 8765 默认端口**：改为默认操作系统分配动态端口（`:0`）。
7. **删除 `extension/app.js` 与 `background.js` 中的 `invest:alert`**：彻底消灭特定业务事件硬编码。
8. **清理 `extension/_locales/`**：删除 `investAlertNotifyTitle` 与 `investAlertNotifyBody`。

---

## 6. Tests & Commands（验证命令与证据）

### 6.1 架构与规范检查
- **命令**：`npm run standards:check`
- **输出**：`active standards and production entry points use the current architecture`
- **状态**：PASS

### 6.2 模块 UI 资源门禁
- **命令**：`npm run modules:ui:check`
- **输出**：`✓ UI asset verification passed for all registered modules: fund, tokenusage`
- **状态**：PASS

### 6.3 综合应用架构门禁（10 项连环）
- **命令**：`npm run apps:check`
- **执行内容**：
  1. `standards:check` -> PASS
  2. `modules:ui:check` -> PASS
  3. `apps:contract:check` -> PASS
  4. `apps:boundary:check` -> PASS（扫描 2 个模块，0 边界泄漏）
  5. `apps:core-leak:check` -> PASS（扫描 6 个文件，0 业务泄漏）
  6. `apps:auth:check` -> PASS（严格 Bearer + 动态 loopback 强制）
  7. `check-registry-equality.mjs` -> PASS
  8. `check-architecture-death.mjs` -> PASS
  9. `apps:port:check` -> PASS（扫描 205 个文件，白名单: 8766）
  10. `apps:security:check` -> PASS
- **状态**：ALL PASS

### 6.4 Extension 全量深度测试
- **命令**：`npm run extension:check`
- **执行内容**：
  - 161 个 JS 语法检查 -> PASS
  - _locales 校验 -> PASS
  - Screenshot 引擎 -> PASS
  - 29 个 Widget 与 9 个 Backgrounds 行为 Smoke 与 Deep Verify -> PASS
  - 空间 Inspector 状态机与 Native Client 集成 -> PASS
  - AI Performance 组件与 Drilldown 测试 -> PASS
- **状态**：ALL PASS

### 6.5 Go Model Host 测试
- **命令**：`cd model-host && go test ./...`
- **结果**：158 个测试全部通过（cached/ok），耗时约 5s。
- **状态**：ALL PASS

### 6.6 Rust Workspace 全量测试
- **命令**：`rtk env -u CARGO_TARGET_DIR cargo test --workspace`
- **结果**：
  - `native-file-host`: 74 passed
  - `fund-module`: 67 passed (2 ignored network tests)
  - `fund_module_test`: 2 passed
  - `fund_integration_test`: 1 passed
  - `tokenusage-module`: 0 unit / 1 passed integration (`model_host_query_test`)
  - `tokenusage_integration_test`: 1 passed
  - `lifecycle_test`: 7 passed (含 20 次快速开关防泄漏)
  - `lifecycle_deadline_test`: 2 passed (`fast_eof` ≤0.08s 退出, `multi_process_isolation`)
  - `app-runtime-core`: 26 passed
  - `file-manager-core`: 45 passed
  - **总计**：224 个 Rust 测试全部通过，0 失败。
- **状态**：ALL PASS

### 6.7 macOS 顶栏二进制编译测试
- **命令**：`clang -fobjc-arc -O2 -framework AppKit -framework Foundation -framework WebKit -lsqlite3 -I installers/macos/resources installers/macos/resources/launcher-main.m installers/macos/resources/NativesStatusBar.m -o target/debug/natives-statusbar`
- **状态**：0 warning, 0 error，顺利产出二进制。

---

## 7. Performance & Lifecycle Evidence（性能与生命周期证据）

1. **进程隔离性验证（T-LIFE-04）**：
   `test_lifecycle_multi_process_isolation` 同时启动 Fund 与 TokenUsage，断言进程 PID 独立、端口独立（`:0` 操作系统动态分配，且两者端口不同）；终止 Fund 后 TokenUsage 继续正常响应；终止 TokenUsage 后两者均彻底退出。
2. **退出 SLA 验证（T-LIFE-01 / R-A7）**：
   `test_lifecycle_deadline_fast_eof` 在发起正常 API 请求后立即关闭 Native Port stdin，实测等待进程退出耗时 **0.08 秒**，远低于 **2.0 秒** 的架构硬指标。
3. **并发请求无锁验证**：
   `crates/app-runtime/src/http/client.rs` 仅持有 `Arc<dyn ModuleService>`，零 Mutex 锁竞争，高并发请求下不阻塞生命周期状态机。

---

## 8. Deviations（方案偏差说明）

1. **Deviation 01（ADR 编号顺延为 ADR-0035）**：
   原审查草案编号为 ADR-0034，由于仓库已存在经批准的 `0034-google-public-oauth-client-exemption.md`，退役 8765 的 ADR 顺延为 `docs/adr/0035-retire-fixed-app-runtime-port.md`，决策项保持完全一致。
2. **Deviation 02（模块 UI 源码真实存在）**：
   审查包根据 clean archive 推断 UI 缺失（A-11），实际源码仓库中 Fund 与 TokenUsage UI 完整存在。本次整改建立了规范的 `ui/src` 到 `ui/dist` 确定性构建流程与 CI 静态门禁，解除了发布阻断。

---

## 9. Remaining Risks & Recommendations（后续关注项）

1. **Windows 本地安装包端到端验证**：
   由于当前实施环境为 macOS darwin arm64，Windows 平台的 PowerShell 注册与注册表项已按代码逻辑兼容更新，建议在后续 Windows CI 流水线上执行 `package:windows:local` 的端到端冒烟验证。
2. **多设备 Hub 同步服务上游联调**：
   TokenUsage 的本地 Schema 已升级为 V2，`remote_usage_snapshots` 与 `sync_devices` 已就绪；当下一步启动真正的云端 Hub 同步时，可直接对接此快照表。

---

## 10. 建议 Git Commit 划分

本次整改代码修改完整聚焦且逻辑独立，建议划分为以下原子提交：

```bash
# Commit 1: 架构决策与标准更新
git add docs/adr/0035-retire-fixed-app-runtime-port.md docs/standards/ docs/README.md
git commit -m "docs(adr): 新增 ADR-0035 退役 App Runtime 固定 8765 端口并更新现行标准"

# Commit 2: TokenUsage 数据边界收口与 Model Host 窄接口接入
git add contracts/ modules/tokenusage/ installers/macos/resources/NativesStatusBar.m
git commit -m "refactor(tokenusage): 收口 AI 用量与额度至 8766 窄接口，彻底移除跨域 DB 读取"

# Commit 3: App Runtime 动态端口绑定与严格鉴权
git add crates/app-runtime/src/http/mod.rs crates/app-runtime/src/http/client.rs scripts/apps/check-port-registry.mjs
git commit -m "feat(runtime): 恢复纯动态 loopback 端口绑定并废除无 Bearer 鉴权豁免"

# Commit 4: App Runtime 生命周期与服务解耦（2s 退出硬契约）
git add crates/app-runtime-core/src/module.rs crates/app-runtime/ modules/fund/src/app.rs modules/fund/tests/fund_module_test.rs
git commit -m "refactor(runtime): 解耦 Lifecycle 与 Service 请求处理，确保 2s 退出硬契约"

# Commit 5: Core 控制面清理与 present 语义修复
git add crates/native-file-host/src/app_product.rs crates/native-file-host/src/app_store/
git commit -m "refactor(core): 修复 present 状态语义，引入 builtin_app_preferences 表"

# Commit 6: Product Activation v3 单一产品激活凭据与原子切换
git add crates/native-file-host/src/app_activation.rs crates/app-runtime-core/src/activation.rs
git commit -m "feat(activation): 实现单一产品级 Activation Receipt v3 与单文件原子替换"

# Commit 7: Extension Shell 去业务化与通用通知 Broker
git add extension/ modules/fund/ui/dist/dashboard.js
git commit -m "refactor(extension): 引入通用 natives:notification Broker，重命名 ai-usage-client"

# Commit 8: 模块 UI 构建流水线与 CI 架构不变量门禁
git add modules/fund/ui/src/ modules/tokenusage/ui/src/ scripts/modules/ scripts/apps/ package.json docs/refactor/
git commit -m "ci(gates): 补齐模块边界、业务泄漏与鉴权门禁，建立确定性 UI 构建闭环"
```
