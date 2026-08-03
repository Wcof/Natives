# 给 Luna 的「个人创意」整改实施提示词

你现在负责在 Natives 中实施“个人创意”闭环整改。先读完本提示词、`docs/README.md`、`docs/standards/README.md`、`docs/standards/technical/01-layering.md`、`02-security.md`、`05-backend.md`、ADR-0013、ADR-0014，以及：

- `docs/architecture/personal-creative-independent-audit.md`
- `docs/architecture/personal-creative-remediation-plan.md`

不要把现有设计文档的“已完成”当作事实；每项能力都要验证生产入口、调用者、持久化、异常处理、资源生命周期和精准测试。

## 0. 实施基线与工作区保护

- Worktree：`/Users/ldh/Downloads/project/AiNative/Natives`
- Branch：`deploy`
- 审计 HEAD：`1b4b1792932e2e24160091c7700a2c092d01e5f2`
- 审计日期：2026-08-03
- 当前工作区非干净，并有未合并路径：`src/components/shell/SettingsPage.tsx`、`Sidebar.tsx`、`settings-navigation.ts`、对应 test、`src/i18n/en.ts`、`src/i18n/zh.ts`。
- 开始每批前重新执行 `git status --short`、`git rev-parse HEAD`，覆盖 HEAD 和 working tree；不得覆盖用户修改或替用户选择冲突 stage。触碰 i18n 前先确认冲突已由用户/合并流程解决。
- 不创建第二套 Target、Runtime 或 SQLite 权威；不 Push；不做历史清理或无关格式化。

## 1. 当前真实能力

已完成且必须复用：

1. 三源 Catalog adapters：Internal Workshop、External GitHub Docker、Local Project。
2. 创作台 `creative-draft` 专用会话；四个生产 Gateway Tool：`write_draft_module`、`read_draft_module`、`rollback_draft_revision`、`lint_draft_module`。
3. 用户显式 `publish_creative_draft` 汇入 `module_manager::write_generated_module`；Agent 不能直接发布。
4. 本地 HTML/Vite/Vue 扫描、结构化 LaunchPlan、路径去重、加密 env、列表事件刷新。
5. Node/Vite 受管 argv、`setpgid`、TERM→5 秒→KILL→`wait`、日志脱敏/轮转、HTTP health。
6. GitHub 外部应用 Docker CLI adapter，稳定 Compose project name、loopback 端口、stop/down。
7. Tauri child WebView 预览，Workshop iframe 继续使用 `allow-scripts allow-forms`。
8. `cargo check -p natives` 在审计基线通过。

未完成、不得宣称完成：

1. 普通助理没有创建/注册/启动应用 Tool 或结构化结果卡。
2. 本地目录扫描不识别 Dockerfile、Compose、Python、Makefile、多服务；Freqtrade 为 unknown/no plan。
3. 没有统一 `Application / StartupPlan / RuntimeInstance / PreviewTarget`；三源分表，运行字段塞在 app 行。
4. 没有 Runtime Owner、实例 CancellationToken、task JoinHandle 集合、cleanup status、跨重启资源 ledger。
5. Local Stop 可忽略失败并写 stopped；orphan kill 不 wait/验证；端口不验证释放。
6. Host `creative_app/local/ai.rs` 直接解密 Key 并调用 Provider，绕过 Daemon/Gateway。
7. child WebView 后续导航可去任意 HTTP/HTTPS、跨应用复用存储、close 失败被吞、未绑定 Runtime。
8. 新创作台与旧 `createTemplate()` 硬编码模板入口并存；WorkshopPage 2,277 行。
9. 状态缺 building/waiting_for_health/degraded/cleanup_running/missing/orphaned。
10. Renderer 日志字符串无界增长；无真实资源 E2E。

## 2. 两条核心断链

### A. 普通助理创建应用

当前：

```text
AssistantWorkbench → run.start → 默认 builtin tools → 写文件/跑终端 → 文本回复
```

缺失：

```text
创建草稿/应用的正式 Tool 或 handoff
→ 结构化 draft/application result
→ Host 用户发布授权
→ Application/StartupPlan
→ Catalog 同 ID 刷新
→ RuntimeInstance start/health/preview/stop
```

