# Subagent C｜Workspace UX / Layout / Views

负责 Multi-Workspace UX、Compact Grid、Free Canvas、Inspector Host、List/Table/Board/Calendar，以及 Shell 迁移 patch。

## 硬边界

- 不复制任何参考项目源码。
- 不越过 file ownership。
- 开发期间不要求跑测试；记录 deferred verification。
- 不为了方便建立第二套数据权威。
- 完成每个 Wave 后写 handoff。

## Task 顺序

### C-001｜创建 WorkspaceCompositionPage 骨架
- 依赖：M-004
- 主要范围：src/components/workspace/WorkspaceCompositionPage.tsx
- 完成条件：只依赖冻结 snapshot contract

### C-002｜实现 WorkspaceSessionProvider/store（前端 active/open metadata）
- 依赖：C-001
- 主要范围：src/components/workspace/session/**
- 完成条件：inactive 仅 metadata/snapshot cache

### C-003｜实现 WorkspaceTabStrip：render/open/active
- 依赖：C-002
- 主要范围：src/components/workspace/tabs/**
- 完成条件：类似浏览器 tab，不占用 domain 数据

### C-004｜实现 tab close/reopen/pin/reorder UX
- 依赖：C-003
- 主要范围：src/components/workspace/tabs/**
- 完成条件：Close!=Delete

### C-005｜实现 create/rename/delete/duplicate UI flows
- 依赖：C-003
- 主要范围：src/components/workspace/tabs/**
- 完成条件：删除有显式确认

### C-006｜实现 switch 时 snapshot-first 无白屏策略
- 依赖：C-002
- 主要范围：src/components/workspace/session/**
- 完成条件：先 cache 后 reconcile

### C-007｜抽取 CompactGrid 组件自旧 Home
- 依赖：C-001
- 主要范围：src/components/workspace/layout/compact/**
- 完成条件：继续 react-grid-layout

### C-008｜保留并泛化 lg/md/sm 12/8/4 断点
- 依赖：C-007
- 主要范围：src/components/workspace/layout/compact/**
- 完成条件：兼容旧布局迁移

### C-009｜实现 edit mode/drag handle/resize/lock/hide wiring
- 依赖：C-007
- 主要范围：src/components/workspace/layout/compact/**
- 完成条件：RGL 独占 transform

### C-010｜实现 compact stop -> in-memory commit -> client persist
- 依赖：C-007
- 主要范围：src/components/workspace/layout/compact/**
- 完成条件：不 per-pointer IPC

### C-011｜实现 compact keyboard move/resize command layer
- 依赖：C-009
- 主要范围：src/components/workspace/layout/compact/**
- 完成条件：可访问性键盘操作

### C-012｜定义 FreeCanvas 类型与 world/screen geometry
- 依赖：M-004
- 主要范围：src/lib/workspace/canvas/types.ts,geometry.ts
- 完成条件：纯函数、无 React 依赖

### C-013｜实现 camera pan/zoom model
- 依赖：C-012
- 主要范围：src/lib/workspace/canvas/camera.ts
- 完成条件：zoom clamp/anchor pointer

### C-014｜实现 CanvasViewport DOM transform
- 依赖：C-013
- 主要范围：src/components/workspace/layout/free-canvas/**
- 完成条件：Widget DOM 保持交互性

### C-015｜实现 CanvasItem drag
- 依赖：C-014
- 主要范围：src/components/workspace/layout/free-canvas/**
- 完成条件：pointer move 只改 memory

### C-016｜实现 CanvasItem resize
- 依赖：C-015
- 主要范围：src/components/workspace/layout/free-canvas/**
- 完成条件：最小/最大尺寸遵循 Widget Definition

### C-017｜实现 selection model / click / marquee
- 依赖：C-012,C-014
- 主要范围：src/lib/workspace/canvas/selection.ts
- 完成条件：单选/多选稳定

### C-018｜实现 SelectionOverlay
- 依赖：C-017
- 主要范围：src/components/workspace/layout/free-canvas/**
- 完成条件：不阻断 Widget 内部交互

### C-019｜实现 snap/alignment engine
- 依赖：C-012
- 主要范围：src/lib/workspace/canvas/snap.ts
- 完成条件：基于 geometry 纯逻辑

### C-020｜实现 SnapGuides visual
- 依赖：C-019
- 主要范围：src/components/workspace/layout/free-canvas/**
- 完成条件：仅交互时显示

### C-021｜实现 free layout stop/debounce persist
- 依赖：C-015,C-016
- 主要范围：src/components/workspace/layout/free-canvas/**
- 完成条件：一次交互一次 commit

### C-022｜实现 Compact/Free 模式切换 UI
- 依赖：C-007,C-014
- 主要范围：src/components/workspace/layout/**
- 完成条件：两套 layout 均持久保留

### C-023｜泛化 ResizableRightPanel 为 Workspace Inspector Host 容器
- 依赖：M-004
- 主要范围：src/components/ui/ResizableRightPanel.tsx,src/components/workspace/inspector/**
- 完成条件：复用现有 panel 不复制 Twenty

### C-024｜实现 Widget selection -> Inspector open/close
- 依赖：C-023
- 主要范围：src/components/workspace/inspector/**
- 完成条件：非编辑模式规则明确

### C-025｜实现 Content/Data/Appearance/Layout section host
- 依赖：C-023
- 主要范围：src/components/workspace/inspector/**
- 完成条件：sections 来自 Definition

### C-026｜实现 Inspector optimistic preview + commit adapter
- 依赖：C-025
- 主要范围：src/components/workspace/inspector/**
- 完成条件：slider pointerup/文本 debounce

### C-027｜定义 RecordSetAdapter/DataViewState contract shell
- 依赖：M-004
- 主要范围：src/lib/workspace/views/**
- 完成条件：仅受控 domain adapter

### C-028｜实现 List/Table view primitive
- 依赖：C-027
- 主要范围：src/components/workspace/views/**
- 完成条件：filter/sort state 外置

### C-029｜实现 Board view primitive + DnD state update contract
- 依赖：C-027
- 主要范围：src/components/workspace/views/**
- 完成条件：不复制 Plane PM board

### C-030｜实现 Calendar view primitive
- 依赖：C-027
- 主要范围：src/components/workspace/views/**
- 完成条件：仅允许注册 date fields

### C-031｜实现 view type/filter/sort/group/hidden field state persistence client wiring
- 依赖：C-028,C-029,C-030
- 主要范围：src/lib/workspace/views/**
- 完成条件：写 workspace_view_states

### C-032｜输出 Wave1 handoff + shell patch intent
- 依赖：C-031
- 主要范围：按任务语义
- 完成条件：Main 可接线

### C-033｜Wave2：实现 Frame model/visual
- 依赖：I-008
- 主要范围：src/lib/workspace/canvas/frames.ts,src/components/workspace/layout/free-canvas/**
- 完成条件：逻辑容器，不是绘图 frame engine

### C-034｜Wave2：实现 Group translate/ungroup
- 依赖：C-033
- 主要范围：src/lib/workspace/canvas/grouping.ts
- 完成条件：只做组合移动/选择

### C-035｜Wave2：实现 z-order/bring forward/send backward
- 依赖：C-033
- 主要范围：src/lib/workspace/canvas/**
- 完成条件：layout zIndex 持久化

### C-036｜Wave2：实现 canvas keyboard nudge/delete/select-all
- 依赖：C-017,C-035
- 主要范围：src/components/workspace/layout/free-canvas/**
- 完成条件：不与输入框快捷键冲突

### C-037｜Wave2：Workspace picker/command palette integration patch intent
- 依赖：C-004
- 主要范围：按任务语义
- 完成条件：交 Main 处理 shared command palette

### C-038｜Wave2 handoff
- 依赖：C-037
- 主要范围：按任务语义
- 完成条件：记录 packaged WebKit 待测点

### C-D01｜Legacy Shell cleanup：提出 RootClient 移除 AssistantWorkspaceProvider patch
- 依赖：I-012
- 主要范围：src/app/RootClient.tsx
- 完成条件：Main 应用

### C-D02｜提出 MainContent assistant/jobs/capabilities legacy alias 清理 patch
- 依赖：I-012
- 主要范围：src/components/shell/MainContent.tsx
- 完成条件：只保留新 IA

### C-D03｜提出 Sidebar/Header/Settings legacy route 清理 patch
- 依赖：C-D02
- 主要范围：src/components/shell/**
- 完成条件：无 Harness/Execution Engine 正式入口

### C-D04｜替换 / 旧 HomeWorkspacePage runtime 入口为 WorkspaceCompositionPage
- 依赖：I-012
- 主要范围：src/components/home/HomeWorkspacePage.tsx,src/app/page.tsx
- 完成条件：不再运行 settings:home_workspace authority

### C-D05｜处理 PersonalOverviewSummary 中重复 Home mount
- 依赖：C-D04
- 主要范围：src/components/settings/PersonalOverviewSummary.tsx
- 完成条件：改成 summary 或移除完整工作区嵌入

### C-D06｜Shell/Layout death handoff
- 依赖：C-D05
- 主要范围：按任务语义
- 完成条件：列出 shared patches

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


# V2 追加｜Domain Page 全量视觉迁移

### V-037｜迁移 Home/Workspace 页面总体背景与编辑态 chrome
- 依赖：V-027,V-018
- 范围：`src/components/home/**; src/app/page.tsx`
- 完成：查看态轻、编辑态清晰；不把所有 widget 强包玻璃卡

### V-038｜迁移 Home widgets 的容器 surfacePolicy
- 依赖：V-016,V-037
- 范围：`src/components/home/widgets/**`
- 完成：bare/surface/glass/crystal 策略按内容类型应用

### V-039｜迁移 Files 主页面与文件列表/网格
- 依赖：V-018,V-027
- 范围：`src/app/files/**; src/components/files/**`
- 完成：文件密集区保持稳定 surface，预览区可用浮层材质

### V-040｜迁移 File preview panes / Markdown / Code / Media
- 依赖：V-039
- 范围：`src/components/files/preview-panes/**; src/components/preview/**`
- 完成：正文可读性优先，不因玻璃导致内容对比不足

### V-041｜迁移 Apps 页面与应用卡片/详情
- 依赖：V-018,V-027
- 范围：`src/app/apps/**; src/components/apps/**; src/components/creative/**`
- 完成：卡片、详情、安装弹窗统一 V2

### V-042｜迁移 Library 页面
- 依赖：V-018,V-027
- 范围：`src/app/library/**; src/components/library/**`
- 完成：列表/卡片/空态/操作栏统一 V2

### V-043｜迁移 Modules/Store/Tools 页面
- 依赖：V-018,V-027
- 范围：`src/app/modules/**; src/app/store/**; src/app/tools/**; src/components/tools/**`
- 完成：旧 route 页面无 V1 残留

### V-044｜迁移 Capabilities 页面与 connector/skill/expert 卡片
- 依赖：V-018,V-027
- 范围：`src/app/capabilities/**; src/components/capabilities/**`
- 完成：高密度信息仍清晰，状态色克制

### V-045｜迁移 AI 页面当前保留的外部 AI Tool Integration UI
- 依赖：V-018,V-027
- 范围：`src/app/ai/**; src/components/ai/**`
- 完成：仅外部工具集成视觉，不复活 Agent Runtime

### V-046｜迁移 Jobs 页面当前保留/重构后的非 Agent 后台任务 UI
- 依赖：V-018,V-027
- 范围：`src/app/jobs/**; src/components/jobs/**`
- 完成：若旧 Agent Jobs 被删除，则只迁移仍存续后台任务

### V-047｜迁移 Usage 页面指标卡、表格和图表
- 依赖：V-025,V-027
- 范围：`src/app/usage/**; src/hooks/useUsageData.ts`
- 完成：浅色图表水彩填充；暗色数据强调克制发光

### V-048｜迁移 Settings 全部子页面表单/卡片/状态
- 依赖：V-030,V-018
- 范围：`src/components/settings/**`
- 完成：Provider/Proxy/General/Appearance 等全部统一

### V-049｜迁移 Terminal 视觉与主题桥接
- 依赖：V-006,V-027
- 范围：`src/components/shell/Terminal.tsx; src/app/styles/terminal.css`
- 完成：ANSI 可读，terminal surface 与 App 主题协调

### V-050｜迁移 onboarding/release/update/screenshot 边缘流程
- 依赖：V-018
- 范围：`src/components/onboarding/**; release/**; update/**; screenshot/**`
- 完成：非主路由也无旧 V1 样式遗漏

### V-051｜统一 drag/resize/selection/snap 编辑态视觉
- 依赖：V-037,V-017
- 范围：`src/components/home/**; src/components/workspace/**`
- 完成：编辑 chrome 在两个主题下都清晰且不喧宾夺主

### V-052｜统一 Inspector/Data View/Board/List/Table/Calendar 材质层级
- 依赖：C-020,C-035,V-018
- 范围：`src/components/workspace/**; src/components/**`
- 完成：数据密集视图优先可读性，不滥用透明 blur

### V-053｜建立全路由视觉迁移清单并逐项签收
- 依赖：V-037,V-050
- 范围：`docs/development/theme-v2-route-checklist.md`
- 完成：11 个 page route + shell/settings/overlay/edge flows 均有状态

### V-054｜建立 dark/light 页面截图基线脚本/清单（只准备，不执行 Gate）
- 依赖：V-053
- 范围：`scripts/**; docs/development/**`
- 完成：开发期只准备 harness，不把测试前移

### V-055｜检查 125%/150% 缩放和窄窗口布局视觉规则
- 依赖：V-053
- 范围：`src/app/styles/**; src/components/shell/**`
- 完成：材质、阴影、边缘高光不因缩放破裂

### V-056｜检查高对比/减少动态/减少透明度 fallback 规则
- 依赖：V-053,V-035
- 范围：`src/app/styles/motion.css; tokens.css`
- 完成：可访问性模式有确定性退化路径

### V-057｜统一 icon chip/avatar/status badge 的双主题材质
- 依赖：V-018
- 范围：`src/components/**`
- 完成：不再散落旧 primary-soft/grayscale 风格

### V-058｜统一 table/list row hover/selected/focus 视觉
- 依赖：V-018
- 范围：`src/components/**; src/app/styles/controls.css`
- 完成：暗色不靠粗亮边；浅色不靠灰块堆叠

### V-059｜统一 context menu/dropdown/tooltip/popover 层级
- 依赖：V-017,V-018
- 范围：`src/components/**`
- 完成：所有浮层共享 V2 elevation 与边缘规则

### V-060｜审计并移除组件中的 V1 class/旧主题名字
- 依赖：V-053
- 范围：`src/components/**; src/i18n/**`
- 完成：生产 UI 不出现 Terminal Volt/Frosted Jasmine

### V-061｜输出 Domain Visual Migration handoff
- 依赖：V-037,V-060
- 范围：`docs/development/visual-domain-handoff.md`
- 完成：列出迁移覆盖、未验证项、Final Gate 关注点

