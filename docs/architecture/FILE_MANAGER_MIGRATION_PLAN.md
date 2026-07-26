# 文件管理器完善实施方案（fanbox 迁移 + 超越）

> 2026-07-26 制定。依据：`FILE_MANAGER_AUDIT.md`（现状清单与差距对照，本方案不重复其内容）。
> 归类：**Hub** 面。
>
> **进度（2026-07-26 更新）**：
> - Phase 1（W1–W6）✅ 完成：提交 ea0012dd / cb508a72 / 012c080a
> - 追加「契约收紧」✅：ts-rs 类型单一来源（`npm run types:generate`）、
>   files-api / file-events 契约模块、detectFileKind/badge TS 副本删除
> - Phase 2（W7–W13）✅ 完成：提交 cb8e34c（W10/W9）、21c282a（W12）、
>   4af9572（W7/W8/W11 后端）、1596617（前端接线）；
>   W13 审查确认已有完整实现（screenshot.rs 稳定性等待 + ScreenshotCard），无需迁移
> - Phase 3（W14–W19）⬜ 未启动；全量 cargo test 待并行会话的协议改造完成后统一跑

## 一、启动门槛（前置分支合并后必做）

进入开发前先做一次校准，避免方案与合并后的代码脱节：

1. 同步分支：`rebase` 或合并最新 `main`（当前工作分支 `deploy`）。
2. **复核审计断言**（前置分支可能触碰同一批文件）：
   - `fs_watch` 是否仍然无前端调用方（全局搜 `fsWatch.`）；
   - `FileBrowser.tsx` / `tauri-adapter.ts` / `ShellLayout.tsx` / `FilePreview.tsx` / `Sidebar.tsx` 是否被前置分支改动；改动了则以合并后代码为准修订对应工作项的落点。
3. 基线验证：`npm run perf:check`、现有测试全绿，作为对照基线。
4. 若前置模块引入了新的事件总线 / store 约定，Phase 1 的接线实现优先复用新约定，不再新造。

## 二、总体结构

三个 Phase，每个 Phase 内的工作项（W 编号）尽量独立成 PR。依赖链只有一条硬约束：

```
W1 fs_watch 接线 ──→ W3 编辑器热重载
                 ──→ W10 文件跟随 live follow
                 ──→ W13 截图直通车
其余工作项互相独立，可按人力并行。
```

| Phase | 主题 | 工作项 |
|---|---|---|
| 1 | 修断线（后端已就位，纯前端为主） | W1–W6 |
| 2 | 迁 fanbox 灵魂体验 | W7–W13 |
| 3 | 还债 + 超越 | W14–W19 |

规模标记：S（≤半天）/ M（1–2 天）/ L（3 天+）。

## 三、Phase 1：修断线

### W1 fs_watch 前端接线（M，最高优先级）

**目标**：文件变更实时感知——卡片点亮「改·N」、目录自动刷新，为 W3/W10/W13 提供数据源。

**落点**：
- 新建 `src/lib/fs-change-filter.ts`：**纯函数**噪声过滤（利于单测）。规则移植 fanbox：路径段以 `.` 开头、后缀 `~/.swp/.tmp/.part/.crdownload/.lock/-journal|shm|wal`（注意 `.tmp` 可能在中段）、忽略目录表；`selfOpened` 记录（自己刚打开的文件 3s 内变更整条丢弃，防 macOS LaunchServices 写 xattr 的假事件）。
- 新建 `src/lib/use-fs-watch.ts`：hook 封装 `nativesAPI.fsWatch.start/stop/onChange`，管理监听集（当前浏览目录；预留 W10 需要的"各终端项目目录"扩展位），目录切换时增量 diff 开关。
- `FileBrowser.tsx` 集成：变更 → `changed` Map（顶层名 → count/files/ts）→ 喂 `FileCard` 已就位的 `heat` / 「改·N」徽章；**单一 sweep 定时器**每秒清理 4.5s 前的记录（禁止每条变更各挂 timer）；250ms 防抖 `loadEntries()` 自动刷新。
- Rust 侧确认 `fs_watch.rs` 已过滤 metadata-only 事件（审计确认已有），不足则在 Rust 层补，前端不做重活。

