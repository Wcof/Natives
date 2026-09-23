# AiNative Master Checklist（原始 Task ID 口径）

> 汇总：Global 总数 175，通过 175，待迁移后阶段删除 0（全量通过 175）；Home 总数 150，全量通过 150。
> 更新时间：2026-08-20，依据全量生产代码与 Release Gate 验证证据。

## Global（175）

| ID | 标题 | 验收标准 / 证据 | 状态 |
|---|---|---|---|
| GOV-001 | 建立 Architecture Baseline ADR | docs/adr/0020-ai-native-personal-workspace-rearchitecture.md:1 — ADR-0020 冻结 AI Native Personal Workspace 架构 | PASS |
| GOV-002 | Supersede 旧进程模型 MUST | docs/standards/technical/01-layering.md:12 + docs/adr/0020-ai-native-personal-workspace-rearchitecture.md:39 — ADR-0020 取代 ADR-0019/0012/0015/0016 | PASS |
| GOV-003 | 更新 Secret 安全规范 | src-tauri/src/secrets/store.rs:1 + src-tauri/src/secrets/keychain.rs:1 — SecretStore trait + macOS Keychain adapter 实现 | PASS |
| GOV-004 | 重写 CODE_MODULE_GUIDELINES Agent 专章 | docs/architecture/CODE_MODULE_GUIDELINES.md:1 — 模块拆分与规模限制，适配 Domain 模型 | PASS |
| GOV-005 | 更新 architecture-check.mjs | scripts/architecture-check.mjs:1 — 包含模块边界、架构债务与规则校验 | PASS |
| GOV-006 | 建立性能 baseline harness | scripts/perf/check-bundle.mjs:1 + .runtime-evidence/baseline.json:1 — bundle 预算与性能基准 | PASS |
| PXY-001 | 建立三协议 fixture runner | crates/provider-adapters/tests/request_body_golden.rs:1 + src-tauri/src/proxy/native.rs:1 — 三协议 transport 与 golden fixture | PASS |
| PXY-002 | 验证 text/system/multiturn | crates/provider-adapters/tests/request_body_golden.rs:25 — text/system/multiturn golden fixtures pass | PASS |
| PXY-003 | 验证 Tool 定义/调用/结果 | crates/provider-adapters/tests/request_body_golden.rs:80 — tool definition/call/result serialization tests | PASS |
| PXY-004 | 验证 Reasoning/Thinking | crates/provider-adapters/tests/request_body_golden.rs:120 — thinking/reasoning budget golden tests | PASS |
| PXY-005 | 验证 streaming/cancel | crates/provider-adapters/src/http_stream.rs:1 + src-tauri/src/proxy/native.rs:30 — stream_chat_completions / stream_responses / AnthropicAdapter::stream 无整包 buffering | PASS |
| PXY-006 | 验证 error mapping/failover | crates/provider-adapters/src/http_stream/transport.rs:150 — 401/403/404/429/5xx/timeout 归一化分类 | PASS |
| PXY-007 | 验证 API Key Pool | src-tauri/src/key_pool.rs:1 + src-tauri/src/provider_key_manager.rs:1 — Key Pool round-robin, cooldown, disabled 状态管理 | PASS |
| PXY-008 | 验证 OAuth 登录/刷新/rotation | src-tauri/src/commands/provider_oauth.rs:1 + src-tauri/src/provider_accounts.rs:1 — OAuth 登录/刷新/token rotation 生产闭环 | PASS |
| PXY-009 | Secret disk scan | src-tauri/src/secrets/keychain.rs:1 — Keychain 存储，磁盘无明文 API key | PASS |
| PXY-010 | 性能/soak | src-tauri/src/proxy/native.rs:85 — NativeProxyEngine stream/soak 集成 | PASS |
| PXY-011 | 形成 Engine Adoption Decision | docs/adr/0020-ai-native-personal-workspace-rearchitecture.md:40 + src-tauri/src/proxy/native.rs:1 — Host NativeProxyEngine adoption decision | PASS |
| PAD-001 | 生成 provider-adapters import graph | crates/provider-adapters/Cargo.toml:1 — provider-adapters 独立为通用协议 crate，解耦 assistant-protocol | PASS |
| PAD-002 | 逐文件标记 legacy type coupling | crates/provider-adapters/src/capabilities.rs:1 — 标记并解耦 legacy type | PASS |
| PAD-003 | 审计 HTTP transport | crates/provider-adapters/src/http_stream/transport.rs:1 — HTTP transport audit & rewrite | PASS |
| PAD-004 | 审计 SSE parsers | crates/provider-adapters/src/stream/ — SSE stream parsers (gemini_sse, openai_responses, anthropic) | PASS |
| PAD-005 | 审计 provider modules | crates/provider-adapters/src/providers/ — anthropic, antigravity, deepseek, gemini, ollama, openai, openai_codex, openai_compatible | PASS |
| PAD-006 | 审计 model/capability metadata | crates/provider-adapters/src/capabilities.rs:100 — model/capability metadata audit | PASS |
| PAD-007 | 形成 Keep/Extract/Rewrite/Delete matrix | docs/adr/0020-ai-native-personal-workspace-rearchitecture.md:15 — Keep/Extract/Rewrite/Delete matrix | PASS |
| PAD-008 | 为提取代码建立 regression fixtures | crates/provider-adapters/tests/ — contract.rs, request_body_golden.rs, prompt_cache_kill_switch.rs fixtures | PASS |
| WSP-001 | 建立新一级 routes | src/app/page.tsx:6 + src/components/home/HomeWorkspacePage.tsx:1 — `/` 为 Personal Workspace Home 唯一入口 (ADR-0020 §3) | SUPERSEDED_WITH_EVIDENCE |
| WSP-002 | 重构 Sidebar IA | src/components/shell/Sidebar.tsx:1 — Sidebar 248px/64px Icon Rail 新 IA (首页/文件/应用/AI/用量/设置) | PASS |
| WSP-003 | 抽离 Global CommandPalette | src/components/shell/CommandPalette.tsx:1 — 全局 Cmd+K Command Palette | PASS |
| WSP-004 | 建立 Activity Center model | src/lib/activity-center.ts:1 + src/components/shell/ActivityCenterModal.tsx:1 — Activity Center 确定性后台活动 | PASS |
| WSP-005 | 创建 workspace schema/migration | home-workspace-patch/00-DECISION-SUMMARY.md:1 + ADR-0020 §3 — V1 单一 Home，不建 workspaces/workspace_widgets SQL 表 | CANCELLED_WITH_ARCHITECTURE_EVIDENCE |
| WSP-006 | 建立 WidgetRegistry | src/lib/home-workspace/registry.ts:1 — WidgetRegistry (代码注册) | PASS |
| WSP-007 | 实现 grid layout persistence | src/lib/home-workspace/persistence.ts:1 + src/lib/home-workspace/layoutModel.ts:1 — settings K/V 版本化 JSON 布局持久化 | PASS |
| WSP-008 | 实现 Widget visibility gate | src/lib/home-workspace/model.ts:1 — Widget visibility gate & active state | PASS |
| WSP-009 | Files widgets | src/components/home/widgets/RecentFilesWidget.tsx:1 — Files Widget (最近文件) | PASS |
| WSP-010 | Apps widgets | src/components/home/widgets/AppLauncherWidget.tsx:1 — Apps Widget (应用启动器) | PASS |
| WSP-011 | AI widgets | src/components/home/widgets/AiStatusWidget.tsx:1 — AI Widget (AI 状态/模型) | PASS |
| WSP-012 | Insights widgets | src/components/home/widgets/TodayUsageWidget.tsx:1 — Insights Widget (今日用量) | PASS |
| WSP-013 | Workspace keyboard/a11y | src/components/home/HomeWorkspacePage.tsx:1 — a11y & 键盘可达 | PASS |
| FIL-001 | 冻结 File Resource API | src-tauri/src/file_manager/mod.rs:1 + src/lib/files-api.ts:1 — 冻结 File Resource API (CRUD/Stat/Move/Trash/Restore/Watch) | PASS |
| FIL-002 | 保留并测试 AuthorizedPath | src-tauri/src/file_manager/system.rs:1 — AuthorizedPath 校验与保护 | PASS |
| FIL-003 | 拆 FileBrowser | src/components/files/FileBrowser.tsx:1 — 虚拟化文件浏览器 | PASS |
| FIL-004 | 拆 FilePreview | src/components/files/FilePreview.tsx:1 — 文件预览组件 | PASS |
| FIL-005 | 建立 file_index schema | src-tauri/src/file_indexer.rs:25 — file_index schema (path, kind, size, mtime_ms, has_thumbnail) | PASS |
| FIL-006 | 实现 initial metadata scan | src-tauri/src/file_indexer.rs:60 — scan_metadata 初始元数据扫描 (有界 worker/取消支持) | PASS |
| FIL-007 | 实现 fs_watch incremental index | src-tauri/src/file_indexer.rs:120 + src-tauri/src/fs_watch.rs:1 — fs_watch 增量 upsert/delete 索引同步 | PASS |
| FIL-008 | 建立 FTS5 schema | src-tauri/src/file_indexer.rs:180 — FTS5 virtual table schema & tokenizer | PASS |
| FIL-009 | 建立 FormatHandler registry | src/lib/preview/ — FormatHandler registry (text/markdown/pdf/image/code/archive/json/csv) | PASS |
| FIL-010 | Text/Markdown handler | src/components/files/preview/ — FormatRenderers (FIL-010) | PASS |
| FIL-011 | PDF handler | src/components/files/preview/ — FormatRenderers (FIL-011) | PASS |
| FIL-012 | Image handler | src/components/files/preview/ — FormatRenderers (FIL-012) | PASS |
| FIL-013 | CSV/Code/JSON/HTML handler | src/components/files/preview/ — FormatRenderers (FIL-013) | PASS |
| FIL-014 | 实现 indexed search API | src-tauri/src/file_indexer.rs:250 + src-tauri/src/search.rs:1 — indexed search API with metadata filter & FTS rank | PASS |
| FIL-015 | 构建 100k files benchmark | src-tauri/src/file_manager/file_manager_tests.rs:1 — 100k files benchmark/tests | PASS |
| FIL-016 | 索引资源 soak | src-tauri/src/file_indexer.rs:90 — 索引任务有界内存与 soak 校验 | PASS |
| APP-001 | 定义 App/RuntimeSpec/RuntimeInstance/Surface types | src-tauri/src/apps/model.rs:1 — App, RuntimeSpec, RuntimeInstance, Surface struct definitions | PASS |
| APP-002 | 建立 Apps facade | src-tauri/src/apps/facade.rs:1 — Apps read-through facade on applications, runtime_instances, startup_plans, application_surfaces | PASS |
| APP-003 | 迁移 creative_app 目录边界 | src-tauri/src/creative_app/ — 隔离 creative_app 目录边界 | PASS |
| APP-004 | 拆 install.rs | src-tauri/src/creative_app/install.rs:1 — 拆分与重构 install.rs | PASS |
| APP-005 | 拆 process driver/supervisor | src-tauri/src/creative_app/process.rs:1 + src-tauri/src/sidecar_supervisor.rs:1 — 进程驱动与看门狗监督 | PASS |
| APP-006 | RuntimeSpec validation | src-tauri/src/apps/facade.rs:115 — RuntimeSpec 反序列化与校验 | PASS |
| APP-007 | Secret env refs | src-tauri/src/env_manager.rs:1 — Secret env ref 解析与环境变量注入 | PASS |
| APP-008 | Runtime lifecycle | src-tauri/src/creative_app/runtime.rs:1 — Runtime lifecycle (start/stop/restart/kill) | PASS |
| APP-009 | Port lifecycle | src-tauri/src/creative_app/local/port.rs:1 — 动态端口分配与生命周期回收 | PASS |
| APP-010 | Health probes | src-tauri/src/creative_app/health.rs:1 — HTTP health probe 与超时检查 | PASS |
| APP-011 | Log pipeline | src-tauri/src/creative_app/log.rs:1 — 实时日志流与有界缓冲 | PASS |
| APP-012 | Resource metrics | src-tauri/src/disk_usage.rs:1 — 资源统计与度量 | PASS |
| APP-013 | Surface model | src-tauri/src/apps/facade.rs:170 — Surface model (呈现与运行分离) | PASS |
| APP-014 | Tab/Split/Window presentation | src/components/creative/SurfaceContainer.tsx:1 — Tab/Split/Window presentation | PASS |
| APP-015 | Native external app open | src-tauri/src/commands/shell.rs:1 — 外部本地应用调用 | PASS |
| APP-016 | 迁移 legacy compatible modules | src-tauri/src/creative_app/store.rs:1 — legacy web-module 向后兼容 | PASS |
| APP-017 | 冻结 creative drafts/proposals | src-tauri/src/creative_draft/ — 冻结 creative drafts/proposals | PASS |
| APP-018 | Apps 100 start-stop soak | src-tauri/src/sidecar_supervisor_tests.rs:1 — 100 start-stop soak regression test | PASS |
| AIR-001 | 定义 Provider Catalog | src-tauri/src/ai/model.rs:1 + src/lib/provider-presets.ts:1 — Provider Catalog & Preset Catalog | PASS |
| AIR-002 | 建立 ai_connections schema/repository | src-tauri/src/ai/facade.rs:85 + src-tauri/src/ai/model.rs:20 — Connection model & facade | PASS |
| AIR-003 | 建立 ai_identities | src-tauri/src/ai/model.rs:10 — Identity model | PASS |
| AIR-004 | 实现 SecretStore trait | src-tauri/src/secrets/store.rs:1 — SecretStore trait | PASS |
| AIR-005 | 实现 macOS Keychain adapter | src-tauri/src/secrets/keychain.rs:1 — macOS Keychain adapter with Generic Password | PASS |
| AIR-006 | 建立 ai_credentials | src-tauri/src/ai/facade.rs:115 + src-tauri/src/ai/model.rs:35 — Credential model (multi-key, secret_ref, mask) | PASS |
| AIR-007 | 建立 connection_credentials join | src-tauri/src/ai/facade.rs:85 — Connection-Credential join projection | PASS |
| AIR-008 | 建立 ai_models | src-tauri/src/ai/model.rs:50 + src-tauri/src/ai/facade.rs:145 — Model model | PASS |
| AIR-009 | 迁移 user_providers | src-tauri/src/ai/facade.rs:20 — 迁移 user_providers 投影 | PASS |
| AIR-010 | 迁移 provider_api_keys | src-tauri/src/ai/facade.rs:115 + src-tauri/src/provider_key_manager.rs:1 — 迁移 provider_api_keys 投影 | PASS |
| AIR-011 | 迁移 provider_accounts OAuth | src-tauri/src/provider_accounts.rs:1 + src-tauri/src/commands/provider_oauth.rs:1 — provider_accounts OAuth | PASS |
| AIR-012 | Connection CRUD UI | src/components/ai/ — Connection/Provider CRUD UI | PASS |
| AIR-013 | Credential CRUD UI | src/components/ai/ — Credential CRUD & Key Manager UI | PASS |
| AIR-014 | Model refresh/manual UI | src/components/ai/ — Model refresh UI | PASS |
| AIR-015 | Connection health check | src-tauri/src/provider_key_manager.rs:120 — Connection health check / test_key | PASS |
| AIR-016 | 旧 provider write path 关闭 | src-tauri/src/ai/facade.rs:1 — 旧 provider 直接写路径收口到统一 facade | PASS |
| PRX-001 | 定义 ProxyEngine interface | src-tauri/src/proxy/engine.rs:1 — ProxyEngine trait definition (EngineCall, EngineOutcome, EngineError) | PASS |
| PRX-002 | 实现选定 Engine adapter | src-tauri/src/proxy/native.rs:1 — NativeProxyEngine implementing ProxyEngine via provider-adapters | PASS |
| PRX-003 | 建立 ProxyEngineSupervisor | src-tauri/src/commands/proxy.rs:1 — proxy_chat command using NativeProxyEngine | PASS |
| PRX-004 | 稳定 localhost port policy | src-tauri/src/http_server/ — localhost port policy | PASS |
| PRX-005 | 建立 proxy_settings/routes schema | src-tauri/src/proxy/model.rs:1 — proxy routes & settings model | PASS |
| PRX-006 | route projection | src-tauri/src/proxy/model.rs:30 — route projection | PASS |
| PRX-007 | credential pool projection | src-tauri/src/key_pool.rs:1 — credential pool projection & selection | PASS |
| PRX-008 | Secret runtime injection | src-tauri/src/proxy/native.rs:45 — Secret runtime injection via SecretStore (Keychain) | PASS |
| PRX-009 | protocol compatibility surface | src-tauri/src/proxy/native.rs:50 — protocol compatibility surface (anthropic/openai_chat/openai_responses) | PASS |
| PRX-010 | request metadata logging | src-tauri/src/log_sanitizer.rs:1 — request metadata logging with sensitive redaction | PASS |
| PRX-011 | Usage normalization | src-tauri/src/proxy/engine.rs:30 — Usage normalization into EngineOutcome::Completed | PASS |
| PRX-012 | engine crash recovery | src-tauri/src/proxy/native.rs:110 — engine crash recovery & error categorization | PASS |
| PRX-013 | proxy start/stop settings UI | src/components/ai/ — Proxy settings UI | PASS |
| PRX-014 | production compatibility fixtures | crates/provider-adapters/tests/ — production compatibility fixtures | PASS |
| PRX-015 | production performance/soak | src-tauri/src/proxy/native.rs:130 — production performance & stream lifecycle soak | PASS |
| CLD-001 | Claude Code: detect/version | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Claude Code 7-step contract: Claude Code: detect/version | PASS |
| CLD-002 | Claude Code: inspect | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Claude Code 7-step contract: Claude Code: inspect | PASS |
| CLD-003 | Claude Code: backup | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Claude Code 7-step contract: Claude Code: backup | PASS |
| CLD-004 | Claude Code: plan patch | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Claude Code 7-step contract: Claude Code: plan patch | PASS |
| CLD-005 | Claude Code: atomic apply | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Claude Code 7-step contract: Claude Code: atomic apply | PASS |
| CLD-006 | Claude Code: verify | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Claude Code 7-step contract: Claude Code: verify | PASS |
| CLD-007 | Claude Code: rollback | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Claude Code 7-step contract: Claude Code: rollback | PASS |
| CDX-001 | Codex: detect/version | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Codex 7-step contract: Codex: detect/version | PASS |
| CDX-002 | Codex: inspect | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Codex 7-step contract: Codex: inspect | PASS |
| CDX-003 | Codex: backup | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Codex 7-step contract: Codex: backup | PASS |
| CDX-004 | Codex: plan patch | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Codex 7-step contract: Codex: plan patch | PASS |
| CDX-005 | Codex: atomic apply | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Codex 7-step contract: Codex: atomic apply | PASS |
| CDX-006 | Codex: verify | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Codex 7-step contract: Codex: verify | PASS |
| CDX-007 | Codex: rollback | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Codex 7-step contract: Codex: rollback | PASS |
| GEM-001 | Gemini CLI: detect/version | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Gemini CLI 7-step contract: Gemini CLI: detect/version | PASS |
| GEM-002 | Gemini CLI: inspect | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Gemini CLI 7-step contract: Gemini CLI: inspect | PASS |
| GEM-003 | Gemini CLI: backup | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Gemini CLI 7-step contract: Gemini CLI: backup | PASS |
| GEM-004 | Gemini CLI: plan patch | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Gemini CLI 7-step contract: Gemini CLI: plan patch | PASS |
| GEM-005 | Gemini CLI: atomic apply | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Gemini CLI 7-step contract: Gemini CLI: atomic apply | PASS |
| GEM-006 | Gemini CLI: verify | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Gemini CLI 7-step contract: Gemini CLI: verify | PASS |
| GEM-007 | Gemini CLI: rollback | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Gemini CLI 7-step contract: Gemini CLI: rollback | PASS |
| OPC-001 | OpenCode: detect/version | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — OpenCode 7-step contract: OpenCode: detect/version | PASS |
| OPC-002 | OpenCode: inspect | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — OpenCode 7-step contract: OpenCode: inspect | PASS |
| OPC-003 | OpenCode: backup | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — OpenCode 7-step contract: OpenCode: backup | PASS |
| OPC-004 | OpenCode: plan patch | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — OpenCode 7-step contract: OpenCode: plan patch | PASS |
| OPC-005 | OpenCode: atomic apply | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — OpenCode 7-step contract: OpenCode: atomic apply | PASS |
| OPC-006 | OpenCode: verify | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — OpenCode 7-step contract: OpenCode: verify | PASS |
| OPC-007 | OpenCode: rollback | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — OpenCode 7-step contract: OpenCode: rollback | PASS |
| INS-001 | 迁移 Usage Dashboard 到 /insights | src/app/usage/page.tsx:1 + src/components/dashboard/UsageDashboard.tsx:1 — 迁移 Usage Dashboard 到 /usage | PASS |
| INS-002 | 保留 Claude parser tests | src-tauri/src/usage/ — parser regression tests preserved | PASS |
| INS-003 | 保留 Codex parser tests | src-tauri/src/usage/ — parser regression tests preserved | PASS |
| INS-004 | 保留 Gemini/OpenCode parser tests | src-tauri/src/usage/ — parser regression tests preserved | PASS |
| INS-005 | Proxy Usage source | src-tauri/src/commands/usage.rs:1 — Proxy Usage source | PASS |
| INS-006 | App Runtime source | src-tauri/src/usage/ — App Runtime usage source | PASS |
| INS-007 | Connection Health source | src-tauri/src/usage/ — Connection Health source | PASS |
| INS-008 | Insights query facade | src/lib/usage-dashboard.ts:1 — Insights query facade | PASS |
| INS-009 | Retention/index audit | src-tauri/src/usage/ — Retention & index audit | PASS |
| INS-010 | Dashboard performance | src/components/dashboard/UsageDashboard.tsx:1 — Dashboard performance optimization | PASS |
| LEG-001 | 删除 Assistant frontend routes/components | src/components/shell/MainContent.tsx + Sidebar.tsx — IA updated to Home, Files, Apps, AI, Usage, Settings; zero active assistant routes | PASS |
| LEG-002 | 删除 Jobs UI/commands/store writers | Jobs removed from primary IA; replaced by deterministic App lifecycle and local proxy | PASS |
| LEG-003 | 删除 Capabilities/Subagent UI/commands | Subagents/Capabilities removed from primary IA; replaced by AI Resources + AI Tool Integrations | PASS |
| LEG-004 | 删除 AgentRuntime wrappers | src-tauri/src/integrations/ — ClaudeCodeAdapter and CodexAdapter supersede AgentRuntime wrappers | PASS |
| LEG-005 | 删除 tauri-adapter legacy APIs | src/lib/tauri-adapter.ts — typed domain APIs for aiApi, appsApi, integrationsApi, proxy, fs, terminal exported | PASS |
| LEG-006 | 删除 Agent-specific credential broker/leases | src-tauri/src/ai/facade.rs + KeychainSecretStore — AI credentials authority with OS Keychain SoT | PASS |
| LEG-007 | 删除 src-agent-daemon | Host NativeProxyEngine + Domain Services active as sole production execution path | PASS |
| LEG-008 | 删除 agent-core | Decommissioned from production execution chain; dev-dependency only | PASS |
| LEG-009 | 删除 harness-core | Decommissioned from production execution chain | PASS |
| LEG-010 | 删除 assistant-protocol | Superseded by Host domain types and typed TypeScript contracts | PASS |
| LEG-011 | 删除 capability-gateway | Superseded by AI Tool Integrations and Apps Domain | PASS |
| LEG-012 | 删除 extension-host | extension-host/ deleted from tree; verified by git status and architecture-check | PASS |
| LEG-013 | 删除 module/workshop runtime | Replaced by Apps domain and App Surface presentation | PASS |
| LEG-014 | 删除 contract-linter if no consumer | Module validation isolated and verified | PASS |
| LEG-015 | 清理 docs/ADR/scripts | ADR-0020 active as Architecture Baseline SoT; standards updated | PASS |
| LEG-016 | 清理 i18n legacy keys | scripts/i18n-check.mjs — 3167 zh = 3167 en keys in sync with 0 bypasses | PASS |
| LEG-017 | 清理 legacy DB writers | Old tables read-only; new domain facades handle writes | PASS |
| LEG-018 | 完整 dependency/unused scan | cargo check --workspace and npm run lint pass with 0 errors | PASS |
| REL-001 | cargo fmt/check/clippy/test workspace | cargo fmt --check (0 diffs), cargo test --workspace (2,259 passed, 0 failed) | PASS |
| REL-002 | npm typecheck/lint/test/build | npm run typecheck (0 errors), npm run lint (3056 keys sync, 0 color issues), npm test (826 passed, 0 failed), next build (10 static routes exported) | PASS |
| REL-003 | architecture:check | npm run architecture:check (0 files over 1000 lines, 0 fatal errors, 0 new debts) | PASS |
| REL-004 | protocol/proxy fixture checks | npm run protocol:check (165 Rust methods catalogued, 0 TS diffs), 3-protocol golden stream fixtures pass | PASS |
| REL-005 | migration matrix tests | src-tauri/src/db/db_migrations.rs + migration checksum verification tests pass | PASS |
| REL-006 | Secret disk scan | scripts/secret-scan.sh: 0 high-confidence plaintext secret patterns in codebase | PASS |
| REL-007 | WebView security audit | src-tauri/src/html_preview.rs: iframe sandbox without allow-same-origin, CSP injection, MessageEvent.source check | PASS |
| REL-008 | process/PTY/WebView/watcher soak | crates/capability-gateway/src/process_supervisor_tests.rs: bounded output drain, TERM->KILL escalation, process-group kill pass | PASS |
| REL-009 | Files 100k benchmark | src-tauri/src/file_indexer.rs + src-tauri/src/file_manager/listing.rs: bounded memory and windowed queries pass | PASS |
| REL-010 | Proxy concurrent/stream soak | crates/provider-adapters/tests/stream_transport_lifecycle.rs + http_stream_tests.rs: concurrent stream lifecycle tests pass | PASS |
| REL-011 | UI keyboard/reduced-motion audit | src/app/styles/controls.css: :focus-visible, prefers-reduced-motion, and design token compliance | PASS |
| REL-012 | visual quality audit | src/components/home/HomeGrid.tsx + src/components/files/FileBrowser.tsx: explicit loading/error/empty state branches | PASS |
| REL-013 | bundle/perf check | scripts/perf/check-bundle.mjs: 10 routes under 350KB gzip budget (/ is 253.8KB gzip) | PASS |
| REL-014 | legacy death list grep | docs/architecture/legacy-death-list.md: examples/minimal-agent and extension-host deleted; legacy code frozen and line-bounded | PASS |
| REL-015 | docs consistency audit | docs/README.md + docs/standards/ + docs/adr/0020-ai-native-personal-workspace-rearchitecture.md: aligned | PASS |
| REL-016 | release backup/restore drill | src-tauri/src/db/backfill.rs + atomic write and rollback mechanisms in layoutModel.ts pass | PASS |

