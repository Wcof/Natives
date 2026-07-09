# G1 参考项目能力提取与复刻矩阵

> 日期: 2026-07-08 · 分析人: AtomCode
> 覆盖: fanbox, claude-code, cc-switch, CodePilot, ghostty

---

## 1. fanbox 产品能力矩阵

### 项目概况
- 架构: Electron + Node.js 本地 HTTP 服务 + 浏览器 SPA
- 核心: 本地文件指挥中心
- 代码量: app.js 4892 行, style.css 1279 行

### 核心功能

| 能力 | fanbox 实现方式 | AiNative 复刻方案 | 优先级 | 落地 Goal |
|------|----------------|-------------------|--------|-----------|
| 文件夹网格/列表视图 | JS 动态渲染 + localStorage 记忆 | Next.js 组件控制 | ✅ 已复刻 | G4 |
| 面包屑路径导航 | 逐级渲染，每级可点击 | 已在 library 页面实现 | ✅ 已复刻 | G4 |
| ⌘K 全局搜索 | 命令面板 + 模糊匹配 + 内容:前缀全文搜索 | CommandPalette 组件已存在 | ✅ 已复刻 | G4 |
| 文件预览 (文本/MD/HTML/图片/视频/PDF) | 右侧面板内嵌，语法高亮 + iframe | 已有 FileBrowser | ⚠️ 部分复刻 | G4 |
| 收藏/最近 | localStorage 持久化 | DB 持久化更优 | 🔄 待规划 | G4+ |
| 项目识别徽章 | Node/Web/Python/Rust/Go/Git | 前端 file-badges.ts | ✅ 已复刻 | — |
| 空态 | 统一 `.empty-state` 组件 | `<EmptyState>` 组件已存在 | ✅ 已复刻 | G3 |
| 交互细节 | 鼠标悬停/点击/快捷键分层 | 统一 ConfirmDialog + Toast | ✅ 已复刻 | G3/G9 |
| AI 整理目录 | 三段式: 扫描提案→审批→执行+回滚 | 可借鉴至 AiNative | 🔄 待规划 | G7+ |
| 终端录像回放 | 黑匣子录制 + 播放 | terminal_recorder.rs 存在 ⚠️ placeholder | 🔄 待规划 | G7 |

### 必须复刻能力 (已基本完成)
- ✅ 文件夹浏览 CRUD — G4 完成
- ✅ Tag 标注系统 — G4 完成
- ✅ 搜索/筛选 — G4 完成
- ✅ 空态/错误态 — G3 完成
- ✅ 统计面板 — G4 完成

### 适合改造引入
- AI 整理目录流程（三段式 + 回滚日志）
- 终端录像回放交互

### 不适合直接引入
- Electron 架构（已迁移至 Tauri）
- Node.js 内置 HTTP 服务器（已替换为 Tauri IPC）

---

## 2. claude-code Agent/Subagent 能力矩阵

### 项目概况
- 架构: Node.js CLI, 官方 npm 包
- 核心: 终端内的 AI 编程助手

### 核心功能

| 能力 | claude-code 实现 | AiNative 方案 | 优先级 | 落地 Goal |
|------|----------------|---------------|--------|-----------|
| Agent 执行模型 | CLI 交互 → LLM → 工具调用 → 结果 | NativeRuntime + AgentRuntime trait | ✅ 已复刻 | G6 |
| Subagent 独立配置 | 每个 Agent 可配不同模型/行为 | Subagent DB + 独立 Provider 绑定 | ✅ 已复刻 | G8 |
| 工具调用 | Read/Write/Edit/Bash/Search 等 | NativeRuntime 工具系统 | ✅ 已复刻 | G6 |
| 权限/安全 | 命令执行前确认/拒绝 | PermissionCenter + 工具 guard | ⚠️ 部分复刻 | G8 |
| 上下文管理 | 系统提示 + 会话历史 + 文件内容 | context_assembler.rs | ✅ 已复刻 | G6 |
| 沙盒设置 | 可配置工作目录/环境变量 | 终端沙盒 cwd 策略 | ✅ 已复刻 | G7 |
| 会话持久化 | 自动保存/恢复 | assistant 会话 DB | ✅ 已复刻 | G6 |
| 错误恢复 | 重试/回退 | TaskManager 状态机 + 重试 | ✅ 已复刻 | G6 |

### 增强点（AiNative > claude-code）
- ✅ 每个 Subagent 独立 URL/Key — G8
- ✅ 多 Key fallback — G8  
- ✅ 可配置供应商绑定 — G5

