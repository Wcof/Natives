# Subagent B｜Design System / Widget Framework

负责 Dark Glow + Liquid Crystal、Design Primitives、Widget Definition/Renderer/Inspector/Adapter/DataBroker，以及现有/新增 Widget。

## 硬边界

- 不复制任何参考项目源码。
- 不越过 file ownership。
- 开发期间不要求跑测试；记录 deferred verification。
- 不为了方便建立第二套数据权威。
- 完成每个 Wave 后写 handoff。

## Task 顺序

### B-001｜盘点 V1 token 与冲突 visual standards
- 依赖：M-004
- 主要范围：src/app/styles/tokens.css,src/lib/design-tokens.ts,docs/standards/ui-ux
- 完成条件：列出保留/替换语义

### B-002｜定义 V2 semantic token contract（主题无关）
- 依赖：B-001
- 主要范围：src/lib/design-tokens.ts
- 完成条件：Surface/Glass/Border/Shadow/Chart/Motion 完整

### B-003｜实现 Dark Glow token map
- 依赖：B-002
- 主要范围：src/app/styles/tokens.css
- 完成条件：低对比深色、克制 glow

### B-004｜实现 Liquid Crystal 独立 light token map
- 依赖：B-002
- 主要范围：src/app/styles/tokens.css
- 完成条件：非 dark 简单反色

### B-005｜定义 reduced motion / reduced transparency fallback
- 依赖：B-003,B-004
- 主要范围：src/app/styles/tokens.css
- 完成条件：可访问性降级

### B-006｜实现 Surface primitive
- 依赖：B-002
- 主要范围：src/components/ui/design-system/Surface.tsx
- 完成条件：bare/surface/elevated 语义

### B-007｜实现 GlassSurface primitive
- 依赖：B-003,B-004
- 主要范围：src/components/ui/design-system/GlassSurface.tsx
- 完成条件：仅局部 blur，无 WebGL

