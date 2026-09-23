# App Center & Fund Managed Sub-App Baseline Freeze (Phase 0)

日期：2026-09-11  
适用：Natives + Natives-App-Fund  
文档定位：Phase 0 基线审计与状态冻结证据（执行《Natives 应用中心与 Fund 托管子应用完整整改实施计划》GATE-0 产物）

---

## 1. 仓库基线与版本状态

### 1.1 Natives 主仓库
- **HEAD Commit**: `762f6d84e442247c7ea2c5c1fd66bff2ce98a4a7` (feat(apps): A3 v4 分块安装协议、Ed25519 签名信任根与托管应用激活)
- **Current Branch**: `deploy`
- **Dirty Files / In-Flight Status**:
  - `crates/app-host-support/` 与 `crates/native-file-host/` 具备 ADR-0027 基础；
  - `docs/adr/0029-unified-suite-preinstalled-apps.md` 已起草但未收敛；
  - `extension/ai-performance/` 与 `model-host/` 完成了 AI 组件 T0–T9 的实现与测试；
  - 所有 Cargo/Go/Extension 测试当前处于通过状态。

### 1.2 Natives-App-Fund 仓库
- **Path**: `/Users/ldh/Downloads/project/AiNative/Natives-App-Fund`
- **Branch**: `main`（初始化工作区，代码已就绪，包含 `src/`, `Cargo.toml`, `Cargo.lock`, `app.json`, `scripts/package.sh`, `AGENTS.md`）
- **Target App ID**: `fund`
- **Package Role**: `managed_extension` / `managed_app`
- **Keychain Namespace**: `com.natives.app.fund`

### 1.3 核心工具链
- `rustc`: 1.96.0 (ac68faa20 2026-05-25)
- `cargo`: 1.96.0 (30a34c682 2026-05-25)
- `node`: v22.23.2
- `npm`: 10.9.8
- `target_os`: macOS (darwin 25.6.0 arm64)

---

## 2. 现状架构审查 (Current Architecture)

当前实现结构：
```text
Natives
├── Core Host (crates/native-file-host)
│   ├── App Store (crates/native-file-host/src/app_store)
│   │   ├── install.rs (v4 catalog-v3 安装事务：install_begin_catalog, chunk, finish, commit)
│   │   └── query.rs (SQLite apps 表查询)
│   ├── app_activation.rs (make_executable, health_probe, register_runtime_host)
│   └── app_dispatch.rs / app_runtime.rs
├── Shared Support Library (crates/app-host-support)
│   ├── lock.rs (FileLock: 每 appId 单实例文件锁 + 4 全局槽位)
│   ├── session.rs / http.rs / framing.rs / origin.rs
├── Extension
│   ├── apps.js (应用中心前端视图与控制器)
│   ├── app-download.js (从 GitHub Release 下载 .nap 载荷)
│   ├── catalog-client.js (获取并验证远程 catalog-v3.json 与 catalog-v3.sig)
│   └── app.html / app.js (托管应用所有者页面沙箱)
└── Fund (Natives-App-Fund)
    ├── src/main.rs (fund-host 独立二进制)
    └── scripts/package.sh (打包为 gzip 单载荷 fund-<version>-<triple>.nap)
```

---

## 3. 现状分发审查 (Current Distribution)

- **现有流程仍为纯网络分发**：
  `apps.js` 依赖 `catalog-client.js` 拉取远端 `catalog-v3.json`；
  安装 Fund 时，`app-download.js` 强制通过 `https://github.com/Wcof/Natives/releases/download/...` 下载 `fund.nap`；
  未提供 Suite Seed 本地分发源，用户首次使用必须经过远端下载。
- **离线与断网脆弱**：
  远端目录 404/超时/断网时，应用中心进入错误态，已安装状态与未安装混淆。

---

## 4. 现状运行时审查 (Current Runtime)

- **进程模型**：Fund Host 为独立原生进程（`fund-host` / `app-exec`），通过 Native Messaging 与 Chrome 通信，提供 127.0.0.1 loopback 业务接口，UI 在 `app.html` 沙箱 iframe 中渲染。
- **单实例与锁机制**：
  `crates/app-host-support/src/lock.rs` 实现了基于系统 `flock` 的排他锁机制，锁文件保留稳定 inode，不在释放时删除。