目标：普通助理可以创建受限草稿或转交 Creative surface，但**正式发布仍只能由用户点击 Host 命令**。绝不把 `write_file` 成功当成应用创建。

### B. 导入并运行现有项目

当前：

```text
目录选择 → HTML/Vite/Vue 扫描 → local_creative_apps.launch_plan_json
→ static HTTP 或 Node Child → 单 HTTP health → child WebView
```

缺失：持续外置卷授权、多候选计划、Compose/Python、多服务、真实 RuntimeInstance、可取消 health/log、可证明 Stop、URL inspect 和恢复。

目标：目录选择后生成可解释且可编辑的 StartupPlan；所有 driver 通过同一 Runtime Owner；停止后以资源实测而非 UI 状态为准。

## 3. P0 / P1 / P2

### P0：先修，未通过不得扩类型

1. Local Stop 忽略 runtime stop 错误，无条件清 `process_identity_json/current_port/open_url` 并写 `installed_stopped`。
2. `local/ai.rs` 在 Host 直调 Provider，违反 Renderer→Host→UDS→Daemon 生产主链。
3. Freqtrade 根 Compose 默认 `trade`，未证明 dry-run 前禁止自动启动；任何通用 Compose 风险扫描必须默认阻断这类 command。
4. Embed WebView 首个 URL local-only，但后续 navigation 允许公网。

### P1：核心闭环

1. 普通助理无正式创建工具/结果卡。
2. 本地 Compose/Python/multi-service 缺失。
3. 无 RuntimeInstance/Owner/CancellationToken/CAS。
4. 启动中的 health 无取消，Stop 被全局锁阻断。
5. Browser 与 Runtime 解耦，关闭不可验证。
6. crash 后只会粗略 orphan，无法重连或证明清理。

### P2：体验

1. 两套创建入口；旧模板流必须删除。
2. WorkshopPage 过大；按钮、Badge、固定尺寸、颜色混用。
3. 状态/错误/日志/外置卷/多服务配置体验不完整。

## 4. 必须遵守的架构与安全不变量

1. 生产路径始终是 Renderer → Tauri Host → UDS → Agent Daemon；Provider/Agent Tool 不在 Host 旁路执行。
2. Host 继续拥有 `natives.db`、目录授权、进程/Docker/WebView；Daemon 拥有 Run、Provider、Gateway、Tool。
3. Agent 不直接写 SQLite、不监管 Docker、不控制 WebView、不获得正式模块目录写权限。
4. 正式 Workshop 发布只有 `write_generated_module` 一条门禁；草稿与发布用同一个 Contract Linter。
5. Workshop iframe 和 Embed child WebView 是不同信任域；Embed 永远没有 Workshop Bridge/Session Token。
6. 每个 Runtime 有唯一 `runtime_instance_id` 和唯一 Owner；application_id 不能替代 runtime id。
7. 每个后台资源绑定该 Runtime 的 root `CancellationToken`，并在 owner registry 记录 JoinHandle/资源 ID。
8. Stop 必须 cancel → TERM → timeout → KILL → wait/reap → 资源验证 → cleanup_completed → stopped。
9. Docker 只按稳定、唯一 Compose project name 和绝对 compose file 操作；禁止全局 stop/prune。
10. 删除本地 Application 只删 Natives 记录/日志/临时资源，不删用户项目。
11. 凭证不进 Renderer、不写日志；扫描不读/发送 `.env`、token、credential、key、pem 内容。
12. UI 复用现有 token、Modal、ConfirmDialog、Toast、EmptyState、Skeleton、助理组件；中英文同步。
13. 不允许用 Button、Struct、Trait、Migration、Schema、No-op 或 Fake 冒充功能完成。

## 5. 真实文件范围

优先在这些现有文件/模块内收敛；新增文件只为拆深模块或新增真实实体，不做预留框架：

