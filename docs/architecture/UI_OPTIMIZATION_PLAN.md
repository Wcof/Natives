# Natives UI 优化计划 — 对齐 fanbox 交互模式

> 参考: `References/fanbox`
> 基线: `feat/25-file-types` 分支
> 日期: 2026-06-16

---

## 总览

| # | 模块 | 核心变更 | 涉及文件 |
|---|------|----------|----------|
| A | Header 顶栏 | 新建全局顶栏（sidebar toggle + breadcrumb + 右侧操作区） | `ShellLayout.tsx` + 新建 `Header.tsx` |
| B | 占用透视 | 新建磁盘占用弹窗 + statusbar 入口 + 右键菜单入口 | 新建 `DiskUsagePanel.tsx`, 改 `FileContextMenu.tsx`, `FileBrowser.tsx` |
| C | 右键菜单增强 | 补齐 fanbox 菜单项（在终端打开、在 Finder 显示、收藏/取消收藏、预览） | `FileContextMenu.tsx`, `FileBrowser.tsx`, `FileList.tsx`, `FileGrid.tsx` |
| D | 个人主页 | 仪表盘 → 个人主页：移除快捷操作，加入 Skill 透视 + 用量分析 | `page.tsx`, `UsagePanel.tsx` |
| E | Terminal 默认收起 | `terminalCollapsed` 默认 `true` | `ShellLayout.tsx` |

---

## A. Header 顶栏 — 对齐 fanbox topbar

### 设计

fanbox 的 topbar 是三区布局：左侧 sidebar toggle、中间 breadcrumb（可滚动）、右侧操作按钮。Natives 目前没有全局顶栏，breadcrumb 和操作按钮散落在 FileToolbar 中。

**方案**: 在 ShellLayout 的 content area 上方插入一个全局 `<header>`，所有 view 共享（文件视图显示 breadcrumb + 操作，非文件 view 只显示 sidebar toggle + view title）。

### 结构

```
┌─────────────────────────────────────────────────────────────────┐
│ [≡]  [Home › Desktop › ProjectX]     [hidden] [sort] [view] [⌘] │
└─────────────────────────────────────────────────────────────────┘
```

- **左区**: sidebar toggle 按钮（复用现有 `onToggle`）
- **中区**:
  - 文件视图: breadcrumb（从 FileBreadcrumb 提取，含 project badge）
  - 其他 view: view title 文本（如 "个人主页"、"AI 工作台"）
- **右区**（仅文件视图可见）:
  - 隐藏文件 toggle（从 FileToolbar 迁移）
  - Sort 分段控件（name/time/size）
  - View mode 分段控件（grid/list）
  - 终端 toggle 按钮

### fanbox 代码参考（像素级复刻）

| 功能 | fanbox 文件 | 行号 | 要点 |
|------|------------|------|------|
| HTML 结构 | `public/index.html` | 68-95 | 三区：`.nav-buttons` + `#breadcrumb` + `.topbar-actions` |
| Topbar 布局 CSS | `public/style.css` | 196-254 | `display:flex; gap:12px; padding:10px 16px; border-bottom; background:var(--bg-2)` |
| Breadcrumb CSS | `public/style.css` | 213-227 | `.crumb` padding:4px 7px, border-radius:6px, `.crumb.last` accent color + bold + accent-soft bg |
| Breadcrumb 渲染 | `public/app.js` | 303-339 | `renderBreadcrumb()` — 遍历 crumbs，`›` 分隔符，root `/` 用 monitor icon，`requestAnimationFrame` 滚到末尾 |
| 项目色点 | `public/app.js` | 321-335 | `.crumb-proj` — 7px 圆点，`hsl(hue 62% 48%)`，与终端 tab 图标同色 |
| 项目 badge | `public/style.css` | 224-226 | `.proj-badge` — 10px, padding:2px 7px, border-radius:20px, green bg |
| 分段控件 CSS | `public/style.css` | 232-238 | `.seg` border:1px solid border, border-radius:7px, `.seg button.active` accent bg |
| Ghost 按钮 CSS | `public/style.css` | 239-244 | `.ghost-btn` — border:1px, bg-3, border-radius:7px, hover 时 accent border |
| 终端按钮强调色 | `public/style.css` | 247-249 | `#btn-terminal` — accent color, accent-soft bg, font-weight:700 |
| 响应式收缩 | `public/app.js` | 2018-2027 | `ResizeObserver` 按顶栏宽度打 `tb-sm/xs/xxs/min` class |
| 响应式 CSS | `public/style.css` | 229-231 | `tb-sm<980` 隐藏 gridsize, `tb-xs<880` 隐藏 toggle, `tb-xxs<790` 隐藏 view+recent, `tb-min<660` 隐藏 sort |
| Sidebar toggle | `public/app.js` | 921-943, 2052 | `toggleSidebar()` — 切换 `sidebar-collapsed` class，persist 到 localStorage |
| Hidden toggle | `public/app.js` | 2132-2133 | checkbox change → `state.showHidden` → `renderFiles()` |
| Sort/View/Grid 分段 | `public/app.js` | 2135-2147 | 点击 button → 更新 state → toggle active class → `renderFiles()` |

