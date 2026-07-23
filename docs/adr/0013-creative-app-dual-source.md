# ADR-0013: 个人创意双来源应用（内部 Workshop + 外部 GitHub 容器）

- **状态**: 已接受
- **日期**: 2026-07-22
- **决策者**: 产品方（用户）
- **关联**: [ADR-0012](./0012-product-identity-workshop-scope.md)、`docs/architecture/creative-app-github-container-install.md`、实施方案《双来源应用升级实施方案》
- **归类**: Workshop 面（统一目录）+ Embed 面（外部容器 GUI）+ Host 面（GitHub / Docker / 持久化）

---

## 上下文

个人创意目前只管理内部 `web-module`（Workshop 静态 SPA + Unique Origin iframe + Bridge）。用户需要在同一目录中管理外部应用：从 GitHub **Release** 获取容器发行物，在本机 Docker 运行，并在主窗口内以**子 WebView** 打开 GUI。

若把外部容器硬塞进既有 `modules` 表与 Workshop 沙箱，会破坏：

1. KI-1～KI-5（内核身份、契约、沙箱、总线、FOUC 等）不变量；
2. ADR-0012 的双轨 / 三面边界；
3. 「无假数据」——不得用旧 DB 值伪造 Docker 运行中。

需要冻结管理面统一、运行时分轨的架构，并明确 v1 非目标。

---

## 决策

### 1. 管理面统一，运行时分轨

| 来源 | `source` | `runtime` | 存储 | 打开表面 | 权限 |
|------|----------|-----------|------|----------|------|
| 内部生成 / 本地包 | `internal` | `workshop_static` | 既有 `modules` 等 | Unique Origin iframe + Bridge | Workshop 权限中心 |
| GitHub Release 容器 | `external_github` | `docker_compose` / `docker_run` | 新表 `external_creative_apps` + `creative_app_env` | 主窗口内 Tauri 子 WebView | 无 Bridge / 无主应用 capability |

- 统一只读投影：`CreativeAppSummary`（列表、状态、动作可用性）。
- **禁止**迁移或改写既有 `modules` 表语义以容纳容器。
- **禁止**外部应用接入 Workshop SDK、Session Token、Bridge 或初始化脚本。

### 2. 产品三面分工（在 ADR-0012 上细化）

```
Workshop 面 — 内部生成应用 + 统一创意目录 UI
Embed 面    — 外部容器应用 GUI（子 WebView，独立 origin）
Host 面     — GitHub API、Docker CLI、安装目录、加密 env、生命周期、安全门禁
```

### 3. 外部获取与运行约束（v1）

- 仅 GitHub Release 资产；**不** clone 源码、**不**本地 build、**不**解析 Release body 自然语言。
- 可识别信号：`docker-compose.y{a}ml` / `compose.y{a}ml` / `natives.compose.zip` / `natives.app.json`。
- Docker Run 必须由清单提供 `image + port`；Compose 可无清单探测。
- 无容器信号 → 不可安装；无 Docker → 阻断新安装（不允许「仅登记」假条目）。
- 仅调用 Docker CLI（参数数组 + `tokio::process::Command`）；不引入 Docker SDK。
- 端口绑定强制 `127.0.0.1`；不静默换随机端口。
- 资源标签：`ai.natives.creative-app.id={id}`；Compose project：`natives-{appId}`；Run 容器名：`natives-ca-{appId}`。

### 4. 子 WebView 安全边界

- 启用 Tauri `unstable`，`Window::add_child` 创建**单个可复用**子 WebView。
- 初始 URL 只能是后端生成的 `http://127.0.0.1:{port}{path}`。
- 导航仅允许 `http` / `https`；阻断 `file:` / `tauri:` / `data:` 与自定义协议。
- Capability 只授权主应用 webview label；外部子 WebView **不**配置 remote capability。
- 新窗口请求在同一子 WebView 内导航或明确阻断，不弹系统浏览器。

### 5. 状态权威

- 外部状态以 Docker 实际探测为准；DB 只保存期望状态与最后一次操作结果。
- 未知 / 探测中禁止显示为「运行中」。
- 成功或失败的安装 / 启停 / 删除广播 `db-state-changed` channel = `creative-app`。
- v1 使用**全局异步变更锁**串行安装与生命周期写操作。

### 6. 密钥与 env

- GitHub Token 与应用 env 使用既有 AES-256-GCM（`env_manager`）加密。
- Token 存 settings；查询接口只返回是否配置 + 掩码；明文提交后立即从前端清空。
- Token 仅用于 GitHub API / 私有 Release 资产下载；**不**自动 Docker Registry 登录。

### 7. 本地产物

```text
~/.natives/creative-apps/{appId}/
├── release/   # 原始 Release 资产
└── runtime/   # 规范化 compose 等运行产物
```

- SQLite 是唯一元数据权威；**不**双写 `meta.json`。
- ZIP 防路径穿越与符号链接逃逸；单文件 ≤ 100 MiB，总计 ≤ 200 MiB。
- 删除：先 Docker 资源 → 再目录 → 再 DB；核心失败保留 `delete_failed`。

---

## 明确不做（v1 非目标）

1. 联网商店上架 / 发现 / 订阅 / 评分。
2. Release body 自然语言解析、源码 clone/build、任意宿主脚本。
3. 无 Docker「仅登记」、随机端口、自动更新。
4. 自动登录 GHCR 或其他镜像仓库。
5. 默认删除 Docker 卷 / 镜像 / 宿主 bind mount 数据。
6. 外部应用 Workshop Bridge、主应用 Tauri API 或宿主凭证注入。
7. Docker SDK、`meta.json` 双写、通用插件运行时接口、并行安装框架。
8. Windows / Linux 专属 Docker 分支（代码保持跨平台；验收以 macOS 为准）。
9. Podman 单独适配；HTTPS 自签或局域网暴露。

---

## 后果

### 正面

- 个人创意成为统一目录，同时不破坏 Workshop 安全模型。
- 外部容器生命周期可追踪、可恢复，状态不造假。
- Host 面集中 GitHub / Docker 能力，前端不提交任意 shell 或下载 URL。

### 负面 / 成本

- 需维护两套存储与生命周期适配器。
- 依赖本机 Docker；无引擎时外部路径完全不可用。
- Tauri `unstable` 子 WebView 存在后续 API 变动风险。

### 中性

- 内部模块继续走 `module_manager`；本 ADR 不复制安装 / 权限 / iframe 逻辑。

---

## 落地检查清单

- [ ] SQLite v7：`external_creative_apps`、`creative_app_env`
- [ ] `creativeApp` API 域 + 子 WebView 控制命令
- [ ] 统一列表投影 + `creative-app` 事件
- [ ] GitHub 探测 / 安装 / Docker 生命周期 / 恢复
- [ ] 个人创意 UI 去除伪商店；GitHub 向导与删除对话框
- [ ] 设置「运行时」页：Docker 状态 + GitHub Token 掩码
- [ ] 中英文 i18n、数据表清单、安全自检
- [ ] KI-1～KI-5 未被外部运行时绕过

---

## 修订

| 日期 | 变更 |
|------|------|
| 2026-07-22 | 初版，升格设计冻结与实施方案为 ADR |