- Renderer：`src/components/shell/WorkshopPage.tsx`、`src/components/creative/*`、`src/components/assistant/*`、`src/hooks/useCreativeAppCatalog.ts`、`useCreativeDrafts.ts`、`src/lib/creative-app.ts`、`local-creative.ts`、`creative-draft.ts`、`assistant-workspace/use-assistant-run.ts`、`tauri-adapter.ts`、`src/i18n/{zh,en}.ts`、`src/app/globals.css`。
- Host commands/store：`src-tauri/src/commands/creative_app.rs`、`creative_draft.rs`、`src-tauri/src/db.rs`、`module_manager.rs`。
- Runtime：`src-tauri/src/creative_app/{model,service,store,state_machine,browser,docker,install}.rs`、`adapters/*`、`local/{path,scan,plan,runtime,lifecycle,logs,deps,store,ai}.rs`、`src-tauri/src/lib.rs`。
- Daemon/Gateway：`src-agent-daemon/src/production.rs`、`run_manager.rs`、`production_tools.rs`、`crates/capability-gateway/src/tools/*`。
- Protocol：`crates/assistant-protocol/`；生成的 `src/types/generated/` 不手改。

## 6. 实施顺序（不得跳过 0–2 直接做 Compose）

### 批次 0：危险操作与资源安全

实施：

- Stop 传播所有停止错误；失败保留 identity/port/url，状态为 stopping/cleanup_failed/orphaned，不得 stopped。
- orphan TERM/KILL 后核实进程组与端口；可 reaping 的 Child 必须 wait。
- 把 local AI 分析搬到 Daemon 受控 Run/Tool；Host 仅输出脱敏 scan、校验并持久化 plan。
- child WebView 所有 navigation 保持 `127.0.0.1/localhost`；close 失败可观测。
- 建立危险 command 分类，`trade`/真实交易类默认阻断。

验收：精准测试证明 stop failure 不假绿、PGID/port 释放、Host 无 Provider adapter 调用、外部 navigation 被拒绝。

### 批次 1：统一领域实体

实施：新增最薄 `applications`、`startup_plans`、`runtime_instances`、`preview_targets`；现有三源表保留 detail，不复制 driver。

验收：三源 backfill 幂等；Catalog/助理卡片/详情同 application_id；同 app 只有一个 active runtime CAS。

### 批次 2：Runtime Owner 与资源回收

实施：Runtime root token + task/resource registry；Local Process 和 Docker 作为 driver；资源事件全部带 runtime_id；Stop 可抢占 start/health。

验收：TERM 忽略强杀、重复 Stop、启动中 Stop、reader/health 结束、crash reconcile、端口释放。

### 批次 3：普通助理创建闭环

实施：给普通助理增加受控 create-draft/handoff Tool 和结构化结果卡；用户 publish 后写 Application/StartupPlan；可启动并预览。

验收：静态 HTML 与 Vite 两场景从普通助理输入开始；Catalog 自动出现且 ID 一致；注册失败可从草稿恢复。

### 批次 4：本地项目扫描/授权

实施：macOS 持续授权/bookmark；多候选 scanner；HTML/Vite/Python/Compose evidence；AI 只提议，Host validator 裁决。

验收：中文空格路径、symlink escape、卷断开/重挂、多 lockfile、扫描 secret 排除。

### 批次 5：本地 Compose Runtime

实施：复用 `docker.rs`；`compose config` 预检；service/profile/build/port/health 结构化；unique project；部分失败补偿。

验收：两个 project 隔离、部分服务失败、日志 task 结束、Stop 不影响其他容器、默认保留 volume。

### 批次 6：URL/Health/Preview

实施：显式 URL → Docker inspect → stdout hint → framework default → 用户配置的可解释优先级；HTTP/TCP/Docker health；PreviewTarget 绑定 runtime；WebView storage 隔离/清理。

验收：随机端口、多 URL/base path、WebSocket、close 释放、停止后旧页面不可见。

### 批次 7：UI 收敛

实施：删除旧 `createTemplate`、`void onInstall` 和无效 startAfterSave 后端字段；拆分 WorkshopPage；状态、卡片、向导、日志、Browser 复用设计系统；Renderer 日志有界。

验收：无重复创建入口；深色/小窗/键盘/ARIA/中文长路径；i18n 同步；状态 action matrix。

### 批次 8：Freqtrade 安全验收

