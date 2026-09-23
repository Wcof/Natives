# Natives 当前架构

> 版本：v4.0 · 2026-09-14
>
> 权威约束见 [`docs/standards/`](../standards/README.md)、ADR-0020、ADR-0027 和 ADR-0029。
> 本文只描述当前可运行产品；历史设计在 [`docs/archive/`](../archive/README.md)。

## 产品边界

Natives 是一个单一用户产品。Chrome Extension 是唯一产品界面，Rust Native Hosts 提供文件和产品配置能力，Go Model Host 只负责 AI Provider、OAuth、Keychain 和 loopback 代理。Launcher 是薄入口，只负责启动、扩展引导和诊断后退出。

```text
Natives installer
├── thin launcher
├── Chrome Extension
├── native-file-host       Files + product/module registry
├── model-host             AI provider/proxy
└── built-in modules       Fund first; shipped in the same product
```

## 运行链路

```text
Chrome Extension
  ├─ Native Messaging → native-file-host   → filesystem / SQLite / App Store
  ├─ Native Messaging → model-host         → Provider / Keychain / loopback
  └─ app.html?app=<id> → direct Native Port → natives-app-runtime (按需独立进程实例)
```

Extension Service Worker 保持无状态，不持有 Native Port、不轮询、不保活。页面按需建立连接，隐藏或关闭时释放连接和定时器。Runtime 进程通过 stdin EOF 在 ≤2 秒内回收会话并彻底退出。

## 产品级模块与统一 App Runtime

Natives 是唯一产品。Fund 等官方能力为内置应用（Built-in App），其内部领域（Portfolio、Ledger、NAV、Import、Storage、Migration 等）为内部模块（Internal Module）。

所有内置应用源码归入 Natives Monorepo 的 `modules/` 目录，在编译期静态编译进唯一产品级可执行文件 `natives-app-runtime`。系统 Native Messaging 仅注册唯一的 `com.natives.app_runtime`（本地开发为 `com.natives.local.app_runtime`），不再针对每个应用生成或注册独立 Native Host。

当用户在 Chrome 中打开某内置应用时，Chrome 按需启动一个 `natives-app-runtime` 进程实例，Runtime 通过 Protocol v2 握手选定模块并注入数据目录。未打开的应用不占用任何专用内存（≈0 dedicated memory）。页面关闭或端口断开后，Runtime 进程在 2 秒内退出，由 OS 直接回收全部内存与资源。

Core 在系统源读取并验证 `product-manifest.json`（Schema 2）与签名，核对 SHA-256。用户的显示、启用、排序写在 Core 库中，应用独占数据存储于 `~/.natives/apps/<appId>/data/`，用户目录下严禁存放可执行文件。

App Center 负责列出模块状态、打开、显示/隐藏、启用/停用、数据清理和重新查看安装引导。模块卡片永远显示已随产品提供的模块，没有“安装 Fund”或远程应用市场。

## 安全与数据

- 页面只能访问受限领域方法；Host 不向页面暴露任意路径、进程、SQLite 或 Secret 明文。
- 模块页面在无 `allow-same-origin` 的 sandbox iframe 中，通过 127.0.0.1 loopback 和短期 bearer session 访问自身 Host。
- Secret 只在 OS Keychain；日志、错误、数据库和扩展存储不得出现明文。
- 所有用户写入先做路径、来源、大小和状态校验；数据清理只删除明确选择的用户数据并保留代码、注册和偏好。
- 更新使用 staging、哈希校验、原子 rename 和失败清理；隐式降级拒绝。

## 性能与可观测性

性能预算以 [`technical/04-performance.md`](../standards/technical/04-performance.md) 为准：冷启动、Extension 包体、Host RSS、空闲回收、20 Widget 和 30 分钟真实 Chrome soak 均须有可复现证据。AI 用量、成本、会话和提醒属于 Data & Usage 域，使用统一事件账本，来源可追溯；估算值必须标明口径。

## 变更规则

修改产品边界先更新 ADR，再同步 Standards 和契约。任何新增模块进入下一版完整 Natives 包，不新增第二套分发链。旧设计只可在归档目录阅读，不可作为实现依据。
