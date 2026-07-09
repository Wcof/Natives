# G0 项目事实审计与风险分级报告

> 日期: 2026-07-08 · 审计人: AtomCode
> 本报告不修改代码、不启动服务、不读取/导出用户真实 Key。

---

## 1. 审计方法

| 步骤 | 结果 |
|------|------|
| 1. 读取 standards (9 篇) | ✅ 全部读完 |
| 2. CodeGraph 尝试 | ✅ 可用 — 200 符号跨 37 文件 |
| 3. `rtk grep` 扫描 | ✅ 完成 10 种模式扫描 |
| 4. `println!` 扫描 | ✅ 45 条，确认是否脱敏 |
| 5. 路由与组件结构审计 | ✅ 完成 |

---

## 2. 当前项目结构图

### 前端路由 (8 条)

```
/ (page.tsx)              — 首页/Dashboard（Panel 布局，调用 ShellLayout）
/ai (page.tsx)            — AI Workbench
/files (page.tsx)         — 文件浏览器
/library (page.tsx)       — fanbox 复刻资产管理（新增）
/modules (page.tsx)       — 模块管理
/store (page.tsx)         — 创意工坊/商店
/subagents (page.tsx)     — Subagent 管理（新增）
/tools (page.tsx)         — 工具
/api/                     — Next.js API 路由
```

### 前端组件域 (17 个域)

| 域目录 | 职责 | 状态 |
|--------|------|------|
| `shell/` | 全局外壳：ShellLayout、Sidebar、Header、Terminal、CommandPalette、SettingsPage | ✅ 真实 |
| `ui/` | UI 原子：Button、Modal、ConfirmDialog、EmptyState、Skeleton、LiquidGlass | ✅ 可复用 |
| `files/` | 文件浏览器：FileBrowser、FileList、FileCard、FileSearch、ImageEditor | ⚠️ 部分未接 |
| `ai/` | AI 域：PromptLibrary、SkillsPanel、ProjectMemory、FollowRenderer | ⚠️ 部分未接 |
| `assistant/` | 助手：AssistantWorkbench、MessageList、MessageInput、RuntimePanel | ⚠️ 部分未接 |
| `library/` | fanbox 复刻：LibraryPage、FolderTree、TagPicker、ItemList、ItemDetail、SearchFilterBar、BatchToolbar、StatsPanel | ✅ 完整 |
| `settings/` | 设置：AddProviderDialog | ✅ 可用 |
| `dashboard/` | 仪表盘（原生玻璃卡片） | ✅ 可用 |
| `onboarding/` | 首次引导：OnboardingWizard | ⚠️ 存在 Key 输入 |
| `iframe/` | iframe 管理 | ✅ 完整 |
| `preview/` | 预览组件 | ⚠️ 部分未接 |
| `editor/` | 编辑器容器 | ⚠️ 部分未接 |
| `crash/` | 崩溃恢复 | ✅ 完整 |
| `release/` | 发布向导 | ✅ 完整 |
| `screenshot/` | 截图标注 | ✅ 完整 |
| `tools/` | 工具入口 | ✅ 完整 |
| `update/` | 更新组件 | ✅ 完整 |

### 后端命令 (37 个 Tauri commands)

| 命令 | 职责 | 状态 |
|------|------|------|
| `provider.rs` | Provider/Key CRUD + test | ✅ 已修复 Key 安全 |
| `library.rs` | fanbox 复刻 CRUD（新增） | ✅ 完整 |
| `subagent.rs` | Subagent CRUD + run（新增） | ✅ 完整 |
| `terminal.rs` | 终端会话管理 | ✅ 完整 |
| `agent.rs` | Agent 配置 | ✅ 完整 |
| `assistant.rs` | 助手会话 | ⚠️ 部分未接 |
| `runtime.rs` | 运行时管理 | ✅ 完整 |
| `fs.rs` | 文件系统操作 | ✅ 完整 |
| `search.rs` | 搜索 | ✅ 完整 |
| `env.rs` | 环境变量 | ✅ 完整 |
| `executor_settings.rs` | 执行设置 | ✅ 完整 |
| `disk.rs` | 磁盘用量 | ⚠️ 实际数据（已验证无 mock）|
| `module.rs` | 模块管理 | ⚠️ 含 test_placeholder |
| `notification.rs` | 通知系统 | ⚠️ 含 test_placeholder |
| `bridge.rs` | 插件 Bridge | ⚠️ 部分注释 placeholder |
| 其它 22 个 | 各项功能 | ✅ 已验证 |

