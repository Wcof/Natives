# 文件管理器模块审计：现状清单 × fanbox 对照 × 迁移方案

> 2026-07-26 调研产出。对照对象：`References/fanbox`（server.js 2450 行 + app.js 5061 行 + Electron 层）。
> 归类：本模块属 **Hub** 面（文件/终端/Agent 工作台基础设施），非 Workshop/Embed。

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
- **预览**：图片（灯箱滚轮缩放）/视频/音频/PDF(iframe)/HTML(sandbox)/CSV/Markdown(Milkdown)/代码(shiki)/压缩包只读清单；`preview/code/git/info` 四 tab；Git diff（Monaco DiffEditor）。
- **编辑**：Monaco（50+ 语言映射，⌘S）、Milkdown（0.8s 空闲自动保存 + frontmatter 保护）、Canvas 图片编辑器（六工具含马赛克、25 步撤销、多格式导出）。
- **搜索**：文件名模糊（自研 fuzzy_score）+ 全文 grep（**rg → 系统 grep → 内置 Rust** 三级回退）+ macOS Spotlight；requestId 竞态守卫。
- **其它**：收藏（DB 持久化 + 跨组件事件）、最近打开（LRU 30，DB+localStorage 双写）、最近修改（后端递归扫描）、磁盘占用透视（可下钻）、缩略图（Semaphore(4) 限流 + mtime cache key + LRU 淘汰）、系统集成（打开/reveal/终端/VS Code、系统剪贴板复制文件/图片）、状态栏。
- **安全**：白名单（$HOME/tmp）+ 黑名单（.ssh/.aws/.kube…）、canonicalize 前拒 `..`、文件名 sanitize、原子写（temp+rename）+ `expectedMtime` 乐观锁（后端已实现）。
- **集成点**：终端双向（`open-terminal` / OSC7 `navigate-files` / terminal-follow 跟随 cd）、Header 事件桥、CommandPalette、Assistant `@` 文件补全（`search.files`）、AIFileOrganizer、活动项目徽章写 localStorage。

### 1.3 i18n 与测试

- `fileBrowser` 144 key + `filePreview` 27 key（zh/en 对齐）；**`imageEditor` 仅 3 key，`ImageEditor.tsx:343` 有硬编码中文**。
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

### 2.2 已迁移但**断线/退化**（P0：修复优先于新迁移）

| # | 问题 | 位置 | 说明 |
|---|---|---|---|
| 1 | **fs_watch 前端未接线**（最大缺口） | `src-tauri/src/fs_watch.rs`（完整）、`tauri-adapter.ts:1661-1668`（已封装）、全项目无调用方 | FileBrowser 不会自动刷新；`FileCard` 的 `heat` 呼吸边框与「改·N」徽章壳子已就位但恒为 0。fanbox 侧这是整条体验主线（见 2.3-A） |
| 2 | **乐观锁形同虚设** | `file_manager.rs:454` 支持 `expectedMtime`+`conflict`，但 `FilePreview.tsx:390,476` 与 Milkdown 保存均不传、不检查 | 外部（尤其 agent）修改会被编辑器静默覆盖。fanbox 有完整冲突弹窗（`app.js` `/api/write` conflict → 「文件已被外部修改，确定覆盖？」） |
| 3 | 编辑器体验缺三件 | Monaco 无自动保存（fanbox 800ms 防抖 + Promise chain 串行）、无外部变更热重载（fanbox 未脏静默重读）、无统一未保存守卫（fanbox `guardDirty()` 覆盖切文件/跳目录/关预览/Esc） | |
| 4 | 截图存素材损坏二进制 | `ShellLayout.tsx` 用 `fs.readFile`(文本)+`writeFileAtomic` 复制 PNG | 应改 `fs.copyEntry` |
| 5 | `fs_clipboard_copy_image` 非 macOS 直接 Err | `file_manager.rs:1169` | Windows/Linux 未实现 |
| 6 | ImageEditor i18n 半途 | 硬编码中文 + 仅 3 key | |
| 7 | Web 模式空壳 | `web-fs-client.ts` 只有一个 throw 的 diskUsage | 浏览器 dev 下文件页整页不可用；要么补齐要么明确降级 UI |

### 2.3 fanbox 有、Natives 未迁移的功能（真缺口）

**A. 变更感知主线（fanbox 灵魂，P0-P1）**

- 监听集管理：当前浏览目录 + 各终端会话项目目录，增量 diff 开关（fanbox `updateWatches()` `app.js:297`；Natives 后端 `fs_watch_start/stop` 已具备，缺前端编排）。
- 噪声过滤：点开头路径段、`~/.swp/.tmp/.part/.lock/-journal|shm|wal` 后缀、mtime+ctime 均超 3s 的 metadata-only 事件（Natives Rust 侧已滤 metadata，前端规则缺）；**`selfOpened`：自己刚打开的文件 3s 内变更整条丢弃**（macOS LaunchServices 写 xattr 产生假变更）。
- 消费链：卡片热度（`--heat` CSS 变量 + 「改·N」徽标 + tooltip 子路径清单，单一 sweep 定时器每秒清理）→ 250ms 防抖自动刷新目录 → 编辑器热重载。
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

- **HEIC/HEIF 透明转码**（fanbox 用 `sips` 转 jpeg 全尺寸缓存）：webview 不支持 HEIC，Natives 大图预览会裂（kind 认得 .heic 但走 convertFileSrc）。
- Markdown 本地相对图片改写（`fixLocalImages`：`![](./图/x.png)` → 可加载 URL，含 `../` 折叠）+ 加载失败 `/fs/` 镜像重试。
- Milkdown **语义无损校验**（`semanticSig`：两份 md 渲 HTML 比对文字+结构骨架，往返有损即锁只读，绝不静默丢内容）。
- HTML 预览注入：测宽脚本 postMessage 自然宽度 → 整页 `transform: scale` 适配窄预览框。
- 大文本截断按 **UTF-8 字符边界回退**（fanbox `server.js:215-220`；需检查 `file_manager.rs:399` 截断是否会切出 �）。
- PDF：双方都是裸 iframe，可顺势升级 pdf.js（页码/缩放/搜索）。

**E. 压缩/解压（P1）**

- 双方都只有只读清单。Natives 仓库已有带路径穿越/符号链接防护的 `safe_extract_zip`（`creative_app/probe.rs:686`），补 `fs_extract_archive` / `fs_compress` 命令 + 右键菜单即可。zip GBK 双解 fanbox/Natives 均已有。

**F. 其它（P1-P2）**

- 侧栏懒加载目录树（`▸/▾` 逐级展开，只列文件夹）——Natives Sidebar 只有平铺入口。
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
