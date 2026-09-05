# ADR-0024：个人空间 = Tabliss 多 Workspace Dashboard（取代 Structured/Free 布局合同）

## 状态

已接受（2026-08-30）

- **取代（范围）**：
  - ADR-0021 §2「Structured Canvas + Free Canvas 双布局」（`structured | free` 词表、`react-grid-layout` 12/8/4 断点、自研 Free Canvas 的 Drag/Resize/Pan/Zoom/Multi-select/Group/Frame/Z-order 合同）——由 **Tabliss 原生布局**取代（见决策 2）。
  - ADR-0023「`newtab.html` 是轻量工作台，不读取文件系统、不创建 Native Messaging 连接」与「新标签页初始 JS（gzip）≤ 50 KB」——由**个人空间短连接 + 新资源预算**取代（见决策 5、6）。
  - ADR-0021 §2 PWSV2 修订 9 的「左侧 floating toolbar Browse/Edit 双态」——个人空间内由**右侧 Inspector** 承载编辑能力，Browse 模式天然安静。
- **保留**：
  - ADR-0021 的 Multi-Workspace 实体边界（Create/Open/Close/Reopen/Pin/Reorder/Rename/Duplicate/Template/软删 Delete）、Host SQLite authority、`workspace_open_tabs` 会话语义、模板生命周期（builtin Classic/Blank/Focus + personal，实例化单事务、升级不覆盖）、Browse 安静纪律（指针移动 IPC/DB 写 = 0）。
  - ADR-0022 的主题权威收敛（主题/语言是全局 `settings`，Workspace 不驱动主题）。
  - ADR-0023 的按需 Host 生命周期、stdin EOF 2 秒退出、60 秒空闲断开、路径安全与 Secret 归 OS Keychain。
- **关联**：`docs/contracts/workspace-v2-contract.md`、`docs/adr/0023-chromium-extension-files-surface.md`、TablissNG 上游（`/Volumes/UNTITLED/本人材料/project/TablissNG`）。

## 背景

产品新标签页（个人空间）要求提供 TablissNG 同级的「背景 + Widget 仪表盘」体验：29 个 Chromium Widget、9 个背景、九宫槽位与自由定位，每个 Workspace 独立保存背景/Widget/配置/布局。ADR-0021 的 Structured/Free 双布局是为「工作台容器」设计的，与 Tabliss 已验证的轻量仪表盘交互（九宫槽位 + free 百分比定位/缩放/旋转）重叠且更重。同时 ADR-0023 禁止新标签页连接 Native Host，使 Workspace 数据只能存浏览器侧，违反 ADR-0021 的 Host SQLite authority。

TablissNG 源码（Chromium 目标）已确认可按 Natives 目标许可证重许可；其实际注册矩阵为：

- **Widgets（24）**：binaryTime, bookmarks, countdown, css, currencyRates, customText, github, greeting, html, ipInfo, links, message, notes, palette, quote, search, since, tallyCounter, time, todo, topSites, trello, weather, workHours。**排除/退役**：nba（上游已注释损坏）、js 与 randomMessage（前者仅 Web 构建启用、后者上游已移除）；以及 2026-09 退役精简的 5 个组件：joke, bitcoin, leetcode, literatureClock, timeTracker（详见修订 2026-09）。
- **Backgrounds（9）**：apod, bing, colour, giphy, gradient, media, online, unsplash, wikimedia。
- **布局模型（Tabliss 原生）**：九宫槽位 `topLeft | topCentre | topRight | middleLeft | middleCentre | middleRight | bottomLeft | bottomCentre | bottomRight` + `free`；`free` 使用 `x/y` 或 `xPercent/yPercent` 百分比定位、`scale`、`rotation`；拖拽/缩放过程零持久化，stop 提交一次。
- **全局偏好（并入 Natives 系统设置）**：locale（仅 `zh_CN`/`en`）、themePreference、accent、timeZone、favicon、highlightingEnabled、hideSettingsIcon。

## 决策