### 涉及文件

| 文件 | 变更 |
|------|------|
| `src/components/shell/Header.tsx` | **新建** — 全局顶栏组件 |
| `src/components/shell/ShellLayout.tsx` | 在 content area 上方渲染 `<Header>`，传递 sidebar toggle、activeView、文件相关 state/callbacks |
| `src/components/files/FileToolbar.tsx` | 移除已迁移到 Header 的控件（hidden toggle, sort, view mode），保留 search + new file/folder + back/forward |
| `src/i18n/zh.ts` + `en.ts` | 新增 header 相关 i18n keys |

### 交互细节

1. **Electron 窗口拖拽**: header 设为 `-webkit-app-region: drag`，交互元素 exempt（`-webkit-app-region: no-drag`）— 参考 fanbox `style.css:886-890`
2. **Breadcrumb 行为**: 点击任意层级跳转、最后一个 segment 高亮（accent color + bold + accent-soft bg）、root `/` 显示为 monitor icon、`requestAnimationFrame(() => bc.scrollLeft = bc.scrollWidth)` 自动滚动到末尾
3. **响应式收缩**: ResizeObserver 观测顶栏自身宽度（非视口），按优先级隐藏：grid-size(980) > hidden toggle(880) > view mode+recent(790) > sort(660)，breadcrumb 吸收所有压缩压力（`flex:1 1 auto; min-width:130px; overflow-x:auto; scrollbar-width:none`），terminal 按钮始终保留
4. **键盘快捷键**: Cmd+B 切换 sidebar（已有，保持）

---

## B. 存储占用透视 — 对齐 fanbox diskPanel

### 设计

fanbox 的 `diskPanel()` 是一个 overlay dialog，展示当前目录下各项的磁盘占用，带比例条，可点击目录钻入。当前 Natives 已有 `DiskUsage.tsx` 组件（基础版）和 `disk-usage.ts` 后端，但没有作为独立 overlay 使用。

**方案**: 将 DiskUsage 升级为 overlay dialog 模式（对齐 fanbox），并增加入口：statusbar "占用透视" 链接 + 右键菜单 "磁盘占用透视"。

### fanbox 代码参考（像素级复刻）

