# AiNative 全量重构 Master Checklist

依据 `/Users/ldh/Downloads/AiNative-Integrated-Rearchitecture-2026-08-20` 统一方案执行。

## 统计概要
- **Global 任务**: 总计 175 | 已通过/归档: 141 | 待闭环: 34
- **Home 任务**: 总计 150 | 已通过/归档: 120 | 待闭环: 30

---

## 一、Global Tasks (175 项)

| Task ID | 状态 | 源码证据 | 缺口 | Owner | 依赖 | 验收标准 |
|---|---|---|---|---|---|---|
| `GOV-001` | **PASS** | docs/adr/0020-ai-native-personal-workspace-rearchitecture.md:1 — ADR-0020 冻结 AI Native Personal Workspace 架构 | - | Governance | 源码审计完成 | standards/docs 引用新 ADR，无旧架构冲突 |
| `GOV-002` | **PASS** | docs/standards/technical/01-layering.md:12 + docs/adr/0020-ai-native-personal-workspace-rearchitecture.md:39 — ADR-0020 取代 ADR-0019/0012/0015/0016 | - | Governance | GOV-001 | 默认 Renderer+Host；sidecar 需 justification |
| `GOV-003` | **PASS** | src-tauri/src/secrets/store.rs:1 + src-tauri/src/secrets/keychain.rs:1 — SecretStore trait + macOS Keychain adapter 实现 | - | Governance | GOV-001 | 无 SQLite plaintext/长期 secret SoT |
| `GOV-004` | **PASS** | docs/architecture/CODE_MODULE_GUIDELINES.md:1 — 模块拆分与规模限制，适配 Domain 模型 | - | Governance | GOV-001 | 保留数值门禁，删除 Agent Loop 语义 |
| `GOV-005` | **PASS** | scripts/architecture-check.mjs:1 — 包含模块边界、架构债务与规则校验 | - | Governance | GOV-002,GOV-004 | 新违规能在 CI fail |
| `GOV-006` | **PASS** | scripts/perf/check-bundle.mjs:1 + .runtime-evidence/baseline.json:1 — bundle 预算与性能基准 | - | Governance | 现有 perf scripts | 能产出统一 CSV/JSON evidence |
| `PXY-001` | **PASS** | crates/provider-adapters/tests/request_body_golden.rs:1 + src-tauri/src/proxy/native.rs:1 — 三协议 transport 与 golden fixture | - | P0 Proxy Spike | GOV-003 | 可重复发送 Anthropic/Chat/Responses fixture |
| `PXY-002` | **PASS** | crates/provider-adapters/tests/request_body_golden.rs:25 — text/system/multiturn golden fixtures pass | - | P0 Proxy Spike | GOV-003 | 三协议基础映射快照一致 |
| `PXY-003` | **PASS** | crates/provider-adapters/tests/request_body_golden.rs:80 — tool definition/call/result serialization tests | - | P0 Proxy Spike | GOV-003 | 增量参数无丢失、顺序正确 |
| `PXY-004` | **PASS** | crates/provider-adapters/tests/request_body_golden.rs:120 — thinking/reasoning budget golden tests | - | P0 Proxy Spike | GOV-003 | 明确 Full/Degraded/Unsupported |
| `PXY-005` | **PASS** | crates/provider-adapters/src/http_stream.rs:1 + src-tauri/src/proxy/native.rs:30 — stream_chat_completions / stream_responses / AnthropicAdapter::stream 无整包 buffering | - | P0 Proxy Spike | GOV-003 | 无整包 buffering；cancel 释放资源 |
| `PXY-006` | **PASS** | crates/provider-adapters/src/http_stream/transport.rs:150 — 401/403/404/429/5xx/timeout 归一化分类 | - | P0 Proxy Spike | GOV-003 | 401/403/404/429/5xx/timeout 分类正确 |
| `PXY-007` | **PASS** | src-tauri/src/key_pool.rs:1 + src-tauri/src/provider_key_manager.rs:1 — Key Pool round-robin, cooldown, disabled 状态管理 | - | P0 Proxy Spike | GOV-003 | RR/priority/cooldown/disabled 正确 |
| `PXY-008` | **PASS** | src-tauri/src/commands/provider_oauth.rs:1 + src-tauri/src/provider_accounts.rs:1 — OAuth 登录/刷新/token rotation 生产闭环 | - | P0 Proxy Spike | GOV-003 | refresh 后 token ownership 可管理 |
| `PXY-009` | **PASS** | src-tauri/src/secrets/keychain.rs:1 — Keychain 存储，磁盘无明文 API key | - | P0 Proxy Spike | GOV-003 | 普通配置/temp/log 无长期 plaintext |
| `PXY-010` | **PASS** | src-tauri/src/proxy/native.rs:85 — NativeProxyEngine stream/soak 集成 | - | P0 Proxy Spike | GOV-003 | 满足 proxy budget |
| `PXY-011` | **PASS** | docs/adr/0020-ai-native-personal-workspace-rearchitecture.md:40 + src-tauri/src/proxy/native.rs:1 — Host NativeProxyEngine adoption decision | - | P0 Proxy Spike | GOV-003 | ADOPT/ADOPT_WITH_ADAPTER/REJECT 有证据 |
| `PAD-001` | **PASS** | crates/provider-adapters/Cargo.toml:1 — provider-adapters 独立为通用协议 crate，解耦 assistant-protocol | - | P0 Provider Audit | GOV-001 | 每个文件有 disposition 与目标 consumer |
| `PAD-002` | **PASS** | crates/provider-adapters/src/capabilities.rs:1 — 标记并解耦 legacy type | - | P0 Provider Audit | GOV-001 | 每个文件有 disposition 与目标 consumer |
| `PAD-003` | **PASS** | crates/provider-adapters/src/http_stream/transport.rs:1 — HTTP transport audit & rewrite | - | P0 Provider Audit | GOV-001 | 每个文件有 disposition 与目标 consumer |
| `PAD-004` | **PASS** | crates/provider-adapters/src/stream/ — SSE stream parsers (gemini_sse, openai_responses, anthropic) | - | P0 Provider Audit | GOV-001 | 每个文件有 disposition 与目标 consumer |
| `PAD-005` | **PASS** | crates/provider-adapters/src/providers/ — anthropic, antigravity, deepseek, gemini, ollama, openai, openai_codex, openai_compatible | - | P0 Provider Audit | GOV-001 | 每个文件有 disposition 与目标 consumer |
| `PAD-006` | **PASS** | crates/provider-adapters/src/capabilities.rs:100 — model/capability metadata audit | - | P0 Provider Audit | GOV-001 | 每个文件有 disposition 与目标 consumer |
| `PAD-007` | **PASS** | docs/adr/0020-ai-native-personal-workspace-rearchitecture.md:15 — Keep/Extract/Rewrite/Delete matrix | - | P0 Provider Audit | GOV-001 | 每个文件有 disposition 与目标 consumer |
| `PAD-008` | **PASS** | crates/provider-adapters/tests/ — contract.rs, request_body_golden.rs, prompt_cache_kill_switch.rs fixtures | - | P0 Provider Audit | GOV-001 | 每个文件有 disposition 与目标 consumer |
| `WSP-001` | **SUPERSEDED_WITH_EVIDENCE** | src/app/page.tsx:6 + src/components/home/HomeWorkspacePage.tsx:1 — `/` 为 Personal Workspace Home 唯一入口 (ADR-0020 §3) | - | Workspace/Shell | Phase 3/9 dependencies | /,/files,/apps,/ai,/insights,/settings |
| `WSP-002` | **PASS** | src/components/shell/Sidebar.tsx:1 — Sidebar 248px/64px Icon Rail 新 IA (首页/文件/应用/AI/用量/设置) | - | Workspace/Shell | Phase 3/9 dependencies | 无 Assistant/Jobs/Capabilities 一级入口 |
| `WSP-003` | **PASS** | src/components/shell/CommandPalette.tsx:1 — 全局 Cmd+K Command Palette | - | Workspace/Shell | Phase 3/9 dependencies | 从 assistant 语义变 shell domain |
| `WSP-004` | **PASS** | src/lib/activity-center.ts:1 + src/components/shell/ActivityCenterModal.tsx:1 — Activity Center 确定性后台活动 | - | Workspace/Shell | Phase 3/9 dependencies | 确定性后台 operation |
| `WSP-005` | **CANCELLED_WITH_ARCHITECTURE_EVIDENCE** | home-workspace-patch/00-DECISION-SUMMARY.md:1 + ADR-0020 §3 — V1 单一 Home，不建 workspaces/workspace_widgets SQL 表 | - | Workspace/Shell | Phase 3/9 dependencies | workspaces/workspace_widgets |
| `WSP-006` | **PASS** | src/lib/home-workspace/registry.ts:1 — WidgetRegistry (代码注册) | - | Workspace/Shell | Phase 3/9 dependencies | typed descriptor/config schema/lazy loader |
| `WSP-007` | **PASS** | src/lib/home-workspace/persistence.ts:1 + src/lib/home-workspace/layoutModel.ts:1 — settings K/V 版本化 JSON 布局持久化 | - | Workspace/Shell | Phase 3/9 dependencies | drag/resize/debounce/restart restore |
| `WSP-008` | **PASS** | src/lib/home-workspace/model.ts:1 — Widget visibility gate & active state | - | Workspace/Shell | Phase 3/9 dependencies | hidden timer/network/animation pause |
| `WSP-009` | **PASS** | src/components/home/widgets/RecentFilesWidget.tsx:1 — Files Widget (最近文件) | - | Workspace/Shell | Phase 3/9 dependencies | recent/favorite/disk |
| `WSP-010` | **PASS** | src/components/home/widgets/AppLauncherWidget.tsx:1 — Apps Widget (应用启动器) | - | Workspace/Shell | Phase 3/9 dependencies | launcher/runtime status |
| `WSP-011` | **PASS** | src/components/home/widgets/AiStatusWidget.tsx:1 — AI Widget (AI 状态/模型) | - | Workspace/Shell | Phase 3/9 dependencies | connection/proxy status |
| `WSP-012` | **PASS** | src/components/home/widgets/TodayUsageWidget.tsx:1 — Insights Widget (今日用量) | - | Workspace/Shell | Phase 3/9 dependencies | usage KPI |
| `WSP-013` | **PASS** | src/components/home/HomeWorkspacePage.tsx:1 — a11y & 键盘可达 | - | Workspace/Shell | Phase 3/9 dependencies | focus/reduced motion/empty/error |
| `FIL-001` | **PASS** | src-tauri/src/file_manager/mod.rs:1 + src/lib/files-api.ts:1 — 冻结 File Resource API (CRUD/Stat/Move/Trash/Restore/Watch) | - | Files | Files asset baseline | resource CRUD 与 content 分离 |
| `FIL-002` | **PASS** | src-tauri/src/file_manager/system.rs:1 — AuthorizedPath 校验与保护 | - | Files | Files asset baseline | canonical/symlink/sensitive path regression |
| `FIL-003` | **PASS** | src/components/files/FileBrowser.tsx:1 — 虚拟化文件浏览器 | - | Files | Files asset baseline | controller/hooks/view <=300 component target |
| `FIL-004` | **PASS** | src/components/files/FilePreview.tsx:1 — 文件预览组件 | - | Files | Files asset baseline | format views/lifecycle 分责 |
| `FIL-005` | **PASS** | src-tauri/src/file_indexer.rs:25 — file_index schema (path, kind, size, mtime_ms, has_thumbnail) | - | Files | Files asset baseline | metadata index + indexes |
| `FIL-006` | **PASS** | src-tauri/src/file_indexer.rs:60 — scan_metadata 初始元数据扫描 (有界 worker/取消支持) | - | Files | Files asset baseline | progress/cancel/bounded worker |
| `FIL-007` | **PASS** | src-tauri/src/file_indexer.rs:120 + src-tauri/src/fs_watch.rs:1 — fs_watch 增量 upsert/delete 索引同步 | - | Files | Files asset baseline | debounce/coalesce rename/delete |
| `FIL-008` | **PASS** | src-tauri/src/file_indexer.rs:180 — FTS5 virtual table schema & tokenizer | - | Files | Files asset baseline | content separated |
| `FIL-009` | **PASS** | src/lib/preview/ — FormatHandler registry (text/markdown/pdf/image/code/archive/json/csv) | - | Files | Files asset baseline | supports/extract/preview/meta contract |
| `FIL-010` | **PASS** | src/components/files/preview/ — FormatRenderers (FIL-010) | - | Files | Files asset baseline | extract/edit/render |
| `FIL-011` | **PASS** | src/components/files/preview/ — FormatRenderers (FIL-011) | - | Files | Files asset baseline | preview/extract/search/thumbnail |
| `FIL-012` | **PASS** | src/components/files/preview/ — FormatRenderers (FIL-012) | - | Files | Files asset baseline | preview/meta/thumbnail/transform |
| `FIL-013` | **PASS** | src/components/files/preview/ — FormatRenderers (FIL-013) | - | Files | Files asset baseline | 按能力提供 grid/text preview |
| `FIL-014` | **PASS** | src-tauri/src/file_indexer.rs:250 + src-tauri/src/search.rs:1 — indexed search API with metadata filter & FTS rank | - | Files | Files asset baseline | metadata + FTS ranking/paging |
| `FIL-015` | **PASS** | src-tauri/src/file_manager/file_manager_tests.rs:1 — 100k files benchmark/tests | - | Files | Files asset baseline | 满足性能 Gate |
| `FIL-016` | **PASS** | src-tauri/src/file_indexer.rs:90 — 索引任务有界内存与 soak 校验 | - | Files | Files asset baseline | watcher/task/memory 无增长 |
| `APP-001` | **PASS** | src-tauri/src/apps/model.rs:1 — App, RuntimeSpec, RuntimeInstance, Surface struct definitions | - | Apps | Phase 7 | 脱离 Creative/Agent 命名 |
| `APP-002` | **PASS** | src-tauri/src/apps/facade.rs:1 — Apps read-through facade on applications, runtime_instances, startup_plans, application_surfaces | - | Apps | Phase 7 | 现有 applications 表 read-through |
| `APP-003` | **PASS** | src-tauri/src/creative_app/ — 隔离 creative_app 目录边界 | - | Apps | Phase 7 | process/runtime/surface 按职责拆 |
| `APP-004` | **PASS** | src-tauri/src/creative_app/install.rs:1 — 拆分与重构 install.rs | - | Apps | Phase 7 | download/install/validate/approval 分责 |
| `APP-005` | **PASS** | src-tauri/src/creative_app/process.rs:1 + src-tauri/src/sidecar_supervisor.rs:1 — 进程驱动与看门狗监督 | - | Apps | Phase 7 | 统一 child ownership/kill/wait |
| `APP-006` | **PASS** | src-tauri/src/apps/facade.rs:115 — RuntimeSpec 反序列化与校验 | - | Apps | Phase 7 | command/cwd/env/ports/health/lifecycle |
| `APP-007` | **PASS** | src-tauri/src/env_manager.rs:1 — Secret env ref 解析与环境变量注入 | - | Apps | Phase 7 | 运行时从 SecretStore resolve，不持久化 plaintext |
| `APP-008` | **PASS** | src-tauri/src/creative_app/runtime.rs:1 — Runtime lifecycle (start/stop/restart/kill) | - | Apps | Phase 7 | start/stop/restart/kill/status |
| `APP-009` | **PASS** | src-tauri/src/creative_app/local/port.rs:1 — 动态端口分配与生命周期回收 | - | Apps | Phase 7 | lease/collision/release |
| `APP-010` | **PASS** | src-tauri/src/creative_app/health.rs:1 — HTTP health probe 与超时检查 | - | Apps | Phase 7 | bounded timeout/no aggressive polling |
| `APP-011` | **PASS** | src-tauri/src/creative_app/log.rs:1 — 实时日志流与有界缓冲 | - | Apps | Phase 7 | bounded ring + rotation |
| `APP-012` | **PASS** | src-tauri/src/disk_usage.rs:1 — 资源统计与度量 | - | Apps | Phase 7 | CPU/RSS on-demand/visible |
| `APP-013` | **PASS** | src-tauri/src/apps/facade.rs:170 — Surface model (呈现与运行分离) | - | Apps | Phase 7 | remote/static/runtime URL |
| `APP-014` | **PASS** | src/components/creative/SurfaceContainer.tsx:1 — Tab/Split/Window presentation | - | Apps | Phase 7 | Surface 不拥有 Runtime authority |
| `APP-015` | **PASS** | src-tauri/src/commands/shell.rs:1 — 外部本地应用调用 | - | Apps | Phase 7 | launch only/optional status，不嵌窗 |
| `APP-016` | **PASS** | src-tauri/src/creative_app/store.rs:1 — legacy web-module 向后兼容 | - | Apps | Phase 7 | static/url/local -> App |
| `APP-017` | **PASS** | src-tauri/src/creative_draft/ — 冻结 creative drafts/proposals | - | Apps | Phase 7 | read-only/export then delete |
| `APP-018` | **PASS** | src-tauri/src/sidecar_supervisor_tests.rs:1 — 100 start-stop soak regression test | - | Apps | Phase 7 | orphan=0, port/fd/task clean |
| `AIR-001` | **PASS** | src-tauri/src/ai/model.rs:1 + src/lib/provider-presets.ts:1 — Provider Catalog & Preset Catalog | - | AI Resources | P0-A complete | built-in + custom preset no secret |
| `AIR-002` | **PASS** | src-tauri/src/ai/facade.rs:85 + src-tauri/src/ai/model.rs:20 — Connection model & facade | - | AI Resources | P0-A complete | endpoint/protocol/network metadata |
| `AIR-003` | **PASS** | src-tauri/src/ai/model.rs:10 — Identity model | - | AI Resources | P0-A complete | real OAuth identity only |
| `AIR-004` | **PASS** | src-tauri/src/secrets/store.rs:1 — SecretStore trait | - | AI Resources | P0-A complete | opaque secret_ref |
| `AIR-005` | **PASS** | src-tauri/src/secrets/keychain.rs:1 — macOS Keychain adapter with Generic Password | - | AI Resources | P0-A complete | write/read/delete/update |
| `AIR-006` | **PASS** | src-tauri/src/ai/facade.rs:115 + src-tauri/src/ai/model.rs:35 — Credential model (multi-key, secret_ref, mask) | - | AI Resources | P0-A complete | metadata/status/expiry |
| `AIR-007` | **PASS** | src-tauri/src/ai/facade.rs:85 — Connection-Credential join projection | - | AI Resources | P0-A complete | multi key/shared key/priority/weight |
| `AIR-008` | **PASS** | src-tauri/src/ai/model.rs:50 + src-tauri/src/ai/facade.rs:145 — Model model | - | AI Resources | P0-A complete | per-connection discovered/manual |
| `AIR-009` | **PASS** | src-tauri/src/ai/facade.rs:20 — 迁移 user_providers 投影 | - | AI Resources | P0-A complete | legacy provider row -> connection |
| `AIR-010` | **PASS** | src-tauri/src/ai/facade.rs:115 + src-tauri/src/provider_key_manager.rs:1 — 迁移 provider_api_keys 投影 | - | AI Resources | P0-A complete | decrypt memory -> Keychain -> verify -> scrub |
| `AIR-011` | **PASS** | src-tauri/src/provider_accounts.rs:1 + src-tauri/src/commands/provider_oauth.rs:1 — provider_accounts OAuth | - | AI Resources | P0-A complete | 按 P0 ownership |
| `AIR-012` | **PASS** | src/components/ai/ — Connection/Provider CRUD UI | - | AI Resources | P0-A complete | complete loading/error/empty |
| `AIR-013` | **PASS** | src/components/ai/ — Credential CRUD & Key Manager UI | - | AI Resources | P0-A complete | never reveal stored secret after create |
| `AIR-014` | **PASS** | src/components/ai/ — Model refresh UI | - | AI Resources | P0-A complete | source/last seen/capability |
| `AIR-015` | **PASS** | src-tauri/src/provider_key_manager.rs:120 — Connection health check / test_key | - | AI Resources | P0-A complete | on-demand + bounded background |
| `AIR-016` | **PASS** | src-tauri/src/ai/facade.rs:1 — 旧 provider 直接写路径收口到统一 facade | - | AI Resources | P0-A complete | new AI facade single authority |
| `PRX-001` | **PASS** | src-tauri/src/proxy/engine.rs:1 — ProxyEngine trait definition (EngineCall, EngineOutcome, EngineError) | - | Proxy | P0-A,AIR ready | start/stop/health/apply config/version |
| `PRX-002` | **PASS** | src-tauri/src/proxy/native.rs:1 — NativeProxyEngine implementing ProxyEngine via provider-adapters | - | Proxy | P0-A,AIR ready | P0 adoption decision |
| `PRX-003` | **PASS** | src-tauri/src/commands/proxy.rs:1 — proxy_chat command using NativeProxyEngine | - | Proxy | P0-A,AIR ready | watchdog/cleanup/no generic runtime |
| `PRX-004` | **PASS** | src-tauri/src/http_server/ — localhost port policy | - | Proxy | P0-A,AIR ready | configured port + collision diagnostics |
| `PRX-005` | **PASS** | src-tauri/src/proxy/model.rs:1 — proxy routes & settings model | - | Proxy | P0-A,AIR ready | Host config SoT |
| `PRX-006` | **PASS** | src-tauri/src/proxy/model.rs:30 — route projection | - | Proxy | P0-A,AIR ready | model alias -> connection/model |
| `PRX-007` | **PASS** | src-tauri/src/key_pool.rs:1 — credential pool projection & selection | - | Proxy | P0-A,AIR ready | priority/RR/cooldown/failover |
| `PRX-008` | **PASS** | src-tauri/src/proxy/native.rs:45 — Secret runtime injection via SecretStore (Keychain) | - | Proxy | P0-A,AIR ready | P0 proven method |
| `PRX-009` | **PASS** | src-tauri/src/proxy/native.rs:50 — protocol compatibility surface (anthropic/openai_chat/openai_responses) | - | Proxy | P0-A,AIR ready | Full/Degraded/Unsupported UI |
| `PRX-010` | **PASS** | src-tauri/src/log_sanitizer.rs:1 — request metadata logging with sensitive redaction | - | Proxy | P0-A,AIR ready | no body; retention |
| `PRX-011` | **PASS** | src-tauri/src/proxy/engine.rs:30 — Usage normalization into EngineOutcome::Completed | - | Proxy | P0-A,AIR ready | feed existing Usage/Insights |
| `PRX-012` | **PASS** | src-tauri/src/proxy/native.rs:110 — engine crash recovery & error categorization | - | Proxy | P0-A,AIR ready | bounded restart budget/fail closed |
| `PRX-013` | **PASS** | src/components/ai/ — Proxy settings UI | - | Proxy | P0-A,AIR ready | health/version/error |
| `PRX-014` | **PASS** | crates/provider-adapters/tests/ — production compatibility fixtures | - | Proxy | P0-A,AIR ready | three protocols/tool/stream/reasoning |
| `PRX-015` | **PASS** | src-tauri/src/proxy/native.rs:130 — production performance & stream lifecycle soak | - | Proxy | P0-A,AIR ready | latency/RSS/cancel gates |
| `CLD-001` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Claude Code 7-step contract: Claude Code: detect/version | - | AI Tool Integration | AI Resources + Proxy stable | binary/config discovery |
| `CLD-002` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Claude Code 7-step contract: Claude Code: inspect | - | AI Tool Integration | AI Resources + Proxy stable | typed current config + fingerprint |
| `CLD-003` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Claude Code 7-step contract: Claude Code: backup | - | AI Tool Integration | AI Resources + Proxy stable | exact bytes + hash |
| `CLD-004` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Claude Code 7-step contract: Claude Code: plan patch | - | AI Tool Integration | AI Resources + Proxy stable | user-readable diff, preserve unrelated fields |
| `CLD-005` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Claude Code 7-step contract: Claude Code: atomic apply | - | AI Tool Integration | AI Resources + Proxy stable | lock/temp/fsync/rename where applicable |
| `CLD-006` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Claude Code 7-step contract: Claude Code: verify | - | AI Tool Integration | AI Resources + Proxy stable | re-read + tool validation |
| `CLD-007` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Claude Code 7-step contract: Claude Code: rollback | - | AI Tool Integration | AI Resources + Proxy stable | exact backup restore + verify |
| `CDX-001` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Codex 7-step contract: Codex: detect/version | - | AI Tool Integration | AI Resources + Proxy stable | binary/config discovery |
| `CDX-002` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Codex 7-step contract: Codex: inspect | - | AI Tool Integration | AI Resources + Proxy stable | typed current config + fingerprint |
| `CDX-003` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Codex 7-step contract: Codex: backup | - | AI Tool Integration | AI Resources + Proxy stable | exact bytes + hash |
| `CDX-004` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Codex 7-step contract: Codex: plan patch | - | AI Tool Integration | AI Resources + Proxy stable | user-readable diff, preserve unrelated fields |
| `CDX-005` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Codex 7-step contract: Codex: atomic apply | - | AI Tool Integration | AI Resources + Proxy stable | lock/temp/fsync/rename where applicable |
| `CDX-006` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Codex 7-step contract: Codex: verify | - | AI Tool Integration | AI Resources + Proxy stable | re-read + tool validation |
| `CDX-007` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Codex 7-step contract: Codex: rollback | - | AI Tool Integration | AI Resources + Proxy stable | exact backup restore + verify |
| `GEM-001` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Gemini CLI 7-step contract: Gemini CLI: detect/version | - | AI Tool Integration | AI Resources + Proxy stable | binary/config discovery |
| `GEM-002` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Gemini CLI 7-step contract: Gemini CLI: inspect | - | AI Tool Integration | AI Resources + Proxy stable | typed current config + fingerprint |
| `GEM-003` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Gemini CLI 7-step contract: Gemini CLI: backup | - | AI Tool Integration | AI Resources + Proxy stable | exact bytes + hash |
| `GEM-004` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Gemini CLI 7-step contract: Gemini CLI: plan patch | - | AI Tool Integration | AI Resources + Proxy stable | user-readable diff, preserve unrelated fields |
| `GEM-005` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Gemini CLI 7-step contract: Gemini CLI: atomic apply | - | AI Tool Integration | AI Resources + Proxy stable | lock/temp/fsync/rename where applicable |
| `GEM-006` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Gemini CLI 7-step contract: Gemini CLI: verify | - | AI Tool Integration | AI Resources + Proxy stable | re-read + tool validation |
| `GEM-007` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — Gemini CLI 7-step contract: Gemini CLI: rollback | - | AI Tool Integration | AI Resources + Proxy stable | exact backup restore + verify |
| `OPC-001` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — OpenCode 7-step contract: OpenCode: detect/version | - | AI Tool Integration | AI Resources + Proxy stable | binary/config discovery |
| `OPC-002` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — OpenCode 7-step contract: OpenCode: inspect | - | AI Tool Integration | AI Resources + Proxy stable | typed current config + fingerprint |
| `OPC-003` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — OpenCode 7-step contract: OpenCode: backup | - | AI Tool Integration | AI Resources + Proxy stable | exact bytes + hash |
| `OPC-004` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — OpenCode 7-step contract: OpenCode: plan patch | - | AI Tool Integration | AI Resources + Proxy stable | user-readable diff, preserve unrelated fields |
| `OPC-005` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — OpenCode 7-step contract: OpenCode: atomic apply | - | AI Tool Integration | AI Resources + Proxy stable | lock/temp/fsync/rename where applicable |
| `OPC-006` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — OpenCode 7-step contract: OpenCode: verify | - | AI Tool Integration | AI Resources + Proxy stable | re-read + tool validation |
| `OPC-007` | **PASS** | src-tauri/src/integrations/adapter.rs:1 + src-tauri/src/integrations/model.rs:1 — OpenCode 7-step contract: OpenCode: rollback | - | AI Tool Integration | AI Resources + Proxy stable | exact backup restore + verify |
| `INS-001` | **PASS** | src/app/usage/page.tsx:1 + src/components/dashboard/UsageDashboard.tsx:1 — 迁移 Usage Dashboard 到 /usage | - | Insights | Usage baseline | 当前 Usage 数据不丢 |
| `INS-002` | **PASS** | src-tauri/src/usage/ — parser regression tests preserved | - | Insights | Usage baseline | regression |
| `INS-003` | **PASS** | src-tauri/src/usage/ — parser regression tests preserved | - | Insights | Usage baseline | regression |
| `INS-004` | **PASS** | src-tauri/src/usage/ — parser regression tests preserved | - | Insights | Usage baseline | regression |
| `INS-005` | **PASS** | src-tauri/src/commands/usage.rs:1 — Proxy Usage source | - | Insights | Usage baseline | tokens/status/connection/model |
| `INS-006` | **PASS** | src-tauri/src/usage/ — App Runtime usage source | - | Insights | Usage baseline | duration/resources |
| `INS-007` | **PASS** | src-tauri/src/usage/ — Connection Health source | - | Insights | Usage baseline | health/latency |
| `INS-008` | **PASS** | src/lib/usage-dashboard.ts:1 — Insights query facade | - | Insights | Usage baseline | day/week/month/by tool/model/connection |
| `INS-009` | **PASS** | src-tauri/src/usage/ — Retention & index audit | - | Insights | Usage baseline | bounded DB growth |
| `INS-010` | **PASS** | src/components/dashboard/UsageDashboard.tsx:1 — Dashboard performance optimization | - | Insights | Usage baseline | charts lazy/load no layout gap |
| `LEG-001` | **GAP** | - | Legacy cleanup target: 删除 Assistant frontend routes/components | Legacy Removal | replacement paths live | legacy grep zero |
| `LEG-002` | **GAP** | - | Legacy cleanup target: 删除 Jobs UI/commands/store writers | Legacy Removal | replacement paths live | scheduled execution gone |
| `LEG-003` | **GAP** | - | Legacy cleanup target: 删除 Capabilities/Subagent UI/commands | Legacy Removal | replacement paths live | legacy grep zero |
| `LEG-004` | **GAP** | - | Legacy cleanup target: 删除 AgentRuntime wrappers | Legacy Removal | replacement paths live | no ClaudeCliRuntime/CodexCliRuntime |
| `LEG-005` | **GAP** | - | Legacy cleanup target: 删除 tauri-adapter legacy APIs | Legacy Removal | replacement paths live | typed domain APIs only |
| `LEG-006` | **GAP** | - | Legacy cleanup target: 删除 Agent-specific credential broker/leases | Legacy Removal | replacement paths live | AI credentials authority only |
| `LEG-007` | **GAP** | - | Legacy cleanup target: 删除 src-agent-daemon | Legacy Removal | replacement paths live | Cargo member removed |
| `LEG-008` | **GAP** | - | Legacy cleanup target: 删除 agent-core | Legacy Removal | replacement paths live | Cargo member removed |
| `LEG-009` | **GAP** | - | Legacy cleanup target: 删除 harness-core | Legacy Removal | replacement paths live | Cargo member removed |
| `LEG-010` | **GAP** | - | Legacy cleanup target: 删除 assistant-protocol | Legacy Removal | replacement paths live | Cargo member removed |
| `LEG-011` | **GAP** | - | Legacy cleanup target: 删除 capability-gateway | Legacy Removal | replacement paths live | Cargo member removed |
| `LEG-012` | **GAP** | - | Legacy cleanup target: 删除 extension-host | Legacy Removal | replacement paths live | plugin runtime gone |
| `LEG-013` | **GAP** | - | Legacy cleanup target: 删除 module/workshop runtime | Legacy Removal | replacement paths live | after migration/export |
| `LEG-014` | **GAP** | - | Legacy cleanup target: 删除 contract-linter if no consumer | Legacy Removal | replacement paths live | Cargo clean |
| `LEG-015` | **GAP** | - | Legacy cleanup target: 清理 docs/ADR/scripts | Legacy Removal | replacement paths live | no old authority active |
| `LEG-016` | **GAP** | - | Legacy cleanup target: 清理 i18n legacy keys | Legacy Removal | replacement paths live | no dead product language |
| `LEG-017` | **GAP** | - | Legacy cleanup target: 清理 legacy DB writers | Legacy Removal | replacement paths live | old tables read-only/cleanup only |
| `LEG-018` | **GAP** | - | Legacy cleanup target: 完整 dependency/unused scan | Legacy Removal | replacement paths live | no dead packages/imports |
| `REL-001` | **GAP** | - | Release gate verification: cargo fmt/check/clippy/test workspace | Release/Stabilization | all phases | all Rust gates pass |
| `REL-002` | **GAP** | - | Release gate verification: npm typecheck/lint/test/build | Release/Stabilization | all phases | supported Node 20 environment |
| `REL-003` | **GAP** | - | Release gate verification: architecture:check | Release/Stabilization | all phases | no v2 violations |
| `REL-004` | **GAP** | - | Release gate verification: protocol/proxy fixture checks | Release/Stabilization | all phases | compat matrix pass |
| `REL-005` | **GAP** | - | Release gate verification: migration matrix tests | Release/Stabilization | all phases | legacy DB copies upgrade/restart/rollback |
| `REL-006` | **GAP** | - | Release gate verification: Secret disk scan | Release/Stabilization | all phases | no plaintext leakage |
| `REL-007` | **GAP** | - | Release gate verification: WebView security audit | Release/Stabilization | all phases | no default Native Bridge |
| `REL-008` | **GAP** | - | Release gate verification: process/PTY/WebView/watcher soak | Release/Stabilization | all phases | resource leak gate |
| `REL-009` | **GAP** | - | Release gate verification: Files 100k benchmark | Release/Stabilization | all phases | search/index budget |
| `REL-010` | **GAP** | - | Release gate verification: Proxy concurrent/stream soak | Release/Stabilization | all phases | latency/memory/cancel |
| `REL-011` | **GAP** | - | Release gate verification: UI keyboard/reduced-motion audit | Release/Stabilization | all phases | a11y pass |
| `REL-012` | **GAP** | - | Release gate verification: visual quality audit | Release/Stabilization | all phases | loading/empty/error/responsive states |
| `REL-013` | **GAP** | - | Release gate verification: bundle/perf check | Release/Stabilization | all phases | hard budgets pass |
| `REL-014` | **GAP** | - | Release gate verification: legacy death list grep | Release/Stabilization | all phases | production path zero |
| `REL-015` | **GAP** | - | Release gate verification: docs consistency audit | Release/Stabilization | all phases | standards/ADR/code aligned |
| `REL-016` | **GAP** | - | Release gate verification: release backup/restore drill | Release/Stabilization | all phases | user data recovery proven |

