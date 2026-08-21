# Refactor Tracker — AiNative 全量重构

依据 `/Users/ldh/Downloads/AiNative-Integrated-Rearchitecture-2026-08-20` 统一方案执行。

任务状态只允许：
- `PASS` — 完成且有源码/测试证据
- `SUPERSEDED_WITH_EVIDENCE` — 已被后续架构/代码取代，附证据
- `CANCELLED_WITH_ARCHITECTURE_EVIDENCE` — 因架构决策取消，附证据
- `BLOCKED_EXTERNAL` — 外部不可控，附证据与解除条件

禁止 `PARTIAL / TODO / DEFERRED / NEXT_PHASE / 留待Release`。

## 权威优先级
1. 真实源码 + 已生效且与新基线一致的 Standards/ADR
2. home-workspace-patch（Home/Workspace/Widget/Navigation/Sidebar/首页 Usage）
3. global-plan
4. 历史 README/旧 Plan

## 目录
- `master-checklist.md` — 唯一 Master Checklist（所有 Global 175 + Home 150 任务）
- `wave0-audit/` — Wave 0 审计结果（来自审计 Subagent）
- `waves/wN/` — 各 Wave 执行记录与证据