| 功能 | fanbox 文件 | 行号 | 要点 |
|------|------------|------|------|
| `diskPanel()` 函数 | `public/app.js` | 1604-1634 | overlay 创建 → `load(p)` 递归加载 → 目录行可钻入 |
| Dialog CSS | `public/style.css` | 597-607 | `.disk-dialog` width:540px, `.disk-row` flex+gap:8px+padding:4px 8px, `.disk-bar` absolute 定位+accent-soft bg |
| Overlay 基础 CSS | `public/style.css` | 746-754 | `.input-overlay` fixed inset:0, bg:rgba(0,0,0,0.42), z-index:60, padding-top:18vh; `.input-dialog` border-radius:14px, padding:18px |
| 磨砂玻璃 | `public/style.css` | 973 | `.input-dialog` — `color-mix(in srgb, var(--bg-2) 85%, transparent)` + `backdrop-filter:blur(20px) saturate(1.4)` |
| 浮层双层阴影 | `public/style.css` | 950-951 | `box-shadow: 0 0 0 0.5px rgba(0,0,0,0.12), 0 12px 40px rgba(0,0,0,0.22)` |
| Statusbar 渲染 | `public/app.js` | 351-364 | `renderStatusbar()` — 项数/文件夹/文件/大小 + "占用透视" 链接 |
| Statusbar CSS | `public/style.css` | 536-543 | `#statusbar` sticky bottom, flex between, font-size:11.5px, backdrop-filter:blur(6px) |
| 右键菜单入口 | `public/app.js` | 1645 | `if (e.isDir) items.push({ label: '磁盘占用透视', fn: () => diskPanel(e.path) })` |
| API 响应格式 | — | — | `{ ok, total, more, items: [{ name, size, isDir }] }` — items 按 size 降序 |

### 涉及文件

| 文件 | 变更 |
|------|------|
| `src/components/files/DiskUsagePanel.tsx` | **新建** — overlay dialog 版本，复刻 fanbox `diskPanel()` |
| `src/components/files/DiskUsage.tsx` | 保留或合并到 DiskUsagePanel |
| `src/components/files/FileBrowser.tsx` | statusbar 添加 "占用透视" 链接 |
| `src/components/files/FileContextMenu.tsx` | 新增 "磁盘占用透视" 菜单项 |
| `src/app/api/fs/du/route.ts` | **新建** — web 模式 API route |
| `src/lib/web-fs-client.ts` | 新增 `diskUsage(path)` 方法 |
| `src/i18n/zh.ts` + `en.ts` | 新增 i18n keys |

### 交互细节（对齐 fanbox）

1. **Dialog 样式**: width:540px, max-width:92vw, border-radius:14px, 磨砂玻璃 bg, 双层阴影
2. **标题**: `"磁盘占用 · " + path.replace(homedir, '~')`
3. **加载态**: `"计算中…（大目录会慢几秒）"` — 居中 text-faint
4. **总计行**: `"共 X.X MB"` + 可选 `" · 只显示前 N 项"`
5. **上级目录行**: `.disk-up` — `"↑ 上一级"`，仅在 `path !== '/'` 时显示
6. **条目行**: 左侧绝对定位 `.disk-bar`（accent-soft bg，宽度 `Math.max(1, Math.round(size/max*100))%`），名称 + 右侧 mono 字体大小
7. **目录可钻入**: 点击目录行 → `load(subPath)` 递归
8. **关闭**: Escape 键 / 点击遮罩

---

## C. 右键菜单增强 — 对齐 fanbox contextMenu

### 设计

当前 Natives 右键菜单只有基础项。对照 fanbox 缺少以下功能：

| fanbox 菜单项 | 适用对象 | Natives 现状 | 需要 |
|---------------|---------|-------------|------|
| 磁盘占用透视 | 目录 + 空白 | ❌ 无 | ✅ 新增 |
| 在终端打开 | 目录 | ❌ 无 | ✅ 新增 |
| 在 Finder 显示 | 文件 + 目录 | ❌ 无 | ✅ 新增 |
| 预览 | 文件 | ❌ 无 | ✅ 新增（触发右侧 preview panel） |
| 收藏 / 取消收藏 | 文件 + 目录 | ❌ 无 | ✅ 新增 |
| 编辑文本/图片 | 特定文件 | ❌ 无 | ⏭ 跳过（后续） |
| AI 整理 | 目录 + 空白 | ❌ 无 | ⏭ 用户明确排除 |

### fanbox 代码参考（像素级复刻）

