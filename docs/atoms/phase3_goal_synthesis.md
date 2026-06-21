# Phase 3 — Goal Synthesis Layer

> Generated: 2026-06-20
> Input: Phase 1 (143 atomic Tasks) + Phase 2 (4 domain Blueprints)

---

## Natives 总体架构设计

### 架构选型: Tauri v2 + Next.js + SQLite + Rust

竞品 FanBox 采用 **Electron + Node.js HTTP Server** 单体架构:
- `server.js`(2180L) — 纯Node HTTP后端，零依赖
- `electron/main.js`(842L) — Electron主进程(node-pty + IPC)
- `public/app.js`(4572L) — 纯DOM前端，无框架

Natives 架构升级为 **三层分离**:

```
┌─────────────────────────────────────────────────────┐
│                   Frontend Layer                     │
│   Next.js 15 + React 19 + Tailwind + shadcn/ui     │
│   xterm.js + Monaco + Milkdown + hljs               │
│   ─── tauri-adapter (唯一IPC出口) ───                │
├─────────────────────────────────────────────────────┤
│                 Infrastructure Layer                 │
│   Tauri v2 Rust Backend                             │
│   ├── file_manager/   (目录/搜索/预览/编辑/缩略图)   │
│   ├── terminal/       (PTY/录像/Agent检测/合盖)     │
│   ├── skills/         (五源扫描/健康/触发/启停)     │
│   ├── agents_usage/   (token统计/配额/OAuth/限额)   │
│   ├── database/       (SQLite WAL + 迁移)           │
│   ├── http_server/    (HTML预览独立端口)             │
│   └── security/       (CSP/IPC白名单/路径校验)      │
├─────────────────────────────────────────────────────┤
│                    Overall Layer                     │
│   Tauri Window Manager (FOUC防护/主题信号)          │
│   Config Manager (原子写+串行化)                     │
│   Event Bus (db-state-changed IPC广播)              │
│   Watchdog (进程健康监控)                            │
└─────────────────────────────────────────────────────┘
```

### 竞品架构 → Natives架构 对照

| 维度 | FanBox (竞品) | Natives2 (升级) |
|------|--------------|-----------------|
| 运行时 | Electron (Chromium+Node.js) | Tauri v2 (系统WebView+Rust) |
| 后端 | Node.js HTTP Server (单文件2180L) | Rust模块化 (多crate) |
| 前端 | 纯DOM (app.js 4572L) | Next.js + React组件化 |
| 通信 | HTTP localhost:4567 | Tauri IPC (零网络监听) |
| PTY | node-pty (Electron原生模块) | portable-pty (Rust原生) |
| 存储 | config.json (文件) | SQLite WAL + config.json |
| 安全 | Host/Origin校验+sandbox | Tauri CSP+IPC白名单+sandbox |
| 包体 | ~150MB (Electron) | ~15MB (Tauri) |
| 内存 | ~300MB (Electron基础) | ~50MB (Tauri基础) |

---

## 模块依赖关系（对比竞品架构的升级点）

### 竞品依赖图 (FanBox)

```
electron/main.js ──require──> server.js
     │                           │
     │ pty:spawn/input/resize    │ HTTP路由
     │ rec:start/event/stop      │ listDir/readFile/walk
     │ wechat:*                  │ searchFiles/grepFiles
     │ clip:image/file           │ writeTextFile/trashPath
     │ drop:save/copy            │ skillsData/skillToggle
     │                           │ claudeUsage/codexUsage
     ▼                           ▼
  preload.js ──contextBridge──> public/app.js
                                    │
                                    │ DOM渲染+fetch(/api/*)
                                    ▼
                              public/index.html
                              public/style.css
                              public/i18n-dict.js
```

**竞品问题**:
1. server.js是2180L单文件上帝对象，所有业务逻辑耦合
2. HTTP localhost通信有DNS rebinding/CSRF风险
3. 前端app.js是4572L单文件，无组件化，无虚拟DOM
4. preload.js contextBridge暴露面过大

### Natives依赖图 (升级)

```
src-tauri/src/
  main.rs ──mod──> file_manager/
                ├──> terminal/
                ├──> skills/
                ├──> agents_usage/
                ├──> database/
                ├──> http_server/    (仅HTML预览用)
                └──> security/

  file_manager/ ──dep──> database/ (缓存/配置)
                  ──dep──> security/ (路径校验)

  terminal/     ──dep──> database/ (录像元信息)
                ──dep──> file_manager/ (cwd检测)

  skills/       ──dep──> database/ (触发统计缓存)
                ──dep──> file_manager/ (文件操作)

  agents_usage/ ──dep──> database/ (offset缓存)
                ──dep──> security/ (OAuth token)

src/app/ (Next.js)
  ──tauri-adapter──> window.nativesAPI.*  (唯一IPC出口)

src/components/
  file-manager/ ──dep──> tauri-adapter
  terminal/     ──dep──> tauri-adapter
  skills/       ──dep──> tauri-adapter
  agents-usage/ ──dep──> tauri-adapter
```