**验收**：终端 `touch`/`rm` 当前目录文件 → 1s 内卡片点亮 + 目录刷新；在应用内打开文件不产生误报；`npm install` 级事件风暴下 UI 不卡（聚合刷新而非逐条渲染）。
**测试**：`fs-change-filter` 单测（噪声规则逐条）；`fs_watch.rs` 补 Rust 测试（当前为零）。

### W2 乐观锁接线 + 冲突弹窗（S）

**目标**：消灭"agent 外部改了文件，编辑器一保存就静默覆盖"。

**落点**：`useFileContent` 保存读取到的 `mtime`；三个写入点（`FilePreview.tsx` 两处 + `MilkdownEditor` 回调）保存时传 `expectedMtime` 并检查返回的 `conflict`；conflict → 弹窗「文件已被外部修改（可能是 agent），覆盖 / 重新加载」，覆盖则以 `expectedMtime: 0` 强写。i18n 新增 key（zh/en 同步）。
**验收**：编辑中用终端改同一文件再保存 → 必弹冲突；正常保存无感知。

### W3 编辑器三件套（M）

**目标**：对齐 fanbox 编辑体验。三个子项：
1. **Monaco 自动保存**：停笔 800ms 防抖 + Promise chain 串行化写盘（防抖到点的保存与离开时 flush 互踩）；⌘S 立即 flush；状态条「N 秒前已保存」。
2. **外部变更热重载**（依赖 W1）：编辑器未脏 → 静默重读磁盘重渲染（加 reloading 锁去重）；脏 → 不动，靠 W2 冲突兜底。
3. **统一未保存守卫 `guardDirty()`**：切文件、跳目录、关预览、Esc 全覆盖；自动保存类编辑器走静默 flush，手动脏检查走确认弹窗。

**验收**：编辑→切走→切回内容不丢；agent 改文件时打开着的未脏编辑器自动跟新。

### W4 截图存素材二进制损坏修复（S）

`ShellLayout.tsx` 的 `onSaveToMaterial` 改用 `fs.copyEntry`，删除 readFile+writeFileAtomic 文本中转。验收：保存 PNG 后可正常打开且字节一致。

### W5 ImageEditor i18n 补全（S）

`ImageEditor.tsx:343` 硬编码中文改走 i18n；`imageEditor` 命名空间从 3 key 补齐全部 UI 文案（工具名/导出/另存为/确认语，预估 15–20 key），zh/en 同步。

### W6 `fs_clipboard_copy_image` 跨平台（S，可选）

Windows/Linux 实现（建议 `arboard` crate）；若暂不做，前端按平台隐藏该菜单项，不给用户报错。

**Phase 1 出口标准**：W1–W5 合并；`heat`/「改·N」有真实数据源；无静默覆盖路径。

## 四、Phase 2：迁 fanbox 灵魂体验

### W7 解压 / 压缩（M）

- Rust：`safe_extract_zip`（现在 `creative_app/probe.rs:686`）**上移为共享模块**（如 `src-tauri/src/archive_ops.rs`），新增命令 `fs_extract_archive`（zip 走共享实现，tar 系走系统 `tar` 子进程；目标目录同名自动去重）与 `fs_compress_entries`（zip crate，支持多选打包）。
- 前端：右键菜单加「解压到当前目录 / 压缩为 zip」；大档案给进度或至少 busy 态；完成后触发刷新（W1 会自动感知）。
- **安全**：解压必须保留路径穿越/符号链接防护；补 Rust 测试（恶意 zip 用例）。

### W8 HEIC/HEIF 透明转码（S–M）

