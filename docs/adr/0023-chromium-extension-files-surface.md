# ADR-0023：Chrome 文件工作台与按需 Native Host

## 状态

已接受（2026-08-28）；开发纵向切片已验证文件协议与按需生命周期，正式安装与发布门禁仍未完成。

## 背景

Natives 的高频入口是用户已经安装并长期使用的 Google Chrome。独立 Electron、CEF 或 Chromium 浏览器会重复承担浏览器进程、更新与扩展兼容成本；纯扩展又不能直接访问任意本地文件。

首次安装可以包含一次 Chrome 启用确认，但日常使用必须只有一个入口，且不得要求用户手动启动或关闭本地服务。空闲资源和安装包体积是产品约束，不是发布后的优化项。

## 决策

V1 采用真实 Google Chrome、Manifest V3 扩展和按需 Rust Native Messaging Host：

```text
Natives 启动入口
  → Google Chrome（用户现有 Profile）
    → Natives 新标签页（纯静态、无 Native 连接）
      → 文件管理页（直接 connectNative）
        → Chrome 按需启动 Rust Host
          → 本地文件系统
```

### 浏览器与页面

- 不引入 Electron、CEF、Tauri 窗口或 Chromium fork。
- `newtab.html` 是轻量工作台，不读取文件系统、不创建 Native Messaging 连接。
- `files.html` 是唯一文件管理 Surface；进入页面时按需直接创建 Native Port。
- Service Worker 只处理工具栏入口、安装引导和页面导航，禁止持有 Native Port、轮询或保活计时器。
- Chrome 原生 Profile、账号、密码、扩展、标签栏和更新机制保持不变。

### Native Host 生命周期

- Native Host 不注册开机启动项、LaunchAgent、Windows Service、daemon、托盘进程或本地 HTTP 服务。
- Chrome 在文件页连接时启动 Host；文件页 `pagehide` 或关闭时主动断开。
- 文件页隐藏且无进行中操作 60 秒后断开，重新可见时按需重连。
- 最后一个 Native Port 关闭后，Host 从 stdin 读到 EOF 并在 2 秒内退出。
- 文件监听只在文件页可见且存在订阅时启用；禁止轮询。
- Host 无窗口、WebView、图形初始化和 GPU 上下文。

### 安装、启动与卸载

- 正式发布时提供有稳定扩展 ID 的 Chrome Web Store MV3 扩展；开发安装使用本地未打包扩展和固定 key。
- macOS/Windows 使用平台原生签名安装器；禁止为安装器引入 Electron、Tauri 或常驻更新器。
- 正式安装器一次完成 Host 落盘、Native Messaging manifest 注册、Chrome Web Store external extension 登记和启动 Chrome；开发安装允许用户手动加载扩展并执行注册脚本。
- Windows/macOS 首次启用扩展的 Chrome 确认不可绕过；除此之外不得要求用户复制扩展 ID、运行命令或手动启动 Host。
- Natives 启动入口只负责打开/聚焦 Chrome 工作台，随后立即退出。
- 卸载器移除 Host、Native Messaging manifest、外部扩展登记和 Natives 启动入口，不遗留进程或启动项。
- 扩展更新交给 Chrome Web Store；Host 只在连接握手时报告协议版本，版本不兼容时显示下载安装提示，不运行后台更新检查。

### 文件能力与安全边界

- 复用 `crates/file-manager-core` 的路径授权、规范化、敏感目录防护和文件操作。
- Renderer 不直接访问任意本地路径，不执行 shell、SQLite、进程或 Secret 操作。
- Native Messaging 使用稳定扩展 ID 白名单、严格消息 schema、操作白名单、消息大小限制和安全错误。
- 大目录列表、预览和搜索必须分页、可取消；删除默认进入系统废纸篓。

## 资源预算

基准环境沿用 `docs/standards/technical/04-performance.md`：Apple Silicon、16 GB、macOS、Release 构建。

| 项目 | V1 门禁 |
|---|---:|
| 扩展 ZIP | ≤ 250 KB |
| Native Host 单架构 Release 二进制 | ≤ 3 MB |
| 平台安装包 | 目标 ≤ 10 MB；超出必须给出文件级归因 |
| 新标签页初始 JS（gzip） | ≤ 50 KB |
| Native Host 空闲 RSS | ≤ 12 MB |
| Native Host 空闲 60 秒平均 CPU | ≤ 0.5% |
| Native Host 新增 GPU 进程/上下文 | 0 |
| 最后端口关闭至 Host 退出 | ≤ 2 秒 |
| 10,000 项目录首屏 100 项 p95 | ≤ 50 ms |
| 热 Native IPC p95 | ≤ 50 ms |

资源门禁必须在相同设备、Release 构建和固定数据集上测量。不得用删除沙箱、站点隔离或路径安全换取性能。

## 取舍

- 首次安装仍需一次 Chrome 安全确认；消费级 Windows/macOS 软件不能静默启用 Chrome 扩展。
- 日常体验只有 Natives 一个启动入口，本地 Host 由 Chrome 自动管理。
- 放弃独立浏览器品牌外壳，换取 Chrome 原生兼容、最小增量内存和最小维护面。
- 保留约 1 MB 的 Rust Host；将文件能力并入 Chromium C++ 最多省一个小进程，却会显著扩大补丁、安全和升级成本。

## 当前证据与缺口

- 当前扩展目录约 64 KB；Release Host 为 1,056,032 bytes（约 1.0 MB），实测空闲 RSS 约 6 MB。
- macOS unsigned development pkg 实测 443,498 bytes；这不是签名、公证或正式分发包体积。
- 1,000 项目录首屏 100 项约 3.0 ms；10,000 项约 28.3 ms；取消 ACK 约 2.86 ms。
- `newtab.html` 与 `files.html` 已分离；`files.html` 直接创建 Native Port，Service Worker 不创建 Native Port、不轮询或保活。Host 在 stdin EOF 时清理 watcher/后台任务并退出。
- 正式 Chrome Web Store ID、签名证书、公证、macOS 正式安装器、Windows 实机安装验证、真实 Chrome 生命周期 E2E 和卸载残留检查仍待完成；当前 Windows 仅有静态/IExpress 安装资产。

只有上述缺口关闭并通过资源门禁，ADR 状态才可改为“已落地”。
