# 文件管理器模块审计：现状清单 × fanbox 对照 × 迁移方案

> 2026-07-26 调研产出。对照对象：`/Volumes/UNTITLED/本人材料/project/fanbox`（server.js 2450 行 + app.js 5061 行 + Electron 层）。
> 归类：本模块属 **Hub** 面（文件/终端/Agent 工作台基础设施），非 Workshop/Embed。

当前文件产品入口为 `extension/newtab.html` → `extension/files.html` → 按需 Native Host；本节早期 Tauri 文件模块清单仅作为迁移审计基线，不代表当前生产入口。

## M0 证据冻结（2026-08-29）

本次冻结只记录可复核证据，不把未完成的 Chrome 实测写成通过：

| 项目 | 证据与结论 |
|---|---|
| 代码基线 | HEAD=`d06e20c1a65660c449535df6e613df05ae89ded1`；工作区已有 1,527 个用户修改路径，未覆盖或回退。 |
| fanbox 参照 | 只读目录没有可启动的 `.app`；以 `assets/screenshot-volt.png`、`assets/screenshot-archive.png`、`assets/screenshot-index.png` 及 `public/app.js` / `server.js` 的真实实现为行为证据。 |
| fanbox 行为 | 左侧快捷入口持续可见；中间目录支持列表/网格、排序、隐藏文件、创建与导入；选中条目后下方/右侧出现预览；源码确认 `/api/roots`、`/api/list`、`/api/recent`、`/api/disk-usage`、`/api/rename`、`/api/create`、`/api/trash` 等文件链路。 |
| Natives 入口 | `manifest.json` 的新标签页为 `newtab.html`；`newtab.html` 仅提供 `files.html` 入口；`files.js` 通过 `native-client.js` 按需连接真实 Native Client；`background.js` 无 Native Port、轮询或保活。 |
| Host 注册 | 当前扩展 ID 与 macOS Native Messaging `allowed_origins` 一致，清单路径指向 `target/debug/native-file-host`；`extension:check` 与开发扩展 ID 自检通过。 |
| 独立磁盘闭环 | 临时目录 Native Messaging 实测 `roots`、父目录 `list_dir`、进入子目录 `list_dir`、stdin EOF 退出均通过；返回条目与实际磁盘文件名一致。 |
| 资源基线 | Release Host 1,481,360 bytes、RSS 6,176 KB、EOF 4 ms、10,000 项目录 p95 30.68 ms；扩展估算包 73,712 bytes，均在 ADR-0023 对应预算内。CPU/GPU 便携探针未实现，保持 unsupported。 |
| Chrome UI | **BLOCKED**：2026-08-29 再次通过 Chrome 控制会话发现真实标签页 `Natives 文件管理`（扩展 ID=`kooobajbofcajlblcannmdckihiejdec`），但接管该 `chrome-extension://.../files.html` 标签页时仍被 Browser URL policy 明确拒绝；未伪造 DOM、连接、关闭或断线恢复结果。 |

M0→M1 差距：Native Host 与磁盘链路已有可复核闭环，安装清单的 Host 路径与 `allowed_origins` 也和当前 Chrome 扩展 ID 一致，但仍缺真实 Chrome 页面可控性；Chrome 控制恢复后，首个实测应从新标签页 0 Host、文件页按需启动、真实目录进入/返回和页面关闭 EOF 开始。

## M1 接缝与开发自测证据（2026-08-29）

| 项目 | 证据与结论 |
|---|---|
| Native Client 接缝 | `extension/native-client.js` 独占真实 `chrome.runtime.connectNative`、请求 pending、超时、写请求计数和 EOF/断线清理；`files.js` 只消费业务响应。`extension/native-client.test.mjs` PASS。 |
| UI Harness | `files.html?ui-harness` 仅开发查询参数加载 `ui-harness.js`；Harness 复用正式 `files.html` DOM、`files.css`、`files.js`，通过真实 DOM click/input/dblclick/change 事件覆盖连接加载、选择、列表/网格、搜索、导航、语言、新建/重命名/废纸篓反馈；发布包清单排除 Harness。代码/语法门禁 PASS，Chrome 页面执行未宣称 PASS。 |
| Extension Self-Test | `files.html?self-test&self-test-root=<caller-owned-temp-root>` 不注入 Mock，使用正式 Native Client，并在页面 `<pre id="natives-test-report">` 生成不含完整文件内容/堆栈的 JSON；页面状态、Host 响应摘要和输入根目录均记录。正式 Chrome 执行因 URL policy BLOCKED。 |
| Host/磁盘 | Release Host 资源门禁 PASS（1,541,552 bytes、RSS 6,192 KB、EOF 5 ms、10,000 项目录 p95 28.15 ms）；Debug Host 独立临时目录真实 framing 验证：`roots.ok=true`，父目录返回 `alpha.txt` / `child`，子目录返回 `beta.txt`，stdin EOF 后退出并清理临时目录。 |
| Host 全链路临时磁盘回归 | 独立临时目录真实 Native Messaging framing 通过：`version`、`roots`、`list_dir`/进入子目录、内容搜索、文本读取、stat、创建、写入、重命名、批量复制/移动、分块导入、watcher `fs_changed`、ZIP 创建/清单/解压、废纸篓；每一步均以实际磁盘状态断言，随后清理临时目录。 |
| 体积门禁 | Extension distributable 估算 75,035 bytes，预算 256,000 bytes；发布清单只包含 `native-client.js`，不包含 `ui-harness.js` 或 `.test.mjs`。PASS。 |
| Host 模块收敛 | `main.rs` 生产代码只保留 Native Messaging 生命周期、调度组合和 EOF 清理；严格协议校验位于 `protocol.rs`，搜索、预览、批次、导入、会话和 Watcher 各自拥有语义模块；`file-manager-core` 继续作为文件规则 Facade。 |
| 最终全量门禁 | `cargo fmt --check`、`cargo test --workspace`（file-manager-core 40/40、native-file-host 36/36）、`extension:check`、`perf:files`、`perf:check` 均 PASS；`perf:check` 内含 Extension 4/4 性能回归。 |
| Chrome UI | **BLOCKED**：控制工具拒绝 `chrome-extension://kooobajbofcajlblcannmdckihiejdec/files.html` 的导航和 DOM 读取；未伪造 Harness/Self-Test/页面关闭/断线恢复的 Chrome 结果。 |
| 文件规模例外 | `extension/files.js` 当前 1,252 行，超过 1,000 行整改阈值；本轮不做高风险整体重写，Native Client 已独立为 `extension/native-client.js`。Extension 页面状态拆分仍是后续可维护性债务，不能以“目前能运行”作为永久例外。 |

