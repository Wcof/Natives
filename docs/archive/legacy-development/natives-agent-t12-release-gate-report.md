# T12 发布门禁报告（PASS/BLOCKED）

## 2026-09-08 子应用资源包整改补充

| 验收项 | 结果 | 本次证据 |
|---|---|---|
| Catalog v2 / Apps 协议 3 / data-resource 包 | **PASS** | `rtk npm run apps:check` |
| 真实 Native Messaging 安装、分段读取、卸载保留数据、明确清除数据 | **PASS** | `rtk npm run apps:integration`；Demo 2.0.0，2 个资源包，读取内容与版本均断言 |
| Rust 迁移、崩溃恢复、跨应用/符号链接/版本防护 | **PASS** | `rtk env -u CARGO_TARGET_DIR cargo test --workspace`；133 tests passed |
| Extension、i18n、资源 Demo、Bing 背景回归 | **PASS** | `rtk npm run extension:check` |
| 性能 | **PASS** | 最终 `rtk npm run perf:check` 全绿；model-host 快照 692.21 ms / 1500 ms |
| Model Host | **PASS** | `rtk go test ./...`；64 tests passed |
| 候选产物与发布 dry-run | **PASS** | `dist/app-release/`；2 个不可变 `.nap`、签名 `catalog-v2.json`；`rtk node scripts/apps/publish-release.mjs` 验证通过 |
| 真实 Chrome/macOS UI：Demo + 第二应用、更新、离线重开 | **BLOCKED** | 本机 Chrome 存在，但 `apps:chrome-native` 缺少 Playwright；受控浏览器安全策略禁止打开 `chrome-extension://`，未绕过 |
| 正式发布 | **NOT RUN** | 本次未获上传/发布授权；只执行 dry-run，不得声明已发布 |

**本次发布判定仍为 BLOCKED。** 自动门禁和真实 Native Messaging 链路已完成；真实 Chrome UI 矩阵及正式发布未完成，不以绿色状态或旧记录替代。

> 日期：2026-08-07
> 分支：`codex/integration-20260806`
> 集成分支 HEAD：`98d4456a`（`fix(t12): green release gates — clippy clean + hermetic daemon/web_search/provider tests`）
> 权威输入：`tesk/00-整改实施总方案.md` + `tesk/12-全量集成发布与文档治理.md`。本报告是 T12 的最终交付物，其余一次性 task pack 已按治理规则移出权威 docs。

## 判定

| 类别 | 结果 |
|---|---|
| 自动门禁（总方案第 9 节 11 项） | **PASS**（全部绿色，见下表） |
| 真实环境矩阵（Docker / Tauri GUI / WebKit / Provider / crash matrix / 打包） | **BLOCKED**（本机无 docker executable、无 Tauri headed GUI 环境；须在具备环境机器执行，见下文清单） |

**发布判定：BLOCKED = 不可发布。** 自动门禁全绿是必要非充分条件；真实环境矩阵未执行完毕前，按 T12 规则不得宣称完成或发布。

## 自动门禁证据（串行执行，同一 HEAD）

| 命令 | 退出码 | 结果 |
|---|---|---|
| `rtk npm run typecheck` | 0 | PASS |
| `rtk npm run lint` | 0 | PASS（i18n 2519/2519 同步；hardcoded colors 无新增违规） |
| `rtk npm run test` | 0 | PASS（frontend 795 tests） |
| `rtk npm run perf:check` | 0 | PASS（/page 217.6KB、/modules 206.1KB、/files 237.4KB，均 < 350KB 预算） |
| `rtk npm run protocol:check` | 0 | PASS（155 methods，TS 与协议同步） |
| `rtk npm run verify:native-engine` | 0 | PASS（AUDIT RESULT=PASS；daemon lib 440 tests、engine 201、task_store 440 等） |
| `rtk cargo fmt --check` | 0 | PASS |
| `rtk cargo clippy --workspace --all-targets -- -D warnings` | 0 | PASS（No issues found） |
| `rtk cargo test --workspace` | 0 | PASS（1838 passed / 15 ignored，44 suites，连续 2 次全绿） |
| `rtk npm --prefix extension-host run typecheck` | 0 | PASS |
| `rtk npm --prefix extension-host run test` | 0 | PASS（16 tests） |

