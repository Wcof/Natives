# Atomcode Token Accounting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让个人主页按 Atomcode 每次模型请求统计 Token，并保留旧 JSONL 数据的回退能力。

**Architecture:** 只修改现有 `atomcode.rs` 扫描器。先从相邻 Snapshot 按 `session_id + turn_id` 汇总逐请求 Token，再遍历 JSONL 获取时间、项目和时长；Snapshot 有该回合时替换 JSONL 用量，否则沿用 JSONL。

**Tech Stack:** Rust、Serde、现有 WalkDir 与 Rust 单元测试

---

### Task 1: 锁定多轮回合统计行为

**Files:**
- Modify: `src-tauri/src/usage/atomcode.rs`
- Test: `src-tauri/src/usage/atomcode.rs`

- [ ] **Step 1: 写失败测试**

在测试模块新增一个临时会话：JSONL 的回合用量为 `100/20/40`，Snapshot 同一回合包含两条模型响应 `100/10/40` 与 `150/20/100`。调用 `scan_session_logs` 后断言输入为 `250`、输出为 `30`、缓存为 `140`、总量为 `280`。

- [ ] **Step 2: 验证测试因仍读取 JSONL 而失败**

运行：

```bash
rtk cargo test --manifest-path src-tauri/Cargo.toml usage::atomcode::tests::uses_snapshot_request_usage_for_multi_round_turn -- --exact
```

预期：FAIL，实际输入仍为 `100`。

### Task 2: 实现 Snapshot 优先、JSONL 回退

**Files:**
- Modify: `src-tauri/src/usage/atomcode.rs`

- [ ] **Step 1: 增加最小反序列化结构**

为 `TurnRecord` 增加 `turn_id`。增加只包含 `messages`、`meta`、`tokens`、`session_id` 和 `turn_id` 的 Snapshot 反序列化结构；`TokenUsage` 派生 `Default` 与 `Clone`。

- [ ] **Step 2: 汇总相邻 Snapshot**

增加一个私有函数读取 `path.with_extension("snapshot")`，遍历带 Token meta 的消息，以 `(session_id, turn_id)` 为键累加 `prompt`、`completion`、`cached`。文件不存在或解析失败时返回空映射。

- [ ] **Step 3: 替换回合用量**

扫描每个 JSONL 会话时读取一次 Snapshot 映射。构造 `Turn` 时使用：

```rust
let usage = snapshot_usage
    .get(&(record.session_id.clone(), record.turn_id))
    .unwrap_or(&record.usage);
```

随后从 `usage` 写入输入、输出和缓存字段，不改变日期过滤、项目解析或时长逻辑。

- [ ] **Step 4: 验证新增测试转绿并保留回退测试**

运行：

```bash
rtk cargo test --manifest-path src-tauri/Cargo.toml usage::atomcode::tests -- --nocapture
```

预期：Atomcode 测试全部 PASS；原有无 Snapshot 测试证明 JSONL 回退仍有效。

### Task 3: 整体验证与提交

**Files:**
- Modify: `src-tauri/src/usage/atomcode.rs`

- [ ] **Step 1: 格式化并检查差异**

```bash
rtk cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
rtk proxy git diff --check
```

预期：均成功且无格式错误。

- [ ] **Step 2: 运行用量模块测试与 Rust 检查**

```bash
rtk cargo test --manifest-path src-tauri/Cargo.toml usage::
rtk cargo check --manifest-path src-tauri/Cargo.toml
```

预期：测试通过；`cargo check` 无错误。

- [ ] **Step 3: 提交代码但不推送**

```bash
rtk git add src-tauri/src/usage/atomcode.rs docs/superpowers/plans/2026-07-15-atomcode-token-accounting.md
rtk git commit -m "修复 Atomcode 多轮请求用量统计"
```
