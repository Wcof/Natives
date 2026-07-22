# 助理 ↔ 执行引擎全链路联动状态

> **日期**: 2026-07-22  
> **分支**: `deploy`（工作树）  
> **Goal**: Native 执行引擎优先修复 — 供应商 SoT、运行生命周期硬终态、首次加载、工具事件与重连  
> **原则**: 诚实能力 — 广告 ⊆ 可调；无假绿。fixture 通过 = **automated-pass**；真桌面 smoke 通过后才可标 **done**

---

## 0. 判定拆分

| 层级 | 问题 | 状态 |
|------|------|------|
| **A. 主联动** | 前端主功能是否接到后端执行引擎？ | **automated-pass**（契约 + 单测 + 生命周期回归） |
| **B. Native 引擎优先修复** | 供应商 SoT / 终态 / 历史 / 冷启动 / 工具事件 | **automated-pass**（本轮改动） |
| **C. 桌面 headed smoke** | 真供应商 + `runtime=native` GUI 点测 | **pending**（未做） |
| **D. CLI 扩展** | Claude/Codex CLI | **out of scope**（本轮不扩展） |

---

## 1. 本轮修复摘要（2026-07-22）

### Agent A — Native 引擎 / 供应商 / 生命周期

| 项 | 状态 | 说明 |
|----|------|------|
| `provider.list` SoT | automated-pass | 优先读 `natives.db` `user_providers` + 活动 Key；镜像表仅 fallback 且过滤无 Key |
| 空 model_cache | automated-pass | 仅暴露真实 `default_model`，不生成假模型目录 |
| `run.start` 校验 | automated-pass | `provider_model_pair_available` 对齐 natives.db |
| detached `start()` 错误 | automated-pass | `fail_run_if_active` 幂等收口 + `Failed` 事件；spawn 不再 `let _ =` |
| 无凭证 | automated-pass | `NO_CREDENTIALS` 终态 + 脱敏错误 |
| daemon 本轮 user message | automated-pass | `ensure_run_for_start` 始终写入 daemon 自有 `trigger_message_id`；不复用 host message id |
| 引擎历史 | automated-pass | `AgentEngine`：history + 恰好一次当前 `user_content`（去重） |

### Agent B — 首次加载 / 失效供应商

| 项 | 状态 | 说明 |
|----|------|------|
| 根级并行加载 | automated-pass | `refreshNavigationFromHost`：project.list + conversation.list + active project |
| 不挂 Workbench 也有会话树 | automated-pass | Provider 冷启动即替换导航 |
| 未注册项目路径 | automated-pass | 进「未关联项目」，不造历史项目节点 |
| 失效供应商 | automated-pass | `resolveModelSelection` 对非空但已删除 provider 返回 `null`；发送拦截 |

### Agent C — 工具事件 / 重连

| 项 | 状态 | 说明 |
|----|------|------|
| 真实 wire fixture | automated-pass | `wire.test.ts` 覆盖扁平 Rust 形状 + host payload 包装 |
| `tool_call_delta` | automated-pass | reducer 按 id/index 合并同一工具卡 |
| `failed` 可见错误块 | automated-pass | live bubble 写入 `error` block |
| 删除客户端假终态 | automated-pass | 不再在 ~40s 后编造 `SUBSCRIBE_ENDED`；进入 recovering + 从 last sequence 重连 |

---

## 2. 验证命令与结果

```bash
# Backend
cargo test -p agent-core --lib -- --test-threads=1
cargo test -p natives-agent-daemon --lib fail_run_if_active -- --test-threads=1
cargo test -p natives-agent-daemon --lib ensure_run_for_start_with_run_id -- --test-threads=1
cargo test -p natives-agent-daemon --lib start_without_run_id_appends_trigger -- --test-threads=1
cargo test -p natives --lib provider_list -- --test-threads=1
cargo check -p natives-agent-daemon --lib
cargo check -p natives --lib

# Frontend
npx tsx --test \
  src/lib/assistant-protocol/wire.test.ts \
  src/lib/assistant-workspace/reducer.test.ts \
  src/lib/assistant-workspace/linkage-regression.test.ts \
  src/lib/provider-model-selection.test.ts \
  src/lib/assistant-workspace/controller.test.ts \
  src/lib/assistant-workspace/full-linkage.e2e.test.ts \
  src/lib/assistant-protocol/frontend-backend-contract.test.ts
```

| 检查 | 结果 |
|------|------|
| agent-core | 51 pass |
| fail_run_if_active / ensure_run_for_start / trigger append | pass |
| provider_list host tests | 2 pass |
| cargo check daemon + natives | ok |
| wire/reducer/linkage/selection | 50 pass |
| controller + full-linkage-e2e + contract | 22 pass |

---

## 3. 桌面验收（仍为 pending）

仅当以下 **真 GUI smoke** 通过后，才可将 B 层标为 **done**：

1. 启动后无需点击项目操作，立即显示真实项目与会话  
2. 当前供应商、`runtime=native` 短问答有文本；失败立即显示错误码/摘要  
3. 要求只读 `list_dir` 时依次看到工具请求/执行/结果与最终回答；切换会话与重启可回放  
4. 连续第二问：模型收到本轮问题且只出现一次  

---

## 4. 一句话

**自动化：Native 优先修复已 automated-pass（供应商 SoT、终态硬约束、daemon 本轮消息、冷启动、工具 delta、无客户端假终态）。真桌面 smoke 未做，不得标 done。**