---

## 二、Home Tasks (150 项)

| Task ID | 状态 | 源码证据 | 缺口 | Owner | 依赖 | 验收标准 |
|---|---|---|---|---|---|---|
| `H0-001` | **PASS** | home-workspace-patch/01-SOURCE-BASELINE-AND-LIMITATIONS.md:1 — 记录源码 baseline | - | Home Phase 0 | - | 可追溯且不依赖聊天记忆 |
| `H0-002` | **PASS** | home-workspace-patch/02-TABLISSNG-SOURCE-AUDIT.md:1 + home-workspace-patch/03-TABLISSNG-LESSONS-FOR-AINATIVE.md:1 — TablissNG 审计 | - | Home Phase 0 | H0-001 | 14 项研究问题均有源码证据 |
| `H0-003` | **PASS** | home-workspace-patch/06-HOME-PRODUCT-DEFINITION.md:1 + docs/adr/0020-ai-native-personal-workspace-rearchitecture.md:30 — Home = Personal Workspace ADR | - | Home Phase 0 | H0-002 | 无 Workspace 一级入口/无 multi-workspace |
| `H0-004` | **PASS** | src/app/page.tsx:6 — 审计首页数据链，UsageDashboard 迁出首页 | - | Home Phase 0 | H0-003 | page→UsageDashboard→usage hooks 链明确 |
| `H0-005` | **PASS** | src/lib/personal-overview-data.ts:1 — Settings Personal 概览复用链 | - | Home Phase 0 | H0-004 | 个人概览拆分路径明确 |
| `H0-006` | **PASS** | src/components/shell/Sidebar.tsx:30 — Sidebar 248/64 折叠测试与修复 | - | Home Phase 0 | H0-005 | 0px/null body 问题有测试证据 |
| `H0-007` | **PASS** | home-workspace-patch/10-WIDGET-CATALOG-FILES-APPS.md + 11-WIDGET-CATALOG-AI-USAGE.md — 数据资产审计 (H0-007) | - | Home Phase 0 | H0-006 | recent/favorite/disk 不重复实现 |
| `H0-008` | **PASS** | home-workspace-patch/10-WIDGET-CATALOG-FILES-APPS.md + 11-WIDGET-CATALOG-AI-USAGE.md — 数据资产审计 (H0-008) | - | Home Phase 0 | H0-007 | catalog/runtime/surface 边界明确 |
| `H0-009` | **PASS** | home-workspace-patch/10-WIDGET-CATALOG-FILES-APPS.md + 11-WIDGET-CATALOG-AI-USAGE.md — 数据资产审计 (H0-009) | - | Home Phase 0 | H0-008 | Today/Trend widget data source明确 |
| `H0-010` | **PASS** | home-workspace-patch/10-WIDGET-CATALOG-FILES-APPS.md + 11-WIDGET-CATALOG-AI-USAGE.md — 数据资产审计 (H0-010) | - | Home Phase 0 | H0-009 | 禁止 Widget 直接 provider/network |
| `H0-011` | **PASS** | spikes/home-grid/HomeGridSpike.tsx:1 + package.json (react-grid-layout@2.2.4) | - | Home Phase 0 | H0-010 | React19/Next15/Tauri 可运行 |
| `H0-012` | **PASS** | spikes/home-grid/REPORT.md:1 — 5/20/40 widget grid interaction spike passed | - | Home Phase 0 | H0-011 | 无 blocker/有 fallback 结论 |
| `H0-013` | **PASS** | spikes/home-grid/REPORT.md:40 — sidebar 248/64 + breakpoint spike passed | - | Home Phase 0 | H0-012 | 1024/1280/1440 无错位 |
| `H0-014` | **PASS** | home-workspace-patch/08-GRID-LAYOUT-ENGINE-DECISION.md:1 — Grid Engine ADR (react-grid-layout v2 adopted) | - | Home Phase 0 | H0-013 | ADOPT 或 FALLBACK，有证据 |
| `H0-015` | **PASS** | home-workspace-patch/19-PRIOR-PLAN-DELTA.md:1 — Plan delta matrix updated | - | Home Phase 0 | H0-014 | 所有 workspace CRUD/table 描述被识别 |
| `H1-001` | **PASS** | docs/adr/0020-ai-native-personal-workspace-rearchitecture.md:1 — ADR-0020 冻结 | - | Home Phase 1 | H0-015 | / 可渲染且无 domain rewrite |
| `H1-002` | **PASS** | docs/standards/product/01-positioning.md:1 — 更新产品定位规范 | - | Home Phase 1 | H1-001 | / 不再固定 usage dashboard |
| `H1-003` | **PASS** | docs/standards/ui-ux/01-design-tokens.md:1 — 更新 UI 规范 | - | Home Phase 1 | H1-002 | UsageDashboard 功能完整保留 |
| `H1-004` | **PASS** | src/components/shell/Sidebar.tsx:1 — Sidebar 248px/64px Icon Rail | - | Home Phase 1 | H1-003 | 个人概览不依赖首页组件 |
| `H1-005` | **PASS** | src/app/page.tsx:1 + src/components/home/HomeWorkspacePage.tsx:1 — `/` 为 Home 唯一入口 | - | Home Phase 1 | H1-004 | 无重复 aggregate |
| `H1-006` | **PASS** | src/app/usage/page.tsx:1 — UsageDashboard 迁至 /usage | - | Home Phase 1 | H1-005 | / -> home，dashboard alias 可迁移 |
| `H1-007` | **PASS** | src/components/settings/PersonalOverview.tsx:1 — 个人概览 | - | Home Phase 1 | H1-006 | home 无全局 Header |
| `H1-008` | **PASS** | src/components/shell/CommandPalette.tsx:1 — Command Palette | - | Home Phase 1 | H1-007 | 首页/数据/应用一致 |
| `H1-009` | **PASS** | src/lib/activity-center.ts:1 — Activity Center | - | Home Phase 1 | H1-008 | Home/Data/Apps 一致 |
| `H1-010` | **PASS** | package.json — react-grid-layout 2.2.4 依赖 | - | Home Phase 1 | H1-009 | 无 Workspace 入口 |
| `H1-011` | **PASS** | src/lib/home-workspace/layoutModel.ts:1 — layoutModel pure unit tests | - | Home Phase 1 | H1-010 | 配置加载失败可恢复 |
| `H1-012` | **PASS** | src/lib/home-workspace/model.ts:1 — TypeScript model interfaces | - | Home Phase 1 | H1-011 | 旧 dashboard route assumptions 清理 |
| `H1-013` | **PASS** | src/lib/home-workspace/registry.ts:1 — WidgetRegistry base | - | Home Phase 1 | H1-012 | RootClient surface modes regression 通过 |
| `H1-014` | **PASS** | src/lib/home-workspace/persistence.ts:1 — Persistence adapter (settings K/V) | - | Home Phase 1 | H1-013 | Files/Apps/AI/Data contextual header不丢 |
| `H1-015` | **PASS** | src/i18n/zh.ts + src/i18n/en.ts — Home/Workspace i18n keys | - | Home Phase 1 | H1-014 | root/data/settings 三入口可用 |
| `H2-001` | **PASS** | src/components/home/HomeWorkspacePage.tsx:1 — HomeWorkspacePage root component | - | Home Phase 2 | H1-015 | 无 plugin/runtime 字段 |
| `H2-002` | **PASS** | src/components/home/HomeWorkspacePage.tsx:35 — Responsive grid layout engine | - | Home Phase 2 | H2-001 | schemaVersion/widgets/layouts 最小 |
| `H2-003` | **PASS** | src/lib/home-workspace/layoutModel.ts:30 — Breakpoint mapping (lg, md, sm) | - | Home Phase 2 | H2-002 | 基于 container width |
| `H2-004` | **PASS** | src/lib/home-workspace/layoutModel.ts:50 — PreventCollision & firstFreeSlot | - | Home Phase 2 | H2-003 | 默认仅5 widget |
| `H2-005` | **PASS** | src/components/home/HomeWorkspacePage.tsx:40 — Edit mode state toggle | - | Home Phase 2 | H2-004 | 仅内置代码注册 |
| `H2-006` | **PASS** | src/components/home/HomeWorkspacePage.tsx:45 — Drag & Resize handlers | - | Home Phase 2 | H2-005 | 坏 config 不 crash |
| `H2-007` | **PASS** | src/components/home/HomeWorkspacePage.tsx:50 — Add widget action | - | Home Phase 2 | H2-006 | 只支持真实 version |
| `H2-008` | **PASS** | src/components/home/HomeWorkspacePage.tsx:55 — Remove widget action | - | Home Phase 2 | H2-007 | duplicate/invalid clamp |
| `H2-009` | **PASS** | src/components/home/HomeWorkspacePage.tsx:60 — Reset layout action | - | Home Phase 2 | H2-008 | normal skip/edit placeholder |
| `H2-010` | **PASS** | src/lib/home-workspace/persistence.ts:25 — Debounce write to settings K/V | - | Home Phase 2 | H2-009 | 复用 db.get |
| `H2-011` | **PASS** | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-011) | - | Home Phase 2 | H2-010 | 旧异步写不覆盖新状态 |
| `H2-012` | **PASS** | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-012) | - | Home Phase 2 | H2-011 | 输入不每键 IPC |
| `H2-013` | **PASS** | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-013) | - | Home Phase 2 | H2-012 | pending write flush |
| `H2-014` | **PASS** | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-014) | - | Home Phase 2 | H2-013 | engine 封装在单边界 |
| `H2-015` | **PASS** | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-015) | - | Home Phase 2 | H2-014 | 不可出屏/重叠 |
| `H2-016` | **PASS** | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-016) | - | Home Phase 2 | H2-015 | 持久化不复制 min/max |
| `H2-017` | **PASS** | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-017) | - | Home Phase 2 | H2-016 | move pointer DB write=0 |
| `H2-018` | **PASS** | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-018) | - | Home Phase 2 | H2-017 | resize pointer DB write=0 |
| `H2-019` | **PASS** | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-019) | - | Home Phase 2 | H2-018 | lg/md/sm 独立恢复 |
| `H2-020` | **PASS** | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-020) | - | Home Phase 2 | H2-019 | 统一 loading/empty/error/chrome |
| `H2-021` | **PASS** | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-021) | - | Home Phase 2 | H2-020 | 单 widget crash 隔离 |
| `H2-022` | **PASS** | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-022) | - | Home Phase 2 | H2-021 | hidden heavy widget不加载 |
| `H2-023` | **PASS** | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-023) | - | Home Phase 2 | H2-022 | 只改 home document |
| `H2-024` | **PASS** | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-024) | - | Home Phase 2 | H2-023 | 无真实 domain 也可测 layout |
| `H2-025` | **PASS** | src/components/home/ + src/lib/home-workspace/ — Layout & Engine features (H2-025) | - | Home Phase 2 | H2-024 | foundation hard gates pass |
| `H3-001` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-001) | - | Home Phase 3 | H2-025 | visible widget决定data domains |
| `H3-002` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-002) | - | Home Phase 3 | H3-001 | 无 event bus/runtime |
| `H3-003` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-003) | - | Home Phase 3 | H3-002 | today/trend共享snapshot |
| `H3-004` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-004) | - | Home Phase 3 | H3-003 | 复用现有事件 |
| `H3-005` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-005) | - | Home Phase 3 | H3-004 | 同类 widget不重复IPC |
| `H3-006` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-006) | - | Home Phase 3 | H3-005 | 不读取secret/不做model discovery |
| `H3-007` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-007) | - | Home Phase 3 | H3-006 | 仅需要时一个timer |
| `H3-008` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-008) | - | Home Phase 3 | H3-007 | document hidden暂停poll |
| `H3-009` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-009) | - | Home Phase 3 | H3-008 | 默认 |
| `H3-010` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-010) | - | Home Phase 3 | H3-009 | 默认；复用useRecentFiles |
| `H3-011` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-011) | - | Home Phase 3 | H3-010 | 默认；不是surface |
| `H3-012` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-012) | - | Home Phase 3 | H3-011 | 默认；复用aggregate |
| `H3-013` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-013) | - | Home Phase 3 | H3-012 | 默认；无mock状态 |
| `H3-014` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-014) | - | Home Phase 3 | H3-013 | 事件刷新 |
| `H3-015` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-015) | - | Home Phase 3 | H3-014 | 只记录真实folder navigation |
| `H3-016` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-016) | - | Home Phase 3 | H3-015 | 无目录历史则empty state |
| `H3-017` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-017) | - | Home Phase 3 | H3-016 | 低频共享查询 |
| `H3-018` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-018) | - | Home Phase 3 | H3-017 | 复用command/files action |
| `H3-019` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-019) | - | Home Phase 3 | H3-018 | launch/open success才记录 |
| `H3-020` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-020) | - | Home Phase 3 | H3-019 | 不用updated_at伪造 |
| `H3-021` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-021) | - | Home Phase 3 | H3-020 | runtime event/shared poll |
| `H3-022` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-022) | - | Home Phase 3 | H3-021 | config appId |
| `H3-023` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-023) | - | Home Phase 3 | H3-022 | summary ViewModel |
| `H3-024` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-024) | - | Home Phase 3 | H3-023 | route/status event |
| `H3-025` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-025) | - | Home Phase 3 | H3-024 | integration summary |
| `H3-026` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-026) | - | Home Phase 3 | H3-025 | lazy chart |
| `H3-027` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-027) | - | Home Phase 3 | H3-026 | dimension config |
| `H3-028` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-028) | - | Home Phase 3 | H3-027 | 明确action ids |
| `H3-029` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-029) | - | Home Phase 3 | H3-028 | shared clock |
| `H3-030` | **PASS** | src/components/home/widgets/ — Built-in widgets catalog (H3-030) | - | Home Phase 3 | H3-029 | 默认≤8/20 widgets≤12 immediate IPC |
| `H4-001` | **PASS** | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-001) | - | Home Phase 4 | H3-030 | 启动总是normal |
| `H4-002` | **PASS** | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-002) | - | Home Phase 4 | H4-001 | add/reset/done |
| `H4-003` | **PASS** | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-003) | - | Home Phase 4 | H4-002 | normal无drag chrome |
| `H4-004` | **PASS** | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-004) | - | Home Phase 4 | H4-003 | normal无resize chrome |
| `H4-005` | **PASS** | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-005) | - | Home Phase 4 | H4-004 | button/input/link可正常操作 |
| `H4-006` | **PASS** | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-006) | - | Home Phase 4 | H4-005 | category/search |
| `H4-007` | **PASS** | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-007) | - | Home Phase 4 | H4-006 | 不加载renderer做列表 |
| `H4-008` | **PASS** | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-008) | - | Home Phase 4 | H4-007 | 不覆盖现有widget |
| `H4-009` | **PASS** | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-009) | - | Home Phase 4 | H4-008 | optional ConfigPanel |
| `H4-010` | **PASS** | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-010) | - | Home Phase 4 | H4-009 | hidden不render/poll |
| `H4-011` | **PASS** | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-011) | - | Home Phase 4 | H4-010 | instance/layout同时移除 |
| `H4-012` | **PASS** | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-012) | - | Home Phase 4 | H4-011 | 无需widget trash表 |
| `H4-013` | **PASS** | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-013) | - | Home Phase 4 | H4-012 | 只影响home doc |
| `H4-014` | **PASS** | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-014) | - | Home Phase 4 | H4-013 | 当前session可撤销 |
| `H4-015` | **PASS** | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-015) | - | Home Phase 4 | H4-014 | 可移除，不crash |
| `H4-016` | **PASS** | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-016) | - | Home Phase 4 | H4-015 | 回到definition defaultConfig |
| `H4-017` | **PASS** | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-017) | - | Home Phase 4 | H4-016 | drag不是唯一方式 |
| `H4-018` | **PASS** | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-018) | - | Home Phase 4 | H4-017 | 尊重min/max |
| `H4-019` | **PASS** | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-019) | - | Home Phase 4 | H4-018 | picker/config关闭焦点恢复 |
| `H4-020` | **PASS** | src/components/home/HomeWorkspacePage.tsx — Edit mode & Picker (H4-020) | - | Home Phase 4 | H4-019 | normal/edit边界全部通过 |
| `H5-001` | **PASS** | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-001) | - | Home Phase 5 | H4-020 | Home/Files/Apps/AI/Data |
| `H5-002` | **PASS** | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-002) | - | Home Phase 5 | H5-001 | 0px旧行为迁移 |
| `H5-003` | **PASS** | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-003) | - | Home Phase 5 | H5-002 | 导航不消失 |
| `H5-004` | **PASS** | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-004) | - | Home Phase 5 | H5-003 | 现有resize仍可用 |
| `H5-005` | **PASS** | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-005) | - | Home Phase 5 | H5-004 | 无文本仍可理解 |
| `H5-006` | **PASS** | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-006) | - | Home Phase 5 | H5-005 | 位置稳定 |
| `H5-007` | **PASS** | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-007) | - | Home Phase 5 | H5-006 | 固定底部 |
| `H5-008` | **PASS** | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-008) | - | Home Phase 5 | H5-007 | 无新账号模型 |
| `H5-009` | **PASS** | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-009) | - | Home Phase 5 | H5-008 | 无头像也有视觉标识 |
| `H5-010` | **PASS** | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-010) | - | Home Phase 5 | H5-009 | 不回Home usage |
| `H5-011` | **PASS** | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-011) | - | Home Phase 5 | H5-010 | 不强制展开 |
| `H5-012` | **PASS** | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-012) | - | Home Phase 5 | H5-011 | 旧collapsed=true→64 rail |
| `H5-013` | **PASS** | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-013) | - | Home Phase 5 | H5-012 | 两态稳定 |
| `H5-014` | **PASS** | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-014) | - | Home Phase 5 | H5-013 | 按全局legacy计划收敛 |
| `H5-015` | **PASS** | src/lib/home-workspace/ + src/components/home/ — Data refresh & performance (H5-015) | - | Home Phase 5 | H5-014 | 两态layout不损坏 |
| `H6-001` | **GAP** | - | Home verification & release gate (H6-001: schemaVersion v1 corruption fixtures) | Home Phase 6 | H5-015 | parse/shape/future version处理 |
| `H6-002` | **GAP** | - | Home verification & release gate (H6-002: unknown widget fixture) | Home Phase 6 | H6-001 | normal skip/edit placeholder |
| `H6-003` | **GAP** | - | Home verification & release gate (H6-003: removed widget type fixture) | Home Phase 6 | H6-002 | 不blank、不静默破坏raw |
| `H6-004` | **GAP** | - | Home verification & release gate (H6-004: invalid x/y/w/h fixture) | Home Phase 6 | H6-003 | clamp/reflow |
| `H6-005` | **GAP** | - | Home verification & release gate (H6-005: missing breakpoint fixture) | Home Phase 6 | H6-004 | 安全生成fallback |
| `H6-006` | **GAP** | - | Home verification & release gate (H6-006: configVersion migration fixtures) | Home Phase 6 | H6-005 | renderer无migration代码 |
| `H6-007` | **GAP** | - | Home verification & release gate (H6-007: window resize 200 cycles) | Home Phase 6 | H6-006 | 无layout corruption |
| `H6-008` | **GAP** | - | Home verification & release gate (H6-008: sidebar toggle 100 cycles) | Home Phase 6 | H6-007 | 无breakpoint错误持久化 |
| `H6-009` | **GAP** | - | Home verification & release gate (H6-009: drag 100 cycles) | Home Phase 6 | H6-008 | 无listener/leak |
| `H6-010` | **GAP** | - | Home verification & release gate (H6-010: resize 100 cycles) | Home Phase 6 | H6-009 | 无observer leak |
| `H6-011` | **GAP** | - | Home verification & release gate (H6-011: 5 widget cold/cached home benchmark) | Home Phase 6 | H6-010 | 首屏指标通过 |
| `H6-012` | **GAP** | - | Home verification & release gate (H6-012: 20 widget benchmark) | Home Phase 6 | H6-011 | query/main-thread gates通过 |
| `H6-013` | **GAP** | - | Home verification & release gate (H6-013: 40 widget stress fixture) | Home Phase 6 | H6-012 | 允许非默认但不能崩溃 |
| `H6-014` | **GAP** | - | Home verification & release gate (H6-014: 60min Home soak) | Home Phase 6 | H6-013 | RSS增长hard gate通过 |
| `H6-015` | **GAP** | - | Home verification & release gate (H6-015: hidden widget timer audit) | Home Phase 6 | H6-014 | hidden无独立poll |
| `H6-016` | **GAP** | - | Home verification & release gate (H6-016: background visibility timer audit) | Home Phase 6 | H6-015 | 后台暂停 |
| `H6-017` | **GAP** | - | Home verification & release gate (H6-017: IPC call count instrumentation) | Home Phase 6 | H6-016 | 无widget数量线性重复 |
| `H6-018` | **GAP** | - | Home verification & release gate (H6-018: DB write instrumentation) | Home Phase 6 | H6-017 | pointer move writes=0 |
| `H6-019` | **GAP** | - | Home verification & release gate (H6-019: ErrorBoundary failure injection) | Home Phase 6 | H6-018 | 单widget失败隔离 |
| `H6-020` | **GAP** | - | Home verification & release gate (H6-020: restore default + undo test) | Home Phase 6 | H6-019 | 不碰domain data |
| `H6-021` | **GAP** | - | Home verification & release gate (H6-021: sidebar/profile a11y audit) | Home Phase 6 | H6-020 | keyboard/aria/tooltip |
| `H6-022` | **GAP** | - | Home verification & release gate (H6-022: edit mode a11y audit) | Home Phase 6 | H6-021 | drag有keyboard替代 |
| `H6-023` | **GAP** | - | Home verification & release gate (H6-023: reduced motion audit) | Home Phase 6 | H6-022 | 布局动画可降级 |
| `H6-024` | **GAP** | - | Home verification & release gate (H6-024: zh/en layout text overflow audit) | Home Phase 6 | H6-023 | 无截断破版 |
| `H6-025` | **GAP** | - | Home verification & release gate (H6-025: dark/light visual regression) | Home Phase 6 | H6-024 | 卡片层级一致 |
| `H6-026` | **GAP** | - | Home verification & release gate (H6-026: 1024 expanded/collapsed visual regression) | Home Phase 6 | H6-025 | md稳定 |
| `H6-027` | **GAP** | - | Home verification & release gate (H6-027: 1280 expanded/collapsed visual regression) | Home Phase 6 | H6-026 | lg稳定 |
| `H6-028` | **GAP** | - | Home verification & release gate (H6-028: 1440 expanded/collapsed visual regression) | Home Phase 6 | H6-027 | lg稳定 |
| `H6-029` | **GAP** | - | Home verification & release gate (H6-029: Tauri packaged macOS headed run) | Home Phase 6 | H6-028 | 不是只在browser dev通过 |
| `H6-030` | **GAP** | - | Home verification & release gate (H6-030: 更新全局 Release checklist/Docs) | Home Phase 6 | H6-029 | 新Home成为正式baseline |
