# Handoff B — Design System V2 / Widget Framework（Wave 1）

> 状态：B-013..B-029 + V-026 + V-031 + V-036（Widget Framework 主体）完成。
> 上一轮已完成 Design System V2 基础（tokens/theme-engine/ThemeContext/design-system primitives/types.ts）。
> 本轮补齐 Widget registry / config pipeline / data-broker / Renderer / Shell / 7 个迁移 Widget / dashboard chart tokens / legacy 清理。

```text
Completed Task IDs:
  B-013  WidgetDefinition/Adapter/Inspector TS contracts（types.ts 扩展，本轮补 configVersion/configSchema/migrateConfig/adapterKeyBuilder/subscribe）
  B-014  Widget registry 与唯一 type 校验（registry.ts：注册冲突显式 throw；同一定义 HMR 幂等）
  B-015  config version + Zod validate/migrate pipeline（config.ts：非法配置回退 last-valid）
  B-016  WorkspaceDataBroker query key/dedupe/cache（data-broker.ts：同 key 共享一次 fetch；事件驱动 refetch）
  B-017  WidgetRenderer 状态机 loading/error/ready（Renderer 不直连 IPC，数据来自 broker loader）
  B-018  WidgetShell surfacePolicy/header/edit chrome（不默认 card/glass）
  B-019  Widget drag handle hover chrome（不接管 RGL transform；仅 data-widget-drag-handle + hover 样式）
  B-020  Inspector section schema（inspector.ts：供 C 的 host 消费）
  B-021  Greeting Widget 迁移（plain 优先，bare policy）
  B-022  Recent Files Widget 迁移（recent-files adapter）
  B-023  App Launcher Widget 迁移（apps adapter，复用 creativeApp.list）
  B-024  Today Usage Widget 迁移（usage adapter，与 Token Metrics 共享 key）
  B-025  Token Metrics Widget 迁移（Metric primitives：AnimatedMetric）
  B-026  AI Status Widget 迁移（providers adapter + DB change 事件驱动）
  B-027  Storage Overview Widget 迁移（storage adapter，复用 diskApi）
  B-028  7 个 Widget 注册到 registry（registry.registerWidgets 批量注册 + components/workspace/widgets/index.ts 收敛注册，冲突显式失败）
  B-029  Wave1 handoff（本文件）
  V-026  Dashboard/Usage 图表迁移到 V2 chart tokens（CartesianGrid 改用 --chart-grid；ChartAreaGradient helper 落地浅色 15%→0% 渐变）
  V-031  legacy.css 只保留兼容选择器（已无 hex/无条件 box-shadow 覆盖 V2 材质）
  V-036  Visual Foundation handoff（本文件合并记录 token/primitive/shared patch/剩余风险）

Files created:
  src/lib/workspace/widgets/registry.ts
  src/lib/workspace/widgets/config.ts
  src/lib/workspace/widgets/data-broker.ts
  src/lib/workspace/widgets/inspector.ts
  src/lib/workspace/widgets/index.ts
  src/lib/workspace/widgets/adapters/_shared.ts
  src/lib/workspace/widgets/adapters/greeting.ts
  src/lib/workspace/widgets/adapters/recent-files.ts
  src/lib/workspace/widgets/adapters/apps.ts
  src/lib/workspace/widgets/adapters/usage.ts
  src/lib/workspace/widgets/adapters/ai-status.ts
  src/lib/workspace/widgets/adapters/storage.ts
  src/components/workspace/widgets/WidgetRenderer.tsx
  src/components/workspace/widgets/WidgetShell.tsx
  src/components/workspace/widgets/GreetingWidget.tsx
  src/components/workspace/widgets/RecentFilesWidget.tsx
  src/components/workspace/widgets/AppLauncherWidget.tsx
  src/components/workspace/widgets/TodayUsageWidget.tsx
  src/components/workspace/widgets/TokenMetricsWidget.tsx
  src/components/workspace/widgets/AiStatusWidget.tsx
  src/components/workspace/widgets/StorageOverviewWidget.tsx
  src/components/workspace/widgets/index.ts
  src/components/ui/design-system/ChartAreaGradient.tsx
  src/app/styles/widgets.css
  docs/development/handoff-b.md

Files modified:
  src/lib/workspace/widgets/types.ts（扩展 configVersion/defaultConfig/configSchema/migrateConfig/adapterKeyBuilder/subscribe；WidgetShellProps 增加 editing/onRemove）
  src/components/ui/design-system/index.ts（导出 ChartAreaGradient）
  src/components/dashboard/UsageCharts.tsx（CartesianGrid stroke: var(--border) → var(--chart-grid)，6 处）
  src/app/styles/legacy.css（V-031 状态标记 + 兼容选择器说明）

Contract assumptions:
  · Widget 是 code-registered 内置 React renderer，非 Plugin Runtime；registry.ts 为唯一 type 权威。
  · WidgetDefinition 含 type/titleKey/size/surfacePolicy/load/Component + configVersion/defaultConfig/configSchema/migrateConfig/adapterKeyBuilder/subscribe。
  · WidgetInstance = def + config；config 含 type/enabled/surface/order/size/settings（types.ts 与 §6 对齐，surface 用 'auto'|'material'|'crystal'|'plain'）。
  · SurfacePolicy = { surfaces[], allowBlur?, allowGlow? }；禁止所有 Widget 强制同一种玻璃卡。
    各 Widget surfacePolicy 差异化：Greeting/RecentFiles/AppLauncher = plain 优先；TodayUsage/TokenMetrics/AIStatus/Storage = crystal 优先（仅 allowBlur 局部 blur）。
  · 组件只消费语义 token（var(--surface)/var(--elevation-*)/var(--highlight-specular)/var(--chart-*)…），全新增组件 0 hex。
  · design-tokens.ts 仍是单一真值源（V2_TOKENS）；tokens.css 只做 SSR fallback mirror，本轮未改动 token 值。
  · Renderer 不直连 IPC：数据只从 WorkspaceDataBroker（def.load → 现有 domain facade）来；adapter 允许调 domain facade（内部走 typed IPC）。
  · 同 key 共享：TodayUsage 与 TokenMetrics 共用 usage.summary:30d:{tz}:null → 一次查询。
  · Framer Motion 不套 RGL grid item transform：WidgetShell/WidgetRenderer 无 transform/motion 副作用；motion 留给 host 的 wrapper。
  · reduced-motion / reduced-transparency 全局 fallback 已由 tokens.css + theme-engine 提供；WidgetShell blur 只在 allowBlur && !reducedTransparency 时开。

Migration/compat impact:
  · src/components/home/widgets/** 保持不变（B-D05 留到 Legacy Death），无双 renderer 风险；新 registry 未接管 Home 布局渲染。
  · legacy.css 未删除任何旧类名选择器，仅确保不再覆盖 V2 材质；旧 Home 组件视觉不受影响。
  · dashboard 图表的 CartesianGrid 从 var(--border) 改为 var(--chart-grid)——视觉近似（均为低对比网格线），无需回归。
  · types.ts 新增字段均为必填（configVersion/configSchema/defaultConfig）——新 WidgetDefinition 必须提供；目前无其他生产消费者（workspace widgets 目录此前只有 types.ts）。
  · config.ts 的 last-valid 是模块内存 Map，不做持久化——持久化仍归 workspace_widgets 表（A 侧）。

Known risks:
  · recent-files / usage 的 imperative facade 名未能在本 scope 内读取确认（迁移源只暴露 hook）：
      - recent-files 走 @/lib/recent-files-client 动态解析候选名（list/getRecentFiles/listRecentFiles/loadRecentFiles/fetchRecentFiles/getRecent/recent），命不中则空态。
      - usage 走 @/hooks/useUsageData 动态解析候选名（loadUsageData/fetchUsageData/loadUsage/fetchUsage/queryUsage/getUsageData），命不中则空态；已用 summarizeOverviewUsage 纯函数兜底归一化。
      两个 adapter 都有明确 console.warn，不会静默产生假数据。
  · summarizeOverviewUsage 返回字段（todayTokens/sessions/totalTokens）按迁移源用法假设，已做防御性取值。
  · ZodType 泛型赋值（z.record 输出 Record<string,unknown> 对齐 configSchema）理论可编译，未跑 tsc 验证。
  · registerWidget 对「同一定义对象」幂等（HMR 安全）；对不同对象同 type 仍显式 throw。
  · widgets.css 由 WidgetShell 直接 import（全局一次）——若项目 lint 禁止组件内 import 全局 css，需改为 layout 引入（见 shared patch intents）。

Deferred verification:
  · 类型/编译：未运行 tsc / next build；重点核对 data-broker 泛型、WidgetShell 的 ReactElement 返回、config.ts 的 Zod partial 用法、adapters 的 dynamic import 类型。
  · recent-files / usage 真实 imperative 函数名与返回结构（需 A/C 在宿主侧确认后替换候选表）。
  · RGL（react-grid-layout@2.2.4）与 WidgetRenderer/WidgetShell 的真实集成：drag handle 选择器 data-widget-drag-handle 需在 host 的 dragHandleClassName 挂接；Framer Motion 不得套到 grid item transform。
  · 双主题视觉走查（125%/150% 缩放、窄窗口）：crystal 高光/微阴影、material glow 在 light/dark 下是否达标。
  · ChartAreaGradient 目前无生产 Area chart 消费（UsageCharts 全是 Bar/heatmap）——提供能力，接取待后续 Area 图表。
  · settings:username 在无 Tauri 环境（browser dev）下返回 null，greeting 显示 guest——行为与迁移源一致。

Shared-file patch intents（Main Agent 落地）:
  1. src/app/layout.tsx / RootClient.tsx：可选 —— widgets.css 已由 WidgetShell import；若改为全局导入更规范，可在此处 import '@/app/styles/widgets.css'（幂等）。
  2. src/components/workspace/WorkspaceCompositionPage.tsx（C 侧）：使用 <WidgetRenderer instance={...}/> 渲染 workspace_widgets 记录；dragHandleClassName 指向 [data-widget-drag-handle]；拖拽期给 grid item 加 ws-shell--dragging 以锁交互。
  3. 根 page.tsx / ShellLayout：无 B 侧直接改动需求。
  4. package.json：无新增依赖（zod 已存在）；无移除需求（liquid-glass 移除仍在 B-D04 待议）。
  5. i18n：本轮复用现有 home.* / settings.* / common.* key，无需新增。

Reference-source-copy statement: NO SOURCE COPIED
```