| 功能 | fanbox 文件 | 行号 | 要点 |
|------|------------|------|------|
| `showContextMenu()` | `public/app.js` | 1636-1658 | 目录 vs 文件分支，`danger:true` 标记废纸篓 |
| `blankMenu()` | `public/app.js` | 2091-2129 | 空白区域右键，`e.target.closest('.item')` 守卫防止与条目菜单冲突 |
| `popupMenu()` 渲染器 | `public/app.js` | 1660-1677 | 创建 `#context-menu` div，`sep` → `.ctx-sep`，`danger` → `.danger` class，viewport clamp 定位 |
| 菜单 CSS | `public/style.css` | 770-781 | `.context-menu` min-width:168px, padding:5px, border-radius:10px; `.ctx-item` padding:7px 12px, font-size:13px, border-radius:6px; `.ctx-item.danger` color:#e06a5b; `.ctx-sep` 1px height |
| 磨砂玻璃 | `public/style.css` | 972 | `.context-menu` — 85% opacity + blur(20px) saturate(1.4) |
| 浮层双层阴影 | `public/style.css` | 950-951 | 同 B 模块 |
| 全局关闭监听 | `public/app.js` | 2127-2129 | `document click` 非菜单区域 → close; `window blur` → close |

### ⚠️ 需要新增的 IPC 通道

当前 Natives 缺少以下 IPC 方法，需要在 `preload.ts` + `main.ts` 中添加：

| 方法 | 用途 | Electron 实现 |
|------|------|--------------|
| `shell.showItemInFolder(path)` | 在 Finder 显示 | `electron.shell.showItemInFolder(path)` |
| `shell.openPath(path)` | 在外部编辑器打开 | `electron.shell.openPath(path)` |
| `terminal.openInDir(dir)` | 在指定目录打开终端 | 创建新 PTY session，发送 `cd "dir"\n` |

### 涉及文件

| 文件 | 变更 |
|------|------|
| `src/components/files/FileContextMenu.tsx` | 重写 — 支持文件/目录/空白三种模式 |
| `src/components/files/FileBrowser.tsx` | 传递新 callbacks |
| `src/components/files/FileList.tsx` | 空白区域右键 `onContextMenu` |
| `src/components/files/FileGrid.tsx` | 空白区域右键 `onContextMenu` |
| `electron/preload.ts` | 新增 `shell.showItemInFolder`, `shell.openPath` |
| `electron/main.ts` | 新增对应 IPC handlers |
| `src/i18n/zh.ts` + `en.ts` | 新增 i18n keys |

### 菜单结构（对齐 fanbox 顺序）

**文件右键:**
```
预览
在编辑器打开（外部）
在 Finder 显示
────────────
复制路径
复制为终端 cd
────────────
⭐ 收藏 / 取消收藏
重命名…
移到废纸篓（红色）          ← danger:true → color:#e06a5b
```

**目录右键:**
```
打开
在终端打开
磁盘占用透视
在 Finder 显示
────────────
复制路径
复制为终端 cd
────────────
⭐ 收藏 / 取消收藏
重命名…
新建文件
新建文件夹
移到废纸篓（红色）
```

**空白区域右键:**
```
新建文件…
新建文件夹…
────────────
磁盘占用透视
```

---

## D. 个人主页 — 重构仪表盘

### 设计

用户明确要求：
1. "仪表盘" → "个人主页"
2. 移除"快捷操作"区域
3. 新增 Skill 透视（复用 SkillsPanel 的 stat cards）
4. 新增用量分析（复用 UsagePanel 的 Claude/Codex/RTK 用量）

### fanbox 代码参考（用量面板样式）