### 不适合直接引入
- claude-code 系统配置修改（`~/.claude/`）
- npm 全局安装方式

---

## 3. cc-switch Provider/配置注入能力矩阵

### 项目概况
- 架构: Tauri + Next.js
- 核心: Claude/Codex/OpenAI 多供应商配置管理

### 核心功能

| 能力 | cc-switch 实现 | AiNative 方案 | 优先级 | 落地 Goal |
|------|---------------|---------------|--------|-----------|
| 多供应商管理 | 预设 38+ 供应商 + 自定义 | provider-presets.ts | ✅ 已复刻 | G5 |
| 多 Key 管理 | 每个 Provider 多个 Key | ProviderKey CRUD + 加密 | ✅ 已复刻 | G5 |
| Key 加密持久化 | AES-256-GCM | env_manager.rs AES-256-GCM | ✅ 已复刻 | G5 |
| URL 管理 | base_url + 可切换 | URL 标准化 + 多 URL 支持 | ✅ 已复刻 | G5 |
| 额度查询 | get_balance API | 已实现 provider_test | ✅ 已复刻 | G5 |
| 配置注入 | 写入第三方 CLI config 文件 | ❌ 不写入全局配置 | 🔄 AiNative 方案 | G6 |
| Skills 配置 | 检测/启用/停用 CLI 插件 | skills.rs | ✅ 已复刻 | G6 |
| 会话管理 | 为每次会话选择不同 Provider | Session ↔ Provider 绑定 | ✅ 已复刻 | G8 |
| 供应商 failover | 一级 Key 失败切二级 | Subagent fallback 策略 | ✅ 已复刻 | G8 |
| 日志脱敏 | 脱敏函数 | log_sanitizer.rs | ✅ 已复刻 | G5 |
| Auth 认证 | GitHub Copilot / Codex OAuth | 未实现 | 🔄 待规划 | G5+ |

### 不适合直接引入
- 修改第三方 CLI 全局配置（cc-switch 的核心能力但违反了 AiNative 安全原则）
- 覆盖 ~/.codex/config.json / claude_desktop_config.json

---

## 4. CodePilot 执行引擎能力矩阵

### 项目概况
- 架构: Electron + Next.js
- 核心: 第三方 AI CLI 执行引擎 + 桌面 UI

### 核心功能

| 能力 | CodePilot 实现 | AiNative 方案 | 优先级 | 落地 Goal |
|------|---------------|---------------|--------|-----------|
| CLI 执行引擎 | spawn Codex/Claude 子进程 | claude_cli.rs / codex_cli.rs | ✅ 已复刻 | G6 |
| 任务状态管理 | idle/running/success/failed/cancelled | TaskManager 状态机 | ✅ 已复刻 | G6 |
| 进程看门狗 | child_process 管理 + 超时 kill | AgentRuntime cleanup | ✅ 已复刻 | G6 |
| 日志脱敏 | log-sanitize.ts (sanitizeLogLine) | log_sanitizer.rs | ✅ 已复刻 | G5 |
| 终端/日志管理 | terminal-manager.ts | terminal.rs + terminal_recorder.rs | ✅ 已复刻 | G7 |
| 终端创建校验 | terminal-create-validation.ts | cwd 沙盒策略 | ✅ 已复刻 | G7 |
| 端口管理 | 动态端口 + 进程 tree kill | http_server.rs 动态端口 | ✅ 已复刻 | G6 |
| 失败恢复 | 失败任务可重试 | TaskManager 重试机制 | ✅ 已复刻 | G6 |
| 环境注入 | sanitizedProcessEnv 去敏感 env | env_injector.ts | ✅ 已复刻 | G5 |
| 测试体系 | unit/e2e/smoke 三层 | cargo test + npm test | ✅ 已复刻 | G10 |

### 必须保留差异
- Electron → Tauri 进程模型不同，`child_process` 替换为 Tauri command
- `taskkill` 跨平台差异 → Rust `std::process::Command`

### 适合改造引入
- terminal-create-validation 策略细化到 AiNative

---

## 5. ghostty 终端机制矩阵

### 项目概况
- 架构: Zig 原生终端
- 核心: 高性能终端渲染 + PTY 管理

### 核心功能

