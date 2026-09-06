# 第三方项目引用说明（REFERENCED_PROJECTS）

> 本文档记录 Natives 仓库引用/借鉴的外部项目与在线服务，以及每处引用
> 在仓库中的具体文件位置，便于 AI 与后续开发者理解来源与边界。
> 权威依据：`docs/adr/0019`、`docs/adr/0020`、`docs/adr/0024`、`docs/adr/0025`、
> `docs/architecture/provider-proxy-architecture.md`。

## 一、Vendored 进仓库的第三方代码

### 1. CLIProxyAPI v7（最小源码 fork，MIT）

| 项 | 值 |
| --- | --- |
| 上游 | `github.com/router-for-me/CLIProxyAPI/v7` |
| 锁定提交 | `f0de1d008fe8881dcb7431cf97b147295874c2b2`（ADR-0020） |
| 版本 | `v7.2.152`（`third_party/cliproxyapi/version.txt`） |
| 许可 | MIT（完整文本保留于 `third_party/cliproxyapi/LICENSE`） |
| fork 说明 | `third_party/cliproxyapi/NATIVES_FORK.md` |

**仓库内文件**（仅保留编译必需子集，故意排除 CLI / 管理 UI / 示例）：

- `third_party/cliproxyapi/go.mod` — module 声明
  `github.com/router-for-me/CLIProxyAPI/v7`
- `third_party/cliproxyapi/go.sum` — 依赖锁
- `third_party/cliproxyapi/sdk/` — SDK（`config` / `auth` / `cliproxy` 等）
- `third_party/cliproxyapi/internal/` — 编译必需的 internal 包与嵌入资源
- `third_party/cliproxyapi/LICENSE`、`NATIVES_FORK.md`、`version.txt`

**Natives 侧引用点**：

- `model-host/go.mod` — `replace github.com/router-for-me/CLIProxyAPI/v7 => ../third_party/cliproxyapi`
- `model-host/internal/cliproxy/` — 对 SDK 的封装（gateway 运行时）
- `model-host/THIRD_PARTY_NOTICES.md` — MIT 许可声明
- `extension/model-*.js`（18 个文件）— 模型设置 UI，消费
  `model-host` 的 Native Messaging 接口（OAuth、额度、使用记录、智能体客户端、
  认证文件）

**fork 修改边界**：仅 usage manager（有界背压队列 + dispatcher 可重启），
不引入 CLI、Management UI 或第二配置/凭证 authority（ADR-0020 §6.1）。

## 二、本地参考项目（未复制进仓库）

以下三个项目**不在本仓库内**，仅存在于开发机
`/Volumes/UNTITLED/本人材料/project/`。阅读本仓库对应代码时，可按本节
反查参考来源；反之，本仓库不包含它们的任何源码。

### 1. EasyCLIProxyAPI

- **参考位置**：`/Volumes/UNTITLED/本人材料/project/EasyCLIProxyAPI`
- **定位**：基于 CLIProxyAPI 的桌面 GUI 代理参考实现，**仅作 UX / 产品 /
  集成参考，永不当架构权威**（ADR-0019 §6）。不复制其 Rust/TS 布局、
  management API、settings/quota/usage DB、auth-file 架构。
- **引用内容 → 本仓库文件**：
  - 使用记录明细字段对齐（Time/Model/Input/Output/Cache/CacheRate/Total/
    Speed/TTFT/Latency/Cost/Status）→
    `extension/model-usage-view.js`、`extension/model-usage-importer.js`、
    `model-host/internal/usage/`
  - 智能体客户端（Agent Clients）生命周期与交互（11 类本机客户端探测、
    配置写入、Claude 角色映射）→
    `extension/model-agent-controller.js`、`model-agent-sections.js`、
    `model-agent-view.js`、`model-host/internal/agentclients/`
  - 认证文件与账号模型管理 → `extension/model-auth-files-view.js`、
    `model-account-models-dialog.js`、`model-host/internal/authfiles/`
- **引用出处**：`docs/adr/0019-unified-model-proxy-authority.md` §4/§6、
  `docs/architecture/provider-proxy-architecture.md`

### 2. TablissNG

- **参考位置**：`/Volumes/UNTITLED/本人材料/project/TablissNG`
- **定位**：个人空间（新标签页）「背景 + Widget 仪表盘」的交互与组件
  注册矩阵参考（ADR-0024）。按 Natives 目标许可证重许可后的**行为参考**，
  非源码复制。