## Foxbox 功能分类（迁移审计入口）

| 分类 | Foxbox 功能 | Natives 状态 |
|---|---|---|
| 文件管理器核心 | roots/磁盘、目录树、面包屑/地址栏、前进后退、列表/网格、排序/隐藏、单多选、收藏/最近、搜索、预览、编辑、自动保存、冲突保护、新建/重命名/复制/移动/副本/废纸篓、ZIP/TAR、Finder 拖放与目录导入、Watcher、磁盘占用、系统打开/显示/剪贴板 | 已迁移到 `extension` + `native-file-host` + `file-manager-core`，双语与响应式布局已接入 |
| 文件联动扩展 | 终端路径定位、终端 cwd 跟随、预览选中文字发送终端、文件拖入终端、截图目录跟随、Git diff/变更收件箱、会话回放 | 可在固定架构内迁移的路径定位与当前目录跟随已实现；终端会话 live follow、截图直通车和 Git/Agent 联动仍是缺口或边界项 |
| 非文件工作台 | Agent Runtime、供应商/账号、Skills、发布向导、第三方发布、嵌入式终端、浏览器标签/密码库、Electron/Tauri 壳 | 明确不迁移；不属于当前文件管理器生产架构 |

## 一、Natives 现状功能清单

### 1.1 代码分布

| 层 | 位置 | 规模 |
|---|---|---|
| 前端组件 | `src/components/files/`（19 文件） | 5603 行 |
| 前端 lib | `use-file-drop` / `useFileContent` / `recent-files-client` / `favorites-client` / `file-icons` / `use-thumbnail` / `web-fs-client` | — |
| IPC 适配 | `src/lib/tauri-adapter.ts:1251-1370`（fs/archive/search/disk/thumbnail/fsWatch） | — |
| Tauri 命令 | `src-tauri/src/commands/{fs,archive,search,disk,thumbnail,watch_preview}.rs` | — |
| Rust 核心 | `src-tauri/src/{file_manager,fs_watch,archive,thumbnail,disk_usage,search}.rs` | file_manager.rs 1547 行 |

### 1.2 已实现功能（摘要）

- **浏览**：网格/列表双视图（sm/md/lg 三档，localStorage 持久化）、IntersectionObserver 增量渲染（200 项/批）、后端排序（自然序 + 文件夹优先）+ 前端排序状态机（`file-sort.ts`，有单测）、隐藏文件、本地即时过滤（⌘F）。
- **导航**：后退/前进/上级、可编辑地址栏（⌘L，支持 `~`，文件路径自动解析为父目录+软选中）、Header 面包屑（`header-file-state` 事件桥）、Sidebar 快速入口 + 收藏。
- **选择与操作**：⌘/Shift 多选、⌘A 全选、F2 重命名、⌘D 副本、⌘C/X/V 应用内文件剪贴板、批量删除/移动、内部拖拽移动（EXDEV 跨卷回退）、外部拖入导入（Finder 文件 + `text/uri-list` 图片落盘）、三态右键菜单（file/dir/blank）。
- **预览**：图片（灯箱滚轮缩放）/视频/音频/PDF(iframe)/CSV/Markdown(Milkdown)/代码(shiki)/压缩包只读清单；`preview/code/git/info` 四 tab；Git diff（Monaco DiffEditor）。
- **预览（Preview Capability V2，2026-08-08 迁移）**：只读预览统一走 `src/lib/preview/`（contracts/classify/errors/registry/service/request-controller/context/diagnostics/composition）+ `src/components/preview/`（PreviewSurface/PreviewRenderer + Markdown/Json/Code/Media/Pdf/Csv/Archive renderers）。Markdown/JSON/Code/Image/Video/Audio/PDF/CSV/Archive 统一进入 pipeline；file source 必须先 `authorizeFile` 再 readText/toAssetUrl（禁止 raw path → convertFileSrc）；fallback 仅限 `not_applicable/unsupported/parse_failed`，fatal（permission/security/io/host）STOP 不降级；每个 Surface（files/assistant/artifact/follow）持有独立 PreviewRequestController，互不 stale/cancel。FilePreview 只读预览经 `previewCapabilityV2Enabled()` flag（localStorage）路由到 PreviewSurface，legacy 可回滚；HTML 预览因 **H0=BLOCKED**（无真实 Tauri/WebKit headed evidence）显式 unsupported，不再 raw `useFileContent` + srcDoc，待 T20→T21→T15。
- **性能（2026-08-08）**：FileGrid/FileList 从 200 项递增渲染改为有界虚拟化（50k 条目 DOM 恒为窗口 1603 节点，O(viewport)）；FileCard/FileRow 单击立即选择（移除固定 200ms 延迟，p95 ≤100ms）；FileBrowser watch 仅直接子项变更触发整目录刷新（深层风暴只点亮顶层子项）；`src-tauri/src/commands/fs.rs` 重命令 async → bounded semaphore(4) → spawn_blocking（20×1ms 顺序阻塞 Host IO 墙钟 25ms→1.2ms）。
- **编辑**：Monaco（50+ 语言映射，⌘S）、Milkdown（0.8s 空闲自动保存 + frontmatter 保护）、Canvas 图片编辑器（六工具含马赛克、25 步撤销、多格式导出）。
- **搜索**：文件名模糊（自研 fuzzy_score）+ 全文 grep（**rg → 系统 grep → 内置 Rust** 三级回退）+ macOS Spotlight；requestId 竞态守卫。
- **其它**：收藏（DB 持久化 + 跨组件事件）、最近打开（LRU 40，DB+localStorage 双写）、最近修改（后端递归扫描）、磁盘占用透视（可下钻）、缩略图（Semaphore(4) 限流 + mtime cache key + LRU 淘汰）、系统集成（打开/reveal/终端/VS Code、系统剪贴板复制文件/图片）、状态栏。
- **安全**：白名单（$HOME/tmp）+ 黑名单（.ssh/.aws/.kube…）、canonicalize 前拒 `..`、文件名 sanitize、原子写（temp+rename）+ `expectedMtime` 乐观锁（后端已实现）。
- **集成点**：终端双向（`open-terminal` / OSC7 `navigate-files` / terminal-follow 跟随 cd）、Header 事件桥、CommandPalette、Assistant `@` 文件补全（`search.files`）、AIFileOrganizer、活动项目徽章写 localStorage。

### 1.3 i18n 与测试