实施：不得硬编码 `freqtrade` 分支；以通用 command risk + Compose plan 识别。默认 compose command `trade` 必须阻断。只允许 Natives-owned 临时 override 的官方 `webserver` 或可证明的 dry-run；URL 以 inspect/health 得出，预期 `http://127.0.0.1:8080/`。

验收：不读取/输出配置 Secret；不修改用户项目；不启动真实交易；不删除 bind mount/volume；不影响原容器；Stop 后当前 project/port 清理。

### 批次 9：失败注入与 E2E

实施并通过 11 个场景：助理静态、助理 Vite、本地导入、Compose、Freqtrade、重复启动、忽略 TERM、Docker 部分失败、Daemon/Host crash、关闭 Preview、外置卷断开。

验收：每场景记录开始前/运行中/停止后的 PID/PGID、container/network、logs task、port、WebView、listener、temp、runtime row；禁止依靠测试结束后的全局 prune 兜底。

## 7. Freqtrade 专项规则

示例目录：`/Volumes/UNTITLED/本人材料/project/freqtrade`。

只读已知事实：根 Compose 的 `freqtrade` 服务使用 `freqtradeorg/freqtrade:stable`，bind mount `./user_data`，端口 `127.0.0.1:8080:8080`，默认 command 为 `trade --config ... --strategy SampleStrategy`。用户配置存在但审计未读取。

强制：

- 不修改该目录任何文件、策略、配置或权限。
- 不读取、输出或上传 config/env 内 Secret。
- 不直接执行该默认 Compose `up`。
- 若要验证，先用脱敏 fixture 完成自动测试；真实目录只在用户再次明确授权后，使用 Natives 自有临时 override 的 `webserver` 或已证明 dry-run。
- 不接管、停止、删除用户原有容器；project name 必须新且唯一。
- 任何 volume/image/network 删除默认禁止。

## 8. 精准验证与最少 Cargo Test

每批先跑最小相关检查，最终再按项目要求扩大；不要一上来跑 workspace 全量测试。

最低 Rust 验证：

```bash
rtk cargo fmt --check
rtk cargo check -p natives
rtk cargo test -p natives creative_app
rtk cargo test -p capability-gateway creative_draft
```

涉及 Protocol 时：

```bash
rtk npm run protocol:check
```

涉及 Renderer 时至少：

```bash
rtk npm run typecheck
rtk npm run lint
rtk npx tsx --test src/lib/creative-app.test.ts src/lib/local-creative.test.ts src/lib/creative-draft.test.ts src/components/creative/DraftPreview.test.ts
```

新增非平凡资源逻辑必须留下一个能失败的精准测试；不要为简单映射堆测试框架。Docker 测试只能用独立 project/fixture，禁止 prune。

## 9. 每批交付格式

每批完成后报告：

1. 变更文件与生产调用链前后差异。
2. 数据迁移与兼容/回滚。
3. 新增/删除的 RPC、Tool、Runtime resource。
4. 资源 Owner、CancellationToken、停止超时、强制清理和恢复证据。
5. 精准测试命令与结果。
6. 仍未完成项；不得把下一批能力提前标绿。

## 10. 绝对禁止

- 禁止 `docker system/container/volume/network prune`。
- 禁止 `docker compose down -v`，除非是自动测试 fixture 且用户明确要求；真实 Freqtrade 永不允许。
- 禁止修改/删除用户 Freqtrade 项目、配置、策略、交易数据。
- 禁止启动真实交易。
- 禁止 Agent 绕过 Capability Gateway 或直接写 Host SQLite。
- 禁止 Renderer 直接 spawn 生产进程或开 SQLite。
- 禁止 Host 直接调用 Provider 作为生产 AI 路径。
- 禁止创建第二套 Runtime、Catalog 或 Provider 配置。
- 禁止放宽 Workshop iframe sandbox、把 Bridge 给 Embed。
- 禁止 Stop 只改 UI/DB；资源未核实释放就不能 stopped。
- 禁止用按钮、类型、Migration、No-op、Fake、未调用函数冒充完成。
- 禁止用 Freqtrade 名称硬编码通用运行器。

从批次 0 开始。若当前未合并工作区会覆盖用户文件，停止该文件的修改并报告冲突，但继续完成不冲突的安全后端工作；不要 stash/reset/restore 用户改动。
