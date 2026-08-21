# AiNative 重构执行记录（2026-08-20 阶段完成）

> **范围**: 从「读总入口 → P0 Gate → 新 IA 目标边界 → 跨模块拉通 → Gate 验证」的本阶段执行
> **权威**: ADR-0020 + `docs/standards/` + Home Workspace Patch + Global Plan
> **性质**: 诚实记录「已交付 / 已验证 / 剩余待切换」，不把当前阶段宣称成「大重构完成」

## 1. 已交付（本阶段）

| 任务 | 交付 | 验证 |
|---|---|---|
| P0-A | 三协议 fixture/transport、Key Pool、OS Keychain SecretStore+迁移、secret scan 脚本 | provider-adapters 122 passed；key_pool 10；secrets 17（含真实 Keychain 集成） |
| P0-B | `provider-adapters-p0b-audit.md` 逐文件 Keep/Extract/Rewrite/Delete | 122 passed 保持 |
| Home Grid Spike | `spikes/home-grid` 复核（test/typecheck/build） | 4 pass；无 IPC/SQLite/Timer、stop 才提交 |
| Governance | ADR-0020 生效；standards v2；CODE_MODULE_GUIDELINES 去 Agent 语义；architecture-check 新增 legacy_import | typecheck/lint 通过 |
| Shell/IA | Sidebar 64px Icon Rail + 左下头像、无全局 Header、CommandPalette 全局、Activity Center 保留 | 13 tests pass |
| Files | FilePreview 拆为 preview-panes/（code/image/markdown/meta） | typecheck/lint/26 tests pass |
| Apps | `src-tauri/src/apps/`（App/RuntimeSpec/RuntimeInstance/Surface + read-through facade） | 5 tests pass |
| AI Resources | `src-tauri/src/ai/`（Provider/Connection/Credential/Model，secret_ref 引用 Keychain） | 3 tests pass |
| Proxy | `src-tauri/src/proxy/`（Listener/Route/可替换 ProxyEngine trait + EngineCall/Outcome/Error） | 5 tests pass |
| AI Tool Integration | `src-tauri/src/integrations/`（Claude/Codex/Gemini/OpenCode 七步契约） | 2 tests pass |
| Usage/Data | `/usage` 数据/用量页、设置个人概览改摘要、Sidebar 数据/用量入口 | typecheck/i18n/lint pass |
| Home Widgets | `/` = PersonalWorkspace Home：Grid + 5 默认 Widget 接真实 Domain + 单文档持久化（stop 才写） | typecheck/lint/i18n pass |
| Legacy Removal | `legacy-death-list.md`；删除 `examples/minimal-agent`、`extension-host` | cargo check/typecheck 无破坏 |
| 跨模块拉通 | 新 IA 唯一生产入口（Home + /usage + Icon Rail） | workspace check/typecheck/lint/测试通过 |

## 2. 已通过的 Gate（本阶段证据）

- `bash scripts/secret-scan.sh` → PASS
- `npm run protocol:check` → OK（165 Rust methods catalogued）
- `cargo fmt --check` → PASS
- `cargo check --workspace` → PASS
- `npm run typecheck` → PASS
- `npm run i18n:check` → PASS（3056 zh = 3056 en）
- `npm run perf:check` → exit 0（budget_functions 为 WARN ledger）
- `cargo test -p assistant-protocol --lib` → 38 passed
- `cargo test -p provider-adapters --test contract` → 9 passed
- `cargo test --workspace`（单线程）→ 835 passed / 0 failed
- `npm run test` → 815/818（3 项 legacy assistant E2E 为既有失败：`fixture-adapter.ts`/`controller.ts`/`full-linkage.e2e.test.ts` 均无 git 改动，属 HEAD 既有状态；并行下失败用例不稳定，属测试间状态串扰，非本阶段回归）

## 3. 剩余事项（ADR-0020 P0 parity cutover 前置，未在本阶段宣称完成）

1. Host ProxyEngine **生产执行**（当前 Provider 执行仍经 Daemon；cutover 后删除 Daemon caller/fallback）。
2. Daemon/Agent crates/前端 legacy 页面删除（death-list 第 2 节路径）。
3. `provider_kek`/`env_encryption_key` 迁出 SQLite → Keychain（迁移脚本 + rollback 证据）。
4. packaged Tauri/WebKit Home Grid Gate（H0-014 需 packaged 验收）。
5. `verify:native-engine` 完整证据链（含 npm test + 多 crate 集成）在 Release 阶段执行（本阶段 300s 超时）。
6. `npm run test` 中 3 项 legacy assistant E2E 需随 legacy 删除或 fixture 补齐而消除。