macOS 用 `sips` 转 jpeg 全尺寸缓存（缓存键含 mtime，落缩略图缓存同级目录）；`FilePreview` 图片分支检测 heic/tiff 走转码端点，灯箱同理。非 macOS 暂回退到「无法预览」卡片。验收：HEIC 大图预览不裂。

### W9 Markdown 本地图片 + 语义无损校验（M）

- `fixLocalImages` 等价物：Milkdown/代码预览渲染前把 `![](./x.png)`、`../` 相对路径与本地绝对路径改写为可加载 URL（`convertFileSrc`），外链 http/data/blob 不动；落盘前把内部 URL 还原为真实路径（fanbox `cleanImgUrls` 等价物）。
- **语义无损校验**（fanbox `semanticSig`）：富文本往返前后各渲一次 HTML，比对「可见文字 + 结构骨架（标签序列 + img src/alt + href）」，有损即锁只读、只允许源码模式修改——绝不静默丢内容。
- 验收：含相对路径图片的 md 正常显示；构造往返有损样例（如复杂 HTML 块）→ 自动锁只读并提示。

### W10 文件跟随 live follow（L，Phase 2 核心）

**目标**：预览面板自动切到 agent 刚写的文件并实时渲染。与现有 `follow-mode.ts`（文件浏览器跟终端 cd）互补共存，建议新建 `src/lib/live-follow.ts` 独立状态机。

移植 fanbox 全部要点（`FILE_MANAGER_AUDIT.md` 第二节 B 有细节行号）：
1. 绑定终端会话；作用域 = 绑定终端当前 cwd；归属消歧（绑定会话 busy 或 8s 内有输出才认这笔写入）。
2. 优先级 html/md(3) > 代码(2) > 图片(1) > 二进制产物(0)；编辑器开着绝不抢屏。
3. **节流而非防抖**：首次 120ms、之后 900ms。
4. 渲染器三件：`liveCode`（重读全文 + 双向夹逼算改动行 + 闪烁滚动）、`liveMd`（尾部贴底滚 / 中间保视口）、`liveHtml`（**双缓冲 iframe 换页零白闪** + 2.5s 强制换页防死循环脚本 + 换页期间新写入攒一次补刷）。
5. 二进制产物出卡片（大小 + 打开/reveal），不实时渲染。
6. 「过程旁白」（从终端尾行提取工具动作播报）标为**可选子项**，可后置。

**验收**：让 agent 在绑定目录连续写 html/md/代码 → 预览自动跟随、无白闪、不抢正在编辑的屏；产物文件出卡片。
**性能**：属高频渲染路径，遵守 `docs/standards/technical/04-performance.md`，合并前跑 `npm run perf:check`。

### W11 终端路径点击定位链（M）

- Rust 新命令 `fs_locate`：四级兜底移植——直接 stat → **空格扩展**（macOS 截屏名，靠文件系统 stat 验证空格边界而非猜测）→ 多根 basename 模糊（共享时间预算、同名取 mtime 最新）→ macOS `mdfind -name`；另加 `fs_verify_paths` 批量 stat。
- 前端：终端渲染层给路径候选加下划线前**先批量验真**（防中文散文误标），结果 LRU 缓存；点击 → `fs_locate` → `navigate-files` + 软选中。scrollback 逻辑行回扫在终端组件内做。
- 顺带补「预览选中文字 → 发到终端」（bracketed paste 包裹），S 级。

### W12 侧栏懒加载目录树（S–M）

Sidebar 快速入口与收藏目录行加 `▸/▾` 展开箭头，点箭头懒加载子目录（只列文件夹、排除隐藏，复用 `fs.listDir`），点行本身跳转；统一高亮当前目录。

### W13 截图直通车（M，依赖 W1）

监听系统截图目录（macOS 读 `defaults read com.apple.screencapture location`，复用 fs_watch）；**轮询等文件大小连续两次不变**再弹浮卡（防半截文件）；浮卡三动作：发终端 / 收进素材（用 `fs.moveEntry`）/ 标注（进 ImageEditor）。