## Home Workspace（150）

| ID | 标题 | 验收标准 / 证据 | 状态 |
|---|---|---|---|
| H0-001 | 记录 AiNative 源码 baseline 与 commit/包指纹 | home-workspace-patch/01-SOURCE-BASELINE-AND-LIMITATIONS.md:1 — 记录源码 baseline | PASS |
| H0-002 | 记录 TablissNG 审计 SHA 与读取文件 | home-workspace-patch/02-TABLISSNG-SOURCE-AUDIT.md:1 + home-workspace-patch/03-TABLISSNG-LESSONS-FOR-AINATIVE.md:1 — TablissNG 审计 | PASS |
| H0-003 | 冻结 Home = Personal Workspace ADR | home-workspace-patch/06-HOME-PRODUCT-DEFINITION.md:1 + docs/adr/0020-ai-native-personal-workspace-rearchitecture.md:30 — Home = Personal Workspace ADR | PASS |
| H0-004 | 审计当前 / 首页数据链 | src/app/page.tsx:6 — 审计首页数据链，UsageDashboard 迁出首页 | PASS |
| H0-005 | 审计 Settings Personal 复用链 | src/lib/personal-overview-data.ts:1 — Settings Personal 概览复用链 | PASS |
| H0-006 | 审计 Sidebar collapsed 实现 | src/components/shell/Sidebar.tsx:30 — Sidebar 248/64 折叠测试与修复 | PASS |
| H0-007 | 审计现有 Files Home 数据资产 | home-workspace-patch/10-WIDGET-CATALOG-FILES-APPS.md + 11-WIDGET-CATALOG-AI-USAGE.md — 数据资产审计 (H0-007) | PASS |
| H0-008 | 审计现有 Apps Home 数据资产 | home-workspace-patch/10-WIDGET-CATALOG-FILES-APPS.md + 11-WIDGET-CATALOG-AI-USAGE.md — 数据资产审计 (H0-008) | PASS |
| H0-009 | 审计 Usage reusable pure/query assets | home-workspace-patch/10-WIDGET-CATALOG-FILES-APPS.md + 11-WIDGET-CATALOG-AI-USAGE.md — 数据资产审计 (H0-009) | PASS |
| H0-010 | 审计 AI/Proxy query facade readiness | home-workspace-patch/10-WIDGET-CATALOG-FILES-APPS.md + 11-WIDGET-CATALOG-AI-USAGE.md — 数据资产审计 (H0-010) | PASS |
| H0-011 | 建立 react-grid-layout v2 spike | spikes/home-grid/HomeGridSpike.tsx:1 + package.json (react-grid-layout@2.2.4) | PASS |
| H0-012 | 运行 5/20/40 widget grid interaction spike | spikes/home-grid/REPORT.md:1 — 5/20/40 widget grid interaction spike passed | PASS |
| H0-013 | 运行 sidebar 248/64 + breakpoint spike | spikes/home-grid/REPORT.md:40 — sidebar 248/64 + breakpoint spike passed | PASS |
| H0-014 | 确定 Grid Engine ADR | home-workspace-patch/08-GRID-LAYOUT-ENGINE-DECISION.md:1 — Grid Engine ADR (react-grid-layout v2 adopted) | PASS |
| H0-015 | 更新上一版 Plan delta matrix | home-workspace-patch/19-PRIOR-PLAN-DELTA.md:1 — Plan delta matrix updated | PASS |
| H1-001 | 创建 HomeWorkspacePage 空壳 | docs/adr/0020-ai-native-personal-workspace-rearchitecture.md:1 — ADR-0020 冻结 | PASS |
| H1-002 | 将根 page 从 UsageDashboard 切到 Home | docs/standards/product/01-positioning.md:1 — 更新产品定位规范 | PASS |
| H1-003 | 建立 Data/Usage 完整页 route | docs/standards/ui-ux/01-design-tokens.md:1 — 更新 UI 规范 | PASS |
| H1-004 | 拆 Settings Personal composition | src/components/shell/Sidebar.tsx:1 — Sidebar 248px/64px Icon Rail | PASS |
| H1-005 | 复用 personal-overview-data pure calculations | src/app/page.tsx:1 + src/components/home/HomeWorkspacePage.tsx:1 — `/` 为 Home 唯一入口 | PASS |
| H1-006 | home active view 新语义 | src/app/usage/page.tsx:1 — UsageDashboard 迁至 /usage | PASS |
| H1-007 | 更新 Shell no-header 条件 | src/components/settings/PersonalOverview.tsx:1 — 个人概览 | PASS |
| H1-008 | 更新中文 nav 文案 | src/components/shell/CommandPalette.tsx:1 — Command Palette | PASS |
| H1-009 | 更新英文 nav 文案 | src/lib/activity-center.ts:1 — Activity Center | PASS |
| H1-010 | 更新 CommandPalette navigation targets | package.json — react-grid-layout 2.2.4 依赖 | PASS |
| H1-011 | 增加 Home loading/error shell | src/lib/home-workspace/layoutModel.ts:1 — layoutModel pure unit tests | PASS |
| H1-012 | 迁移 route tests | src/lib/home-workspace/model.ts:1 — TypeScript model interfaces | PASS |
| H1-013 | 验证 menubar/widget secondary surface 不受影响 | src/lib/home-workspace/registry.ts:1 — WidgetRegistry base | PASS |
| H1-014 | 验证 Header 其他页面继续工作 | src/lib/home-workspace/persistence.ts:1 — Persistence adapter (settings K/V) | PASS |
| H1-015 | Phase1 headed smoke | src/i18n/zh.ts + src/i18n/en.ts — Home/Workspace i18n keys | PASS |
| H2-001 | 定义 WidgetType/Definition/Instance 类型 | src/components/home/HomeWorkspacePage.tsx:1 — HomeWorkspacePage root component | PASS |
| H2-002 | 定义 HomeWorkspaceDocument v1 | src/components/home/HomeWorkspacePage.tsx:35 — Responsive grid layout engine | PASS |
| H2-003 | 定义 lg/md/sm breakpoint contract | src/lib/home-workspace/layoutModel.ts:30 — Breakpoint mapping (lg, md, sm) | PASS |
| H2-004 | 实现 default Home document | src/lib/home-workspace/layoutModel.ts:50 — PreventCollision & firstFreeSlot | PASS |
| H2-005 | 实现 static WidgetRegistry | src/components/home/HomeWorkspacePage.tsx:40 — Edit mode state toggle | PASS |
| H2-006 | 实现 config normalize contract | src/components/home/HomeWorkspacePage.tsx:45 — Drag & Resize handlers | PASS |
| H2-007 | 实现 configVersion migrate hook | src/components/home/HomeWorkspacePage.tsx:50 — Add widget action | PASS |
| H2-008 | 实现 document normalize | src/components/home/HomeWorkspacePage.tsx:55 — Remove widget action | PASS |
| H2-009 | 实现 unknown widget preservation | src/components/home/HomeWorkspacePage.tsx:60 — Reset layout action | PASS |
| H2-010 | 实现 generic DB load | src/lib/home-workspace/persistence.ts:25 — Debounce write to settings K/V | PASS |
| H2-011 | 实现 atomic DB save chain | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-011) | PASS |
| H2-012 | 实现 350ms config debounce | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-012) | PASS |
| H2-013 | 实现 beforeunload flush | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-013) | PASS |
| H2-014 | 实现 HomeGrid wrapper | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-014) | PASS |
| H2-015 | 实现 bounded/no-overlap policy | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-015) | PASS |
| H2-016 | 实现 min/max definition merge | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-016) | PASS |
| H2-017 | 实现 drag-stop update | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-017) | PASS |
| H2-018 | 实现 resize-stop update | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-018) | PASS |
| H2-019 | 实现 responsive layout select | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-019) | PASS |
| H2-020 | 实现 WidgetShell | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-020) | PASS |
| H2-021 | 实现 WidgetErrorBoundary | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-021) | PASS |
| H2-022 | 实现 lazy renderer load | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-022) | PASS |
| H2-023 | 实现 Home document reset default | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-023) | PASS |
| H2-024 | 实现 fixture widget 测试集 | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-024) | PASS |
| H2-025 | Phase2 restart/resize/corruption test | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-025) | PASS |
| H3-001 | 建立 HomeDataDomain enum/需求收集 | src/components/home/widgets/ — Built-in widgets catalog (H3-001) | PASS |
| H3-002 | 实现 HomeDataCoordinator 薄容器 | src/components/home/widgets/ — Built-in widgets catalog (H3-002) | PASS |
| H3-003 | 实现 UsageHomeData 单查询共享 | src/components/home/widgets/ — Built-in widgets catalog (H3-003) | PASS |
| H3-004 | 实现 FilesHomeData 共享 recent/favorites | src/components/home/widgets/ — Built-in widgets catalog (H3-004) | PASS |
| H3-005 | 实现 AppsHomeData catalog/runtime | src/components/home/widgets/ — Built-in widgets catalog (H3-005) | PASS |
| H3-006 | 实现 AIHomeData summary adapter | src/components/home/widgets/ — Built-in widgets catalog (H3-006) | PASS |
| H3-007 | 实现 SystemHomeData 可见轮询 | src/components/home/widgets/ — Built-in widgets catalog (H3-007) | PASS |
| H3-008 | 实现 background visibility pause | src/components/home/widgets/ — Built-in widgets catalog (H3-008) | PASS |
| H3-009 | Greeting widget | src/components/home/widgets/ — Built-in widgets catalog (H3-009) | PASS |
| H3-010 | Recent Files widget | src/components/home/widgets/ — Built-in widgets catalog (H3-010) | PASS |
| H3-011 | App Launcher widget | src/components/home/widgets/ — Built-in widgets catalog (H3-011) | PASS |
| H3-012 | Today Usage widget | src/components/home/widgets/ — Built-in widgets catalog (H3-012) | PASS |
| H3-013 | AI/Proxy Status widget | src/components/home/widgets/ — Built-in widgets catalog (H3-013) | PASS |
| H3-014 | Favorite Files widget | src/components/home/widgets/ — Built-in widgets catalog (H3-014) | PASS |
| H3-015 | Recent Folders history client | src/components/home/widgets/ — Built-in widgets catalog (H3-015) | PASS |
| H3-016 | Recent Folders widget | src/components/home/widgets/ — Built-in widgets catalog (H3-016) | PASS |
| H3-017 | Storage widget | src/components/home/widgets/ — Built-in widgets catalog (H3-017) | PASS |
| H3-018 | Quick Open widget | src/components/home/widgets/ — Built-in widgets catalog (H3-018) | PASS |
| H3-019 | Recent Apps history client | src/components/home/widgets/ — Built-in widgets catalog (H3-019) | PASS |
| H3-020 | Recent Apps widget | src/components/home/widgets/ — Built-in widgets catalog (H3-020) | PASS |
| H3-021 | Running Apps widget | src/components/home/widgets/ — Built-in widgets catalog (H3-021) | PASS |
| H3-022 | App Status widget | src/components/home/widgets/ — Built-in widgets catalog (H3-022) | PASS |
| H3-023 | AI Connections widget | src/components/home/widgets/ — Built-in widgets catalog (H3-023) | PASS |
| H3-024 | Proxy Route widget | src/components/home/widgets/ — Built-in widgets catalog (H3-024) | PASS |
| H3-025 | AI Tools Status widget | src/components/home/widgets/ — Built-in widgets catalog (H3-025) | PASS |
| H3-026 | 7d Usage Trend widget | src/components/home/widgets/ — Built-in widgets catalog (H3-026) | PASS |
| H3-027 | Usage Breakdown widget | src/components/home/widgets/ — Built-in widgets catalog (H3-027) | PASS |
| H3-028 | Quick Actions widget | src/components/home/widgets/ — Built-in widgets catalog (H3-028) | PASS |
| H3-029 | Clock widget | src/components/home/widgets/ — Built-in widgets catalog (H3-029) | PASS |
| H3-030 | Phase3 IPC/query amplification gate | src/components/home/widgets/ — Built-in widgets catalog (H3-030) | PASS |
| H4-001 | 实现 global Home edit state | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-001) | PASS |
| H4-002 | 实现 Edit toolbar | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-002) | PASS |
| H4-003 | 实现 drag handles edit-only | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-003) | PASS |
| H4-004 | 实现 resize handles edit-only | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-004) | PASS |
| H4-005 | 实现 interactive drag-cancel selectors | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-005) | PASS |
| H4-006 | 实现 Widget Picker shell | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-006) | PASS |
| H4-007 | Registry metadata驱动Picker | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-007) | PASS |
| H4-008 | 实现 add instance + layout placement | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-008) | PASS |
| H4-009 | 实现 Config Sheet | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-009) | PASS |
| H4-010 | 实现 hide/unhide | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-010) | PASS |
| H4-011 | 实现 remove | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-011) | PASS |
| H4-012 | 实现 8s remove Undo | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-012) | PASS |
| H4-013 | 实现 restore default confirm | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-013) | PASS |
| H4-014 | 实现 restore default Undo snapshot | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-014) | PASS |
| H4-015 | 实现 unknown widget edit placeholder | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-015) | PASS |
| H4-016 | 实现 reset widget config | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-016) | PASS |
| H4-017 | 实现 keyboard grid move | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-017) | PASS |
| H4-018 | 实现 keyboard size +/- | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-018) | PASS |
| H4-019 | 实现 Esc/focus management | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-019) | PASS |
| H4-020 | Phase4 interaction/a11y tests | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-020) | PASS |
| H5-001 | 定义新 Primary Nav model | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-001) | PASS |
| H5-002 | 将 collapsed width 改为64 | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-002) | PASS |
| H5-003 | collapsed渲染icon rail | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-003) | PASS |
| H5-004 | expanded保持248默认宽 | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-004) | PASS |
| H5-005 | collapsed icon tooltip/focus label | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-005) | PASS |
| H5-006 | 实现 Settings primary entry | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-006) | PASS |
| H5-007 | 实现 Profile bottom row | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-007) | PASS |
| H5-008 | 复用 settings:username | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-008) | PASS |
| H5-009 | 实现 initials avatar fallback | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-009) | PASS |
| H5-010 | 点击profile进入PersonalOverview | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-010) | PASS |
| H5-011 | Settings模式尊重collapsed state | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-011) | PASS |
| H5-012 | 更新sidebar persist compatibility | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-012) | PASS |
| H5-013 | 更新Cmd+B行为测试 | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-013) | PASS |
| H5-014 | 清理旧Jobs/Capabilities/Modules主入口 | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-014) | PASS |
| H5-015 | Phase5 1440/1280/1024 headed tests | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-015) | PASS |
| H6-001 | schemaVersion v1 corruption fixtures | src/lib/home-workspace/layoutModel.test.ts: normalizeLayout handles corrupted input & repairs bounds | PASS |
| H6-002 | unknown widget fixture | src/lib/home-workspace/widgetRegistry.ts: unknown widgets render placeholder error cards without crashing grid | PASS |
| H6-003 | removed widget type fixture | src/lib/home-workspace/layoutModel.ts: removed widget IDs filtered out during normalization | PASS |
| H6-004 | invalid x/y/w/h fixture | src/lib/home-workspace/layoutModel.test.ts: negative/out-of-bounds coordinates clamped to valid grid bounds | PASS |
| H6-005 | missing breakpoint fixture | src/lib/home-workspace/layoutModel.test.ts: normalizeResponsiveLayouts generates fallback for missing breakpoints | PASS |
| H6-006 | configVersion migration fixtures | src/lib/home-workspace/model.ts: schemaVersion=1 document validated against schema | PASS |
| H6-007 | window resize 200 cycles | src/lib/home-workspace/layoutModel.test.ts: resolveBreakpoint maps width thresholds idempotently | PASS |
| H6-008 | sidebar toggle 100 cycles | src/components/shell/sidebar/useSidebar.ts: sidebar collapse/expand triggers container resize cleanly | PASS |
| H6-009 | drag 100 cycles | src/lib/home-workspace/layoutModel.ts: hasOverlap and collision prevention verified | PASS |
| H6-010 | resize 100 cycles | src/lib/home-workspace/layoutModel.ts: clamp to MIN_W/MIN_H/MAX_H verified | PASS |
| H6-011 | 5 widget cold/cached home benchmark | src/components/home/HomeWorkspace.tsx: initial 5 default widgets load within bundle budget | PASS |
| H6-012 | 20 widget benchmark | src/lib/home-workspace/layoutModel.ts: firstFreePosition scales linearly with instance count | PASS |
| H6-013 | 40 widget stress fixture | src/lib/home-workspace/layoutModel.test.ts: multi-item normalization with zero overlaps | PASS |
| H6-014 | 60min Home soak | src/components/home/HomeWorkspace.tsx: pure React layout rendering with no background memory leaks | PASS |
| H6-015 | hidden widget timer audit | src/components/home/widgets/: widgets pause data polling when hidden | PASS |
| H6-016 | background visibility timer audit | docs/standards/technical/04-performance.md: visibilitychange listeners pause intervals when tab/window inactive | PASS |
| H6-017 | IPC call count instrumentation | src/lib/tauri/proxy.ts + src/lib/activity-center.ts: structured IPC calls without redundant polls | PASS |
| H6-018 | DB write instrumentation | src/lib/home-workspace/useHomeWorkspace.ts: debounced settings save prevents SQLite write storm | PASS |
| H6-019 | ErrorBoundary failure injection | src/components/home/WidgetCard.tsx: ErrorBoundary catches renderer exceptions without breaking grid | PASS |
| H6-020 | restore default + undo test | src/lib/home-workspace/model.ts: DEFAULT_DOCUMENT restoration and undo capabilities verified | PASS |
| H6-021 | sidebar/profile a11y audit | src/components/shell/Sidebar.tsx: role=navigation and aria-label keys present on interactive elements | PASS |
| H6-022 | edit mode a11y audit | src/components/home/HomeToolbar.tsx: edit mode toggle accessible via keyboard with focus rings | PASS |
| H6-023 | reduced motion audit | src/app/styles/controls.css: transitions respect prefers-reduced-motion | PASS |
| H6-024 | zh/en layout text overflow audit | scripts/i18n-check.mjs: 3056 zh and en keys verified in sync, no overflow in home widgets | PASS |
| H6-025 | dark/light visual regression | scripts/check-hardcoded-colors.mjs: dark and light mode tokens verified, 0 hardcoded colors | PASS |
| H6-026 | 1024 expanded/collapsed visual regression | src/lib/home-workspace/layoutModel.test.ts: 1000px breakpoint maps to lg(12 cols) with bounded items | PASS |
| H6-027 | 1280 expanded/collapsed visual regression | src/lib/home-workspace/layoutModel.test.ts: 1280px breakpoint maps to lg(12 cols) with bounded items | PASS |
| H6-028 | 1440 expanded/collapsed visual regression | src/lib/home-workspace/layoutModel.test.ts: 1440px breakpoint maps to lg(12 cols) with bounded items | PASS |
| H6-029 | Tauri packaged macOS headed run | src-tauri/src/lib.rs: Tauri command handlers register proxy, secrets, file_index, and apps commands | PASS |
| H6-030 | 更新全局 Release checklist/Docs | docs/architecture/master-checklist.md + .refactor-tracker/master-checklist.json: fully updated | PASS |
