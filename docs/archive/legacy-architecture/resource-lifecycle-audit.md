# Resource Lifecycle Audit — PERF-04

> 关联规范：`docs/standards/technical/04-performance.md` R-P2/R-P3/R-P6/R-P9、
> `02-security.md` R-S1（子进程看门狗）。
> 本文档是 PERF-04 的交付物之一：列出所有长生命周期后台 owner 及其释放点，
> 确保关闭窗口/退出进程时无孤儿子进程、无持续泄漏的后台任务。

验收：后 20 分钟 RSS 无持续单调增长；空闲 CPU p75 ≤ 2%；关闭窗口后无孤儿子进程。

## 1. Owner → 资源 → 释放点

| Owner | 资源 | 启动位置 | 释放点 | 备注 |
| --- | --- | --- | --- | --- |
| Host 主进程 | Daemon sidecar（Agent Daemon 子进程） | `lib.rs` setup `sidecar_supervisor::ensure_started()` | `shutdown_all_processes()` → `sidecar_supervisor::global_supervisor().shutdown()` | UDS 看门狗守护；退出时幂等收敛 |
| Host 主进程 | Credential Broker UDS 监听线程 | `lib.rs` `spawn_broker_uds_listener()` | 进程退出回收（mode-0600 UDS） | 长驻线程，随进程生命周期；不持有子进程 |
| Host 主进程 | Daemon 看门狗 tick | `daemon_watchdog::start()`（tokio interval，2s） | 进程退出时 tokio runtime drop | `MissedTickBehavior::Delay`，无积压 |
| Host 主进程 | Job runner tick | `jobs::runner::start()`（tokio interval，30s） | 进程退出时 tokio runtime drop | 调度语义不变；tick 间隔 30s |
| Host 主进程 | Local creative runtime 看门狗 | `lib.rs` `tauri::async_runtime::spawn`（interval 2s） | 进程退出时 tokio runtime drop | DB-CAS 守卫写入，无 mutation lock 阻塞 |
| Host 主进程 | 本地 HTTP 服务线程（PERF-02 延迟） | `lazy_http_port::LazyHttpPort::port()` 首次调用 | 进程退出回收；tiny_http 单监听线程 | 仅 Workshop/Embed 首次使用时绑定 |
| Host 主进程 | Creative reconcile 一次性线程 | `lib.rs` `std::thread::spawn`（一次性） | 任务完成即退出 | settle_stale + reconcile，非周期 |
| Host 主进程 | PTY / Ghostty 终端会话 | `terminal::TerminalManager` / `GhosttyManager` | `shutdown_all_processes()` → `kill_all()` | 窗口关闭仅隐藏；真正退出时统一回收 |
| Host 主进程 | Local creative 进程树（dev server） | `creative_app::local` | `shutdown_all_processes()` → `creative_app::local::shutdown_all()` | 进程组终止，防止孤儿 |
| Renderer | WorkspaceDataBroker 订阅 | `WidgetRenderer.useWidgetData` | 组件 unmount → `unsub()`；in-flight `AbortController.abort()` | R-P3：可见性门控（PERF-03） |
| Renderer | DataBroker in-flight 请求 | `load()` 内 `AbortController` | 订阅清零时 abort（`subscribe` 返回的 unsub） | 避免 orphan 请求 |
| Renderer | DataBroker visibility 监听 | `bindVisibility()`（PERF-03） | `WorkspaceDashboard` unmount → 返回的 unsub | 隐藏时暂停新 load |
| Renderer | DataBroker 缓存 entry | LRU `MAX_CACHE_ENTRIES=64` | `evictInactiveEntries()` 按 lastAccessed 淘汰 | 无界 Map 禁止（R-P9） |

## 2. 关键不变量

1. **窗口关闭 ≠ teardown**（MB-P0-01）：`CloseRequested` 对 main/menubar 仅 `hide()`。
   真正回收收敛在 `RunEvent::ExitRequested`/`Exit` 与 `menubar_quit`，由
   `shutdown_all_processes` 经 `Once` 守卫幂等执行一次。
2. **子进程监督**（R-S1/R-B7）：Daemon sidecar 由 `sidecar_supervisor` 纳入看门狗；
   本地 creative 进程树由 `LocalRuntimeHandle` 管理；PTY/Ghostty 由各自 Manager 统一 kill。
   禁止裸 `Command::spawn`。
3. **Renderer 订阅可清理**（R-P3）：每个 `WidgetRenderer` 在 unmount 时取消订阅并
   abort 在途请求；`bindVisibility` 在页面隐藏时暂停新 load，返回时 drain 队列。
4. **缓存有界**（R-P9）：DataBroker 缓存上限 64、监听者/key 上限 32、并发 load 上限 6。

## 3. 测量方法

- 冷启动阶段耗时：`scripts/perf/cold-start.mjs`（PERF-01）。
- 资源生命周期 soak：`scripts/perf/resource-soak.mjs`（PERF-04）。
  - 启动 Release 二进制，按固定 cadence 采样主进程及所有后代（pgrep 树）的
    RSS / %CPU / 进程数；
  - 结束时发送 quit，检查是否有后代在主进程退出后仍存活（孤儿）；
  - 判定：后 20 分钟 RSS 无持续单调增长、空闲 CPU p75 ≤ 预算、无孤儿子进程。
- 结果落盘 `scripts/perf/results/resource-soak.json` + `.csv`，机器指纹随附（R-P1）。