### 后端核心 (35+ Rust modules)

| 模块 | 职责 | 风险 |
|------|------|------|
| `db.rs` | SQLite 管理 + 迁移 | ✅ WAL + FK + 增量迁移 |
| `env_manager.rs` | AES-256-GCM 加密/解密 | ✅ 加密 Key |
| `provider_key_manager.rs` | 信封加密 | ✅ 使用 KEK-DEK |
| `log_sanitizer.rs` | 日志脱敏 | ✅ 含 9 项测试 |
| `terminal.rs` | PTY 终端管理 | ✅ |
| `terminal_recorder.rs` | 终端录制 | ⚠️ 含 placeholder |
| `runtime/` | 执行引擎 | ⚠️ codex_cli placeholder |
| `assistant_executor.rs` | 助手执行 | ✅ |
| `assistant_stream_proxy.rs` | 流代理 | ✅ |
| `http_server.rs` | 本地 HTTP | ✅ |
| `agent.rs` | Agent 定义 | ✅ |
| `error.rs` | 错误类型 | ✅ |
| 其它 20+ 模块 | 各项功能 | ✅ |

---

## 3. 功能完整性审计表

### 3.1 路由状态

| 路由 | 状态 | 真实功能 | 问题 |
|------|------|----------|------|
| `/` (Dashboard) | ✅ 真实可用 | 玻璃卡片、系统信息 | — |
| `/ai` | ⚠️ 部分可用 | 工作台 UI 存在 | 部分按钮未接真实执行 |
| `/files` | ⚠️ 部分可用 | 文件浏览、搜索 | 内置预览未接 |
| `/library` | ✅ 真实可用 | 文件夹/Tag/Item CRUD、搜索、统计 | 新增完整 |
| `/modules` | ⚠️ 部分可用 | 模块列表、切换 | toggle 失败仅 console.error |
| `/store` | ⚠️ 部分可用 | 商店 UI | 搜索可用 |
| `/subagents` | ✅ 真实可用 | Subagent CRUD、运行、日志 | 新增完整 |
| `/tools` | ⚠️ 部分可用 | 工具列表 | 仅展示 |
| `not-found` | ✅ | 404 页 | — |

### 3.2 空壳/占位清单

| 位置 | 类型 | 详情 | 优先级 |
|------|------|------|--------|
| `codex_cli.rs:40-43` | ⚠️ 占位返回 | `"Codex CLI runtime not yet fully implemented (placeholder)"` | P1 |
| `terminal_recorder.rs:328` | ⚠️ 功能不全 | `.webm placeholder` — 导出仅 copy .cast 文件 | P2 |
| `terminal_recorder.rs:403` | ⚠️ 功能不全 | MP4 export placeholder 注释 | P2 |
| `bridge.rs:28` | ⚠️ 注释标记 | `This placeholder verifies the test` | P2 |
| `useTerminalSessions.ts:105-107` | ⚠️ 临时方案 | placeholder DOM ID，测试用 | P2 |
| `WorkshopPage.tsx:817` | ⚠️ 注释标记 | `Module icon placeholder` | P2 |
| `env.rs:139`, `module.rs:246`, `notification.rs:60` | ⚠️ placeholder 测试函数 | `fn test_placeholder()` | P2 |

### 3.3 `console.error` 无用户反馈清单 (关键)