**升级点**:
1. ✅ 后端模块化: 每个领域独立Rust模块，单文件<500L
2. ✅ IPC替代HTTP: Tauri command零网络监听，无DNS rebinding风险
3. ✅ 前端组件化: React组件<200L，虚拟DOM+懒加载
4. ✅ 单一IPC出口: tauri-adapter封装，前端不直接调Tauri API
5. ✅ SQLite持久化: 替代内存Map缓存，进程重启不丢状态

---

## 状态机系统总览

### 全局状态机

```
AppLifecycle: launching → theme_ready → window_shown → running → closing → exited
```

### 领域状态机矩阵

| 领域 | 状态机 | 状态数 | 关键转换 |
|------|--------|--------|---------|
| FileView | idle→navigating→loaded→(previewing\|editing\|searching) | 5 | navigate触发listDir |
| FileEdit | clean→dirty→saving→(saved\|conflict) | 4 | expectedMtime冲突检测 |
| FileFollow | off→following→paused→following | 3 | 绑定终端tab cwd |
| FileSearch | idle→typing→searching→results→idle | 4 | fuzzyScore+grep+Spotlight |
| TerminalTab | spawning→ready→(idle\|busy)→exiting→closed | 5 | pty:spawn→onData→onExit |
| AgentSession | none→launching→running→idle | 4 | proc检测驱动 |
| Recording | off→recording→(pruning\|exporting)→off | 4 | asciinema cast格式 |
| WechatConnect | uninstalled→installed_off→...→connected | 6 | QR过期自动刷新 |
| LidGuard | sleep_normal→sleep_disabled→sleep_normal | 2 | 终端数>0时禁休眠 |
| SkillScan | idle→scanning→scanned→(health_checking→health_checked) | 4 | 30s缓存自动刷新 |
| SkillToggle | enabled→disabling→disabled→enabling→enabled | 4 | _disabled/迁移 |
| UsageFetch | idle→fetching→(success\|stale\|error)→idle | 4 | 60s自动刷新 |
| OAuthToken | valid→expired→refreshing→(valid\|error) | 4 | expiresAt检测 |
| Trash | idle→confirming→trashing→(done\|error) | 4 | 废纸篓可恢复 |

**竞品对比**: 竞品状态分散在app.js全局变量和server.js闭包中，无显式状态机定义。Natives用React useState/useReducer+Rust enum显式建模，可追踪可调试。

---

## 性能优化总结（重点说明如何超越竞品性能）

### 启动性能

| 指标 | FanBox (Electron) | Natives2 (Tauri) | 提升倍数 |
|------|-------------------|------------------|---------|
| 冷启动时间 | ~3s | ~0.5s | **6x** |
| 包体大小 | ~150MB | ~15MB | **10x** |
| 基础内存 | ~300MB | ~50MB | **6x** |

### 文件操作性能

| 操作 | FanBox (Node.js) | Natives2 (Rust) | 提升倍数 |
|------|-----------------|-----------------|---------|
| 模糊搜索 | 单线程walk+fuzzyScore | rayon并行walk+SIMD fuzzy | **4-8x** |
| 目录列表 | 同步readdir+stat | tokio并行readdir+stat | **2-3x** |
| 大文件读取 | 256KB截断+同步read | 流式读取+零拷贝 | **2x** |
| 缩略图生成 | sips子进程 | Rust image crate原生解码 | **3x** |
| 文件监听 | fs.watch+stat噪声过滤 | notify crate+FSEvents | **2x** |
| grep搜索 | 单线程顺序读文件 | rayon并行读+memchr | **4-6x** |

### 终端性能

| 操作 | FanBox (node-pty) | Natives2 (portable-pty) | 提升 |
|------|-------------------|------------------------|------|
| PTY数据传输 | Electron IPC JSON序列化 | Tauri Event字节流 | **2x** |
| 录像写盘 | fs.createWriteStream | tokio BufWriter异步 | **1.5x** |
| cwd检测 | lsof子进程(100-200ms) | Rust /proc直接读(<1ms) | **100x** |

### 数据持久化性能

| 操作 | FanBox (内存Map) | Natives2 (SQLite WAL) | 提升 |
|------|-----------------|----------------------|------|
| Agent统计缓存 | 进程重启全量重扫8天日志 | SQLite持久化offset，增量解析 | **10x+** |
| Skills触发统计 | 内存Map，重启全量重扫 | SQLite存储，重启秒级恢复 | **10x+** |
| 配置读写 | JSON文件_cfgChain串行化 | SQLite事务+JSON fallback | **2x** |

### 前端渲染性能

| 操作 | FanBox (纯DOM) | Natives2 (React) | 提升 |
|------|---------------|------------------|------|
| 文件列表渲染 | 全量innerHTML | React虚拟DOM diff | **3-5x** |
| 大目录滚动 | 全量DOM节点 | react-virtualized虚拟滚动 | **10x+** |
| 预览面板 | 全量渲染 | React.lazy懒加载 | **2x** |
| 搜索结果 | 全量替换 | 增量更新+key优化 | **2x** |

### 性能超越核心策略