## 本提交修复的发布门（自 T12 接手时即红）

T12 接手时 `cargo clippy` 6 个警告、`cargo test --workspace` 4+ 失败、多条 flaky。`98d4456a` 做了根因修复（14 文件，+208/−104）：

1. **Provider rate-limit 测试**：`execute_provider_test` 建 HTTP client 时经 `global_proxy_for_daemon → get_main_conn()`，纯 `#[test]` 下 DB pool 未初始化 → 请求从未发出（2e1f1c03 引入的回归）。`global_proxy_for_daemon` 现把 DB 不可用视为「无代理直连」，恢复回归前行为。
2. **Claude usage 测试**：3 个测试用硬编码 14 天窗口，真实会话文件过期后 `in_range=0`；改为从文件自身时间戳推导窗口 / 从 epoch 扫描，日期无关；`claude_env_lock` 抗 Mutex 中毒。
3. **web_search 测试**：`SearchBackend::new` 的 SSRF 校验做真实 DNS 解析（环境 flaky）；测试改用公网 IP 字面量端点（免 DNS、SSRF 仍生效），并串行化 3 个触碰进程级 backend 的测试。
4. **Daemon 并行 hermetic**：`RunManager::new()`（cfg(test)）读进程级 env，并行测试泄漏互相污染 → `new_with_store` fail-closed panic。`store_from_env` 现默认内存态（测试须显式 `set_test_db_override` 才用 store）。`cancel_mid_run` 获得每测试临时 store + tool-started 信号（引擎在已取消 token 下会于循环顶部直接 abort）；tool_grant 测试各自隔离 store；熔断测试指向临时 DB；`with_ledger_db` 清理 override；`rpc_dispatch_contract` 集成测试串行化共享 DB。
5. **Clippy**：creative_app 模块 6 lint（manual_inspect / useless_format / large_enum_variant(Box) / double_ended_iterator_last / needless_borrow / needless_lifetimes）。

## BLOCKED 项（须在具备 Docker + Tauri GUI + WebKit + 可控 Provider fixture 的机器串行执行）

| 场景 | 当前状态 | 必做检查 |
|---|---|---|
| Docker Compose/Run 隔离 | BLOCKED（无 docker executable） | compose up、部分失败、stop 后容器/网络/端口清零；禁止 prune 兜底 |
| Tauri headed 多 app/多窗口 | BLOCKED | minimize/restore/close、Host crash、WebView close failure |
| WebKit Profile/Cookie/OAuth/下载/上传/剪贴板/新窗口策略 | BLOCKED | 不支持的能力必须删除广告并显示限制 |
| Python/Binary Host 可信执行 | BLOCKED | Host 重算 hash、换文件重新授权、解释器身份、symlink/TOCTOU、TERM→KILL→wait |
| Agent proposal 全链（普通 Assistant→durable→Host 校验→批准→注册→启动→health→window→stop） | BLOCKED | 需真实 Host + GUI 链路 |
| Runtime crash matrix（Event/ledger/checkpoint 关键提交点 kill） | 单测覆盖部分 | 需真实进程 kill 矩阵；恢复结果只能 Safe/Confirm/Blocked |
| 历史 DB v8→当前 / 损坏 / 重复 active / backfill repair 迁移 | 单测绿色（db.rs repair/backfill 测试在 1838 内） | 需真实历史 DB fixture 验收 |
| Release `.app/.dmg` smoke | BLOCKED | 干净用户目录、升级与卸载不删用户项目、深浅主题/小窗/键盘/ARIA/中文长路径/reduced-motion |

## 未完成项（非自动门禁）

- 真实环境矩阵全部场景（上表）在具备环境机器执行并回收 PASS/FAIL。
- 最终发布由用户按 BLOCKED 判定自行决策；未授权不 push、不建 PR。

## 文档治理（已完成）

- `docs/audit/`、`docs/plans/`、`docs/roadmap/` 一次性 task pack 已删除（30 文件、7384 行，commit `1be258b1`）。
- ADR-0017 编号冲突已重编号（`0018-versioned-native-builtin-prompt-replacement.md`）并修引用（`1be258b1`）。
- Extension capability flag 诚实化（`extensions:true` + audit 对照 `IMPLEMENTED_METHODS`，`2f1608ae`）。
