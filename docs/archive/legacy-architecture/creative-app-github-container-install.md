# 个人创意 · 外部应用安装（GitHub Release → 容器）设计冻结

> **状态**: 已升格实施（见 [ADR-0013](../adr/0013-creative-app-dual-source.md)）  
> **日期**: 2026-07-22  
> **来源**: grill-me 会话（个人创意增强 / GitHub 安装 / 内置浏览器展示）  
> **关联**: [ADR-0012](../adr/0012-product-identity-workshop-scope.md)、[ADR-0013](../adr/0013-creative-app-dual-source.md)、[module-workshop-kernel-runtime.md](./module-workshop-kernel-runtime.md)、`docs/standards/product/01-positioning.md`  
> **归类**: **Workshop 面（目录与管理）** + **Embed 面（外部运行时展示）**；**非** Workshop 静态沙箱租户扩展

---

## 一、用户需求（原话意图 → 产品表述）

### 1.1 一句话

在 **个人创意** 里统一管理两类东西：  
1）**内部项目**——在 Natives 内由 AI 按规范生成页面；  
2）**外部项目**——从 GitHub **Release（不是 clone 源码）** 以 **容器方式** 安装部署到本机，装完后在创意列表里有快捷入口，点开用 **内置浏览器** 直接访问；并支持 **启动 / 关闭 / 删除**。

### 1.2 核心目的

| 目的 | 说明 |
|------|------|
| 发现与获取 | 把「比较 OK 的」开源/自有项目，通过 GitHub 地址装进本机 |
| 可部署 | 优先 Docker/Compose 发行形态，本机真正跑起来 |
| 统一入口 | 安装后出现在个人创意，与内部生成项同一列表管理 |
| 可打开 | 内置浏览器（WebView/浏览器壳）展示 GUI |
| 可运维 | 启动、关闭、删除，而不是只收藏一个死链 |

### 1.3 明确不做（本设计范围）

| 不做 | 原因 |
|------|------|
| `git clone` 源码后本地构建 | 用户纠正：读 **Release**，不拉开发树 |
| 外部项目塞进 Workshop Unique Origin 沙箱 | Docker/独立 origin/登录态与 KI-5 等不变量冲突 |
| 无容器信号时降级为静态 zip / 源码安装 | 外部路径冻结为 **容器优先** |
| 联网商店上架/发现/订阅 | ADR-0012 P2，本设计不碰 |
| 把 MCP/Agent 与创意应用揉进同一 manifest | 继续双轨；本设计只管「创意应用」目录与外部容器适配器 |

---

## 二、已拍板决策（grill 会话）

| # | 议题 | 决策 |
|---|------|------|
| D1 | 统一到哪一层 | **管理面统一、运行时分轨**：同一创意列表与启停删；内部 = Workshop 沙箱；外部 = 独立 origin + 容器进程 |
| D2 | 显示底座 | **内置浏览器** 作为 GUI 展示壳（内部/外部都走浏览器表面，安全模型不同） |
| D3 | 两种来源 | **内部**：AI 按规范生成；**外部**：GitHub Release 容器安装 |
| D4 | 生命周期 | 安装后支持 **启动 / 关闭 / 删除**（UI 统一，适配器实现不同） |
| D5 | 安装交互 | **一键安装 + 手动安装** 并存；一键失败回落手动并向导带上已探测结果 |
| D6 | 外部获取方式 | **GitHub 地址 → 读 Releases**，**不 clone 源码** |
| D7 | 外部可装形态 | **容器优先**：compose 资产 和/或 镜像元数据；两者都无 → **不可装** |
| D8 | 探测规则 | **Compose 方案 + 镜像 run 方案都认**；并存时并列，一键走更高置信（通常 compose） |
| D9 | GitHub 范围 | **公开仓零配置**；**可选 GitHub Token** 读私有 Release；一键默认 **latest**（排除 pre-release）；手动可选 tag / 可含 pre-release |

---

## 三、后续议题（按推荐补全，供实施直接采用）

> 以下为 grill 未逐题拍板、按「追求效果 + 与 ADR-0012/无假数据一致」写入的推荐冻结。实施方案若要改，应显式改本节并回写原因。

### D10 · 本机无 Docker 时（推荐：**硬依赖 + 可「仅登记」手动例外**）