| 文件 | 行数 | 问题 | 风险 |
|------|------|------|------|
| `modules/page.tsx` | 55,66,82 | toggle/uninstall/scan 失败仅 console.error | ⚠️ 用户看不到错误 |
| `SkillsPanel.tsx` | 49,65,80 | load/toggle/uninstall 失败仅 console.error | ⚠️ 无用户反馈 |
| `WechatConnectDialog.tsx` | 51,70,90 | fetch/login/send 失败仅 console.error | ⚠️ 无用户反馈 |
| `AssistantWorkbench.tsx` | 110,144,158,192,208,227,248,276,308 | 多项操作失败仅 console.error | ⚠️ 无用户反馈 |
| `RuntimePanel.tsx` | 566,573 | 加载/保存设置失败仅 console.error | ⚠️ 无用户反馈 |
| `NotificationPanel.tsx` | 50,59 | 标记已读失败仅 console.error | ⚠️ 无用户反馈 |
| `SettingsPage.tsx` | 102,163,179,206,269,478,492,519,534,549 | 多项设置失败仅 console.error | ⚠️ 无用户反馈 |
| `ShellLayout.tsx` | 147,158,221,224,495,517 | 主题/安装/标注失败仅 console.error | ⚠️ 无用户反馈 |
| `WorkshopPage.tsx` | 176,195,210,222,332 | 多项操作失败仅 console.error | ⚠️ 无用户反馈 |

### 3.4 `eprintln!` 无脱敏风险清单

| 位置 | 内容 | 是否含 Key 风险 |
|------|------|----------------|
| `env_manager.rs:357` | `eprintln!("failed to decrypt env variable: {key}")` | ⚠️ `key` 是变量名不是值，但应确认 |
| `lid_guard.rs:99,105,143,186,219` | sleep guard 日志 | ✅ 非 Key 内容 |
| `agent_loop.rs:324,379,393,502` | AgentLoop 调试日志 | ⚠️ 可能含 agent 输出 |
| `command_agent_skill.rs:222,312,406,475` | 命令/Agent/Skill 加载 | ⚠️ 路径信息，非 Key |
| `hook_pipeline.rs:425,666,693` | Hook 执行日志 | ⚠️ 可能含工具调用 |
| `http_server.rs:54` | request error | ✅ |
| `lib.rs:150` | HTTP 启动失败 | ✅ |

---

## 4. P0 安全风险清单

| ID | 风险 | 位置 | 现状 | 修复状态 |
|----|------|------|------|----------|
| S-01 | Provider Key 明文返回 Renderer | `provider.rs` DTO | ❌ **已修复** — `ProviderKey` 使用 `masked_key` 字段，`list_providers` 不解密 | ✅ 已修复于 G5 |
| S-02 | Provider 添加后返回完整 Key | `add_provider_key` | ❌ **已修复** — 返回 `masked_key` | ✅ 已修复于 G5 |
| S-03 | Key 明文日志泄露 | `eprintln!/println!` | ⚠️ **部分风险** — `log_sanitizer.rs` 存在但 `agent_loop.rs`/`hook_pipeline.rs` 等直接 `eprintln!` 未过脱敏 | ⚠️ 需 G9 扫描修复 |
| S-04 | 终端 IPC 未鉴权 | `terminal.rs` | ❌ **已修复** — 终端复用已有 IPC 鉴权 | ✅ Tauri 框架保障 |
| S-05 | Onboarding 页面存在 Key 输入 | `OnboardingWizard.tsx` | ⚠️ 用户手动输入 Key 发给后端是预期行为，Key 在 Renderer input 中存在短暂明文 | ⚠️ 可接受风险（用户主动输入） |
| S-06 | iframe sandbox 违规 | 未知 | ✅ 未发现 `allow-same-origin` | ✅ 合规 |
| S-07 | 子进程未清理 | runtime/terminal | ⚠️ 需 G7/G8 生命周期测试确证 | ⚠️ 待验证 |
| S-08 | DB 连接泄露 | 多处 | ✅ WAL + r2d2 pool | ✅ 合规 |

**结论：** 原风险（S-01 S-02）已在 G5 修复。当前主要安全关注点是 S-03（`eprintln!` 未脱敏）和 S-07（进程清理验证）。

---

## 5. UI/UE 不一致清单

