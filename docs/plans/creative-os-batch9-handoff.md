# Creative OS Batch 9 交接报告 — Attached Local 与 Remote 非受管应用

> 交接时间：2026-08-04
> 分支：`codex/creative-os-b9-20260804-214000`（base = Batch 8 final `6d8140d`）
> 主工作区，未新建 worktree

## 1. Batch / Task / 提交

| Task | 提交 | 说明 |
|---|---|---|
| CR-901 Attached Local | `c2c116b` | OwnershipMode=attached，loopback URL 校验，probe，删除仅删记录 |
| CR-902 Remote | `c2c116b` (同 CR-901) | OwnershipMode=remote，approved origins 策略，永无 Tauri capability |

- base SHA：`6d8140d`（Batch 8 final）
- final 代码 stable：`c2c116b`

## 2. 改动文件

| 文件 | 变化 |
|---|---|
| `src-tauri/src/creative_app/non_owned.rs` | 新文件：非受管 driver（attached/remote）+ probe + 6 测试 |
| `src-tauri/src/creative_app/model.rs` | OwnershipMode、NonOwnedApp、NonOwnedProbe |
| `src-tauri/src/creative_app/mod.rs` | non_owned 模块 |

## 3. 安全设计

- **Attached**：URL 必须 loopback（127.0.0.1/localhost）；probe 短超时优雅报 unreachable；delete 仅删记录，无 stop/kill 可表达
- **Remote**：navigation 仅限 approved origins；无 approved origins 则全部阻断；永不授予 Tauri capability
- **honest lifecycle**：attached/remote 的 start/stop 恒为 false（`lifecycle_actions`）

## 4. 测试结果

| 命令 | 结果 |
|---|---|
| `cargo check` | PASS（0 errors，2 个 FFI 命名 warning） |
| `cargo fmt --check` | PASS |
| `creative_app::` 178/178 | PASS（+6 from Batch 8） |
| `typecheck` | PASS |
| `lint` | PASS（2420 键） |
| `test` 763/763 | PASS |
| `protocol:check` | PASS（154 methods） |

## 5. 遗留风险

- **DB 表未建**：NonOwnedApp 模型定义但未落表（可复用 applications + startup_plans ownership_mode）
- **UI 未接入**：非受管 app 的展示/打开路径待前端集成
- **`commands::provider` 单测挂起**：环境性

## 6. 下一批（B10）继承

- commit：`c2c116b`（schema 22）
- OwnershipMode（managed/attached/remote）已就绪，可用于 agent proposal 的 ownership 表达
- 禁止假设：不要假设非受管 app 有 DB 行（模型就绪，表待建）