| 能力 | ghostty 实现 | AiNative 方案 | 优先级 | 落地 Goal |
|------|-------------|---------------|--------|-----------|
| PTY 管理 | Termio + Thread 架构 | terminal.rs portable-pty | ✅ 已复刻 | G7 |
| 终端解析 | Parser + CSI + OSC + DCS | ghostty_vt.rs（独立于 Termion） | ✅ 已复刻 | G7 |
| 终端渲染 | Metal/OpenGL/WebGL 三后端 | xterm.js（前端 xterm-addon-fit） | ✅ 已复刻 | G7 |
| 配置系统 | Config.zig 全面配置 | Settings 页面 UI 配置 | ✅ 已复刻 | G7 |
| 多 Tab 管理 | apprt TabGroup | useTerminalSessions.ts | ✅ 已复刻 | G7 |
| 终端回放 | 未直接提供 | terminal_recorder.rs | 🔄 待优化 | G7 |
| 性能优化 | GPU 渲染 + 增量更新 | xterm.js 默认基于 canvas | ✅ 可用 | G7 |
| 会话恢复 | — | useTerminalSessions + DB 持久化 | ✅ 已复刻 | G7 |

### 不适合直接引入
- Zig 代码直接搬入 Rust 项目（架构完全不兼容）
- GPU renderer（xterm.js 在 Tauri webview 中更合适）
- macOS 专属配置（Platform 配置项）

### 参考价值
- 终端状态机设计（Parser → Screen → Render）
- 配置分层（CLI args → 配置文件 → 默认值）

---

## 6. 综合复刻优先级矩阵

| 优先级 | 能力来源 | 能力 | 落地状态 | 剩余工作 |
|--------|---------|------|---------|---------|
| **P0** | fanbox | 资产管理系统（文件夹/Tag/搜索/统计） | ✅ G4 完成 | 无 |
| **P0** | G0 审计 | Provider Key 安全修复 | ✅ G5 完成 | 无 |
| **P0** | cc-switch/CodePilot | Provider/URL/Key 加密+脱敏管理 | ✅ G5 完成 | 无 |
| **P0** | G5A | SenseNova 测试供应商 | ✅ G5A 完成 | 需用户手动验证 |
| **P1** | CodePilot/claude-code | 执行引擎 (Native + CLI) | ✅ G6 完成 | 无 |
| **P1** | ghostty/CodePilot | 安全终端会话 | ✅ G7 完成 | terminal_recorder placeholder |
| **P2** | claude-code/cc-switch | Subagent 多 URL/Key 增强 | ✅ G8 完成 | 无 |
| **P2** | fanbox/CodePilot/cc-switch | 全局交互细节补全 | ✅ G9 完成 | 74 处 console.error |
| **P2** | all | 空壳功能清理 | ✅ G9 完成 | 6 处低优先级 |
| **P0** | all | 最终验收测试 | ✅ G10 完成 | 217 测试通过 |
| **P0** | all | G11 最终验收 | ✅ 完成 | 8 条路由/安全扫描 |

### 未落地能力 (需后续规划)

| 能力 | 来源 | 原因 | 建议 |
|------|------|------|------|
| AI 整理目录 | fanbox | 依赖 G7 终端 + G6 执行引擎成熟 | G7+ 追加 |
| 终端录像回放 | fanbox | terminal_recorder 占位 | G7 增量优化 |
| OAuth 认证 | cc-switch | 不紧急，无明确需求 | G5+ |
| GPU 终端渲染 | ghostty | xterm.js 已够用 | 长期优化项 |
| 第三方 CLI 配置写入 | cc-switch | 违反安全原则 | 永不实现 |

---

## 7. 审计统计

| 指标 | 数值 |
|------|------|
| 参考项目 | 5 个 |
| 可复刻能力 | 35+ 项 |
| 已完整复刻 | ~25 项 |
| 部分复刻/占位 | ~5 项 |
| 不适合引入 | ~5 项 |
| 待规划 | ~3 项 |

---

## 8. 结论

五个参考项目的能力已全部提取并映射到 G2-G11。核心发现:

1. **fanbox + claude-code 最高优先级** — 产品基座 + Agent 执行模型
2. **cc-switch 的配置注入能力需安全改造** — 不写入第三方 CLI 配置
3. **CodePilot 的执行引擎和测试体系可直接参考** — Tauri 适配已完成
4. **ghostty 的终端机制** — 状态机设计可参考但 Zig→Rust 转换成本高
5. **AiNative 已复刻 25+/35 项核心能力** — 剩余为低优先级或待规划能力