- 安装拉取镜像/启动前检测 Docker 引擎与 Compose 可用性。
- **默认**：不可用 → **阻断**安装/启动，引导安装或启动 Docker Desktop（或等价引擎）；**禁止**显示为「运行中」。
- **手动模式可选**：「仅添加创意条目、暂不 pull」（状态 = `runtime_missing`），有引擎后再启动。
- **禁止**：无 Docker 时假装用静态/外链降级完成「容器安装」。

### D11 · 一键 vs 手动分界（推荐：**置信度门槛**）

| 条件 | 模式 |
|------|------|
| 唯一高置信方案（标准 compose 资产可解析端口，或 `natives.app.json` 完整；或单一镜像 + 端口元数据）且 Docker 可用、凭证齐 | 允许 **一键** |
| 多方案冲突、缺端口、需任意额外 shell、私有仓无 token、仅 pre-release | **强制手动** 或一键入口灰掉并说明原因 |
| `proc-command` / 任意脚本 | **永不进一键**（本外部路径甚至不作为一等适配器；见 D12） |
| UI | 始终保留「手动安装」入口 |

### D12 · 外部运行时适配器集合（推荐：v1 仅容器两条）

| 适配器 | v1 | 说明 |
|--------|----|------|
| `docker-compose` | ✅ | Release 含 compose 资产（见 D14） |
| `docker-run` | ✅ | 镜像引用 + 端口等元数据 |
| `workshop-static` | ✅（内部） | 既有 AI 生成 / 合规静态模块，不经 GitHub Release 容器路径 |
| `static-server` / `proc-command` / `url-only` | ❌ v1 不做 | 避免与「容器优先、不降级」冲突；可 P1 再开 |

### D13 · 产品对象模型（推荐：**创意应用 Creative App**）

个人创意列表中的一等条目：

```text
CreativeApp {
  id                    // 内核生成，非用户/模型随意指定
  source: internal | external_github
  title, icon, description
  runtime: workshop_static | docker_compose | docker_run
  state: ...            // 见 D16
  entry: { openUrl, healthUrl? }
  lifecycle: { start, stop, delete }  // 由适配器实现
  external?: { owner, repo, releaseTag, assetIds, imageRef?, composePath?, tokenRef? }
  internal?: { moduleId, contractId, domain... }  // 既有 Workshop 字段
}
```

- **禁止**用单一旧 `modules` manifest 硬揉 SPA 与容器进程（ADR-0012 双轨精神）。
- 列表 UI 可投影为同一张卡片；存储与权限命名空间分开。

### D14 · Release 可识别信号（推荐：约定资产 + 可选清单）

**Compose 方案命中（任一）：**

- 资产名：`docker-compose.yml` / `docker-compose.yaml` / `compose.yml` / `compose.yaml`
- 或：`natives.compose.zip`（内含 compose，可选 `.env.example`）
- 或：`natives.app.json` 中声明 `runtime: docker-compose` 且指出 compose 资产名

**镜像 run 方案命中：**

- `natives.app.json` 中 `image` + `port`（推荐）
- 或 Release body 中的约定块（实施时可先只支持 JSON 清单，body 解析作增强）

**`natives.app.json` 最小建议字段（发行方可选，有则一键更稳）：**

```json
{
  "name": "my-app",
  "runtime": "docker-compose",
  "composeAsset": "docker-compose.yml",
  "image": "ghcr.io/org/app:1.2.3",
  "port": 8080,
  "openPath": "/",
  "healthPath": "/health",
  "env": [{ "key": "API_KEY", "required": false }]
}
```

无清单时：尽量从 compose 端口映射推断；推断失败 → 仅手动，要求用户填主机端口与打开路径。

### D15 · 安装流水线（推荐：完整七段）

```text
1. 解析 GitHub URL / owner/repo
2. 鉴权：公开 or Token
3. 拉 Release 列表 → 一键取 latest 非 pre；手动可选 tag
4. 扫资产与 natives.app.json → 生成方案列表（compose / run）
5. 用户确认（一键=隐式确认唯一方案）→ 下载资产到本地安装目录
6. 供给：compose up / docker pull+run（需 Docker）
7. 就绪：health 或 TCP/HTTP 探针通过 → state=running，可打开内置浏览器
```