- 文件页与图片编辑器文案均由 `extension/_locales/{zh_CN,en}/messages.json` 提供并做 key 对齐；图片编辑器的工具、颜色、大小、格式、质量、文字输入和保存提示均已本地化。
- 测试：排序/kind/收藏/搜索等纯函数有覆盖；Rust file_manager 13 例、fs/search/disk 命令有例。
- **空洞**：全部组件零 RTL 测试；`fs_watch.rs` / `archive.rs` / `thumbnail.rs` 零测试。

## 二、fanbox 对照结论

### 2.1 Natives 已超越 fanbox 的部分（无需回迁）

| 功能 | 说明 |
|---|---|
| 多选/批量操作 | fanbox 只有单选，无框选、无批量 |
| 应用内文件剪贴板 | fanbox 无 ⌘C/X/V 文件操作 |
| 应用内拖拽移动 | fanbox 显式排除内部路径拖拽 |
| 可编辑地址栏 | fanbox 只有面包屑 |
| 全文搜索回退链 | rg→grep→内置，比 fanbox 纯 JS grep 强 |
| 缩略图管线 | Semaphore 限流 + LRU，fanbox 只有 inflight 去重 + 400MB 裁剪 |
| 路径安全 | fanbox `resolvePath()` 刻意不拦越权（纯本机工具）；Natives 有白/黑名单沙箱 |
| 批量删除/复制/移动命令 | fanbox 均为单项 API |
| 网格密度与目录导入 | Natives 支持小/中/大网格、目录选择器和相对路径分块导入；fanbox 网格密度更完整但无独立目录选择入口 |

### 2.2 已迁移但**断线/退化**（P0：修复优先于新迁移）

| # | 问题 | 位置 | 说明 |
|---|---|---|---|
| 1 | **旧 Tauri fs_watch 前端未接线**（历史缺口） | `src-tauri/**` 仅保留迁移证据；现行 `extension/files.js` 已接 `watch_start/stop`、250ms 合并与变更提示 | 旧 FileBrowser 路径不会刷新；Chromium Extension 主线已迁移，真实 Chrome 证据仍受控制策略阻断 |
| 2 | **旧 Tauri 乐观锁未接线**（历史缺口） | 现行 Extension 保存链已传 `expectedMtime` 并处理 `conflict` | 旧 Tauri 编辑器说明保留作溯源；当前缺真实 Chrome 外部并发修改证据 |
| 3 | 旧编辑器体验缺三件（历史缺口） | 现行 Extension 已有 800ms 自动保存、外部变更重载/冲突标记与统一 `guardDirty()` | live follow 等 fanbox 深化体验仍未迁移 |
| 4 | 截图存素材损坏二进制 | `ShellLayout.tsx` 用 `fs.readFile`(文本)+`writeFileAtomic` 复制 PNG | 应改 `fs.copyEntry` |
| 5 | `fs_clipboard_copy_image` 非 macOS 直接 Err | `file_manager.rs:1169` | 已补 Linux `wl-copy`/`xclip` 与 Windows PowerShell 原生图片剪贴板；macOS HEIC/HEIF 复用受控 JPEG 转码；工具缺失时明确返回能力错误 |
| 6 | ImageEditor i18n 半途 | 图片编辑器操作、颜色/大小/格式/质量/文字提示均已接入 zh/en key；仅 PNG/JPEG/WebP 格式名保持标准缩写 | `extension/_locales/{zh_CN,en}/messages.json` 与 `files.js` |
| 7 | Web 模式空壳 | `web-fs-client.ts` 只有一个 throw 的 diskUsage | 浏览器 dev 下文件页整页不可用；要么补齐要么明确降级 UI |

### 2.3 fanbox 有、Natives 未迁移的功能（真缺口）

**A. 变更感知主线（fanbox 灵魂，P0-P1）**

- 监听集管理：当前浏览目录（现行 Extension 已接 `watch_start/stop`）；各终端会话项目目录与增量 diff 开关仍属未迁移能力。
- 噪声过滤：点开头路径段、`~/.swp/.tmp/.part/.lock/-journal|shm|wal` 后缀、mtime+ctime 均超 3s 的 metadata-only 事件（Natives Rust 与前端规则均已过滤）；**`selfOpened`：自己刚打开的文件 3s 内变更整条丢弃**（macOS LaunchServices 写 xattr 产生假变更）。
- 消费链：卡片热度（`--heat` CSS 变量 + 「改·N」徽标 + tooltip 子路径清单，单一 sweep 定时器每秒清理）→ 250ms 防抖自动刷新目录 → 后续跟随选择按 900ms 节流 → 编辑器热重载。
- 变更收件箱 + 会话回放：Natives 已有 `ChangeInbox.tsx` / `SessionReplay.tsx`（Assistant 域），**需确认其数据源是否接 fs 事件**，还是仅 agent 工具调用记录；fanbox 版是纯 FS 事件（去重上限 100 / 时间轴上限 3000）。

**B. 文件跟随 live follow（P1，与 Agent 域联动的核心体验）**

fanbox `app.js:4505-4839`：预览面板自动切到 agent 刚写的文件并实时渲染。要点全在细节里：
- 绑定终端 tab + 作用域=该终端当前 cwd；归属消歧（绑定 tab busy 或 8s 内有输出才认）。
- 优先级 html/md(3) > 代码(2) > 图片(1) > 产物(0)，防低价值文件抢屏；编辑器开着绝不抢。
- **节流而非防抖**（首次 120ms、后续 900ms）。
- `liveCode`：重读全文 + `changedRange()` 双向夹逼算改动行 + 闪烁滚动；`liveMd`：尾部贴底滚/中间保视口；`liveHtml`：**双缓冲 iframe 换页零白闪** + 2.5s 强制换页防死循环脚本。
- 二进制产物出卡片（大小 + 打开/reveal）不实时渲染；底部「过程旁白」从终端尾行提取工具动作实时播报。
- 注意：Natives 现有 `follow-mode.ts` 的 terminal-follow 是「文件浏览器跟终端 cd」，与此不同，两者可共存。

**C. 终端×文件联动细节（P1）**

- 终端内路径点击定位：`/api/locate` 四级兜底（直接 stat → **空格扩展**（macOS 截屏名）→ scrollback 逻辑行回扫 → 多根 basename 模糊 → mdfind），**划线前 `/api/term-verify` 批量 stat 先验真**（防中文散文误标链接）+ LRU 缓存。Natives 侧未见对应链路。
- 预览选中文字 → 「发到终端」（bracketed paste 包裹防逐行误执行）。
- 拖文件进终端插入 shell 转义路径（Natives 需确认）。

**D. 预览/编辑细节（P1）**