| 功能 | fanbox 文件 | 行号 | 要点 |
|------|------------|------|------|
| `usagePanel` 对象 | `public/app.js` | 2806-2889 | `bar()` 渲染进度条，`render()` 构建完整面板 |
| 用量进度条 CSS | `public/style.css` | 173-183 | `.usage-bar` height:4px, bg-3; `.usage-bar i` accent bg; `.usage-bar i.danger` #d4453a; `.usage-num.danger` red+bold |
| 用量三联 CSS | `public/style.css` | 184-185 | `.usage-trio` flex gap:4px, `.usage-trio span` bg-3, border-radius:6px |
| Token 格式化 | `public/app.js` | 2812-2817 | `fmtTok()` — B/M/k 格式化 |
| 重置时间格式化 | `public/app.js` | 2818-2825 | `fmtReset()` — "周X HH:MM 重置" |
| 危险阈值 | `public/app.js` | 2831 | `v >= 85 ? ' danger' : ''` — 85% 以上变红 |

### 涉及文件

| 文件 | 变更 |
|------|------|
| `src/app/page.tsx` | 重写 — 移除 Quick Actions，新增 Skill 透视 + 用量分析区域 |
| `src/components/ai/UsagePanel.tsx` | 提取为可复用组件（当前绑定在 AiWorkbench 中），导出独立版本 |
| `src/i18n/zh.ts` + `en.ts` | `dashboard.title` → `个人主页`，移除 quickActions 相关 keys，新增 skill/usage 相关 keys |

### 新布局

```
┌─────────────────────────────────────────────────┐
│  个人主页                                        │
│  AI 时代的桌面应用容器                            │
├─────────────────────────────────────────────────┤
│  ┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐           │
│  │ 数据库 │ │ 模块  │ │ 数据  │ │ 版本  │           │
│  │ 已连接 │ │ 3/5  │ │ 12MB │ │ 0.1  │           │
│  └──────┘ └──────┘ └──────┘ └──────┘           │
├─────────────────────────────────────────────────┤
│  SKILL 透视                          [查看全部 →] │
│  ┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐ ┌───────┐ │
│  │ 总计  │ │ 活跃  │ │ 休眠  │ │ 异常  │ │ 描述预算│ │
│  │  12  │ │   8  │ │   4  │ │   0  │ │ 32%   │ │
│  └──────┘ └──────┘ └──────┘ └──────┘ └───────┘ │
├─────────────────────────────────────────────────┤
│  用量分析                          [刷新]        │
│  Claude Code                                     │
│  5h 窗口  [████████░░] 82%                       │
│  周配额   [███░░░░░░░] 30%  周三 18:00 重置      │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐            │
│  │ 12K 近5h │ │ 45K 今日 │ │ 128K 本周│            │
│  └─────────┘ └─────────┘ └─────────┘            │
│  Codex                                           │
│  5h 窗口  [████░░░░░░] 40%                       │
├─────────────────────────────────────────────────┤
│  最近使用模块                                    │
│  [模块1] [模块2] [模块3] ...                      │
├─────────────────────────────────────────────────┤
│  最近打开文件                                    │
│  file1.ts  ~/project/src  2m                    │
│  file2.tsx ~/project/src  5h                    │
├─────────────────────────────────────────────────┤
│  最近活动                                        │
│  ● 模块已安装  3h                               │
│  ● 配置已更新  1d                               │
└─────────────────────────────────────────────────┘
```

### 交互细节

1. **Skill 透视区**: 4 个 stat cards + 描述预算条，点击 "查看全部" 跳转 AI Workbench Skills tab
2. **用量分析区**: 复刻 fanbox `usagePanel.bar()` 样式 — 4px 高进度条，85%+ 变红（`#d4453a`），token 三联（近5h/今日/本周），重置时间显示
3. **最近使用/文件/活动**: 保持现有逻辑

---

## E. Terminal 默认收起

### 设计

fanbox 的终端默认是收起的（HTML 中 `#terminal-panel` 带 `hidden` class，启动时不读取 `fb_term_open`）。

**方案**: 将 `terminalCollapsed` 默认值改为 `true`。已保存的用户状态优先恢复。

### fanbox 代码参考

| 功能 | fanbox 文件 | 行号 | 要点 |
|------|------------|------|------|
| 默认隐藏 | `public/index.html` | 115 | `<section id="terminal-panel" class="hidden">` |
| `term.open()` | `public/app.js` | 2245-2254 | remove `hidden` class → `applyDock()` → `newTab()` → persist `fb_term_open=1` |
| `term.close()` | `public/app.js` | 2255-2263 | add `hidden` class → remove active → persist `fb_term_open=0` |
| 启动时不恢复 | — | — | **无代码读取 `fb_term_open`** — 每次启动都是收起状态 |