失败：可诊断错误（无容器信号 / Docker 未开 / pull 失败 / 端口占用 / health 超时），一键失败进入手动并保留探测结果。

### D16 · 状态机（推荐）

```text
discovered → installing → installed_stopped
                ↓              ↓
            install_failed   starting → running → stopping → installed_stopped
                                ↓                    ↓
                           start_failed          (可再 start)
任何已安装态 → deleting → (removed)
runtime_missing：已登记但引擎不可用（仅手动「仅添加」）
```

- UI **无假数据**：端口、版本、运行状态必须来自探测/Docker/健康检查真实来源；未知显示「未知/检测中」，不编造「在线」。

### D17 · 启动 / 关闭 / 删除语义

| 动作 | 内部（workshop_static） | 外部（docker_*） |
|------|-------------------------|------------------|
| 启动 | 打开/挂载沙箱 iframe 或内置浏览器表面到模块 URL | `compose up -d` / `docker start` 或等价；探针通过后打开 |
| 关闭 | 卸下展示表面、可停保活 | `compose stop` / `docker stop`；**默认不删容器**以便快启 |
| 删除 | 卸模块；domain 数据是否清除单独确认 | stop → 移除容器（compose down）→ 删本地安装目录与创意条目；**卷/镜像是否删除**默认：容器与 compose 资源删，**镜像保留**；高级选项可「同时删镜像/卷」 |

### D18 · 端口与打开 URL（推荐）

- 主机端口：来自清单 / compose 映射 / 用户覆盖。
- 冲突：启动前检测；冲突则手动改映射或提示失败，**不静默改到随机端口**（避免书签失效）；若用户勾选「自动避开冲突」可作为手动高级项。
- `openUrl`：`http://127.0.0.1:{port}{openPath}`（默认 path `/`）。
- 仅本机回环；v1 不强调局域网暴露。

### D19 · 环境变量与密钥（推荐）

- compose/run 所需 env：安装向导收集；**密钥走现有凭证/安全存储**，不进明文 SQLite 日志。
- `.env.example` 可预填键名；值由用户填。
- 不把 GitHub Token 注入容器，除非用户显式配置某 env。

### D20 · 更新（推荐：v1 手动检查 Release）

- 卡片可「检查更新」：对比当前 `releaseTag` 与 latest 可装 Release。
- 更新 = 停 → 拉新资产/镜像 → 起 → 健康检查；失败可回滚到上一 tag（若本地仍保留）。
- v1 **不做**静默自动更新（与「插件不自动联网更新」精神一致）。

### D21 · 内置浏览器表面（推荐）

- 外部应用：独立 origin 的 WebView/内置浏览器标签（**Embed 策略**），弱/零 Bridge；**不**套 Workshop `allow-same-origin` 沙箱假设。
- 内部应用：继续现有 iframe 沙箱 + Bridge。
- 用户感知：都在 Natives 内打开；开发者感知：两套策略。

### D22 · 安全边界（推荐 v1）

- 只跑用户明确安装的镜像/compose；不执行 Release 内任意脚本作为安装器。
- compose 文件需经基础校验（禁止明显离谱的 privileged 可在 v1 警告，P1 可策略化）。
- 容器网络默认 bridge；不默认挂载宿主机敏感目录；若 compose 要求 mount，手动模式展示风险摘要。
- 个人创意列表事件驱动刷新（安装/启停/删成功后广播），与 ADR-0012 热上架精神一致。

### D23 · 存储位置（推荐）

- 外部安装根目录：`~/.natives/creative-apps/{appId}/`（或项目既有数据根下等价路径）
  - `release/` 下载资产
  - `runtime/` compose 工作副本、生成的 `.env`
  - `meta.json` 与 DB 条目双写时以 **DB 为权威列表**，磁盘为产物
- 内部模块：保持现有 module_manager 路径，不迁入上述目录。

### D24 · UI 入口（推荐）

- **个人创意** 主列表：内部 + 外部同一信息架构（来源徽章区分「生成」/「GitHub」）。
- **添加**：
  - 生成内部模块（既有）
  - **从 GitHub 安装**（本设计）→ 一键 / 手动
- 设置：GitHub Token（可选）、Docker 状态展示（真实检测结果）。