- **HEIC/HEIF 透明转码**（fanbox 用 `sips` 转 jpeg 全尺寸缓存）：Natives macOS Host 已用受限 `sips` 临时 JPEG 转码；非 macOS 明确返回不支持。
- Markdown 本地相对图片改写（`fixLocalImages`：`![](./图/x.png)` → 可加载 URL，含 `../` 折叠）+ 加载失败回退；Natives 已通过受控图片导入和 sandbox 预览覆盖。
- Milkdown **语义无损校验**（`semanticSig`：两份 md 渲 HTML 比对文字+结构骨架，往返有损即锁只读，绝不静默丢内容）。
- HTML 预览注入：测宽脚本 postMessage 自然宽度 → 整页 `transform: scale` 适配窄预览框。
- 大文本截断按 **UTF-8 字符边界回退**（fanbox `server.js:215-220`；Natives `ops.rs` 已回退到合法边界并有单测）。
- PDF：Natives 使用 sandbox iframe 并设置 `page=1&zoom=page-width`，预览操作区已提供上一页/下一页；内容搜索对 ≤20MiB PDF 尝试受限 `pdftotext`（最多 20 页/512KiB 输出），未安装该系统工具时显式无命中。

**E. 压缩/解压（P1）**

- 双方都只有只读清单。Natives 现已提供 `extract_archive` 与 `create_zip`：ZIP 与 TAR 家族走受控系统工具，预检拒绝路径穿越/符号链接并保持不覆盖；创建仅接受同目录输入并依赖系统 `zip`，Host 真实发出阶段进度并支持取消后清理不完整归档。zip GBK 双解 fanbox/Natives 均已有。

**F. 其它（P1-P2）**

- 占用透视：fanbox 状态栏可递归统计当前目录真实磁盘占用并按子项排序；Natives 现已提供受限按需 `disk_usage` 扫描（10 万条上限，符号链接不跟随），返回子项明细、不可读项计数并接入状态栏入口。
- 目录导入：fanbox 外部拖入按目标目录写入；Natives 另提供原生目录选择器，按 `webkitRelativePath` 重建层级后走分块导入。

- 侧栏懒加载目录树（`▸/▾` 逐级展开，只列文件夹）——Natives 已实现懒加载、竞态保护和最多 200 条展开状态持久化。
- 回合快照（影子 git：GIT_DIR 在应用目录、`--work-tree` 指项目零污染；15s 节流、40 tag 滚动、回滚前先自动存档）——可归 Agent 域立项。
- 截图直通车（watch 截图目录 + **轮询等文件大小两次不变**防半截文件 + 浮卡：→终端/收进素材/标注）。
- 删除交互：文件秒删不确认、文件夹确认一次、toast 明示「可从废纸篓恢复」。
- 提示音（Web Audio 合成，可静音）与卡片点亮动效——`editRipple` 壳已在，接上 A 的数据源即可。
- 值得抄的注释级经验：**选中/收藏变化只改单个 DOM class，绝不重建网格**（重建导致全部缩略图重新解码，是点击卡顿元凶）——Natives 增量渲染下同样适用。

## 三、Natives 自身债务（迁移时顺手偿还，"比之前更好"）

1. `FileBrowser.tsx` 1547 行、~20 个 useState、跨组件通信全靠 `window` CustomEvent（`navigate-files`/`header-file-action`/`file-flash`… 无常量、无类型），另有 `window.__pendingNavigateFiles` 等补丁式全局量 —— 接缝在但契约隐性。建议：事件名+payload 类型化为单一模块（真实接缝），FileBrowser 拆 浏览状态机/剪贴板/选择 三块。
2. `detect_file_kind` / `detect_project_badge` 双端各一份且已漂移（TS 多 `.styl/.mod/.sum/.mm`，Rust 多 `.heic/.tiff`）—— 收敛为 Rust 单一来源随 entries 下发，删 TS 副本。
3. 测试空洞：fs_watch/archive/thumbnail Rust 零测试；组件零测试；无「拖入→导入→刷新」E2E。
4. `library/FolderTree.tsx` 树是平的（`childrenOf` 死代码）——属 Library 域，勿与文件树混淆。

## 四、建议迁移顺序

| 批次 | 内容 | 理由 |
|---|---|---|
| **P0 修断线** | ①fs_watch 前端接线（监听编排 + 噪声/selfOpened 过滤 + heat/改·N + 防抖刷新） ②乐观锁接线 + 冲突弹窗 ③编辑器自动保存/热重载/guardDirty ④截图二进制 bug ⑤ImageEditor i18n | 后端都已就位，纯前端工作量；①是后续 B/F 动效的数据源 |
| **P1 迁灵魂** | ⑥文件跟随 live follow ⑦解压/压缩 ⑧HEIC 转码 + md 本地图片 + 语义无损校验 ⑨终端路径定位链 ⑩侧栏目录树 ⑪截图直通车 | fanbox 区别于普通文件管理器的核心体验，且与 Agent 域强协同 |
| **P2 还债+超越** | ⑫FileBrowser 拆分 + 事件契约类型化 ⑬kind/badge 单一来源 ⑭回合快照（Agent 域立项） ⑮pdf.js ⑯Web 降级策略 ⑰测试补齐 | 深化与长期可维护性 |

约束提醒：涉及 UI 文案必须 zh/en 同步；fs_watch 高频事件注意 `docs/standards/technical/04-performance.md` 预算（`npm run perf:check`）；新功能 PR 归类声明 **Hub**。

## 五、Chromium 迁移首个纵向切片（2026-08-27）

当前仓库新增 `extension/`：Manifest V3 文件管理扩展通过 `chrome_url_overrides.newtab` 将新标签页设为 `newtab.html`，并通过工具栏按钮从任意标签页打开独立的 `files.html`。正式 Native Host 为 `crates/native-file-host`，复用 `crates/file-manager-core` 的文件授权内核，以 Native Messaging stdio 协议提供目录、预览、批量操作、导入和 watcher 方法，并支持列表/搜索分页、批量取消与 notify 变化推送。

