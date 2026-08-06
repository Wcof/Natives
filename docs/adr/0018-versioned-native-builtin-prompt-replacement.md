# 0018. Versioned Native Builtin Prompt Replacement

- **Status**: Accepted
- **Date**: 2026-07-28
- **Decision source**: Project owner approval in the Native Harness design review

## Context

Native Engine 拥有内建的系统级 Surface Prompt（如基础 Agent 行为准则、系统边界指令等）。在 Harness 之前的版本中，这些 Surface Prompt 是硬编码在代码或组件内部的默认文本。工程与高级用户需要能够根据项目或团队需求，对 Native 代码拥有的 Builtin Surface Prompt 进行版本化完全替换与还原，同时保证替换行为受控、版本可追溯、防静默漂移。

另一方面，能力库（Capability Hub）拥有的 Prompt（例如 Agent Profile、Expert Directives）仍然属于 Capability Hub 的权威边界，不能通过 Harness 直接被重写。

## Decision

1. **批准 Native 代码拥有的 Builtin Surface Prompt 可由 Harness 版本化完全替换**：
   - Blueprint Schema 升级至 Version 4，引入 `builtin_prompt_replacements: Vec<BuiltinPromptReplacementSpecV4>`。
   - 单个 `BuiltinPromptReplacementSpecV4` 包含 `surface_id`、`markdown` 与 `base_default_digest`。

2. **配额与漂移保护规则**：
   - 只能替换由 Native Engine 注册的合法 `surface_id`（如 `builtin:surface:default_system`）。
   - 单个 Replacement 的 Markdown 上限为 64 KiB；所有 Natives-owned Prompt 替换文本合计上限 256 KiB。
   - 替换生效时必须记录创建时的 `base_default_digest`（代码默认文本的 SHA-256）。若代码升级导致底层默认文本的 digest 改变，草稿校验或保存将提示 `harness_prompt_source_changed` 漂移告警，强制用户重新确认。
   - 删除某 Surface 的 replacement 记录即代表“恢复当前代码默认值”。

3. **权威隔离**：
   - Capability Hub 拥有的 Profile / Expert Prompt 保持只读投影，Harness 仅做摘要与顺序呈现，严禁篡改其内容。

## Consequences

- 允许用户在不修改引擎代码的前提下定制与版本化替换 Native 内置 Surface Prompt。
- 保证了旧版本 Blueprint (v1–v3) 和历史 Run 快照的向前兼容。
- 在应用升级更新底层默认 Prompt 时能够显式发现漂移并安全确认。