### D25 · i18n 与合规

- 所有用户可见文案中英同步。
- PR 必须声明：Hub / **Workshop（列表）** / **Embed（外部打开）**；web-module vs 外部容器适配器。
- 不在 `/store` 等位置表述「已联网上架」完成态（ADR-0012）。

---

## 四、端到端体验（目标效果）

### 4.1 外部：一键

1. 个人创意 → 添加 → 从 GitHub 安装  
2. 粘贴 `https://github.com/org/repo`  
3. 系统读 latest Release → 命中唯一 compose 方案 → Docker 可用  
4. 一键安装：下载 → pull/up → health → 列表出现「运行中」  
5. 点击 → 内置浏览器打开 `http://127.0.0.1:port/`  
6. 可关闭（停容器）、再启动、删除  

### 4.2 外部：手动

1. 同前，但多方案或需填端口/env/选 tag  
2. 方案卡片：Compose vs Run；选 tag；填端口与打开路径  
3. 确认后同流水线；失败可看日志与重试  

### 4.3 内部（不变哲学）

1. AI 按规范生成静态页 → Workshop 安装/热上架  
2. 同一列表展示 → 启动即打开沙箱页 → 关闭/删除按模块语义  

### 4.4 不可装

- Release 无 compose 资产且无镜像元数据 → 明确「此仓库没有可安装的容器发行」，**不** clone、不静态降级。

---

## 五、与现有架构的接缝

| 现有能力 | 用法 |
|----------|------|
| `module_manager` / Workshop KI | **仅内部** web-module；外部不走 write_generated 契约门禁 |
| iframe-manager / 沙箱 | 内部；外部改 Embed 浏览器表面 |
| `sidecar_supervisor` 等进程监督 | 可作 Docker 生命周期监督的参考/复用点（实施时评估，不在本文写死 API） |
| 凭证存储 | GitHub Token、容器 env 密钥 |
| 列表热刷新 | 安装/启停/删成功广播 → 个人创意订阅 |

**新接缝（实施需建）：**

1. GitHub Release 客户端（公开 + Token）  
2. Release 资产探测器（方案生成）  
3. Docker/Compose 适配器（start/stop/delete/health）  
4. Creative App 持久化与状态机  
5. 个人创意 UI：双来源列表 + 添加向导 + 启停删  
6. 外部用内置浏览器打开 `openUrl`  

---

## 六、实施拆解建议（非排期，仅工作包）

1. **领域模型与 DB**：Creative App 表/命名空间、状态枚举、与 module 表隔离  
2. **Docker 探测与适配器**：compose / run、日志、健康检查  
3. **GitHub Release 管道**：解析 URL、list releases、download asset、私有 Token  
4. **安装向导 UI**：一键/手动、方案卡片、错误态  
5. **列表与生命周期 UI**：启动/关闭/删除、来源徽章、真实状态  
6. **内置浏览器 Embed 打开**：外部 URL；与内部沙箱入口分流  
7. **设置**：GitHub Token、Docker 状态  
8. **测试**：无 Docker、无可装资产、端口冲突、一键失败回落、删除不留假入口  
9. **文档**：必要时升格 ADR；更新 glossary/定位若引入「创意应用」正式术语  

---

## 七、验收清单（设计级）

- [ ] 内部生成项与外部 GitHub 容器项出现在同一「个人创意」列表，来源可区分  
- [ ] 外部仅通过 GitHub Release 容器信号安装；无信号时不可装且文案诚实  
- [ ] 不 clone 源码作为安装路径  
- [ ] 一键与手动均可用；一键失败进入手动并保留探测结果  
- [ ] 公开仓零配置；Token 可装私有 Release  
- [ ] 启动/关闭/删除对外部容器语义正确；删除后列表无残留假在线  
- [ ] 运行中可在内置浏览器打开真实本地 URL  
- [ ] 无 Docker 时不出现虚假「运行中」  
- [ ] 外部不进入 Workshop Unique Origin 租户模型  
- [ ] i18n 双语文案齐全；无假数据字段  

---

## 八、修订记录

| 日期 | 变更 |
|------|------|
| 2026-07-22 | 初版：grill-me 已拍板 D1–D9 + 推荐补全 D10–D25，供实施方案使用 |