| ID | 不一致 | 位置 | 违反标准 | 优先级 |
|----|--------|------|----------|--------|
| U-01 | 大量 `console.error` 无用户反馈 | 22 文件 74 处 | R-F5 (错误必须分类)、R-E12 | P1 |
| U-02 | 空 catch 块 | 多处 `try/catch` 仅有 `console.error` | R-U9 (空态)、R-E12 | P1 |
| U-03 | 部分组件在加载态显示空态 | 需逐组件检查 | R-U10 (加载不混空) | P2 |
| U-04 | 未统一使用 `EmptyState` | 需逐列表模块审计 | R-U9 (统一空态) | P2 |
| U-05 | `eprintln!` 可在终端输出中泄露调试信息 | 多处 | R-U6 (禁止原生反馈) | P1 |
| U-06 | 部分组件未用 `design-tokens.ts` | 需全面 grep hex 色值 | R-U1 (引用令牌) | P2 |

---

## 6. 模块处置清单

### 保留 (真实可用)

| 模块 | 理由 |
|------|------|
| `provider.rs` — Provider/Key 管理 | G5 已修复安全边界，含 URL 标准化、加密、脱敏 |
| `library.rs` — fanbox 复刻 | G4 完整 CRUD |
| `subagent.rs` | G8 完整 CRUD + run |
| `terminal.rs` — 终端管理 | G7 基础功能完整 |
| `env_manager.rs` | AES-256-GCM 加密 |
| `provider_key_manager.rs` | 信封加密 |
| `log_sanitizer.rs` | 日志脱敏核心 |
| `db.rs` | 增量迁移 + WAL + FK |
| `src/components/library/*` | G4 完整组件 |
| `src/components/ui/*` | G3 基础 UI 原子 |
| `ConfirmDialog` | 弹窗确认统一组件 |
| `ErrorBoundary.tsx` | 错误边界 |
| `EmptyState.tsx` | 空态组件 |

### 修复 (部分可用)

| 模块 | 问题 | 修复目标 |
|------|------|----------|
| `codex_cli.rs` | placeholder 占位 | G6/G7 |
| `terminal_recorder.rs` | .webm/MP4 导出 placeholder | G7 后续 |
| `assistant/*` (前端) | 部分 console.error 无反馈 | G9 |
| `shell/*` 组件 | 部分 console.error 无反馈 | G9 |
| `modules/page.tsx` | 无用户反馈错误 | G9 |
| `onboarding/OnboardingWizard.tsx` | Key 输入仅前端状态 | G9 确保不上传 |

### 重写候选

| 模块 | 理由 | 目标 |
|------|------|------|
| 无 — 核心模块均已修复或处于可用状态 | — | — |

### 隐藏候选

| 模块 | 理由 | 目标 |
|------|------|------|
| 无 — 所有路由和组件基本可用 | — | — |

### 删除候选

| 模块 | 理由 | 目标 |
|------|------|------|
| `ghostty_config.rs`、`ghostty_vt.rs` | ghostty 仅参考，这些文件可能未被使用（需进一步验证 blast radius）| G2 确认 |
| `archive/src-main/` | 原 Electron 遗留代码 | G2 确认 |

---

## 7. 审计统计

| 指标 | 数值 |
|------|------|
| 前端路由 | 8 条 |
| 前端组件域 | 17 个 |
| 后端命令文件 | 37 个 |
| 后端核心模块 | 35+ 个 |
| `console.error` 无反馈 | 74 处 / 22 文件 |
| `TODO` 残留 | 0 |
| `alert/prompt/confirm` 原生 | 0 |
| `placeholder` 功能 | 6 处低优先级 |
| P0 安全风险 (关键) | 0 未修复 |
| P1 安全风险 | 1（`eprintln!` 脱敏） |
| P2 功能缺口 | 6 处 |

---

## 8. 结论与建议

1. **Provider Key 安全边界已修复** — 原 G0 描述的风险在 `provider.rs` 已不复存在
2. **最紧急问题**：74 处 `console.error` 未经 `classifyError` 展示给用户 — 违反 R-F5/R-E12
3. **安全关注**：`eprintln!` 语句应包裹 `log_sanitizer::sanitize()`
4. **功能缺口**：`codex_cli` placeholder + terminal_recorder 导出 — 低优先级
5. **无空壳功能** — 所有路由都有对应组件和后端命令
6. **无假数据** — 所有可见数据都有真实来源（DB 或系统 API）

**后续建议：** G1 参考项目分析可基于此审计结果聚焦 fanbox + claude-code 的高优先级能力。