### B-008｜实现 Panel / Elevated / Divider primitives
- 依赖：B-006
- 主要范围：src/components/ui/design-system/**
- 完成条件：统一层级

### B-009｜实现 MetricBlock
- 依赖：B-006
- 主要范围：src/components/ui/design-system/MetricBlock.tsx
- 完成条件：值/label/delta/empty 统一

### B-010｜实现 AnimatedMetric（自研）
- 依赖：B-009
- 主要范围：src/components/ui/design-system/AnimatedMetric.tsx
- 完成条件：Framer/CSS 实现，不复制 Midday

### B-011｜实现 ChartFrame + config + tooltip primitives
- 依赖：B-002
- 主要范围：src/components/ui/design-system/chart/**
- 完成条件：包现有 Recharts，主题来自 token

### B-012｜实现 Skeleton/Empty/Error primitives
- 依赖：B-006
- 主要范围：src/components/ui/design-system/**
- 完成条件：Widget 通用状态

### B-013｜定义 WidgetDefinition / Adapter / Inspector TS contracts
- 依赖：M-004
- 主要范围：src/lib/workspace/widgets/types.ts
- 完成条件：code-registered, no runtime/plugin

### B-014｜实现 Widget registry 与唯一 type 校验
- 依赖：B-013
- 主要范围：src/lib/workspace/widgets/registry.ts
- 完成条件：注册冲突失败显式

### B-015｜实现 config version + Zod validate/migrate pipeline
- 依赖：B-013
- 主要范围：src/lib/workspace/widgets/config.ts
- 完成条件：invalid config 可回退

### B-016｜实现 WorkspaceDataBroker query key/dedupe/cache
- 依赖：B-013
- 主要范围：src/lib/workspace/widgets/data-broker.ts
- 完成条件：同 key 共享请求/订阅

### B-017｜实现 WidgetRenderer 状态机 loading/error/ready
- 依赖：B-012,B-014,B-016
- 主要范围：src/components/workspace/widgets/WidgetRenderer.tsx
- 完成条件：Renderer 不直连 IPC

### B-018｜实现 WidgetShell surfacePolicy/header/edit chrome
- 依赖：B-006,B-007,B-017
- 主要范围：src/components/workspace/widgets/WidgetShell.tsx
- 完成条件：不默认 card/glass

### B-019｜实现 Widget drag handle hover chrome（不接管 RGL transform）
- 依赖：B-018
- 主要范围：src/components/workspace/widgets/**
- 完成条件：普通模式极简

### B-020｜实现 Inspector section schema/custom inspector bridge
- 依赖：B-013
- 主要范围：src/lib/workspace/widgets/inspector.ts
- 完成条件：供 C 的 host 消费

### B-021｜迁移 Greeting Widget 到新 Definition
- 依赖：B-014,B-018
- 主要范围：src/components/workspace/widgets/**
- 完成条件：支持 bare policy

### B-022｜迁移 Recent Files Widget + Files Adapter
- 依赖：B-016,B-018
- 主要范围：src/components/workspace/widgets/**
- 完成条件：仍复用 recent-files/files domain

### B-023｜迁移 App Launcher Widget + Apps Adapter
- 依赖：B-016,B-018
- 主要范围：src/components/workspace/widgets/**
- 完成条件：不复制 Apps domain

### B-024｜迁移 Today Usage Widget + Usage Adapter
- 依赖：B-010,B-016
- 主要范围：src/components/workspace/widgets/**
- 完成条件：同 Usage query 可共享

### B-025｜迁移 Token Metrics Widget
- 依赖：B-010,B-016
- 主要范围：src/components/workspace/widgets/**
- 完成条件：使用 Metric primitives

### B-026｜迁移 AI Status Widget + Provider/Proxy Adapter
- 依赖：B-016
- 主要范围：src/components/workspace/widgets/**
- 完成条件：DB change/event driven

### B-027｜迁移 Storage Overview Widget
- 依赖：B-016
- 主要范围：src/components/workspace/widgets/**
- 完成条件：复用现有 disk domain

### B-028｜为 7 个迁移 Widget 注册 config/sizes/surface policy
- 依赖：B-021,B-027
- 主要范围：src/lib/workspace/widgets/registry.ts
- 完成条件：registry 完整

### B-029｜输出 Wave1 handoff
- 依赖：B-028
- 主要范围：按任务语义
- 完成条件：列出 contract 假设与 shared patch

### B-030｜Wave2：实现 Cost / Token IO / Cache metrics widgets
- 依赖：I-008
- 主要范围：src/components/workspace/widgets/**
- 完成条件：复用 Usage aggregate

### B-031｜Wave2：实现 AI Work Time / Session widgets
- 依赖：B-030
- 主要范围：src/components/workspace/widgets/**
- 完成条件：无重复采集

### B-032｜Wave2：实现 Provider/Model/Project distribution charts
- 依赖：B-011,B-030
- 主要范围：src/components/workspace/widgets/**
- 完成条件：ChartFrame 统一

### B-033｜Wave2：实现 Tool Status / Proxy Status 拆分 widgets
- 依赖：B-026
- 主要范围：src/components/workspace/widgets/**
- 完成条件：外部工具状态只读

### B-034｜Wave2：实现 Notes/Prompt Snippets/Links 的基础 Widget visual/inspector
- 依赖：I-008
- 主要范围：src/components/workspace/widgets/**
- 完成条件：数据写入走 workspace context command

### B-035｜Wave2：完成 motion/hover/loading 统一
- 依赖：B-030,B-034
- 主要范围：src/components/ui/design-system,src/components/workspace/widgets
- 完成条件：不干扰 drag transform

### B-036｜Wave2 handoff
- 依赖：B-035
- 主要范围：按任务语义
- 完成条件：记录性能待测项

### B-D01｜Legacy visual cleanup：统一 prompt-context-injector V2 规则
- 依赖：I-012
- 主要范围：src/lib/prompt-context-injector.ts
- 完成条件：不再“一律禁止 glass”也不“一律 glass”

### B-D02｜统一 UI standards：Dark Glow/Liquid Crystal/Surface Policy
- 依赖：B-D01
- 主要范围：docs/standards/ui-ux/**
- 完成条件：唯一可执行视觉规范

### B-D03｜评估并删除未使用旧 WebGL LiquidGlass 组件
- 依赖：B-D01
- 主要范围：src/components/ui/LiquidGlass.tsx
- 完成条件：若无生产引用则删除

### B-D04｜提出 package.json 移除 liquid-glass-react 的 patch（仅在无引用时）
- 依赖：B-D03
- 主要范围：package.json
- 完成条件：Main 最终应用

### B-D05｜清理旧 Home widgets 迁移源（确认新 registry 已接管后）
- 依赖：B-036,I-012
- 主要范围：src/components/home/widgets/**
- 完成条件：无双 renderer

### B-D06｜Design/Widget death handoff
- 依赖：B-D05
- 主要范围：按任务语义
- 完成条件：无参考项目源码复制声明

## Handoff 模板

```text
Completed Task IDs:
Files created:
Files modified:
Contract assumptions:
Migration/compat impact:
Known risks:
Deferred verification:
Shared-file patch intents:
Reference-source-copy statement: NO SOURCE COPIED
```


# V2 追加｜Design System V2 / 全局视觉

### V-005｜建立 Design System V2 token taxonomy
- 依赖：B-002
- 范围：`src/lib/design-tokens.ts; src/app/styles/tokens.css`
- 完成：至少覆盖 canvas/surface/material/border/highlight/shadow/text/state/chart/motion

### V-006｜重构 theme-engine 为 V2 单一主题应用器
- 依赖：V-005
- 范围：`src/lib/theme-engine.ts`
- 完成：CSS 与 TS 不再维护两套互相漂移的颜色真值

### V-007｜重构 ThemeContext 并消除首帧主题闪变
- 依赖：V-006,V-002
- 范围：`src/context/ThemeContext.tsx; src/app/layout.tsx`
- 完成：SSR 初值、持久化、DOM data-theme 一致

### V-008｜定义 Dark Glow 背景与 surface 层级
- 依赖：V-005
- 范围：`src/app/styles/tokens.css`
- 完成：暗色不是纯黑墙；至少 canvas/base/raised/floating 四级可辨

### V-009｜定义 Dark Glow glow/focus/selected 语义
- 依赖：V-008
- 范围：`src/app/styles/tokens.css`
- 完成：glow 只用于焦点、选中、数据强调；不形成霓虹污染

### V-010｜定义 Liquid Crystal 独立浅色 palette
- 依赖：V-005
- 范围：`src/app/styles/tokens.css`
- 完成：非 dark 机械反色；背景为霜白/冷白层次

### V-011｜实现 Liquid Crystal 顶层 Specular Highlight token
- 依赖：V-010
- 范围：`src/app/styles/tokens.css`
- 完成：玻璃/卡片顶部存在可复用纯白高光线语义

### V-012｜实现 Liquid Crystal 多层微阴影体系
- 依赖：V-010
- 范围：`src/app/styles/tokens.css`
- 完成：card/popup/modal 使用多层扩散阴影，避免粗黑阴影

### V-013｜实现 Liquid Crystal Typography hierarchy
- 依赖：V-010
- 范围：`src/app/styles/tokens.css; src/lib/design-tokens.ts`
- 完成：正文深石墨灰，次级中性灰，避免大面积纯黑

### V-014｜实现双主题 border/edge/highlight 体系
- 依赖：V-008,V-010
- 范围：`src/app/styles/tokens.css`
- 完成：边界主要靠材质与高光，粗边框只保留必要控件

### V-015｜实现双主题 chart semantic tokens
- 依赖：V-008,V-010
- 范围：`src/app/styles/tokens.css`
- 完成：暗色可克制发光；浅色 area fill 约 15%→0%

### V-016｜实现双主题 elevation/shadow tokens
- 依赖：V-012
- 范围：`src/app/styles/tokens.css; src/lib/design-tokens.ts`
- 完成：card/popup/modal/dragging 四级 elevation 完整

### V-017｜实现 MaterialSurface primitive
- 依赖：V-005,V-014
- 范围：`src/components/ui/design-system/MaterialSurface.tsx`
- 完成：支持 base/raised/floating/inset，禁止调用者传 hex

### V-018｜实现 CrystalSurface primitive
- 依赖：V-011,V-012,V-015
- 范围：`src/components/ui/design-system/CrystalSurface.tsx`
- 完成：浅色具高光、透明折射感、微阴影；暗色退化为语义 surface

### V-019｜实现 GlowEdge primitive
- 依赖：V-009,V-015
- 范围：`src/components/ui/design-system/GlowEdge.tsx`
- 完成：仅 selected/focus/status 使用且强度受 token 控制

### V-020｜重写 Card/Panel preset 到 V2 surface policy
- 依赖：V-015,V-016
- 范围：`src/lib/design-tokens.ts; src/components/ui/design-system/**`
- 完成：移除 V1 固定 surface+1px border 假设

### V-021｜重写 Modal/Popover/Dialog overlay 材质
- 依赖：V-016
- 范围：`src/components/ui/Modal.tsx; ConfirmDialog.tsx; ShortcutHelp.tsx`
- 完成：暗色/浅色均有独立材质，overlay 不脏、不糊

### V-022｜重写 Input/Button/Select/Tab/Segmented states
- 依赖：V-014,V-018
- 范围：`src/app/styles/controls.css; src/components/ui/**`
- 完成：hover/focus/pressed/disabled/selected 双主题状态完整

### V-023｜重写 Toast/Skeleton/Empty/Error 状态
- 依赖：V-018
- 范围：`src/components/ui/Toast.tsx; Skeleton.tsx; EmptyState.tsx; ErrorBoundary.tsx`
- 完成：所有反馈态符合 V2 材质与文本层级

### V-024｜重写 scrollbar/selection/focus ring
- 依赖：V-014
- 范围：`src/app/styles/motion.css; shell.css; controls.css`
- 完成：系统级细节双主题一致且可访问

### V-025｜建立 V2 chart wrapper / area gradient helper
- 依赖：V-013
- 范围：`src/components/ui/design-system/**; src/components/dashboard/**`
- 完成：Recharts 不直接散落颜色；浅色 15%→0% 面积填充

### V-026｜迁移 Usage/Dashboard 图表到 V2 chart tokens
- 依赖：V-025
- 范围：`src/components/dashboard/**; src/app/usage/**; src/components/home/widgets/**`
- 完成：所有折线/面积/柱图不依赖旧 grayscale chart

### V-027｜重构 Shell 根背景与材质层级
- 依赖：V-008,V-010,V-015
- 范围：`src/components/shell/ShellLayout.tsx; src/app/styles/shell.css`
- 完成：App 背景、主内容、浮层建立统一空间层次

### V-028｜重构 Sidebar 为双主题材质并保留折叠能力
- 依赖：V-027
- 范围：`src/components/shell/Sidebar.tsx; sidebar/**; shell.css`
- 完成：暗色克制辉光、浅色晶透；折叠态视觉一致

### V-029｜重构 CommandPalette/Notification/RightPanel
- 依赖：V-017,V-027
- 范围：`src/components/shell/CommandPalette.tsx; NotificationPanel.tsx; RightPanel.tsx`
- 完成：浮层材质统一，层级与焦点正确

### V-030｜重构 Settings 主题选择预览与命名
- 依赖：V-006,V-010
- 范围：`src/components/settings/**; src/components/shell/SettingsPage.tsx; src/i18n/**`
- 完成：界面明确显示 暗黑流光 / 晶透液态；旧名仅迁移兼容

### V-031｜清理 legacy.css 中与 V2 冲突的纯色/无阴影覆盖
- 依赖：V-018,V-027
- 范围：`src/app/styles/legacy.css`
- 完成：legacy 只保留兼容选择器，不再覆盖 V2 材质

### V-032｜重写 hardcoded color lint allowlist
- 依赖：V-005
- 范围：`scripts/check-hardcoded-colors.mjs; docs/standards/ui-ux/**`
- 完成：除语义源/品牌/语言徽标/注释笔色等白名单外禁止硬编码

### V-033｜审计并处置 liquid-glass-react / LiquidGlass 旧实现
- 依赖：V-016
- 范围：`src/components/ui/LiquidGlass.tsx; package.json`
- 完成：无生产引用则删除依赖；有引用则迁移后删除

### V-034｜建立 Theme V2 component showcase 页/开发面板
- 依赖：V-015,V-018,V-025
- 范围：`src/components/settings/** or dev-only showcase`
- 完成：一次可检查所有 primitive 在 dark/light 的状态矩阵

### V-035｜建立材质性能预算与 reduced-transparency fallback
- 依赖：V-016,V-020
- 范围：`src/app/styles/**; docs/standards/ui-ux/**`
- 完成：低性能/减少透明度场景不依赖高成本全屏 blur

### V-036｜输出 Visual Foundation handoff
- 依赖：V-006,V-031
- 范围：`docs/development/visual-v2-handoff.md`
- 完成：记录 token、primitive、共享文件改动与剩余风险

