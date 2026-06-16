# Liquid Glass 重构 — 任务总览

## 目标

将 Natives 项目从纯 CSS 变量体系迁移到 **Tailwind CSS + WebGL 液态玻璃设计系统**，支持深色/浅色双主题切换。

## 可直接复制的 Agent 提示词

| Agent | 提示词文件 | 可并行 |
|-------|-----------|--------|
| **A** — Tailwind 基础设施 | [agent-prompt-A.md](agent-prompt-A.md) | ✅ 立即 |
| **B** — 新组件 | [agent-prompt-B.md](agent-prompt-B.md) | ✅ 立即 |
| **C** — 集成层 | [agent-prompt-C.md](agent-prompt-C.md) | ⏳ 等 A+B 完成 |

## 详细任务文档

| 任务 | 文档 |
|------|------|
| A | [liquid-glass-task-A.md](liquid-glass-task-A.md) |
| B | [liquid-glass-task-B.md](liquid-glass-task-B.md) |
| C | [liquid-glass-task-C.md](liquid-glass-task-C.md) |
| 合并指南 | [liquid-glass-merge-guide.md](liquid-glass-merge-guide.md) |

## 文件清单

### 新建文件（5 个）
- `tailwind.config.ts` — Agent A
- `postcss.config.mjs` — Agent A
- `src/context/ThemeContext.tsx` — Agent B
- `src/components/ui/LiquidGlass.tsx` — Agent B
- `src/components/ui/Modal.tsx` — Agent B

### 重写文件（4 个）
- `src/app/globals.css` — Agent A
- `src/lib/theme-engine.ts` — Agent C
- `src/app/layout.tsx` — Agent C
- `src/components/shell/Sidebar.tsx` — Agent C

### 小改动文件（2 个）
- `src/components/shell/ShellLayout.tsx` — Agent C（改默认主题名）
- `package.json` — Agent A（加 devDependencies）