这是可运行的开发迁移切片。2026-08-27 起 Next/Tauri 默认不再承载文件管理 UI；文件页面只存在于 `extension/files.html`，新标签页为独立的 `extension/newtab.html`。旧 AI、Apps、Workspace、Jobs、Usage 等 `src/app/**/page.tsx` 入口已移除，相关组件暂存为迁移材料，不再进入文件生产路由图。`files.js` 通过 `native-client.js` 创建正式 Native Port；Service Worker 不创建 Native Port、不轮询或保活。Native Host 的注册清单位于 `extension/native-host-manifest.json`，可用 `extension/install-native-host.mjs` 自动安装，安装步骤位于 `extension/README.md`；扩展 ID 必须通过 Chrome Native Messaging `allowed_origins` 白名单配置。Host 在 stdin EOF 时清理 watcher 与后台任务并退出。全文搜索结果现保留行号上下文，搜索分页后修复连续多选锚点索引，避免 Shift/方向键选择丢失；watcher 事件现以最多 30 条持久化“最近修改”记录呈现在侧栏，不执行全盘扫描；CSV 文本预览增加 64KB/200 行/32 列有界表格视图，JSON/JSONC 增加 64KB 有界结构化预览并使用字符串感知注释剥离，图片安全白名单新增 BMP；PDF 通过独立 `pdf_preview` 方法限制为 512KiB、校验 `%PDF-` 头并在面板 iframe 中预览，编辑器仍保留原始内容。

独立 Host 当前 macOS Release 二进制为 1,190,432 bytes，实测 RSS 约 6,080 KB，stdin EOF 后约 4 ms 退出；正式 target 仅依赖 file-manager-core、notify/open/trash 等文件能力，不链接 Tauri、Agent、Provider 或 SQLite。扩展后台无轮询。当前机器实测（Release，临时目录）目录分页 10,000 项 p95 28.05 ms，扩展估算包体 37,211 bytes；搜索立即取消、批量进度/取消、watcher `fs_changed` 与 EOF 清理均有 Native Messaging 集成证据。真实 Google Chrome 指定扩展页当前被浏览器控制 URL policy 拒绝，因而 newtab/files UI、页面关闭和断线恢复没有可宣称的 Chrome E2E 证据。Windows 当前只有静态/IExpress 安装资产，尚未实机验证。平台签名、公证、CWS ID 和 Windows 实机仍是发布证据阻塞，不是代码闭环阻塞。

本轮补充：预览项切换时立即释放旧 PDF/媒体 Blob URL，避免连续浏览产生无界的浏览器对象 URL 累积。
文件页和新标签页首屏默认文案现与 `zh_CN` manifest 默认 locale 对齐（包含重试、分页、预览调宽和弹窗按钮），语言切换即使在异步 locale 加载前触发也会重新应用目标文案；生命周期检查覆盖中文首屏占位符，避免中文选择器与英文界面不一致。
新标签页入口同样使用中文首屏文案，并为“打开文件管理”保留稳定的 `open-files` 交互目标，便于真实 Chrome 入口验证。
文件页新增轻量 Foxbox 皮肤选择：Volt（终端）、Archive（档案）、Index（索引）；仅切换共享设计 token，布局、权限与文件数据流不变，主题偏好写入本地存储且中英文 key 同步。
`pdf_preview` 的 Host schema/分发回归测试已覆盖有效 PDF MIME 与数据、临时目录调用链及无效头拒绝。
另以独立临时目录通过 Native Messaging framing 实测 `pdf_preview` 返回 `application/pdf` 与非空 Base64 数据，结果 PASS。
视图模式、排序字段/方向与隐藏文件偏好现通过既有本地存储恢复，减少重复设置操作。
音视频新增受控 `media_preview`（常见音频/视频 MIME、512KiB 上限、原生控件）；核心测试覆盖未知格式与超限拒绝，真实临时 MP3 Native Messaging 闭环返回 `audio/mpeg`，结果 PASS。
媒体预览现增加 MP3/WAV/OGG/MP4/MOV/WebM 魔数校验，伪装媒体在进入浏览器播放器前拒绝。
最近修改记录的本地存储写入现做 500ms 防抖，watcher 高频事件只合并写入，不增加事件处理路径的无界开销。文件条目现提供直接星标收藏入口，点击不会触发行选择或打开。
Markdown 本地图片相对路径现做段级规范化，支持安全的 `./`/`../` 引用并拒绝越过根目录、绝对路径与外链。
相对图片路径守卫同时识别 Windows 反斜杠、盘符和 UNC 绝对路径，跨平台统一分段规范化。
图片预览现增加 PNG/JPEG/GIF/WebP/BMP 魔数校验，扩展名伪装内容会明确拒绝并走系统打开回退。
JSONC 结构化预览同时容忍字符串外尾逗号，始终保留磁盘上的原始文本用于编辑。
文件页新增 ⌘/Ctrl+F 直接聚焦并选中文件搜索框，减少浏览器默认查找对文件操作流的干扰。
搜索框聚焦时按 Escape 现在会清空关键词并恢复当前目录，同时递增搜索 token 防止旧响应回写。
搜索命中摘要的行号现在以独立文本节点前置，保留关键词 `<mark>` 高亮，收起/展开均不再丢失高亮。
多选锚点现保持在最后一次非范围选择位置，连续 Shift+方向键会正确扩展范围，而不是跳回首项。

## 六、ADR-0023 生命周期与安装整改计划（Agent A 执行）

以下切片必须顺序完成；每个切片只运行覆盖改动的最小检查，P4 才运行完整门禁。

### P0 · 固定基线与失败信号

- 记录当前扩展 ZIP、Release Host、安装目录体积；记录 Host RSS、60 秒空闲 CPU、GPU 进程增量。
- 增加可重复生命周期脚本：未开文件页无 Host；打开后存在；关闭最后文件页 2 秒内退出。
- 用固定 1,000/10,000 项目录保存首屏与热 IPC 基线。
- 验收：测试必须稳定证明新标签页无 Host、文件页按需启动，且关闭最后文件页后 Host 在 2 秒内退出。

### P1 · 最小零常驻纵向闭环

- `newtab.html` 只展示工作台和文件入口；Manifest 新标签页已指向它。
- `files.html` 已直接管理 Native Port；`background.js` 不连接 Host、不转发 Native 消息。
- Service Worker 只保留 toolbar 点击、安装引导与标签页导航。
- 文件页实现 `pagehide` 断开、隐藏空闲 60 秒断开、重新可见按需重连；有进行中写操作时不得提前断开。
- Host 在 stdin EOF 后立即停止 watcher、取消任务并退出。
- 验收：新标签页 0 Host；文件页可浏览真实目录；关闭/隐藏后满足 R-P12；断线后恢复且错误明确。

### P2 · 单入口安装、启动与卸载

- 先取得 Chrome Web Store 稳定扩展 ID；开发环境使用固定 key 保持 ID 稳定。
- 将 Host 和启动模式收敛到最少产物；启动入口只打开/聚焦扩展工作台并立即退出。
- macOS 使用系统 `pkgbuild/productbuild`，Windows 使用平台原生轻量安装技术；禁止引入安装框架 Runtime。
- 安装器注册 Native Host 和 Web Store external extension，启动 Chrome 等待用户一次启用确认。
- 卸载器删除二进制、manifest、registry/preferences 和启动入口；重复安装/升级/卸载必须幂等。
- 验收：干净系统只运行一个安装包；用户除 Chrome 启用确认外不复制 ID、不运行命令、不手动启停服务。

