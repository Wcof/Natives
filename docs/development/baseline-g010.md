# G-010 完整基线检查记录

- **日期**: 2026-08-22
- **分支/HEAD**: `feat/v2-workspace-design-system` @ 3127120（dirty worktree，见 `WORKTREE-INVENTORY.md`）
- **背景**: Wave 0（G-001..G-009）完成后、首条纵向切片（A/C 组）启动前的全量基线。

## 结果（10 项全绿）

| # | 检查 | 命令 | 结果 |
|---|---|---|---|
| 1 | TypeScript | `rtk npm run typecheck` | exit 0 |
| 2 | Lint（含 i18n colors scan） | `rtk npm run lint` | exit 0（staleCount=0） |
| 3 | 单测（Renderer） | `rtk npm run test` | exit 0（0 fail / 0 cancel / 0 skip） |
| 4 | 性能预算 | `rtk npm run perf:check` | exit 0 |
| 5 | 颜色规范 | `rtk npm run colors:check` | exit 0 |
| 6 | 架构守护 | `rtk npm run architecture:check` | exit 0 |
| 7 | i18n 对齐 | `rtk npm run i18n:check` | exit 0（zh/en 1:1） |
| 8 | 协议同步 | `rtk npm run protocol:check` | exit 0（TS types aligned with assistant-protocol core surface） |
| 9 | Rust 格式 | `rtk cargo fmt --check` | exit 0 |
| 10 | Rust 全量测试 | `rtk cargo test --workspace` | **2268 passed / 16 ignored / 0 failed**（51 suites，180.5s） |
| 11 | 原生引擎 | `rtk npm run verify:native-engine` | exit 0 |

（Extension Host 未执行：本切片未触及 `extension-host/`。）

## Failures 分记（验收要求：existing/new 分开）

- **Existing failures（基线既有失败）**: 0
- **New failures（本轮引入）**: 0

## 备注

1. `cargo test --workspace` 为首次在本机全量通过；其中含 G-008 新增测试 `workspace::service::revision_tests::check_revision_rejects_stale_and_skips_when_none`（raw SQL fixture）。
2. G-008 附带发现的 `store::create_workspace` 空表 `MAX(position)` NULL→`InvalidColumnType` 疑似既有缺陷（HEAD 既有代码，非本轮引入）**不在本基线计为 failure**（未被任何现有测试/路径触发，v27 无条件种子 home），已登记台账移交 A 组 P0 核查。
3. 本基线为 A/C 组纵向切片的"before"参照；切片完成后复跑同一命令集作为"after"证据。