1. **Rust零成本抽象**: 编译期优化，无GC停顿，无V8 JIT预热
2. **rayon数据并行**: 搜索/walk/grep全并行，充分利用多核
3. **SIMD指令加速**: fuzzyScore用std::simd子序列匹配
4. **零拷贝I/O**: Rust mmap+slice，无Node Buffer拷贝
5. **SQLite WAL**: 并发读写无锁，持久化缓存重启不丢
6. **系统WebView**: 复用OS渲染引擎，不额外加载Chromium
7. **tokio异步运行时**: 无回调地狱，零分配future

---

## 风险与安全总结

### 安全防线 (五大防线对标竞品)

| 防线 | FanBox | Natives2 | 升级 |
|------|--------|----------|------|
| 1. 通信安全 | HTTP localhost+Host/Origin校验 | Tauri IPC(无网络监听)+CSP | ✅ 无DNS rebinding风险 |
| 2. 沙箱隔离 | sandbox iframe+独立端口 | 同方案+Tauri scope限制 | ✅ 更细粒度资源控制 |
| 3. 路径校验 | validName+resolvePath+previewPathAllowed | 同方案+Tauri fs scope | ✅ 双重防护 |
| 4. 进程隔离 | Electron contextIsolation+contextBridge | Tauri IPC白名单+command scope | ✅ 最小权限原则 |
| 5. 数据安全 | config.json明文 | SQLite+AES-256-GCM(凭证) | ✅ 凭证加密存储 |

### 迁移风险矩阵

| 风险 | 严重度 | 概率 | 缓解 |
|------|--------|------|------|
| node-pty→portable-pty API差异 | 高 | 中 | 保留node-pty sidecar fallback |
| xterm.js WebGL在Linux WebView不工作 | 中 | 中 | 预检测+Canvas降级 |
| macOS废纸篓AppleScript在Tauri不工作 | 低 | 低 | Rust osascript CLI同等实现 |
| Spotlight mdfind仅macOS | 中 | 确定 | Linux/Windows用grep兜底(竞品同方案) |
| Claude/Codex jsonl格式变化 | 中 | 低 | 版本检测+graceful degradation |
| TLS指纹拦截需保留curl | 低 | 确定 | Rust Command调curl，token经stdin |
| 微信ClawBot依赖OpenClaw | 中 | 确定 | 初版跳过，后续sidecar实现 |
| 合盖保持需sudoers | 低 | 低 | 安装时一次性配置 |
| 前端i18n双语同步 | 低 | 中 | CI脚本自动检测缺失key |
| SQLite迁移逻辑 | 中 | 中 | sqlx migrate+版本化schema |

### 安全红线 (MUST)

1. **MUST**: 前端只通过 `window.nativesAPI` (tauri-adapter) 访问后端
2. **MUST**: 凭证用 AES-256-GCM 加密存储 (env_manager.rs)
3. **MUST**: Plugin iframe sandbox 必须 `allow-scripts allow-forms`，禁止 `allow-same-origin`
4. **MUST**: IPC command 注册必须带 `access_scope` 限制
5. **MUST**: 路径校验拒绝空字节+目录穿越
6. **MUST**: POST写操作必须校验Origin (HTML预览独立端口)
7. **MUST**: 原子写必须 temp+fsync+rename
8. **MUST**: 配置读写必须串行化(防last-writer-wins)

### 跨平台风险

| 平台 | 关键差异 | 缓解 |
|------|---------|------|
| macOS | sips/qlmanage/mdfind/AppleScript/Keychain | Rust条件编译+CLI调用 |
| Linux | 无mdfind/无sips/废纸篓依赖gio/trash-cli | grep兜底+image crate+gio trash |
| Windows | 无login shell/废纸篓用VB/无lsof | PowerShell+VB SendToRecycleBin+WinAPI |

---

## 实施优先级

### P0 — 核心骨架 (必须先完成)
1. Tauri v2 + Next.js 项目骨架
2. tauri-adapter IPC通信层
3. SQLite数据库初始化+迁移
4. FOUC防护(窗口隐藏→theme_ready_signal→显示)

### P1 — 文件管理器核心
5. list_dir + Breadcrumb + 项目检测
6. read_file + RichPreview (文本/MD/HTML/图片)
7. search_files + fuzzyScore + grepFiles
8. write_text_file + 原子写 + 冲突保护
9. trash_path + rename_path (三平台废纸篓)
10. fs.watch + 噪声过滤 + 变更徽标
11. File_GridListView + File_SortFilter + 键盘导航

### P2 — 终端核心
12. PTY spawn/resize/kill (portable-pty)
13. xterm.js + 多标签页
14. cwd双向联动 + 路径可点击
15. Agent启动按钮 + 进程检测
16. 终端录像 + 回放 + 导出

### P3 — Skills + Agents Usage
17. 五源聚合扫描 + 健康检查
18. 触发统计 + Context预算
19. Claude token统计 + 官方限额
20. Codex用量 + 配额过期守卫

### P4 — 高级功能
21. 文件跟随 + 实时渲染
22. 图片编辑器
23. Monaco/Milkdown编辑器
24. 微信ClawBot (可选)
25. AI整理 + 发版向导