### 1. 个人空间 = 固定三栏 Shell

`space.html` 取代 `newtab.html` 成为新标签页（`chrome_url_overrides.newtab = "space.html"`）：

```text
左侧栏（默认 248px，可拖拽调宽）        中央 Dashboard（全铺）         右侧 Inspector（默认 360px，240–620px）
Natives 品牌 + 全局搜索                 Tabliss Dashboard              总览：背景 / Widget 目录 / 实例列表
「空间」分组：Workspace 树               （Shadow DOM 隔离）            实例配置：该 Widget 完整设置
「文档」分组：桌面/下载/文档 → files.html                               导入 / 导出 / 重置
底部：设置（系统设置弹窗）
```

- 个人空间不显示文件页的 `toggle-sidebar`，避免与 Dashboard 的 Widget 显隐工具重复；左侧栏保持可见并沿用 190–420px 拖拽调宽与宽度持久化。文件页自身的侧栏折叠行为不变。
- Inspector 不提供「移至底部」与「最大化」；窄屏（≤1180px）改为全屏覆盖。
- 中央 Dashboard 全铺且不得影响 `files.html` 文件页布局。

### 2. Tabliss 原生布局（取代 Structured/Free）

- 布局词表冻结为 Tabliss 的 `position` 九宫 + `free`；DB 与 wire 一律使用该词表。ADR-0021 的 `structured|free`、断点三元组、`workspace_layouts` 表**不再**用于个人空间。
- Widget 实例显示/变换字段沿用 Tabliss `WidgetDisplay` 白名单子集：`position, x, y, xPercent, yPercent, scale, rotation, colour/useAccentColor, fontSize, fontWeight, fontStyle, textDecoration, textOutline*, customClass, disabled`。
- 指针拖拽/缩放/旋转过程中 Host 写入为 0，只在 stop/commit 提交一次（延续 R-B11）。
- 焦点模式（focus）隐藏 Inspector 与交互、全屏沿用上游交互。

### 3. Dashboard 隔离

- Tabliss Dashboard 渲染在 **Shadow DOM** 内；Tabliss 样式与用户 Custom CSS 只允许注入该 Shadow Root，不得影响 Natives 侧栏、Inspector、系统设置与 `files.html`。
- Custom HTML / Wikimedia 等远端 HTML 经原生 DOM allowlist 清洗后进入 Shadow Root；Custom CSS 以 text 注入 Shadow Root 内 `<style>`；禁止远程脚本与任意代码执行（`js` widget 按上游 Web 目标口径不迁移）。

### 4. 数据与协议：Host SQLite 单一权威

- 库文件 `~/.natives/natives.db`（rusqlite bundled SQLite），增量幂等迁移建立最小模型：
  - `settings(key TEXT PK, value TEXT, updated_at)`：全局外观/语言/时区/favicon/accent。
  - `workspaces(id TEXT PK, name TEXT NOT NULL, sort_order INTEGER NOT NULL, pinned INTEGER NOT NULL DEFAULT 0, background_json TEXT NOT NULL, template_source TEXT, deleted_at TEXT, revision INTEGER NOT NULL, updated_at TEXT NOT NULL)`。
  - `workspace_open_tabs(workspace_id TEXT PK REFERENCES workspaces(id), sort_order INTEGER, is_pinned INTEGER, opened_at TEXT, last_active_at TEXT)`：Close=删行，Reopen=插行。
  - `workspace_widgets(id TEXT PK, workspace_id TEXT NOT NULL, key TEXT NOT NULL, order INTEGER NOT NULL, enabled INTEGER NOT NULL, config_json TEXT NOT NULL, display_json TEXT NOT NULL, config_version INTEGER NOT NULL)`。
  - `workspace_templates(id TEXT PK, origin TEXT CHECK(origin IN ('builtin','personal')), name TEXT, payload_json TEXT NOT NULL, created_at TEXT)`；内置 `classic`/`blank`/`focus` 由代码提供 manifest。
