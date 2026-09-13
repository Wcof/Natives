# App Runtime Refactor Baseline（版本冻结）

> 记录时间：2026-09-11
> 用途：Natives 单 Runtime + 托管子应用架构整改（方案 P0 前）版本冻结。
> 本文件为整改前快照，后续阶段不得修改本文件的历史记录部分。

## 1. Natives 主仓库

- **HEAD**: `762f6d84e442247c7ea2c5c1fd66bff2ce98a4a7`
- **Branch**: `deploy`
- **HEAD 提交**: `feat(apps): A3 v4 分块安装协议、Ed25519 签名信任根与托管应用激活`
- **工作区状态**: 未提交变更 95 项（`git status --short`），主要包括：
  - Rust：`crates/app-host-support/*`、`crates/native-file-host/*`（app_activation / app_dispatch / app_store / protocol / workspace_store 等）
  - 文档：ADR-0020 / ADR-0027 / managed-app-contract / 06-sub-apps / 00-glossary / 01-positioning / 01-layering / 02-security / 03-data
  - Extension：`app-*.js`、`apps.*`、`catalog-client.js`、`native-*client.js`、`_locales/*`、`manifest.json`、`plugins/*`、`space-*.js`
  - 删除：`extension/app-module-registry.js`、`extension/apps/catalog-v2.json`、`extension/apps/catalog-v2.sig`、`extension/apps/demo-ui.js`、`extension/apps/fund-ui.js`
- **diff 概况**: 大量未提交改动（A3 v4 分块安装协议相关工作），详见 `git diff`（快照时未提交，整改在其之上继续）。

## 2. Natives-App-Fund 仓库

- **路径**: `../Natives-App-Fund`
- **HEAD**: 无（`main` 分支 **没有任何提交**，所有文件均为未跟踪状态）
- **工作区状态**: 全部未跟踪 — `.cargo/`、`.github/`、`.gitignore`、`AGENTS.md`、`Cargo.lock`、`Cargo.toml`、`README.md`、`app.json`、`docs/`、`scripts/`、`src/`、`dist/`（未跟踪）
- **src 结构（现状）**: `api.rs`、`fixed.rs`、`import.rs`、`ledger.rs`、`lib.rs`、`main.rs`、`migration.rs`、`nav.rs`、`portfolio.rs`、`storage.rs`、`ui.rs`
- **备注**: Fund 仓库尚未建立 Git 历史；方案第 2 节要求的 Fund HEAD 以“无提交”记录。**风险**：整改前应先建立初始提交以获得可回滚基线（属 Fund 仓库自身操作，不改变架构）。

## 3. 工具链

| 工具 | 版本 |
|---|---|
| rustc | 1.96.0 (ac68faa20 2026-05-25) |
| cargo | 1.96.0 (30a34c682 2026-05-25) |
| node | v22.23.2 |
| Chrome | 152.0.7977.83 |

## 4. 当前 catalog version / managed app contract version

- 快照时点：工作区未提交状态，`docs/contracts/managed-app-contract.md` 处于已修改（未提交）状态。
- 当前 contract 版本号与 catalog 版本号以工作区文件实时内容为准（见 ADR-0027/ADR-0029/managed-app-contract 现行文本），本文件不做推测性抄录，避免与实际不一致。

## 5. 现有测试结果

- 2026-09-11 P0 完成后实测：`rtk env -u CARGO_TARGET_DIR cargo test --workspace`
  → **PASS**（107 passed; 0 failed; 0 ignored，含 native-file-host app_store/workspace_store/protocol 与 app-host-support）。
- Extension 侧 `rtk npm run extension:check` 未在基线时点执行，P4/P5 涉及扩展改动时补跑。

## 6. 已知架构事实（整改输入）

- Fund 曾以独立 `fund-host` / `app-exec` 进程模型存在；本轮目标为 builtin Library 接入（方案 R-01～R-05）。
- macOS Gatekeeper/syspolicyd 对未签名载荷的评估问题已记录在项目 Memory（ad-hoc 签名会被 SIGKILL），这是去 executable 化的直接动因之一。
- `crates/app-host-support` 当前以本地 path 依赖方式被 Fund 使用（Cargo.toml 有切换注释标记）。
