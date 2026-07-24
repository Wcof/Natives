# 个人创意 · 本地项目收敛方案

> 状态：实施中；基线：`deploy@4f130256`；关联：[ADR-0013](../adr/0013-creative-app-dual-source.md)

## 目标与边界

`local_project` 是个人创意的第三来源：用户选择已有本地 HTML、Vue 或 Vite 项目，Natives 只保存启动配置与加密环境变量；绝不复制、构建或删除项目源码。静态项目由主进程 HTTP 服务提供，Node 项目以受控 argv 在 `127.0.0.1` 启动，并在内置浏览器打开。

保留统一列表、打开、启动、停止、删除、日志和依赖安装；不增加通用命令执行、无容器降级或联网商店。

## 当前收敛项

| 项 | 约束 | 落点 | 验收 |
|---|---|---|---|
| 进程退出 | 后端每 2 秒轮询受管 `Child`，不依赖页面存活 | `src-tauri/src/lib.rs` | dev server 自行退出后卡片变为 stopped |
| 依赖安装 | 固定 npm/pnpm/yarn argv、清空继承 env、独立进程组 | `local/deps.rs` | 取消/退出不遗留安装子进程 |
| 原子写入 | 应用记录与加密 env 同一 SQLite transaction | `commands/creative_app.rs` | 任一 env 写失败时无半创建/半更新 |
| 日志 | stdout/stderr 按该应用 env 值及通用模式脱敏 | `local/runtime.rs`、`local/logs.rs` | 输出 env 值不进入事件、内存或文件日志 |

## 运行规则

1. 所有 Smart、Custom 与 AI 方案均经 `validate_launch_plan`；包管理器脚本读取真实 `package.json` body，拒绝 shell 构造。
2. `autoOpen` 由前端在成功启动后决定是否调用内置浏览器；`startAfterSave` 只是同一前端事务内紧接的显式 start，不把 UI 行为隐藏进 create RPC。
3. 退出轮询与用户生命周期操作共享既有全局变更锁。该锁是有意的串行化：个人创意操作量低；若将来出现并行需求，再按 app id 分片。
4. 删除只删除 Natives 的记录、加密 env 与日志，绝不触碰所选项目目录。

## 验收与回归

- `cargo check -p natives`
- `npm run typecheck`
- `npx tsx --test src/lib/local-creative.test.ts src/lib/creative-app.test.ts`
- 手工：HTML 静态项目、带依赖的 Vite 项目、错误 env/端口、进程自行退出、关闭应用后重新启动。

## 已知边界

- 只监督由当前 Natives 会话创建并仍被 runtime manager 持有的子进程；应用崩溃后的残留进程由启动时 reconcile 标为 orphan，必须由用户确认停止或重启。
- 日志脱敏是防泄漏层，不应用日志作为保存凭证的渠道。
