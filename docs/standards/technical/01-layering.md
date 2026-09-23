# 技术 01 · 进程、分层与通信

> 版本：4.0.0 · 日期：2026-09-14
> 依据：ADR-0020、ADR-0023、ADR-0027、ADR-0029

## 当前生产拓扑

```text
Chrome/Chromium
├─ newtab.html / files.html / apps.html / model settings
│  └─ domain client → Native Messaging
├─ app.html
│  ├─ short Core verification → Files Host
│  └─ direct Native Port → Built-in Module Host → 127.0.0.1 sandbox iframe
└─ Service Worker（无状态，无 Native Port）

Natives.app
└─ 打开 Chrome / 定位扩展 / 安装引导 / 简短诊断 → 退出
```

#### R-T1 · Surface 与 Host 职责固定

- **等级**：MUST
- Extension Page 只拥有 UI、交互草稿和派生缓存。
- Files Host（`native-file-host`）拥有文件、Workspace、Core App Store 和产品配置。
- Model Host（`model-host`）拥有 Provider、Credential、Proxy、AI Tool Integration 和 Usage。
- App Runtime（`natives-app-runtime`，对应 `com.natives.app_runtime` / `com.natives.local.app_runtime`）是官方内置应用的通用运行时宿主；每个活动 Built-in App 实例按需拉起一个 App Runtime Process，独占承载该模块业务、业务库和运行实例。
- Launcher 不拥有业务状态或业务 UI。
- 一个 Host 不得借“复用”接管另一个 Host 的领域能力；严禁把内置应用业务编译进 Files Host（Core != App Runtime）。

#### R-T2 · 数据 authority 唯一

- **等级**：MUST
- 每个持久数据集只有一个写入 owner；页面、缓存、activation 和安装收据都是投影。
- 跨 owner 操作使用窄请求和稳定标识，不共享可写数据库连接。
- 迁移期间不允许新旧路径双写；切换完成后删除旧 caller 和 fallback。

#### R-T3 · 依赖方向

- **等级**：MUST

```text
Page entry
  → Page/domain controller
    → Domain client / renderer
      → Native protocol or pure helper
        → Host domain
          → storage / filesystem / keychain / network
```

- 下层不得 import 页面或 DOM。
- Widget/背景 renderer 不得成为跨领域 backend。
- `crates/` 只放已有两个真实调用方共享的协议、安全或领域逻辑。

#### R-T4 · 跨信任域只走受控通道

- **等级**：MUST
- Extension 与 Host 只走 Chrome Native Messaging 的长度前缀 JSON 帧。
- `app.html` 先经 Core 核验，再直连由 Core 决定名称的 App Host。
- App iframe 只走两阶段 `postMessage` 握手和带 bearer 的 loopback HTTP。
- 页面不能传可执行路径、Host 名称、安装路径或任意命令。
- 不得用本地通用 HTTP、WebSocket、UDS 或 Service Worker Port 建第二 IPC。

#### R-T5 · Native Port 拥有生命周期

- **等级**：MUST
- Files/App Host 由打开页面的 Native Port 按需启动；stdin EOF 触发同一确定性 shutdown。
- Model Host 默认相同；只有用户显式开启 resident 后允许单实例 worker 留存，且必须可关闭。
- Service Worker 不持有 Native Port、轮询或 keepalive。
- Host 不注册登录启动项，不演化成通用 daemon。

#### R-T6 · Launcher 是严格例外

- **等级**：MUST
- 只允许一个可见 `/Applications/Natives.app`。
- Launcher 可以打开随包本地 HTML 指南、Chrome 扩展页和扩展目录；成功交接或用户关闭后退出。
- 不承载 Home、Files、Apps、AI、Usage、Settings，不运行后台服务。

#### R-T7 · 内置模块运行隔离

- **等级**：MUST
- 所有官方 Built-in App 使用统一 `natives-app-runtime` executable。一个活动 App Surface 对应一个 Runtime Process。Runtime Process 一次只允许绑定一个 appId。
- 每个模块使用稳定 appId、独立数据目录（`~/.natives/apps/<appId>/`）和独立 SQLite 库；模块目录由 Runtime 注入，模块禁止自行探测产品或用户路径。
- 通用 `app.html` 不包含模块业务分支；模块 UI 在无 `allow-same-origin` 的 sandbox iframe 中通过 127.0.0.1 动态端口 loopback 运行。
- 模块内部职责（如持仓、账本、NAV、导入、迁移）可以拆分，但属于模块内部组件，严禁产生第二产品、第二应用卡片或第二安装对象。

## 合规自检

- [ ] 页面、Files、Model、模块和 Launcher owner 没有混用。
- [ ] 无第二 IPC、第二数据库 writer 或静默 fallback。
- [ ] Service Worker 无状态。
- [ ] 非 resident Host 随 EOF 退出。
- [ ] 新进程边界有 ADR。
