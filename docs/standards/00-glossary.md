# 00 · 现行术语

> 版本：4.0.0 · 日期：2026-09-14

## 规则强度

| 关键词 | 语义 |
|---|---|
| **MUST** | 绝对约束；偏离须先补 ADR |
| **SHOULD** | 强约定；偏离须记录理由 |
| **MAY** | 建议 |

## 文档角色

- **Standard**：当前实现必须遵守的规则，只存在于 `docs/standards/`。
- **ADR**：记录为什么作出或取代一项决策；superseded ADR 只供追溯。
- **Architecture**：当前代码、迁移和验证证据，不覆盖 Standard。
- **Archive**：历史方案和报告，不是开发输入。
- **Source of Truth**：某类持久状态唯一允许写入的 authority。
- **Projection**：从权威状态派生的 UI、缓存、收据或快照，不得反向成为第二权威。

## 产品与进程

- **Natives**：用户安装和更新的唯一完整产品。
- **Extension Page**：当前唯一业务 UI Surface，包括 Home、Files、Apps、Model/Usage 和通用 `app.html`。
- **Thin Launcher**：唯一 `/Applications/Natives.app`；只打开 Chrome、定位扩展、展示安装引导和简短诊断，交接后退出。
- **Files Host**：`native-file-host`；Files、Workspace、Core App Store 和产品配置的 Native Messaging owner。
- **Model Host**：单用途 `model-host`；Provider、Credential、Local Proxy、AI Tool Integration 和 Usage 的 owner，不拥有 Files。
- **App Runtime**：`natives-app-runtime`；Natives 产品级官方内置应用通用运行时（Single Runtime Binary），对应 Native Messaging Host `com.natives.app_runtime`（本地开发为 `com.natives.local.app_runtime`）。
- **Built-in App Runtime Process**：官方内置应用在用户打开时按需拉起的独立进程实例。每个活动 App Surface 对应一个 Runtime Process，进程一次只绑定一个 `appId`，关闭时彻底回收全部资源。
- **Native Port**：Extension Page 通过 `chrome.runtime.connectNative` 建立的连接；默认拥有对应 Host 生命周期。
- **Service Worker**：仅做无状态浏览器事件协调；不持有 Native Port、轮询或本地服务。

## 应用与模块

- **Built-in App（内置应用 / 内置模块）**：随完整 Natives 安装、更新和修复的业务能力，如基金。在 App Center 具有卡片、稳定 appId、独立数据目录、独立 UI、独立生命周期，但无独立产品身份、无独立 Release、无独立可执行文件，全部静态编译进统一 `natives-app-runtime`。
- **App ID**：内置应用的稳定内部身份，用于注册、偏好、数据目录和运行隔离；不是独立产品身份。
- **App Surface**：`app.html` 中的受限 sandbox iframe。
- **Runtime Instance**：用户打开模块后的一次受监督运行；关闭与安装状态无关。
- **Internal Module（内部模块）**：内置应用内部的职责拆分（如 Fund 内部的 Portfolio, Ledger, NAV, Import, Storage, Migration），属于该应用的代码组件，不拥有应用中心卡片、独立 Native Host 或独立进程。
- **First-use Initialization**：首次打开时以当前用户权限创建或迁移数据；不是安装，不下载代码。
- **Product Update**：更新整个 Natives。不存在模块级更新。

## AI 与用量

- **Provider**：模型厂商身份。
- **Connection**：真实 upstream endpoint、协议和网络配置。
- **Credential**：可独立轮换和启停的认证材料引用；持久 Secret 在 OS Keychain。
- **AI Tool Integration**：Claude Code、Codex、Z Code、Atom Code、Cursor、Pi、Hermes Agent、Harness 等工具的检测、配置和用量采集。
- **Usage Event**：来自真实代理请求、账单或工具会话的规范化用量记录。
- **Cost**：按可追溯价格版本计算或由账单提供的金额；估算必须明确标注。

## 数据与生命周期

- **Core App Store**：Files Host 所有的内置模块登记与偏好数据；不是在线商店或下载目录。
- **Product Manifest**：完整产品内固定文件和模块载荷的签名组合清单。
- **EOF Shutdown**：Native Messaging stdin 关闭后，Host 取消任务、关闭端口并退出。
- **Resident Mode**：用户显式允许 Model Host 在页面关闭后保留单实例 worker；默认关闭。
- **Bounded Resource**：有容量、超时、失效和释放条件的缓存、队列、监听、进程或句柄。

## 历史术语

Tauri Workbench、通用 Daemon、Assistant、Agent Runtime、Jobs、Capabilities、Harness、
Workshop、Plugin Runtime、Catalog v3、Suite Seed 和模块 `.nap` 分发均为历史术语。
它们只可出现在 superseded ADR、归档和死亡证明中。
