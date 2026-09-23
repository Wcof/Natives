# ExecutionPolicySnapshotV1 Contract

## 目标

把“设置页当前值”转成“本 Run 的不可变执行策略快照”。

## Shape

```json
{
  "version": 1,
  "runtimeId": "native",
  "runtimeSource": "application_default",
  "settingsRevision": 8,
  "maxSteps": 80,
  "disabledTools": ["run_terminal"],
  "fallbackUsed": false,
  "unavailablePolicy": "fail"
}
```

## Invariants

- `disabledTools` 只能收紧，不能扩权。
- `maxSteps` 必须在 daemon hard bounds 内。
- `runtimeId=codex_cli` 在 app-server 未完成前必须被 daemon reject。
- explicit/conversation external runtime unavailable 不允许 silent fallback。
- application default 才能按 `fallback_native` 降级。
- Run 创建后 snapshot 不随 Settings 修改。
- Resume/Retry 必须明确继承还是重新解析：
  - Resume：继承 source Run policy snapshot。
  - Retry：默认继承 source Run snapshot；用户显式“用当前设置重试”才重新解析。
  - Continue/Fork：默认继承 source snapshot，除非产品明确提供 override。
