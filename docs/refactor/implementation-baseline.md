# Natives 架构整改实施基线报告（Implementation Baseline）

- 记录日期：2026-09-28
- 执行环境：macOS darwin arm64
- 基线 Commit：`0a6d46584f923dd9a25df9bf0501755a2afc4e85`
- 当前分支：`deploy`
- Git 工作区状态：Clean（无未暂存/未跟踪改动）

---

## 1. 运行环境与工具链

| 工具 | 版本 | 验证命令 | 状态 |
|---|---|---|---|
| Node.js | v22.23.2 | `node --version` | 正常 |
| npm | 10.9.8 | `npm --version` | 正常 |
| Rustc | 1.96.0 (ac68faa20 2026-05-25) | `rustc --version` | 正常 |
| Cargo | 1.96.0 (30a34c682 2026-05-25) | `cargo --version` | 正常 |
| Go | go1.26.6 darwin/arm64 | `go version` | 正常 |
| RTK CLI | /opt/homebrew/bin/rtk | `which rtk` | 正常 |

---

## 2. 基线测试套件执行结果

在修改任何业务与架构代码前，全量运行已有测试套件，确认基线 100% 绿灯：

1. **Active Standards Check**：
   - 命令：`npm run standards:check`
   - 结果：PASS
2. **Extension Check**：
   - 命令：`npm run extension:check`
   - 结果：PASS（161 个 JS 语法检查、_locales 检查、Screenshot、Space、AI Performance、Lifecycle 均通过）
3. **App Architecture Check**：
   - 命令：`npm run apps:check`
   - 结果：PASS（manifest contract、app projection、registry equality、architecture death、port registry 8765/8766、app security 全部通过）
4. **Go Model Host Tests**：
   - 命令：`cd model-host && go test ./...`
   - 结果：PASS（158 tests passed，耗时约 12s）
5. **Rust Workspace Tests**：
   - 命令：`rtk env -u CARGO_TARGET_DIR cargo test --workspace`
   - 结果：PASS（213 tests passed, 2 ignored network tests，耗时约 16s）

---

## 3. 架构审查结论源码复核事实

对审查报告中指出的核心问题进行代码现场勘验，确认事实无误：

1. **A-01（TokenUsage 跨域读库与文件探测）**：
   - `modules/tokenusage/src/collector/host_sync.rs` 存在 `resolve_usage_db_path()`，直接探测 `$HOME/.natives/usage.db` 并以 SQLite 打开只读同步。
   - `modules/tokenusage/src/limits/mod.rs` 存在 `resolve_model_host_state_path()`，直接探测 `~/Library/Application Support/Natives/model-host/state.json` 与 `~/.config/natives/model-host/state.json`。
   - `modules/tokenusage/src/collector/pricing.rs` 维护了一套独立模型定价与成本重算表。
2. **A-02（Runtime 请求持有全局 Mutex 阻断 2s 退出 SLA）**：
   - `crates/app-runtime/src/http/client.rs:93` 在 `handle_api` 时一直持有 `module.lock()`。
   - `crates/app-runtime/src/controller/stopping.rs:29` 的 `enter_stopping()` 同样必须获取 `module.lock()`。
   - Fund 网络请求 timeout 可达 10s，若在请求期间发生 EOF，shutdown 过程会被锁阻塞，无法保证 ≤2s 退出 SLA。
3. **A-03（8765 固定端口与多模块进程模型冲突）**：
   - `crates/app-runtime/src/http/mod.rs:95` 默认尝试绑定 8765，只有被占用时才回退动态端口。
   - `crates/app-runtime/src/http/client.rs:62-81` 对 `/api/tray/state` 和 `/api/limits` 存在无 Bearer、无 sandbox Origin 的豁免。
4. **A-04（present 状态语义缺陷）**：
   - `crates/native-file-host/src/app_product.rs:80` 依然通过 `apps/<appId>/runtime/` 目录存在与否来判断 `present`。
   - 而 `crates/native-file-host/src/app_store/product.rs:603` 新架构配置成功后会主动 `remove_dir_all("runtime")`，导致状态矛盾。
5. **A-05（多模块产品激活非单事务）**：
   - `crates/native-file-host/src/app_store/product.rs:595` 先提交 SQLite 事务，再逐个 app 写 `activation.json`，出现错误只回滚 product meta，未能回滚 app rows。
6. **A-06（Extension 业务事件泄漏）**：
   - `extension/app.js:416` 与 `extension/background.js:33` 明确硬编码了 `invest:alert`，解析 `symbol`、`changePercent`、`isUp` 等投资业务字段。
7. **A-07（Core 控制面残留 Managed App 概念）**：
   - `crates/native-file-host/src/app_store/types.rs` 仍包含 `KIND_MANAGED_LOCAL`、`runtime_spec_json`、`host_registered` 等旧模型字段。
8. **A-11（UI 资源状态）**：
   - 现场核查确认：源码仓库中 `modules/fund/ui/dist` 与 `modules/tokenusage/ui/dist` 文件完整存在。需要建立源码 `ui/src` 到 `ui/dist` 的规范构建流程与 CI 校验门禁。

---

## 4. 实施偏差记录（Deviations Pre-registration）

- **ADR 编号顺延**：审查包设计了 `adr-drafts/0034-retire-fixed-app-runtime-port.md`。由于仓库中已有 `docs/adr/0034-google-public-oauth-client-exemption.md`（已接受状态），退役 8765 固定端口的 ADR 顺延编号为 **ADR-0035**（`docs/adr/0035-retire-fixed-app-runtime-port.md`），标题与决策内容保持完全一致。
