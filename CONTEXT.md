# Natives — 项目术语表

> 本文件是 Natives 项目的领域词汇表。所有术语以用户（产品方）视角定义，不含实现细节。

## 核心概念

### Natives
AI 时代的桌面应用容器（"AI Steam Base"）。定位为类似 Steam 的生态基座，用户在其中浏览、安装、运行页面级插件，内置终端和环境管理。

### 模块（Module）
用户在 Natives 中安装和运行的功能单元。每个模块是一个完整的 HTML/JS/CSS 页面，从本地目录 `~/.natives/modules/` 发现和加载。等同于"插件"。

### 插件（Plugin）
同"模块"。在技术上下文中更强调其运行时形态（iframe 中加载的页面）。

### 基座（Host / Shell）
Natives 容器本身，提供三栏布局、终端、模块管理、环境注入等基础设施。区别于用户安装的"模块"。

### 创意工坊（Workshop）
内置的插件浏览和管理界面。类似 Steam 创意工坊，用户可以在这里发现、安装、管理模块。

---

## 通信与安全

### Bridge API
插件与基座之间的通信接口，通过 `window.natives.*` 暴露。包含数据读写、主题获取、通知发送、生命周期管理等能力。

### Session Token
插件实例与基座之间的会话凭证。用于验证 postMessage 和 HTTP 请求的合法性。

**握手方式**：两阶段握手 — iframe 加载后主动向基座请求 token，基座验证后下发。插件重载时自动重新请求。详见 [[ADR-0001-session-token-handshake]]。

### 权限（Permissions）
插件通过 `manifest.json` 声明所需能力（如 `env.read`、`notification.send`）。用户安装时确认授权，运行时基座强制检查。

### 来源验证（Source Verification）
基座验证 postMessage 来自正确 iframe 的机制。因 sandbox iframe 的 origin 为 `"null"`，改用 `MessageEvent.source` 窗口引用匹配 — 基座创建 iframe 时保存 `contentWindow`，收到消息时检查 `event.source` 是否匹配。详见 [[ADR-0002-postmessage-origin-verification]]。

---

## 布局与界面

### 三栏布局
Natives 的主界面结构：左侧边栏（模块列表）+ 中间主内容区（iframe 容器）+ 右侧面板（工坊/设置）+ 底部终端。

### 主题（Theme）
Natives 的视觉风格系统。通过 `data-theme` 属性切换，所有颜色/间距/圆角通过 CSS 变量注入。内置三套主题：Terminal Volt（暗色）、Warm Archive（暖色亮色）、Editorial Index（编辑风格）。

### 终端（Terminal）
底部可折叠的完整 PTY 终端，支持 TUI 程序、窗口调整、多会话。启动时自动注入用户配置的环境变量。

---

## 环境与配置

### 环境配置（Env Profile）
一组环境变量的集合（如 API Key、代理地址）。用户可以创建多组配置（如"工作"、"个人"），终端启动时自动注入当前激活的配置。

### 凭证（Credentials）
存储在环境配置中的敏感信息（API Key 等），使用 AES-256-GCM 加密存储在 SQLite 中。

---

## 插件生命周期

### 心跳（Heartbeat）
插件定期向基座发送的存活信号（每 5s）。连续 3 次缺失（15s）标记为无响应，再 10s 无恢复标记为已崩溃。详见 [[ADR-0006-iframe-crash-detection]]。

### 状态分层（State Layers）
插件状态的保留策略：热层（当前可见，JS 内存保留）、温层（最近 5 个后台，隐藏但保留）、冷层（超出温层，销毁）、持久层（插件主动通过 `natives.db.set()` 保存）。详见 [[ADR-0005-plugin-state-preservation-strategy]]。

### 插件间通信（IPC）
插件之间的消息传递机制。因 sandbox 限制，所有消息经基座主进程中转，支持定向发送（`send`）和广播（`broadcast`）。详见 [[ADR-0003-plugin-ipc-main-process-relay]]。

---

## 设计哲学

### 不造轮子（No Domain Wheel Reinvention）
"完全不写"的适用范围是**插件层**：不自己写 AI 客户端、代码编辑器等，直接嵌入现有工具。**基座层必须自建**：容器本身就是轮子，没有现成替代品。详见 [[ADR-0007-domain-wheel-reinvention-clarification]]。

---

## 文件管理

### 文件浏览器
基座内置的文件管理界面。支持网格视图（缩略图卡片）和列表视图（详细行），嵌入三栏布局的中间内容区。侧边栏提供快捷入口（桌面、文档、下载等）和收藏夹。

