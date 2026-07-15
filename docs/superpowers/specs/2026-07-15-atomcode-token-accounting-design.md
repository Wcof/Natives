# Atomcode Token 统计修复设计

## 问题

当前扫描器直接读取 Atomcode 的回合 JSONL。JSONL 的 `prompt` 仅保存回合最后一次模型请求的输入上下文，而 `/cost` 会累加回合内每次模型请求，导致个人主页严重少算。

## 设计

- 继续以 JSONL 作为回合索引，提供时间、会话、项目和时长。
- 同目录存在 `<session>.snapshot` 时，按 `session_id + turn_id` 汇总其中每条消息的 `meta.tokens`。
- 一个回合存在 Snapshot 明细时，用逐请求合计替换 JSONL 的压缩用量；不存在时回退 JSONL。
- `cached` 是 `prompt` 的子集，总 Token 仍为 `prompt + completion`。
- 不增加依赖、缓存层或新抽象；修改限制在 Atomcode 扫描器及其测试。

## 边界

Snapshot 会随上下文压缩删除旧消息，因此已经被压缩且只剩 JSONL 的旧回合仍可能少算。现有文件无法精确恢复这部分历史；本次采用可恢复数据中的最高精度，不伪造缺失值。

## 验收

- 多轮回合累加每次模型请求的输入、输出和缓存 Token。
- Snapshot 缺失的回合继续按 JSONL 统计。
- 时间范围、项目、会话时长及缓存不重复计入总 Token 的既有行为保持不变。
- Atomcode 单元测试与 Rust 检查通过。