**Phase 2 出口标准**：W7–W12 合并（W13 可顺延）；live follow 在真实 agent 会话中验收通过。

## 五、Phase 3：还债 + 超越

### W14 FileBrowser 拆分 + 事件契约类型化（L）

- 新建 `src/components/files/file-events.ts`：`navigate-files` / `header-file-action` / `header-file-state` / `file-renamed` / `file-trashed` / `file-flash` / `open-terminal` 事件名常量化 + payload 类型 + 类型安全的 dispatch/listen 封装——把隐性契约变成真实接缝。
- `FileBrowser.tsx`（1547 行）拆三块：导航/条目状态机、选择与剪贴板、拖拽与操作编排；顺手清掉 `window.__pendingNavigateFiles` / `__pendingSelectFile` 两个补丁式全局量（改为事件 payload 或状态机内 pending 字段）。
- 拆分**只搬不改**行为，靠 W19 的组件测试兜底。

### W15 kind / 项目徽章单一来源（S）

`detect_file_kind` / `detect_project_badge` 收敛为 Rust 实现，随 `fs_list_dir_detailed` entries 下发；TS 侧 `src/types/file.ts` 副本删除（保留类型定义），消除已发生的双端漂移。

### W16 回合快照（影子 git）——**Agent 域单独立项**（L）

fanbox 方案：GIT_DIR 放应用目录、`--work-tree` 指项目（项目零污染），agent 进入 busy 即静默快照，15s 节流、每项目 40 tag 滚动、回滚前先自动存档、拒绝家目录级大目录。价值高但归属 Agent/会话域，不占本模块排期，另开 ADR。

### W17 PDF 升级 pdf.js（S–M）：页码/缩放/文内搜索；bundle 体积走懒加载。

### W18 Web 模式降级决策（S）：二选一——`web-fs-client.ts` 补齐只读能力（列目录/读文件走 HTTP），或文件页在浏览器模式渲染明确的「需要桌面端」占位 UI。不允许维持现状的整页异常。

### W19 测试补齐（M，随各 W 分摊 + 收尾）

- Rust：`fs_watch.rs` / `archive.rs`(+W7 archive_ops) / `thumbnail.rs` 从零补齐；恶意 zip、事件过滤、缓存淘汰用例。
- 前端：`fs-change-filter` / `live-follow` 状态机 / `file-events` 单测；FileBrowser 拆分后的 RTL 组件测试（选择、剪贴板、右键菜单）。
- E2E（Playwright，可选收尾）：拖入导入 → 自动刷新 → 预览 → 编辑保存 一条主链路。

## 六、PR 与流程约束

1. **一个工作项一个 PR**（S 级可合并同 Phase 相邻项），PR 描述声明归类 **Hub**，W16 除外（Agent 域）。
2. UI 文案变更必须 zh/en 同步（i18n lint 硬门）。
3. W1/W10 等高频路径 PR 必须附 `npm run perf:check` 结果，预算不过不合。
4. 每个 Phase 结束打一次基线（测试全绿 + perf 基线），再进下一 Phase。
5. 移植 fanbox 逻辑时**连同其注释里的经验一起移植**（如"选中只改单个 DOM class 绝不重建网格"），落为代码注释或本目录文档。

## 七、里程碑概览

| 里程碑 | 内容 | 可感知成果 |
|---|---|---|
| M0 | 前置模块合并 + 第一节校准 | 方案修订版 |
| M1 | Phase 1 完成 | 文件变更实时点亮/自动刷新；编辑不再静默覆盖；自动保存 |
| M2 | Phase 2 完成 | live follow、解压/压缩、HEIC、终端路径定位——体验全面超过 fanbox 原版 |
| M3 | Phase 3 完成 | 模块拆分 + 契约类型化 + 测试兜底，长期可维护 |
