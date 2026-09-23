# Natives 安装与应用中心当前实施方案

> 版本：v4.0 · 2026-09-14
>
> 本文件是执行入口。Natives 是唯一用户产品；Fund、Portfolio、Ledger、NAV、Import 和 Migration 是同一安装包内的内置模块。模块没有下载、独立安装、独立更新、独立卸载、在线目录、`.nap` 或独立 Release。

## 1. 产品边界

- 用户只安装、更新和修复 `/Applications/Natives.app`（Windows 为同一产品的本地候选包）。
- Chrome Extension、Rust Hosts、Model Host 和全部内置模块由同一产品版本交付。
- App Center 是模块入口和偏好管理页：显示所有随包模块，提供打开、显示/隐藏、启用/停用、设置和数据清除。
- “我的应用”始终显示已随包存在的 Fund 卡片；不能依赖 `apps` 表中的安装记录，也不能出现“安装 Fund”“下载”“更新模块”按钮。
- 首次使用只做当前用户的数据初始化和产品投影；代码、Host 注册和模块文件不会在 App Center 或模块首次打开时下载。
- 模块隐藏只改变侧栏偏好，停用只改变运行资格；两者都不删除代码。数据清除是单独确认的危险操作，默认保留凭据和业务数据以外的明确范围由用户选择。

## 2. 安装和首次使用

安装器必须把主入口、固定扩展目录、Native Messaging Hosts、产品清单、签名材料和全部模块文件作为一个完整产品写入受限系统源。安装器不写用户数据库、activation 或业务数据，也不以 root 执行用户迁移。

用户流程：

```text
安装 Natives
  → “应用程序”中可见 Natives.app
  → 双击 Natives
  → Launcher 打开随包离线 onboarding/index.html 和 Chrome 扩展页
  → 用户开启开发者模式并选择包含 manifest.json 的固定扩展目录
  → 扩展前台通过 origin 校验的 apps:handshake
  → Host 自动完成一次 per-user 产品投影
  → 进入空间，App Center 直接显示 Fund
```

Launcher 是薄入口，只负责打开 Chrome、定位随包扩展目录、显示状态/诊断、复制目录和重新检测，然后退出。它不承载空间、文件、AI 或基金 UI，不读取 Chrome profile 私密文件，不启动常驻通用服务。

Chrome 普通发行版仍要求用户完成开发者模式和“加载已解压的扩展程序”；不实现静默 CRX、企业策略注入、无障碍自动点击或 Web Store 后备链。离线 HTML 必须无网络、无业务脚本、无实时成功状态，包含版本、三步操作、常见故障、真实目录和中英文/键盘/深浅色支持。

## 3. Host 与协议

`apps:handshake` 在验证 caller origin 后，如果完整产品源存在且当前用户尚未配置，自动调用内部产品投影事务。浏览器和 App Center 不可调用产品配置方法。

当前协议白名单：

| 方法 | 用途 |
|---|---|
| `apps:handshake` | 验证 origin、协议、平台和版本，并触发一次产品投影 |
| `apps:list` / `apps:get` | 返回固定模块、用户偏好、真实可用性和产品状态 |
| `apps:product_status` / `apps:health` | 读取产品源、generation 和 Host 健康 |
| `apps:set_enabled` / `apps:set_sidebar` | 修改模块运行资格和导航偏好 |
| `apps:clear_data` | 经确认、带 requestId 的范围化数据清除 |
| `apps:open_onboarding` | 打开随包固定离线指南 |

旧的 `apps:install_*`、`apps:suite_prepare`、`apps:recover`、目录/分块/Seed/独立发布方法必须不存在于白名单、dispatch、客户端、测试和活动文案中。未知方法在任何文件、数据库或注册变更前失败。

产品投影必须校验产品版本、固定模块 ID、文件摘要、签名、路径、Host 身份和 generation；使用既有 product lock、staging、journal、原子切换和恢复。全部固定模块就绪后才报告 configured，失败时保留上一个完整版本。

## 4. App Center 交付要求

1. `apps:list` 返回有效产品清单中的全部固定模块，即使用户表为空或模块从未打开。
2. 卡片显示名称、说明、版本/可用性、打开入口和必要错误；状态来自实际产品文件与注册核验，unknown 不得显示为成功。
3. 打开只校验固定模块和 generation，并聚焦当前 Chrome profile 的模块页；不得跨 profile 接管或启动隐式下载。
4. 显示/隐藏、启用/停用和排序只写现有偏好；停用事务持有运行锁，避免与模块写入并发。
5. 清除数据执行双确认和幂等 requestId，只清用户明确选择的 data/imports/cache/logs/credentials 范围；保留代码、签名、注册、activation 及导航偏好。
6. 更新入口统一为“更新 Natives”。新模块只能通过新的完整 Natives 版本加入。
7. UI 使用空间局部 tokens 和现有组件，不引入 Host 文件管理颜色、第二套主题、React/Tauri 页面或独立模块 UI。

## 5. 当前实现与门禁

代码入口：

- `extension/apps.js`、`extension/app.js`：列表、打开、偏好、清除数据和 onboarding。
- `crates/native-file-host/src/app_dispatch.rs`、`app_store/`：协议、产品投影、固定模块查询和数据边界。
- `crates/app-runtime/`、`crates/app-runtime-core/`：统一 Runtime、模块 Host 的路径、锁、激活和协议支持（ADR-0031）。
- `scripts/installer-package.mjs`、`scripts/extension-package.mjs`：完整产品候选构建；不生成模块包或扩展 ZIP/current 元数据。

每次修改后运行最小相关检查；整合前运行：

```sh
rtk npm run standards:check
rtk npm run extension:check
rtk npm run perf:check
rtk npm run apps:integration
rtk env -u CARGO_TARGET_DIR cargo fmt --check
rtk env -u CARGO_TARGET_DIR cargo test --workspace
rtk go test ./...
```

本地候选可称 A-Local/B-Local；只有真实平台签名、公证、干净系统安装→扩展加载→App Center→Fund 使用→重启→完整产品 N+1 更新→修复/数据保留证据齐全后，才可称正式 Release。未执行的平台或真实浏览器路径必须标记 pending，不得用 fixture 或直连 Host 代替。

历史实施记录和被淘汰方案位于 [`docs/archive/`](../archive/README.md)，不参与当前执行入口。
