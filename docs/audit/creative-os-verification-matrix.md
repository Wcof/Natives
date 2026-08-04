# Creative OS 核查矩阵

> 证据行号以 `deploy@9584c3c2` 为准。详情见 `creative-os-issue-verification.md`。

| 编号 | 疑似问题 | 核查状态 | 严重度 | 证据位置 | 建议动作 | 所属批次 |
|---:|---|---|---|---|---|---|
| 01 | 应用身份和 Source 解析 | CONFIRMED | P0 | `commands/creative_app.rs:328-354,386-415`; `runtime_store.rs:40-59` | Browser 只读 resolve；清理幽灵 identity | 0/T01 |
| 02 | PreviewTarget 写入顺序 | CONFIRMED | P0 | `commands/creative_app.rs:337-354,391-415`; `browser.rs:46-78,115-125` | WebView ack 后提交，失败补偿 | 0/T03 |
| 03 | Static Stop 后 URL | CONFIRMED | P0 | `http_server.rs:374-510`; `local/lifecycle.rs:490-494` | instance token 路由，stop revoke | 0/T03 |
| 04 | 全局 MutationLock | PARTIALLY_CONFIRMED | P1 | `service.rs:14-58`; `lib.rs:357-389` | app lock + 有界全局 semaphore | 0/T04 |
| 05 | RuntimeInstance 资源所有权 | CONFIRMED | P1 | `local/runtime.rs:56-117`; `adapters/mod.rs:101-267` | registry/event/resource 全带 runtime id | 0/T05 |
| 06 | 活跃实例 DB 唯一性 | CONFIRMED | P0 | `runtime_store.rs:107-143`; `db.rs:930-949` | partial unique + 原子 CAS | 0/T02 |
| 07 | 状态迁移 affected rows | CONFIRMED | P1 | `runtime_store.rs:145-199,320-374`; source stores | 0 行 typed conflict | 0/T02 |
| 08 | Restart 失败语义 | CONFIRMED | P0 | `local/lifecycle.rs:440-550`; `adapters/mod.rs:234-314` | verified stop barrier | 0/T03 |
| 09 | Stop/Delete/Window 关系 | CONFIRMED | P1 | lifecycle adapters; Browser commands | runtime/window分离，endpoint联动 | 0–1/T03,T06 |
| 10 | Crash/Orphan 恢复 | PARTIALLY_CONFIRMED | P0 | `lib.rs:328-440`; `db.rs:1204-1397` | identity-proof reconcile + 故障注入 | 0/T05 |
| 11 | 单例 Child WebView | CONFIRMED | P1 | `browser.rs:12-18,46-78` | dynamic window id/label | 1/T06 |
| 12 | WindowInstance 缺失 | CONFIRMED | P1 | Browser/model/schema 全域 | 新增真实 window registry | 1/T06 |
| 13 | BrowserProfile/Cookie | NEEDS_RUNTIME_VERIFICATION | P1 | `browser.rs:61-75`; 无 profile API | 平台 spike 后落 profile | 2/T08,T09 |
| 14 | OAuth 导航 | NEEDS_RUNTIME_VERIFICATION | P1 | `service.rs:66-105`; `browser.rs:65-66` | 临时授权 surface/allowlist | 2/T08,T09 |
| 15 | 下载/上传/剪贴板/新窗口 | PARTIALLY_CONFIRMED | P2 | Creative Browser 无 handler/policy | 独立 permission + 平台实验 | 2/T08,T09 |
| 16 | Source/Runtime 类型覆盖 | CONFIRMED | P1 | `model.rs:42-56,523` | 逐 driver 增量扩展 | 3/T12,T13 |
| 17 | Managed/Attached/Remote | CONFIRMED | P1 | `model.rs:914-920`; lifecycle | ownership mode，non-owning stop | 3/T12,T13 |
| 18 | LaunchPlan 单服务 | CONFIRMED | P1 | `model.rs:445-521` | ServiceInstance/Endpoint | 3/T11 |
| 19 | Python/Binary Runtime | CONFIRMED | P1 | scanner evidence；runtime enum 无支持 | 复用 Process owner，显式批准 | 3/T13 |
| 20 | Agent 启动计划安全 | PARTIALLY_CONFIRMED | P1 | `local/ai.rs`; `local/plan.rs`; UDS protocol | versioned proposal + Host gate | 4/T14 |
| 21 | 健康检查范围 | CONFIRMED | P1 | `local/runtime.rs:469-519`; `docker.rs:459-485` | typed probes/readiness/liveness | 3/T11 |
| 22 | 日志实例隔离 | CONFIRMED | P2 | `local/logs.rs:321-362`; runtime registry | runtime/service cursor logs | 0–3/T05,T11 |
| 23 | CPU/内存/端口监控 | CONFIRMED | P2 | Creative 全域无 sampler/metrics | 后置按需 snapshot | 3/T11 |
| 24 | 端口冲突 | PARTIALLY_CONFIRMED | P1 | local port selection；Docker inspect | 短租约/统一 endpoint allocation | 3/T12 |
| 25 | Operation Journal | CONFIRMED | P1 | schema/service 无 operation entity | 最小 phase/compensation journal | 0/T04 |
| 26 | CSP | PARTIALLY_CONFIRMED | P1 | `http_server.rs:14-19,90-127` | Workshop/Local/Remote策略分离 | 0/T07 |
| 27 | Host/Origin/Traversal | NOT_REPRODUCED | P3 | `http_server.rs:155-200,374-500` | 保留回归；不另造修复 | 持续 |
| 28 | Bridge Body Limit | CONFIRMED | P1 | `http_server.rs:55-66,561-580` | 413 + 端点限额 + 有界并发 | 0/T07 |
| 29 | Tauri Capability 隔离 | NOT_REPRODUCED | P3 | `capabilities/default.json`; child label | 动态 label 继续默认拒绝 | 0/T07 |
| 30 | WorkshopPage 复杂度 | CONFIRMED | P2 | `WorkshopPage.tsx` 2,012 行 | 按真实 flow 拆 controller | 1–4/T10 |
| 31 | 前端 Busy State | PARTIALLY_CONFIRMED | P2 | `useCreativeAppCatalog.ts:109-135`; Browser open | 从 Operation/Window event 投影 | 0–1/T04,T10 |
| 32 | Dock/多窗口/Task Switcher | CONFIRMED | P1 | Creative 全域无 registry/UI | Window 数据驱动的最小 Shell | 1/T06,T10 |
| 33 | ApplicationSurface | CONFIRMED | P1 | `OpenTarget`; 单 Preview/open_url | 稳定 surface 映射 runtime endpoint | 1/T06 |
| 34 | Migration/Manifest 版本 | PARTIALLY_CONFIRMED | P1 | `db.rs:892-1090,1204-1397`; plan v1 | additive schema upgrader/历史 fixtures | 0–3/T02,T12 |