### 涉及文件

| 文件 | 变更 |
|------|------|
| `src/components/shell/ShellLayout.tsx` | `terminalCollapsed` 初始值改为 `true` |

### 注意事项

- fanbox 的行为是**永远不恢复**终端状态（每次启动都收起）
- Natives 当前会从 DB 恢复终端状态 — 保持此行为（用户体验更好），仅将默认值从 `false` 改为 `true`

---

## 实施顺序

```
Phase 1: E (Terminal 默认收起)          — 1 行改动，零风险
Phase 2: D (个人主页重构)               — 独立页面，不影响其他 view
Phase 3: C (右键菜单增强)               — 增量添加菜单项 + 新增 IPC 通道
Phase 4: B (存储占用透视)               — 依赖 C（右键菜单入口）
Phase 5: A (Header 顶栏)               — 最大改动，涉及布局重构
```

---

## i18n 新增 Keys 汇总

### zh.ts 新增

```ts
header: {
  personal: '个人主页',
  aiWorkbench: 'AI 工作台',
},
fileBrowser: {
  // ... 现有 keys 保持 ...
  openInTerminal: '在终端打开',
  revealInFinder: '在 Finder 显示',
  diskUsage: '磁盘占用透视',
  diskUsageTitle: '磁盘占用',
  diskUsageTotal: '共 {total}',
  diskUsageShowFirst: '只显示前 {count} 项',
  diskUsageUp: '↑ 上一级',
  diskUsageLoading: '计算中…（大目录会慢几秒）',
  favorite: '收藏',
  unfavorite: '取消收藏',
  openInPreview: '预览',
},
dashboard: {
  title: '个人主页',
  skillInsights: 'Skill 透视',
  usageAnalysis: '用量分析',
  viewAllSkills: '查看全部',
  // 移除: quickActions, openTerminal, installModule, openSettings, fileBrowser
}
```

### en.ts 新增

```ts
header: {
  personal: 'Home',
  aiWorkbench: 'AI Workbench',
},
fileBrowser: {
  openInTerminal: 'Open in Terminal',
  revealInFinder: 'Reveal in Finder',
  diskUsage: 'Disk Usage X-Ray',
  diskUsageTitle: 'Disk Usage',
  diskUsageTotal: 'Total: {total}',
  diskUsageShowFirst: 'Showing first {count} items',
  diskUsageUp: '↑ Parent directory',
  diskUsageLoading: 'Calculating… (large directories may take a moment)',
  favorite: 'Favorite',
  unfavorite: 'Unfavorite',
  openInPreview: 'Preview',
},
dashboard: {
  title: 'Home',
  skillInsights: 'Skill Insights',
  usageAnalysis: 'Usage Analysis',
  viewAllSkills: 'View All',
}
```

---

## 验证清单

- [ ] `rtk tsc --noEmit` — 零错误
- [ ] `rtk next build` — 构建成功
- [ ] Web 模式: 个人主页正常显示（skill/usage 数据为空时优雅降级）
- [ ] Web 模式: 右键菜单正常工作（无 Electron API 时降级）
- [ ] Web 模式: 占用透视通过 API route 工作
- [ ] Electron 模式: 全部功能正常
- [ ] Terminal 默认收起，用户手动展开后刷新保持状态
- [ ] Header 在文件视图显示 breadcrumb + 操作，在其他 view 显示 title
- [ ] 响应式收缩: 窗口变窄时 header 操作区按优先级隐藏
- [ ] 右键菜单: "在 Finder 显示" 调用 `shell.showItemInFolder`
- [ ] 右键菜单: "在终端打开" 创建新终端 tab 并 cd
- [ ] 占用透视: 目录可钻入，Escape/遮罩关闭