- **引用内容 → 本仓库文件**：
  - Widgets（24 项上游注册矩阵，2026-09 退役 5 项后仓库保留 27 个实现文件）
    → `extension/plugins/widgets/`
  - Backgrounds（9 项）→ `extension/plugins/backgrounds/`
  - 九宫槽位 + 自由定位（百分比/缩放/旋转）交互 →
    `extension/space.js`、`extension/space-*.js`
  - Tabliss v2/v3 导入/导出兼容 → `extension/space-importer.js`、
    `crates/native-file-host/src/workspace_store/import.rs`
  - Workspace 数据模型（Host SQLite authority）→
    `crates/native-file-host/src/workspace_store/`
- **引用出处**：`docs/adr/0024-personal-space-tabliss-workspaces.md`、
  `docs/contracts/workspace-v2-contract.md`

### 3. FundVal-Live（AGPL-3.0，只审计、不复制）

- **参考位置**：`/Volumes/UNTITLED/本人材料/project/FundVal-Live`
- **定位**：基金应用（Phase F）的行为规格审计对象。**禁止复制任何源码**，
  技术栈（Django/React/Tauri/PostgreSQL/Redis）全部禁止引入（ADR-0025）。
- **仅提取的行为规格**：SourceRegistry、source fallback 链（requested /
  resolved / fallback_level 可观测）、行业穿透算法、持仓回放不变式、
  统一 provenance（source / fetched_at / as_of / confidence）。
- **本仓库涉及文件**：当前仅
  `docs/adr/0025-apps-packaging-and-extension-runtime.md`（D15 / §Fund 节）。
  后续 `Natives-App-Fund`（独立 crate，未创建）将包含
  `docs/reference/fundval-local-audit.md` 审计文档。

## 三、在线公共数据服务（浏览器侧 fetch）

扩展仅在 `extension/manifest.json` 的 `host_permissions` 中声明的域名，
由浏览器直接请求（ADR-0025：网络 I/O 全部留在浏览器侧，Host 不联网）。

| 域名 | 用途 | 使用文件 |
| --- | --- | --- |
| `api.open-meteo.com` / `geocoding-api.open-meteo.com` | 天气/地理编码 | `extension/plugins/widgets/weather.js` |
| `open.er-api.com` | 汇率 | `extension/plugins/widgets/currency-rates.js` |
| `api.coingecko.com` | 加密货币行情 | `extension/plugins/widgets/currency-rates.js` |
| `api.ipify.org` | IP 信息 | `extension/plugins/widgets/ip-info.js` |
| `github-contributions-api.jogruber.de` | GitHub 贡献图 | `extension/plugins/widgets/github.js` |
| `i.pravatar.cc` | 头像占位 | `extension/manifest.json`（host 声明） |
| `bing.biturl.top` | 必应每日壁纸（第三方镜像） | `extension/plugins/backgrounds/bing.js` |
| `api.unsplash.com` / `images.unsplash.com` | Unsplash 背景 | `extension/plugins/backgrounds/unsplash.js` |
| `api.nasa.gov` / `apod.nasa.gov` | APOD 背景 | `extension/plugins/backgrounds/apod.js` |
| `api.wikimedia.org` / `upload.wikimedia.org` | Wikimedia 背景 | `extension/plugins/backgrounds/wikimedia.js` |
| `api.giphy.com` | Giphy 背景 | `extension/plugins/backgrounds/giphy.js` |
| `www.google.com/s2/favicons` | 网站 favicon | `extension/plugins/widgets/links.js`、`bookmarks.js` |
| `duckduckgo.com` / `en.wikipedia.org` / `www.google.com` | 搜索建议 | `extension/plugins/widgets/search.js` |

测试侧 mock：`extension/space-plugins-smoke.test.mjs`、
`extension/space-deep-verify.test.mjs`（离线断言，不产生真实网络请求）。

## 四、压缩包范围说明（本仓库 archive）

- **包含**：全部 git 跟踪文件（`extension/`、`crates/`、`model-host/`、
  `third_party/cliproxyapi/`、`docs/`、`scripts/`、`installers/`、
  根配置与本文档）。
- **不包含**：任何构建产物 —— `node_modules/`、`target/`、`dist/`、
  `*.db`、`.env`、凭据文件（均已被 `.gitignore` 排除且未跟踪）。
- **不包含**：第二节所列本地参考项目（EasyCLIProxyAPI / TablissNG /
  FundVal-Live）——它们不在本仓库内。