- **激活机制**：
  `crates/native-file-host/src/app_activation.rs` 在 commit 时验证平台签名、设置可执行权限、执行 `--health` 探针并在 Chrome 注册目录生成 `com.natives.app.<hash>.json` 清单。

---

## 5. 现状权威来源审查 (Current Source of Truth)

- **当前缺陷**：
  前端首屏过于依赖远端 `catalog-v3.json`。若远端目录无响应，已安装应用展示可能受到阻碍。
- **目标规范定义**：
  `Local Registry > Suite Seed > Remote Catalog`：
  - 本地 SQLite `apps` 表是“用户已安装什么”的唯一权威；
  - `Suite Seed` 是“初始套件随附什么”的权威；
  - `Remote Catalog` 仅作为“线上有何更新/新应用发现”的辅助服务。

---

## 6. 当前风险与缺陷清单 (Current Risks)

1. **首次打开触发远端下载**：用户安装 Natives Suite 后，应用中心未预置 Fund，首次使用仍尝试下载 `fund.nap`，违背“单一产品、一次安装”的体验目标。
2. **下载与安装耦合**：`install.rs` 仅接受前端分块上传的 Base64 字节流，没有针对本地安全 `SuiteSeed` 的载荷解析与安装事务抽象。
3. **本地开发信任链摩擦**：若从外部网络下载未认证的二进制，容易触发 macOS Gatekeeper / Quarantine 弹窗。本地开发必须采用本机构建的二进制并建立独立开发测试模式。
4. **回滚与数据覆盖风险**：`app_store/updates` 中回滚逻辑尚未严格区分“运行时代码回滚”与“数据库结构/数据恢复”，需确保迁移后有新写入时禁止危险降级。
5. **账本写入一致性**：Fund 领域的 CSV 导入与手动交易需统一通过 Ledger Command 校验，防止超卖或直接插入破坏仓位平衡。

---

## 7. 实施差异 (Implementation Delta)

根据计划，从 Phase 1 到 Phase 11 的演进路径如下：
- **P1**: 收敛 ADR-0029 / ADR-0027 / 06-sub-apps / managed-app-contract 规范，废止过时文档；
- **P2**: 抽象 `PackageSource`（支持 `SuiteSeed` 与 `Remote`），统一校验流水线，消除重复安装逻辑；
- **P3**: 构建 `fund.nap` 与 `suite-seed.json`，将预装载荷打包进 Suite Installer；
- **P4**: Core 启动时执行 Seed 自动协调（全新安装自动准备，已装版本保持或安全升级，绝不逆向降级）；
- **P5**: 应用中心重构为 Local Registry 优先首屏，远端 Catalog 异步刷新仅提示 Update；
- **P6**: 首次打开 Fund 零网络请求，缺失文件时走受控 Seed 修复而非盲目下载；
- **P7**: Core 统一控制 Host 生命周期，保证 enable/disable 原子性与崩溃恢复；
- **P8**: 隔离 Package Rollback 与 Data Rollback，版本兼容性检查失败立即 fail-closed；
- **P9**: 统一 Fund 账本写入 API，禁止绕过校验的 CSV 直写与超卖；
- **P10**: 建立 `npm run apps:dev` 隔离开发环境，支持本机构建与免 Developer ID 联调；
- **P11**: 真实浏览器完成 7 大端到端场景回归验收（GATE-11）。

---

## GATE-0 验收结论

- [x] Natives 完整 HEAD 与工作区状态已记录；
- [x] Natives-App-Fund 状态与代码基线已核实；
- [x] 当前工具链版本与运行环境已明确；
- [x] 当前 Package 流程、安装事务、运行时锁、激活流、下载流与 macOS 打包脚本已完整审计；
- [x] 产物 `docs/development/app-center-fund-baseline.md` 写入完成。

**GATE-0: PASS**，允许推进至 Phase 1。