### P3 · 资源收敛

- 新标签页保持纯静态，初始 JS gzip ≤ 50 KB；预览、搜索和缩略图按需加载。
- 文件列表继续分页/虚拟化，超过 200 项不得一次创建完整 DOM。
- watcher 仅监听当前可见目录，事件 250 ms 合并；禁止轮询和永久动画。
- Release 构建裁剪符号和无用依赖，但不得削弱路径、消息或扩展 ID 校验。
- 增加包体预算脚本和进程生命周期检查到 `perf:check` 的扩展门禁。
- 验收：ADR-0023 资源表全部通过，同设备保存可比较证据。

### P4 · 发布门禁与 Legacy 删除

- 在 Chrome Stable 的干净 Profile 上验证首次安装、启用、启动、文件操作、隐藏恢复、Chrome 重启和卸载。
- 覆盖 macOS Apple Silicon/Intel 与 Windows 10/11；签名、公证和 Web Store 审核状态必须真实。
- 删除仅服务旧 Tauri Files/开发安装流程的生产入口和依赖；保留迁移证据，不保留双 production fallback。
- 同步中英文文案和安装文档。
- 最后一次运行适用的 TypeScript、lint、test、perf、Rust fmt/test 与扩展检查；ADR-0023 只有在实机证据齐全后改为“已落地”。

### 明确不做

- Electron、CEF、Chromium fork、Tauri 安装壳。
- 开机启动、后台 daemon、托盘保活、本地 HTTP 中继、后台更新轮询。
- 自研浏览器标签栏、密码库、扩展商店或 Google 同步。
- 为未来 AI、供应商、应用中心或插件 Runtime 预留抽象层。

## 七、持续能力矩阵（2026-08-28）