## 状态与严重度统计

| 维度 | 统计 |
|---|---|
| 核查状态 | CONFIRMED 22；PARTIALLY_CONFIRMED 8；NOT_REPRODUCED 2；FALSE_POSITIVE 0；OBSOLETE 0；NEEDS_RUNTIME_VERIFICATION 2 |
| 严重度 | P0 6；P1 21；P2 5；P3 2 |

## 重点失败矩阵

| 操作 | 失败点 | 当前 DB | 当前 WebView/资源 | 结论 |
|---|---|---|---|---|
| Browser show | DB connection/upsert | 无/旧 Preview | WebView 仍尝试显示 | 错误被吞，状态分叉 |
| Browser show | add child | 新 Preview 已写 | 无 WebView | stale Preview |
| Browser show | navigate/bounds/show | 新 Preview 已写 | 旧页/隐藏/尺寸旧 | Preview 与实际 URL/可见性不符 |
| Browser close | DB clear | clear 错误被吞 | 继续 close | close 可成功但 Preview 残留 |
| Browser close | WebView close | Preview 已删 | WebView/BrowserState仍活 | DB 假 closed |
| Local stop | process/port/Compose verify | source cleanup_failed；runtime 被外层写 stopped | 资源可能仍活 | restart 可错误继续 |
| Static stop | source row清 URL | runtime stopped | Host route仍读 source root | 旧 URL 继续可用 |

## 资源 Owner 矩阵

| 资源 | 当前 key | 目标 key | 风险 |
|---|---|---|---|
| PID/PGID | source app id | runtime id | 晚到事件/重启串实例 |
| Docker container/project | app label/稳定 name | runtime/service id + app label | 旧资源与新运行混淆 |
| Port/URL | source row + runtime mirror | endpoint id/runtime id | TOCTOU/停止不撤销 |
| Logs/health task | app id | runtime/service id | 跨运行混合 |
| Window/WebView | 固定 label | window id | 必然只有一个窗口 |
| Cookie/storage | 平台默认 store | browser profile id | 隔离/持久性未知 |

## 本轮命令与结果

| 命令 | 结果 | 说明 |
|---|---|---|
| `git status --short --branch` / `git branch --show-current` / `git log -1 --oneline` | PASS | `deploy@9584c3c2`；原有未跟踪 `.cargo-target-shared/` |
| `npm install --dry-run --ignore-scripts` | PASS | 依赖解析成功；dry-run 未改 dependency files |
| `npm run typecheck` | PASS | `tsc --noEmit` |
| `npm run lint` | PASS | ESLint；zh/en 2,405 keys 同步；hardcoded-color 无新增违规 |
| `npm run test` | PASS | 462 顶层条目、756 tests，0 fail |
| `npm run build` | PASS | Next 15 static export，14/14 pages |
| `npm run perf:check` | PASS（串行复跑） | `/page` 215.0 KB、`/modules/page` 203.4 KB、`/files/page` 234.7 KB，均低于 350 KB；第一次与独立 `next build` 并行时因共享 `.next/types` 竞争失败，非稳定代码失败 |
| `npm run protocol:check` | PASS | TS/Rust protocol sync OK，catalogued methods 154 |
| `cargo fmt --check` | PASS | 无格式差异 |
| `cargo check -p natives` | PASS | dev profile 完成 |
| `cargo test -p natives creative_app` | **FAIL** | 89 pass / 1 fail；`creative_app::store::tests::modules_survive_v8` 仍断言 schema `8`，migration 实际为 `14`；单独串行复跑仍失败 |
| `cargo test --workspace` | **FAIL** | 在 `agent-core` 167 pass / 2 fail：`completes_simple_text_turn`、`session_end_hook_fires_after_success` 均 `PERSISTENCE_FAILED`；各自串行复跑仍失败 |
| `docker version` / `docker compose version` / `docker ps` | BLOCKED | 当前环境无 `docker` executable；未运行任何容器/Compose 项目 |
| `git diff --check` | PASS | 仅新增本任务 5 个 `docs/audit/` 文件；未修改生产源码 |

Rust 失败均原样保留，未修改或弱化测试。`modules_survive_v8` 是当前 migration 测试陈旧断言；`agent-core` 两项不属于 Creative App 调用链，但会阻断 workspace 全绿，需由其 owner 单独诊断 persistence fixture。