- 所有 mutation 携带 `expectedRevision` 乐观并发；不匹配返回类型化冲突错误。创建/复制/模板实例化/重置/删除在**单事务**完成。
- 跨新标签页经 `BroadcastChannel('natives-workspace')` 广播 revision 失效通知；浏览器存储只保存首帧缓存与可丢弃网络缓存，**不是**第二权威。
- Native Messaging 新增类型化方法族（全部进入协议白名单与 params 校验）：
  `workspace_session`（session snapshot：openedTabs + activeWorkspaceId + workspaces metadata + revision）、`workspace_snapshot`（单 Workspace 全量）、`workspace_create`、`workspace_rename`、`workspace_reorder`、`workspace_pin`、`workspace_duplicate`、`workspace_delete`（软删）、`workspace_open/close/reorder_tabs`、`workspace_widget_upsert/remove/reorder`、`workspace_background_save`、`workspace_instantiate_template`、`workspace_save_from_tabliss`（Tabliss v2/v3 JSON 导入，校验+预览+事务回滚）、`settings_get/set`。
- Chrome 不允许读取另一扩展私有存储：不提供「自动读取旧 Tabliss 扩展数据」，仅提供 v2/v3 JSON 文件导入。

### 5. ADR-0023 修订：个人空间允许短连接 Native Host

- `space.html` 打开时按需 `connectNative`；`pagehide`/关闭断开；隐藏且空闲 60 秒断开、可见按需重连——生命周期规则与 `files.html` 完全一致。
- Service Worker 约束不变：不得持有 Native Port、状态、轮询或保活计时器。
- 拖拽/缩放/旋转期间 Host 写入为 0；pointermove 不得触发 IPC。

### 6. Manifest、权限与网络

- Manifest 增加 `search`、`identity`；`bookmarks`、`topSites` 为可选权限（对应 Widget 首次使用时请求）。
- 网络 CSP 仅允许已登记 API 域名（api.github.com、api.trello.com、api.giphy.com、api.unsplash.com、api.nasa.gov、api.open-meteo.com 等）；禁止远程脚本、eval 与任意 URL 代理。
- Trello OAuth：Service Worker 仅执行一次性 `launchWebAuthFlow` 并把 token 直接交给 Host；Host 存 OS Keychain，以固定 Trello 域名 + 动作枚举代理 boards/lists/cards/labels 及卡片增删改移动；拒绝通用 URL 代理；超时、响应大小上限、参数白名单、可取消。
- GIPHY/Unsplash/NASA/Trello key 缺失时保留入口并显示双语「未配置」状态；不提交占位 key、不伪装成功。首个无 key 构建允许通过。

### 7. 构建与语言（偏离说明）

- 统一使用仓库既有零构建 vanilla ES Module + Shadow DOM + 浏览器原生能力实现 Shell 与插件注册（与 files 页一致，无第二包管理器、无 React、无 Rspack、无 iframe/WebView）。插件契约对齐 Tabliss `Config`（key/name/dashboard/settings/defaultData）的 vanilla 化。
- 语言只支持 `zh_CN`/`en`，两套文案键严格同步（沿用 `_locales` 断言）。

### 8. 资源预算（取代 ADR-0023 V1 门禁中对应行）

在同一设备（Apple Silicon/16 GB/macOS/Release）实测后按「实测值 × 1.1 向上取整到固定粒度」设定，作为新的固定门禁：

| 项目 | 新门禁 |
|---|---:|
| 扩展 ZIP（gzip 估算） | ≤ 250 KiB（保持原 ADR-0023 门禁；实测 105 KB） |
 ≤ 100 KiB（首切片无 Rspack；Rspack 落地后按 350 KB 初始 JS 上限重测） |
| 新标签页初始 JS（gzip） | 维持 ADR-0023 ≤ 50 KB 门禁（vanilla 路线无独立打包） |
 ≤ 350 KB（Rspack/React 落地后生效；首切片沿用 ZIP 门禁） |