### 文件预览
右侧面板中的文件内容预览。支持 Markdown WYSIWYG 编辑（Milkdown）、HTML 沙箱实时渲染、代码语法高亮、图片/视频/音频/PDF 内联播放、压缩包内容列表。

### 文件搜索
Cmd+K 全局搜索的文件扩展。支持模糊文件名搜索（评分算法）和 `content:` 前缀全文搜索，范围可在当前文件夹和全盘之间切换。

### 原子写入
文件写入的安全机制：写入临时文件 → fsync → rename。防止并发编辑（如 Agent 和用户同时编辑同一文件）导致数据损坏。通过 mtime 冲突检测实现。

---

## AI 工作台

### Agent 变更监控
实时显示 Agent 对文件的修改。文件卡片在 Agent 写入时闪烁（强度与变更频率成正比），跟随模式自动跳转到 Agent 当前编辑的文件。

### 跟随模式
终端与文件浏览器的联动模式。终端跟随模式：切换目录时终端自动 cd。文件跟随模式：文件视图自动跳转到 Agent 当前编辑的文件。

### 变更收件箱
聚合本次会话中所有被 Agent 修改的文件列表，方便用户快速回顾工作成果。

### 会话回放
通过时间轴滑块逐步重放 Agent 触摸过的文件，帮助用户理解 Agent 的工作过程。

### 项目记忆
扫描 Claude Code 和 Codex 的会话日志（`~/.claude/projects/` 和 `~/.codex/sessions/`），显示任何项目文件夹的历史 AI 会话。支持恢复会话。

### Skills X-Ray
扫描本机所有 Agent Skills（5 个来源目录），显示触发统计、健康状态，支持启用/禁用/卸载。

### Agent 用量追踪
显示 Claude Code 的 5 小时窗口用量、周配额、本地 Token 统计，以及 Codex 的用量和计划类型。

### RTK 用量分析
显示 RTK（Rust Token Killer）的 Token 节省统计、命令历史、使用趋势。

### AI 文件整理
"AI 提议→人工审核→基座执行"的文件整理工作流。AI 只读取文件元数据（不读内容），每个建议附带理由，用户逐项审核后执行，支持一键撤销。

---

## 工具

### 发布向导
Node 项目的发布管理工具。检查 package.json、git 状态、CHANGELOG、gh CLI，支持一键版本号递增、CHANGELOG 更新、GitHub Release。

### 截图快递
监控系统截图目录，新截图时弹出浮动卡片，提供三个操作：发送到终端（作为 Agent 上下文）、保存到素材目录、打开标注编辑器。

### 图片标注
内置的图片编辑工具。支持画笔、箭头、文字、模糊/遮挡等标注操作，可进行格式转换和压缩。

### 飞入终端（Fling to Terminal）
选中文本后出现"Send to Terminal"按钮，点击后文本以飞行动画发送到终端，使用 bracketed paste 模式注入，终端面板发光反馈。

### 热度发光（Heat-based Glow）
文件卡片在 Agent 修改时显示与变更频率成正比的发光效果。`--heat` CSS 变量驱动 glow 强度，涟漪动画从图标中心扩散，左上角显示变更计数角标。

### 骨架屏（Skeleton Loading）
数据加载时显示的灰色脉冲占位符，替代"Loading..."文本。支持文本行、卡片、头像、表格四种变体，提升感知性能。

### 键盘快捷键帮助
Cmd+/ 打开的快捷键面板，展示所有可用键盘快捷键。毛玻璃背景，kbd 标签样式。

### Toast 通知
全局共享的通知系统，支持 info/success/error/warning 四种类型，自动 3s 消失，底部右侧定位。

### 噪音过滤
变更收件箱和跟随模式自动排除系统文件（.git、node_modules、.next、dist 等），避免干扰。变更收件箱支持去重计数（同一文件多次变更显示 ×N）。

### 导航历史
文件浏览器支持 Cmd+[ / Cmd+] 后退/前进，维护历史栈，工具栏显示 ← → 按钮。

---

## 版本与兼容性

### 语义化版本（SemVer）
Natives 和插件都遵循语义化版本：主版本.次版本.修订号。主版本号变更表示不兼容的 API 变更，次版本号表示向后兼容的功能新增，修订号表示向后兼容的问题修正。

### minNativesVersion
插件在 `manifest.json` 中声明的最低基座版本要求。主版本号不同则阻止加载，次版本号不同则警告但允许运行。

### 插件更新（Plugin Update）
数据保护式更新流程：备份 module_data → 替换插件文件 → 保留 module_data → 重新加载。用户手动触发，不自动更新。