| fanbox 行为 | Natives 当前状态 | 差距 | 验证证据 |
|---|---|---|---|
| roots/磁盘、目录分页、排序、隐藏文件、面包屑与前进后退 | 已接入 Native Host 与 Extension 页面；分页扫描按小批次裁剪，目录树展开也有旧响应保护；地址栏支持 ⌘L 与文件路径定位；排序字段/方向、列表/网格视图、隐藏显示和递归搜索偏好可持久化；前进/后退历史按路径保留最多 100 个目录的滚动位置，并支持 ⌘/Ctrl+[ 与 ] 快捷导航；最近修改支持手动刷新、单飞锁、mtime 排序和来源目录提示；目录右键新增 fanbox 式占用透视，可对任意已授权子目录复用有界 `disk_usage` 扫描；占用结果改为独立对话框，支持受限子目录点击下钻与父目录返回，并以有界比例条显示子项占用 | 缺真实 Chrome 页面证据 | Native Host 10k 目录 p95 28.0ms；`extension:check` |
| 最近修改文件 | Host 复用有界 `recent_files` 扫描（忽略缓存/构建目录、3.5 秒/30,000 条上限），扩展初始化后按授权 roots 异步合并前 30 项；支持手动刷新单飞锁、mtime/size/kind 有界持久化、来源目录提示，并继续接收 watcher 实时更新；失效路径会清理元数据 | 缺真实 Chrome 侧栏重载证据 | `recent_files` file-manager-core 测试；Host/Extension 检查；`perf:files` |
| 侧栏宽度与布局偏好 | 已增加 fanbox 风格侧栏分隔条，支持鼠标拖拽及键盘方向键/Home/End 调整，宽度限制 180–360px 并通过本地存储恢复；工具栏拆为导航与操作两组，视图尺寸按钮保持横排，操作按钮按宽度分组换行，New 菜单按锚点定位；主区/预览/侧栏在 1180/980/520px 断点弹性重排；网格密度下调为 132/96/180px，并对状态栏、工具栏子项加最小宽度与断词约束，避免窄窗口横向溢出；内容区纵向填充，空白区保持可拖放并显示拖入高亮 | 缺真实 Chrome 拖拽与多宽度截图 | `extension:check`、`perf:files`；Chrome E2E BLOCKED |
| 侧栏折叠与工作区扩展 | 已增加 fanbox 风格折叠按钮及 `⌘/Ctrl+B` 快捷键，折叠状态通过本地存储恢复，预览右侧/底部布局均保持可用 | 缺真实 Chrome 折叠截图 | `extension:check`、`perf:files` |
| 列表/网格、单选/多选/范围选择、快捷键与上下文菜单 | 已实现，含键盘 ContextMenu/Shift+F10、Home/End/PageUp/PageDown（Page 键按可视区域动态步长）、搜索态 Enter 直接更新预览、Shift+Enter 直接在编辑器打开、`/` 当前目录筛选与 ⌘/Ctrl+F；列表图标按目录、文本、图片、音视频、PDF、压缩包区分；网格图片缩略图按可见性懒加载、失败回退图标，并展示体积元数据；网格图片卡片按 S/M/L 视图自适应缩略图尺寸；网格方向键按行列移动；排序字段与方向持久化；列表/网格条目提供 fanbox 风格星标收藏快捷切换，收藏状态在列表、侧栏与上下文菜单之间即时同步；条目与空白区域右键均可刷新、创建和导入；上下文菜单“复制路径”写入换行分隔的纯文本路径，Clipboard API 不可用时使用受控 textarea 回退；普通非图片文件上下文菜单补充“复制文件”，统一调用 Host `copy_paths`；键盘 Enter 与双击共享预览/最大化流程；复制/剪切后状态栏显示模式和项目数，并可一键清除内部剪贴板 | 缺真实 Chrome 操作截图与系统剪贴板实测 | `extension:check`、`perf:files` |
| 预览面板最大化/还原、Esc 快速退出、拖拽/键盘调宽、右侧/底部布局 | 已实现固定预览栏内的最大化覆盖层，编辑器/PDF/HTML 在最大化时扩展到工作区；文本、图片、视频双击会复用预览并直接最大化，其他类型仍交给系统应用；分隔器按方向支持鼠标拖拽、方向键/Home/End 调整宽度或高度并持久化；可切换预览到底部并记忆偏好，重载时同步切换按钮的图标、aria-pressed、分隔器方向及尺寸边界；文本编辑器关闭预览前保存滚动与选区状态；图片预览可聚焦，Enter/Space 与点击均可打开灯箱；灯箱关闭或切换预览时清空图片引用与请求标识，避免大图残留 | 缺真实 Chrome 操作截图 | `extension:check`、`perf:files` |
| PDF 页码反馈与边界 | 已从受控预览字节中提取有限页数，显示 `当前页/总页数`，并通过预览面板“上一页/下一页”控件更新 Blob viewer 的页码锚点；未知页数仍允许前进且不会越过 1 | 复杂 PDF 对象流可能无法统计；缺真实 Chrome 页面证据 | `extension:check`、`perf:files` |
| 媒体预览解码失败反馈 | 原生音视频控件触发解码错误时，移除失效控件并显示错误与重试/系统打开动作 | 缺真实 Chrome 编解码器差异实测 | `extension:check`、`perf:files` |
| 图片基础编辑 | 受控 PNG/JPEG/GIF/WebP/BMP/AVIF 预览可进入 Canvas 编辑，支持旋转、水平/垂直翻转、画笔、矩形框、箭头、文字、带四角手柄与确认按钮的可调裁剪、马赛克，以及各 25 步有界撤销/重做；历史按钮按栈状态禁用并支持 `⌘/Ctrl+Shift+Z`，支持 PNG/JPEG/WebP 质量选择、显式取消并以新文件保存；图片 dirty guard 会在离开前复用统一确认流程，原图不被覆盖 | fanbox 更完整的自由绘制交互和真实 Chrome 编辑截图仍缺 | `extension:check`、`perf:files` |
| 搜索/目录加载 debounce、取消、旧响应保护 | 已实现当前目录/递归文件名搜索与大小写不敏感子序列模糊匹配，按精确/前缀/连续度返回相关性分数并在搜索态优先排序；Native 递归扫描增加 4 秒总时间预算，PDF 提取子进程另有 2 秒读取上限，超时返回 `truncated` 或能力状态；`content:` 全文命中按有界命中次数与最近 7 天 mtime 生成强度分数并保留首个命中行摘要，PDF 在受限 `pdftotext` 不可用时返回能力状态而非伪装成普通无结果；命中摘要支持 Tab/Enter/Space 展开并具备按钮语义，Enter 同步打开预览并定位命中行；通过来源路径定位到跨目录结果后，目录加载会恢复目标选择并触发预览；搜索结果超过当前页时显示“可继续加载”状态，Native 达到扫描上限时追加“结果可能不完整”警告，截断状态仅在当前 token 响应确认后写入；新增可见的 fanbox 式“当前目录/全机”范围按钮，搜索框按 Tab 或 `⌘K` 可切换全局/当前目录模式，范围清空时同步恢复按钮状态并取消旧请求；聚合已授权 roots 时按 pageOffset 请求，每根取有界四页窗口后去重并正确开放分页；全机结果显示来源目录并可一键定位到结果所在目录；清空搜索会退出全局模式，Esc 清空查询并恢复目录，目录导航也会取消全部并行请求并由 token 丢弃旧响应；正常导航会持久化最后目录，启动时通过 Host `stat` 校验后恢复，失效路径安全回退根目录 | 缺真实 UI 延迟与取消观测 | Native Host 模糊匹配/内容搜索边界测试；`extension:check`；Chrome 页面证据缺失 |
| 新建、重命名、复制/移动/副本、批量处理、废纸篓 | 已实现批量结果、部分失败、实时进度与可见取消按钮；导入、副本（`duplicate_batch`）及 Host 批量复制/移动/废纸篓均支持逐项取消并保留已完成项；进度事件统一显示阶段与已完成/总数；完整及部分成功的批量移动、拖拽移动和剪贴板移动会按 `errors/skipped` 过滤迁移 recent/recentModified/favorites 路径，扩展统一回调复用 `migrateBatchPaths`；冲突自动去重数量与单文件最终名称可见；新建文件/文件夹后自动刷新并选中新条目，文件可直接进入预览；批量复制/副本失败重试会刷新列表、只重选失败源项并恢复滚动位置；副本取消或部分失败时保留可操作选择与滚动位置；复制/移动对话框取消或部分失败时保留失败/未完成项目选择并恢复滚动位置；废纸篓 `trash_batch` 失败路径保留为专用重试操作；删除交互对齐 fanbox：文件直接移入废纸篓，文件夹/混合选择只确认一次；状态栏新增受控“打开废纸篓”，仅唤起系统 Trash/Recycle Bin，不读取或代理废纸篓内容 | 缺真实 Chrome 页面证据；单个文件系统调用仍不可抢占 | Native Host `moved/errors/skipped` 响应；`trash_batch.errors[].path` 与重试断言；`open_trash` Host schema/编译检查；`migrateBatchPaths` 生命周期断言；临时目录 Native Messaging 集成验证；`duplicate_batch` Host 测试；`extension:check`、`perf:files` |
| Finder 拖入、目录拖放目标、分块导入 | 已实现目录命中高亮、批量移动、粘贴/拖放取消、分块导入与字节进度；取消会清理未提交临时文件；导入、拖拽移动及 URI 复制的失败项保留为可重试列表，不重复已完成项，重试恢复原选择、滚动位置与目标目录；导入、拖拽移动及 URI 复制完成后会刷新目标目录并选中最后落盘条目；条目拖出提供受控 `text/uri-list` 与内部 JSON 路径数据，目录目标同时接受普通文件 URI 与外部图片 URL；外部 Finder 同时携带非 JSON `text/plain` 时由捕获层优先按文件导入，避免误走内部移动；主文件区使用拖拽深度计数，进入子元素时保持稳定高亮，drop/dragend 统一清理 | 缺真实文件拖入/拖出验证 | `cancelled_streaming_import_drops_temporary_file`；`extension:check`、`perf:files`；Host 临时目录批量验证 |
| 文本/Markdown/JSON/CSV/ZIP/JAR/TAR/TGZ/GZ 与受控预览 | 已实现安全文本编辑、ZIP/JAR/TAR、独立 GZ 及压缩 TAR（`.tgz`/`.tar.gz`/`.tar.bz2`/`.tar.xz` 通过无 Shell、输出有界的系统工具）清单（仅读头部/名称，限条目与声明大小，ZIP 名称按 UTF-8/GBK 解码；仅对可安全列出的归档进入内置预览，其余交给系统应用）；ZIP 与 TAR 家族归档可从上下文菜单及归档预览操作栏解压到受授权目录，选中文件可创建 ZIP，创建完成后自动选中新归档；预检拒绝符号链接、路径穿越和超大内容且不覆盖既有文件；创建阶段可显示校验/压缩状态、真实进度并在 Host 侧终止子进程；PNG/JPEG/GIF/WebP/BMP/AVIF 受控预览与图片编辑入口（AVIF 校验 `ftypavif/ftypavis` 魔数；macOS HEIC/HEIF/TIFF/TIF 通过无 shell 的 `sips` 临时 JPEG 转码并受 512KiB 限制）、HTML sandbox 原地预览（CSP 且禁脚本）、Markdown 本地图片并发缩略加载（懒加载与失败隔离），编辑区可粘贴/拖入或通过工具栏多选图片并通过分块导入插入真实相对引用；冲突时由 Host 自动生成唯一名称，引用使用最终落盘路径；Markdown 工具栏支持源码/阅读切换及加粗/斜体/代码/列表/标题/链接快捷插入（链接先校验 http/https），文本预览工具栏与 HTML/媒体/归档错误态统一提供编辑器打开、复制文件、复制路径，且所有受控预览分支都提供一致的“在编辑器打开”；PDF 预览保留原生 iframe 查找快捷键、提供显式查找按钮和分页控制，内容搜索对已授权 ≤20MiB PDF 尝试受限 `pdftotext`，非支持类型提供恢复动作 | 图片/PDF/HTML 预览有大小与能力边界；归档创建仍依赖系统 zip 且仅允许同目录输入；PDF 搜索依赖系统 pdftotext；缺 Chrome 页面证据与 HEIC 实机样本 | `file-manager-core` ZIP 创建/解压/取消临时目录测试；Native Messaging `create_zip` 临时目录闭环（2 文件、成员校验、EOF）；archive/image/import 边界测试；Extension/Host 门禁；Chrome 页面证据缺失 |
| 自动保存、原子写入、expectedMtime、dirty guard、冲突处理 | 已接入编辑器保存队列、冲突重载/覆盖；文本编辑器按路径有界保存滚动位置与选区，重命名/移动时随路径迁移，切换文件后恢复；保存开始时显示处理中状态，写入返回会校验编辑器对象仍有效，避免切换文件后旧响应改写新工具栏 | 缺真实外部并发修改证据 | `write_file_dispatch_reports_mtime_conflict_without_overwrite`；`extension:check`；缺 Chrome 页面证据 |
| watcher 实时刷新、变更提示、选择/滚动保持 | 已接入 250ms 合并刷新、变更高亮与短暂热度光晕、选择与滚动保持；前端过滤所有临时文件后缀、SQLite sidecar 及自打开文件 3 秒内的假变更；可选“跟随修改”自动选中并预览当前目录内的外部修改文件，dirty 时不抢焦点，用户手动选择会清除待跟随目标，并在长目录中滚动到跟随条目；HTML sandbox 预览会复用 watcher 刷新 Blob 内容；编辑器重读仅针对实际变更路径，并用独立 token 丢弃旧异步响应；普通文本刷新保留编辑器滚动位置与选区；搜索态变更会复用当前搜索条件并恢复视口；重复变更显示“改·N”徽标并在 3 秒后有界清理；变更收件箱现保留 `created/removed/modified` 类型、显示对应标签，并提供仅清理会话内记录的“清空本次变更”入口；导航后延迟 watcher 刷新会校验目录快照，丢弃旧目录事件；干净预览对应文件被删除时自动关闭预览，脏编辑仍保留以避免数据丢失 | 缺真实 Chrome 外部文件变更观测 | release Host 临时目录 `watch_start` → `fs_changed` 集成验证；`extension:check`；`perf:files` 生命周期检查 |
| Host 断线恢复与页面关闭清理 | 已实现指数退避重连与 EOF 清理；批量任务期间 stdin EOF 也会 join 并退出；Host 断线时干净预览会清理 Blob、灯箱与选中态，脏编辑保留以避免数据丢失，且不会为取消中的预览再次触发重连 | 缺真实 Chrome 断线/关闭实测 | Native Host EOF 门禁；临时批量 EOF 集成验证 6ms；`extension:check`、`perf:files`；Chrome E2E BLOCKED |
| 文件/目录在终端打开 | 不迁移；当前架构禁止终端、任意进程和 cwd 接口 | 不适用 | Host schema 与 Extension 静态检查 |
| 复制文件/图片到系统剪贴板 | Extension 上下文菜单调用 Host `copy_paths` / `copy_image`，复用受授权系统剪贴板（macOS 文件对象与图片类型，其他平台按能力回退） | 缺真实 Chrome/系统剪贴板证据 | Host 全量测试；`extension:check` |
| 终端输出路径定位 | Host `locate` 先拒绝路径片段，再在默认授权根内以 8 层深度、50 条上限做 basename 模糊匹配；macOS 未命中时受控调用 Spotlight `mdfind` 兜底（2 秒进程超时），结果逐条复核授权与 stat；地址栏额外支持终端复制出的引号路径与转义空格 | 缺真实终端滚动内容与 Chrome 证据 | `locate_rejects_unsafe_queries`、`fuzzy_name_match_accepts_ordered_fragments_only`；Host/Extension 检查 |
| 侧栏目录树展开状态 | 懒加载目录树，展开路径最多持久化 200 条，页面重载后恢复并继续受 Host 授权过滤；watcher 事件命中已展开节点时仅刷新该节点的直接子目录，使用 token 丢弃乱序响应并清理脱离 DOM 的刷新器，失败时在节点内提供可访问的重试按钮，保留展开状态并避免全树重建；树行可作为 fanbox 风格的拖放目标接收移动、导入和 URI 复制 | 缺真实 Chrome 重载/实时刷新/树行拖放证据 | `extension:check`；`perf:files`；Chrome E2E BLOCKED |
| Chrome 新标签页 → 打开文件管理端到端流程与截图 | 当前 Chrome 控制工具对 `chrome-extension://kooobajbofcajlblcannmdckihiejdec/files.html` 的导航与 DOM 读取均被 URL policy 拒绝；不把历史截图或 Harness 结果当作本轮实测 | **BLOCKED**：缺真实 Chrome 页面、截图、语言切换、临时目录文件操作、关闭/断线恢复证据 | Chrome 控制工具返回的 URL policy 拒绝；Extension/性能门禁 |
| fanbox live follow、终端联动、HEIC/PDF 深化 | 文件 watcher 跟随修改已迁移；终端 tab/cwd/output 驱动的 live follow 未迁移；HEIC 已完成；PDF 已有 sandbox iframe、页码翻页和受限 `pdftotext` 搜索 | live follow 依赖被删除的 Agent/终端会话域，不能在固定 Extension + Native Host 架构内直接移植；PDF 搜索受系统工具可用性和格式复杂度限制 | 架构边界 GAP，不引入第二运行时 |
