# Natives 整改执行 — Agent 任务简报

> **用途**: 把以下「---  开始复制  ---」到「---  结束复制  ---」之间的内容，整段粘贴给执行 agent。
> 计划细节在 `docs/superpowers/plans/2026-06-16-remediation-plan.md`，提示词只负责框架与纪律。

---

## ---  开始复制  ---

你是 **Natives** 项目的整改执行 agent。Natives 是一个 Electron + Next.js + SQLite 桌面应用（"AI Steam Base"），代码在 `/Users/ldh/Downloads/project/AiNative/Natives`。

# 你的任务
基于已写好的整改计划，修复审计发现的功能缺陷、假数据、UI 合规、i18n、错误处理等问题。整改分三轮（P0/P1/P2），全部完成。

# 强制前置阅读（动手前必须完成，否则会做错）
1. 读 `docs/standards/README.md` 和 `docs/standards/00-glossary.md` —— 理解 **MUST/SHOULD/MAY** 语义与规则四件套结构。这是你的合规判据。
2. 读整改计划**全文**：`docs/superpowers/plans/2026-06-16-remediation-plan.md` —— 每个 Round 的任务、file:line 锚点、代码改动建议、验收清单都在里面。
3. 开始某个任务前，按计划文末「关联源文件」加载对应规范篇（如改前端读 `frontend/`，改 UI 读 `ui-ux/`）。

# 最重要的纪律（违反即返工）

**① 严格按 Round 1 → Round 2 → Round 3 顺序，不可跳序、不可并行轮次。**
存在硬依赖：Round 2 的语义令牌（`--danger` 等）必须先于颜色迁移；`useAsyncData` hook 必须先于 EmptyState 落地。Round 1 的 `ConfirmDialog` 依赖 Round 2 令牌（计划附录 C 第 3 点给了临时方案）。

**② 每个任务动手前，先 Read 确认行号。**
计划里的 file:line 是审计时快照，代码会漂移。用 Read/Grep 重新定位，不要盲改。

**③ i18n 改动必须 zh.ts 和 en.ts 同时改，改完立刻跑键对等脚本。**
脚本在计划附录 A。任何只改一边的提交都不合格。

**④ 假数据红线（R-F2）：拿不准就显示空态，绝不编值。**
用户可见数值必须有真实来源。修假数据时，若后端暂无法提供真实值，优先「隐藏字段 + 空态」而非「先填个值」。

**⑤ 遇 MUST 冲突，停下询问，不要擅自破例。**
若某改动必然违反 standards 里的 MUST，停下来记录在计划附录 B「执行疑问」，向我确认后再继续。

# 特别提醒：Round 1 任务 1.1 不是小事
`src/main/session-scanner.ts` 的问题**不是「时间戳假」那么简单**——它的 `tool_use` 匹配逻辑根本匹配不到任何事件（真实日志里 tool_use 嵌在 `assistant.message.content[]`，顶层没有），导致 SessionReplay 的 `filesModified`/`fileTimestamps`/`skillsUsed` **永远是空数组，功能完全失效**。计划里给了完整重写代码，务必用真实 JSONL 日志（`~/.claude/projects/**/*.jsonl`）写测试验证修复有效，不要只改个时间戳就交差。

# 验证（每个 Round 完成后必须全绿才能进下一轮）
所有命令在项目根目录执行。本项目装了 RTK，**请给命令加 `rtk` 前缀**（省 token，如 `rtk npm run typecheck`）：

```bash
rtk npm run typecheck   # tsc --noEmit
rtk npm run lint        # next lint
rtk npm run test        # tsx --test src/**/*.test.ts
```

外加 i18n 键对等脚本（计划附录 A，改过 i18n 就跑）。

每个 Round 对应的验收清单在计划附录 C，逐条核对。

# 交付规范
- **每个 Round 一个 commit**（不要把三轮揉成一个巨 commit）。Round 1 内部任务较多，可按任务再细分 commit，但同一 Round 内推进。
- commit message 用 `fix:` / `refactor:` / `style:` / `i18n:` 等常规前缀，简述改了什么。例如：
  - `fix(session-scanner): parse tool_use from assistant.content, use real timestamps`
  - `i18n: sync en.ts dashboard keys, add recentlyOpened`
  - `refactor: replace confirm() with ConfirmDialog component`
- **不要 push，不要开 PR**，本地 commit 即可，我会审查。
- **不要改 `docs/standards/`**（规范是约束源，不是被改对象），除非我另行授权。

# 进度报告（每完成一个任务向我汇报一次）
用这个格式，让我能快速判断是否继续：
```
✅ 任务 X.Y 完成
- 改动文件: a.ts, b.tsx
- 关键决策: （如有偏离计划的地方，说明理由）
- 验证: typecheck ✓ / lint ✓ / test ✓（或失败原因）
- 下一步: 任务 X.Z
```

# 现在开始
先完成前置阅读，然后从 **Round 1 任务 1.1（session-scanner 修复）** 开始。任务 1.1 涉及功能正确性，务必先写/改测试再改实现。完成后按上面的格式向我汇报，等我确认或你确认验证全绿后继续任务 1.2。

如果前置阅读中发现计划某处与现状代码不符（行号/逻辑漂移），先指出来再动手。

## ---  结束复制  ---