| Native Host 单架构 Release 二进制 | ≤ 4 MB（实测 3.37 MiB × 1.1 ≈ 3.7 MiB，向上取整到 4 MB 粒度；引入 rusqlite bundled SQLite 后重测固定） |
| 平台安装包 | 维持 ≤ 10 MB，超出必须逐文件归因 |
| Native Host 空闲 RSS / CPU / 退出延迟 / 文件 p95 / IPC p95 | 沿用 ADR-0023 不变 |

- 禁止用 source map、未使用语言、其它平台构建或未启用插件撑大基线；现有文件 Surface 不得回退超过 5%。
- 每次门禁变更必须附同设备三方实测（当前 Natives / 未修改 Tabliss Chromium / 集成后）。

## 测试与验收基线

- Rust：幂等迁移、revision 冲突、软删过滤、复制/模板单事务、未知字段拒绝。
- 前端：29+9 注册矩阵、默认 Unsplash+Time+Greeting（无 key 显式未配置）、九宫/free 定位、缩放旋转、刷新恢复、pointermove 零写入。
- 安全：Custom HTML/XSS 清洗、Custom CSS 不出 Shadow Root、远端 HTML 清洗、非法 URL 拒绝、token 不入页面/存储/日志、Trello 代理拒绝任意路径。
- Shell：Workspace 树 CRUD、左侧栏调宽恢复、唯一 Widget 显隐按钮、右 Inspector 上下文切换/调宽/窄屏覆盖、Dashboard 全铺且不影响文件页。
- 数据：Workspace 隔离、Tabliss v2/v3 导入校验、失败回滚、导出重导入等价。
- 最终门禁：`npm run extension:check`、`npm run perf:check`、`cargo fmt --check`、精确 Rust 测试。

## 后果

### 正面

- 新标签页获得 Tabliss 同级仪表盘能力；Workspace 数据获得 Host SQLite 单一权威，跨设备与跨页一致。
- 布局合同收敛到 Tabliss 原生模型，删除 Structured/Free 双实现与 react-grid-layout 依赖面。
- Shadow DOM 隔离让用户自定义样式与 Natives 产品面完全隔离。

### 成本与约束

- ADR-0021 的 Structured/Free、react-grid-layout 断点与自研 Free Canvas 合同作废；后续若工作台容器需要自由画布，须新 ADR。
- 新标签页允许 Native 短连接后，`newtab.html` 静态无连接的测试口径作废，必须补连接生命周期测试。
- Host 引入 SQLite 依赖，二进制预算必须重测固定；rusqlite 使用 bundled特性以保证离线可构建。

## 修订

- **2026-09 资源预算上调**：扩展 ZIP 门禁由 250 KiB 上调至 **300 KiB**（+20%，用户指令），为「智能体配置」（Agent Clients，迁移自参考 GUI）与模型服务多页面功能预留空间；其余门禁不变。
- **2026-09 组件精简契约**：根据产品低噪音与维护成本审计，正式退役 5 个组件（`widget/joke`, `widget/bitcoin`, `widget/leetcode`, `widget/literatureClock`, `widget/timeTracker`），正式支持组件契约由 29 个调整为 24 个。
  - 启动阶段由 SQLite 幂等迁移从所有 Workspace 及个人模板 `payload_json` 中清理这 5 类退役组件实例；
  - 后端白名单 `WIDGET_KEYS` 严格收敛为 24 个，拒绝新增退役组件；
  - Tabliss v2/v3 配置导入时遇到上述 5 个退役 key 时主动忽略跳过，不计入有效导入组件数，未知 key 仍报错回滚；
  - 移除对应外部网络权限（`jokeapi.dev`, `mempool.space`, `alfa-leetcode-api.onrender.com`）。
- 本 ADR 取代 ADR-0021 §2、ADR-0023 §「浏览器与页面」中 newtab 相关行与资源预算行；其余条款继续有效。后续组件全量迁移、React+Rspack 落地、Trello/Keychain 切片各自独立验收，不阻塞本 ADR 的架构效力。
