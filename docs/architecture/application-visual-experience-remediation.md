# 全应用视觉与交互体验整改

> 状态：方案冻结，Wave 1–3 已启动并完成首批集成。  
> Natives 审计基线：`60107ab2d21e`；首批集成 HEAD：`1943c57`（2026-08-12）。
> Waku 参考：`/Volumes/UNTITLED/本人材料/project/waku`；审阅时仓库报告 HEAD `925c7533cd88744cd76531f8a938e07cbb1b0213`，工作树非干净，因此本方案只引用已核对的源码行为，不把该 commit 当作可复现截图基线。  
> 产品面：Hub 主工作台为主，覆盖 Workshop 管理面、Embed 宿主 Chrome 与 Menubar Widget；不改变 web-module / capability 双轨。  
> **执行包**：本文保留设计总纲与首批集成基线；通用历史整改仍以 `/Users/ldh/Downloads/project/uitask/README.md` 为入口。2026-08-23 新增的 Workspace 画布组件专项以本文 §12 为仓库内唯一实施入口，避免把 Git 外路径当作该专项规范来源。
> **存储约束**：本任务严格禁止创建或使用 Git Worktree、临时 clone 或完整仓库副本；所有 Agent 必须复用 Natives 当前工作目录与当前分支，通过文件所有权与 Wave 顺序隔离并行改动。

## 0. 权威、范围与完成定义

实现必须先读：

- [`../README.md`](../README.md) 与 [`../standards/README.md`](../standards/README.md)。
- [`../standards/ui-ux/01-design-tokens.md`](../standards/ui-ux/01-design-tokens.md)、[`02-interaction.md`](../standards/ui-ux/02-interaction.md)、[`03-feedback.md`](../standards/ui-ux/03-feedback.md)。
- [`../standards/frontend/01-structure.md`](../standards/frontend/01-structure.md)、[`02-state-and-data.md`](../standards/frontend/02-state-and-data.md)、[`03-i18n.md`](../standards/frontend/03-i18n.md)。
- [`../standards/product/02-feature-spec.md`](../standards/product/02-feature-spec.md) 与 [`../standards/technical/04-performance.md`](../standards/technical/04-performance.md)。
- [ADR-0010](../adr/0010-global-neutral-spectrum.md)；涉及产品三面时同时读 [ADR-0012](../adr/0012-product-identity-workshop-scope.md)。

权威顺序仍为 standards > 产品冻结 ADR > 其它 ADR > architecture。本文只能收窄实施，不得放宽 MUST；确需改变全局中性色板、Shell 产品边界、Widget Liquid Glass 或新增/废除 MUST 时，先写 ADR。

本轮不是单页换肤，也不是只修首页。完成意味着：

1. 全局视觉语言、Shell、通用控件和主要业务面使用同一层级与交互语法。
2. 主路径在深色、浅色、默认窗口和最小窗口下均可用，无截断的必要操作。
3. 所有异步表面覆盖 loading / error / success，并按数据域覆盖 empty / partial / stale；不以假数据撑版面。
4. 鼠标、键盘、焦点、减少动态效果和中英文长文案均完成验收。
5. 同设备 Release 构建下满足性能预算，并留下可比较的前后证据。
6. `typecheck`、`lint`、`test`、`perf:check` 与触及区域专项检查全部通过。

非目标：

- 不复制 Waku 的 GPUI 技术栈、provider 数据模型或原始 SVG 资产。
- 不创建第二套主题引擎、CSS reset、Toast、Modal、EmptyState、Resizable Panel 或命令面板。
- 不改变 Renderer → Host → Daemon、Workshop sandbox、Embed 隔离或数据权威。
- 不用装饰动画、假指标、假通知或固定示例数据制造“完成截图”。

## 1. Waku 参考证据与转译原则

本次已核对 Waku 的 `AGENTS.md`、`src/theme.rs`、`src/app/render.rs`、`src/app/components.rs`，并检索 `sidebar.rs`、`composer.rs`、`command_palette.rs` 与 `transcript_view.rs`。可确认的设计特征是：

- 中性石墨色表面为主，颜色只承担品牌小范围点缀或状态语义。
- Sidebar、主内容、raised、composer、inset、terminal、overlay 有明确语义层级，不靠连续边框堆出结构。
- 主工作区是可折叠/可调宽的侧栏、内容和右面板；resize handle 只在 hover/drag 时加强。
- Transcript 保持连续阅读列；上下文动作渐进披露，但 hover 能力同时有键盘焦点路径。
- Composer 固定在任务上下文附近，权限、排队、停止、模型和发送状态形成一个连续操作区。
- Command Palette 管理打开前焦点、结果高度、空态、异步搜索和关闭后的焦点恢复。
- Toast 是有边界的 raised overlay，宽度受控，状态由图标 + 颜色共同表达，可键盘关闭。
- 动画尊重 reduce-motion；长列表和逐帧路径要求虚拟化、纯内存渲染和后台计算。

### 1.1 Adopt / Adapt / Reject

| 结论 | Waku 证据 | Natives 落点 | 验收 |
|---|---|---|---|
| Adopt | 清晰的 canvas / sidebar / surface / raised / inset / overlay 层级 | 映射到现有 `--background`、`--sidebar`、`--surface`、`--control-*`、`--vibe-*`，不建平行令牌 | 同一层级在 dark/light 中语义一致 |
| Adopt | 紧凑桌面 Chrome、系统 UI 字体、代码/数值等宽 | Shell、工具栏、表格、Agent 时间线统一字体角色和密度 | 1440×900 信息密度提高但不拥挤 |
| Adopt | 可调宽面板与只在 hover/drag 时显性的 resize affordance | 复用既有可调宽 Shell/RightPanel/Terminal，不增加布局状态源 | 拖拽、键盘路径、持久化均不回退 |
| Adopt | Transcript 连续阅读列、工具活动分组、次要动作渐进披露 | `assistant/**` 时间线、工具块、Diff、消息操作 | 长会话仍可快速扫读，必要操作不依赖 hover |
| Adopt | Command Palette 的异步状态与焦点恢复 | 复用 Shell/Assistant 既有 Palette，统一结果行、分组、空态 | Cmd/Ctrl 入口、Escape、上下键、回车、焦点归还完整 |
| Adopt | Overlay 强边界、受控宽度、图标 + 文本语义 | 复用 `Toast`、`Modal`、`ConfirmDialog`、`Tooltip` | 不使用原生 alert/prompt/confirm |
| Adopt | reduce-motion、键盘可达、大于图形本身的命中区 | 通用控件、图标按钮、动态状态 | 仅键盘可完成核心路径；减少动态效果下无装饰循环 |
| Adapt | Waku 的原生 Sidebar vibrancy | 使用 Natives 的 Tauri 透明链路与 `--vibe-*` 材质层 | 不把 GPUI/native API 复制进 Renderer |
| Adapt | Waku 珊瑚品牌色、蓝色 resize/gauge | Natives 继续遵守 ADR-0010；普通选中/强调使用中性色，彩色仅语义 | 第一方页面无装饰彩色漂移 |
| Adapt | Waku 围绕单一 coding-agent 的三栏 | 映射为多域 Shell：导航 / 主内容 / 上下文面板 / 可折叠终端 | Files、Workshop、Settings 不被强塞进聊天布局 |
| Adapt | Waku 的像素尺寸和紧凑行高 | 先映射现有 `SPACING`、`FONT_SIZE`、`BORDER_RADIUS`，缺口再补令牌 | 无页面私有魔法数字 |
| Adapt | hover 才出现消息操作 | 鼠标可渐进披露，但键盘 focus 和触屏/无 hover 场景必须可发现 | 操作不以 hover 为唯一入口 |
| Reject | 直接复制 Waku 色值、SVG、GPUI 组件或 shader | 继续使用 Natives 语义令牌、`lucide-react` 与现有 LiquidGlass | 无来源不明资产或重复组件 |
| Reject | 用彩色 accent 作为导航选中、主按钮或图表装饰 | 选中态走中性反差，语义色只表达 danger/warning/success/info | 符合 ADR-0010 |
| Reject | 把 vibrancy/blur 铺到正文、代码、终端、Diff、表单 | Content/Reading 维持稳定近不透明表面 | 长文本和代码边缘清楚 |
| Reject | 为参考效果新增第二套主题运行时或新视觉依赖 | 复用 `theme-engine.ts`、`design-tokens.ts`、CSS 变量和已装依赖 | 无新增依赖即可完成 |
| Reject | 参考仓的 seed/mock 数据和 provider 私有流程 | Natives 只显示真实 Host/Daemon/用户数据 | 无假数据与假零值 |

## 2. 目标视觉系统

### 2.1 品牌与色彩

- 品牌基底保持黑、白、灰；深浅主题只反转语义角色，不改原始中性色板。
- Canvas 承担窗口背景；Sidebar/Navigation 允许轻材质；Surface 承担内容；Raised 承担浮层或强调容器；Inset 承担代码、终端、输入内部区域。
- 主操作通过明暗反转、字号、位置和留白建立优先级，不使用装饰彩色。
- danger、warning、success、info 必须同时有图标/文案，不以颜色作为唯一信号。
- 图表继续使用 `--chart-volume-0..8`；分类数据必须有真实标签和值。
- dark/light 的正文对比度达到 WCAG AA 4.5:1，关键图标和边界至少 3:1。

### 2.2 材质与层级

| 层 | 目标 | 禁止 |
|---|---|---|
| Window / Root | 透明链路完整，无 FOUC 白/黑底 | 根节点实色遮挡 |
| Shell | 轻 tint、适量 blur、细分隔，不抢内容 | 大面积高饱和、强 glow |
| Navigation | 与内容可辨但不过度立体，选中态明确 | 每行卡片化、重复粗边框 |
| Content | 稳定、近不透明、长时间阅读舒适 | 强 blur、位移、色差、动态折射 |
| Floating | Raised surface、强边界、受控阴影与宽度 | 无边界漂浮、透明到读不清 |
| Widget | 单一高质量 Liquid Glass 主卡 | 第二套浮窗框架或多层玻璃卡片套娃 |

层级优先由表面明度和留白表达；边框只用在确需分隔或表示交互边界的位置。连续卡片列表优先使用组、分区标题和行状态，不把每行变成独立“网页卡片”。

### 2.3 字体、图标与密度

- 品牌、页面标题、激活面包屑使用 `--font-display`；正文使用 `--font-ui`；代码、路径、数值、Token、时间和终端使用 mono 角色。
- 同一信息层级在 dark/light 中字号、字重和行高一致；主题切换不改变布局。
- 图标统一使用现有 `lucide-react`；常用图标保持同一笔画和视觉尺寸。
- 图标按钮的命中区域大于图形本身，必须有 Tooltip 与 i18n aria-label。
- 默认密度服务桌面生产力：工具栏紧凑、正文舒展、表格可扫读；不得用缩小字号换取“高级感”。

### 2.4 控件状态矩阵

所有 Button、IconButton、NavItem、Tab、ListRow、Input、Select、MenuItem、CardAction 必须至少覆盖：

| 状态 | 视觉要求 | 交互要求 |
|---|---|---|
| Default | 层级清晰、无多余边框 | 语义和可点击范围明确 |
| Hover | 轻微 surface/tint 变化 | 不移动布局，不作为唯一信息入口 |
| Pressed | 比 hover 更强的即时反馈 | 指针抬起前不触发重复操作 |
| Selected | 中性强对比 + 必要的图标/文字 | 与 focus 独立表达 |
| Focus-visible | 清晰焦点环，跨主题可见 | Tab 顺序符合阅读顺序 |
| Disabled | 降低强调但仍可辨识 | 不响应；必要时解释禁用原因 |
| Loading | 尺寸稳定，显示进度 | 防重复提交；保留可取消路径（若适用） |
| Error | 语义色 + 文案/图标 | 提供可执行的修复或重试动作 |

## 3. 目标布局与信息架构

### 3.1 Shell

Natives 保留现有多域 Shell，不改产品三面。目标结构是：

```text
Window
├─ Header：窗口级导航、当前上下文、少量全局动作
├─ Body
│  ├─ Sidebar：一级域、项目/会话局部导航，可折叠/调宽
│  ├─ Main Content：当前任务的唯一主舞台
│  └─ Right Panel：检查器、详情、活动；按上下文出现
└─ Terminal：任务辅助面，可折叠/调高，不与主内容争夺默认焦点
```

规则：

- 一个视图只允许一个明确主标题和一个主操作；次要操作收敛到工具栏、上下文菜单或 Command Palette。
- Header 不重复 Sidebar 已表达的一级导航；RightPanel 不复制主内容的完整表单。
- 面板折叠后主操作仍可达；关闭面板必须归还焦点到触发控件。
- 默认窗口 1440×900 展示完整布局；最小窗口 960×600 通过折叠次要面板和收敛工具栏维持主路径，不横向裁掉必要操作。
- 面板宽度/终端高度继续由 Shell 单一状态源管理，不在页面内另存。

### 3.2 页面骨架

每个第一方页面优先使用同一骨架：

1. 页面标题区：标题、简短状态/范围说明、唯一主操作。
2. 工具区：搜索、筛选、排序、视图切换；无工具时不保留空栏。
3. 内容区：列表/网格/时间线/表单，使用真实数据状态。
4. 上下文区：选中项详情或任务活动，按需进入 RightPanel。
5. 状态区：loading、empty、error、partial、stale 与重试/下一步。

长表单按语义分组并保持“编辑中”和“已保存”可区分；破坏性操作远离主操作并必须确认。列表选择不自动执行破坏性动作。

### 3.3 自适应档位

| 档位 | 参考尺寸 | 行为 |
|---|---:|---|
| Wide | 1440×900 | Sidebar + Main + RightPanel 可共存；Terminal 按用户状态 |
| Regular | 1180×760 | 优先保留 Sidebar + Main；RightPanel 按上下文覆盖或收窄 |
| Minimum | 960×600 | Sidebar 可折叠，RightPanel 单独切换，工具栏收敛；主操作始终可见 |
| Menubar | 400×600 | 独立轻量 surface，只渲染个人概览；不加载 Shell/Assistant |

断点是行为合同，不是新增一套页面。业务组件不得自行选择与 Shell 冲突的断点。

## 4. 目标交互流程

### 4.1 导航与查找

- Sidebar 负责可见、稳定的一级入口；Command Palette 负责快速跳转、命令和跨域搜索。
- Cmd/Ctrl+K 打开全局 Palette；上下键移动、Enter 执行、Escape 关闭，并恢复打开前焦点。
- 页面内筛选只筛当前数据集，不复制全局搜索能力。
- 打开详情优先在现有 RightPanel/详情区展示；只有需要完整工作空间时才导航页面。

### 4.2 创建、编辑与删除

- 创建：入口 → 最少必要信息 → 明确预览/权限 → 用户确认 → 真实进度 → 完成后的下一步。
- 编辑：进入时显示当前真实值；未保存改动可辨识；成功用非阻塞反馈；失败保留用户输入。
- 删除/卸载/撤销授权：显示确切对象和影响范围；危险按钮使用 danger 语义；焦点默认不落在危险操作。
- 长任务允许取消时必须显示取消入口；不可取消时明确说明，不伪装按钮。

### 4.3 Agent 主流程

- Transcript 是阅读主轴；tool、reasoning、diff、plan、permission 按事件顺序呈现，不把过程拆成互不相关卡片墙。
- Composer 在会话上下文内稳定停靠；模型、权限、附件、队列、发送/停止保持空间位置稳定。
- 权限与 AskUser/Plan Approval 是阻塞主流程的内联决策，不用瞬时 Toast 替代。
- 运行、任务、产物、上下文、审计和事件留在 ActivityInspector；默认只突出当前最相关状态。
- 连接中断使用浮层/横幅，不挤压整条时间线；恢复后保留滚动与输入状态。

### 4.4 状态与反馈

| 场景 | 渠道 | 必须提供 |
|---|---|---|
| 保存、复制等瞬时结果 | Toast | 自动消失、不抢焦点、图标 + 文案 |
| 可回顾系统事件 | Notification Center | 持久记录、已读状态、真实时间 |
| 删除、授权、发布等决策 | Modal / inline approval | 焦点陷阱、Escape、焦点归还、影响说明 |
| 页面自身数据 | Inline state | loading/error/success；按域补 empty/partial/stale |
| 长任务 | Inline progress / activity | 当前阶段、真实状态、取消/重试能力 |

## 5. 页面级整改规格

| 表面 | 主要文件 | 目标规格 | 必验收路径 |
|---|---|---|---|
| Root / Home | `src/app/RootClient.tsx`、`src/app/page.tsx` | 正确 surface 早分流；首页以真实最近活动/快捷入口构成明确首屏 | main/menubar 分流、空数据、主题就绪 |
| Shell | `src/components/shell/ShellLayout.tsx`、`Header.tsx`、`Sidebar.tsx`、`MainContent.tsx`、`RightPanel.tsx`、`Terminal.tsx` | 统一 Chrome、面板层级、选中态、resize、折叠与焦点；终端保持阅读表面 | Wide/Regular/Minimum、面板开关、终端拖拽 |
| Assistant | `src/components/assistant/**` | 连续 Transcript、清晰 Composer、分组工具活动、明确审批和连接状态；减少卡片噪声 | 新会话、运行中、排队、审批、错误、长会话 |
| Jobs | `src/components/jobs/**` | 列表/表单/运行历史共享状态语法；计划、启停、运行结果易扫读 | 创建、编辑、启停、空历史、失败重试 |
| Files | `src/components/files/**` | 导航、搜索、列表/网格、选择、预览和上下文菜单组成一条稳定工作流 | 大目录、空目录、加载、错误、键盘选择、预览 |
| Workshop / Creative | `src/components/shell/WorkshopPage.tsx`、`src/components/shell/workshop/**`、`src/components/creative/**` | 目录、创作、安装、权限、日志、运行状态层次清楚；来源/信任域不混淆 | 三来源、安装向导、删除确认、草稿失败保留 |
| Capabilities | `src/components/capabilities/**` | Skills/连接器/专家保持统一列表—详情—编辑模式；信任与启用状态明显 | 导入、编辑、信任、禁用、无结果、Hub stale |
| Settings | `src/components/shell/SettingsPage.tsx`、`src/components/settings/**` | 导航稳定；表单按域分组；保存/验证/错误位置一致；密集引擎信息可扫读 | 主题即时切换、Provider、Routing、Harness、长文案 |
| Modules / Store / Tools | `src/app/modules`、`src/app/store`、`src/app/library`、`src/app/tools` | 与页面骨架一致，历史 `/store` 文案不暗示联网商店已交付 | empty/error/installed/disabled、路由切换 |
| Overlays | `src/components/ui/{Modal,ConfirmDialog,Toast,EmptyState,Skeleton,ShortcutHelp}.tsx`、两套 `CommandPalette.tsx` | 状态、尺寸、焦点、阴影、间距和键盘行为统一；可复用现有原子 | Tab loop、Escape、焦点归还、多 Toast、无结果 |
| Menubar | `src/components/menubar/**` | 保留单卡 Polished Crystal；真实个人概览；隐藏时无动画/轮询 | 400×600、dark/light、missing/partial/stale/error |

任何页面整改都必须先列出现有组件和调用方，再决定复用、删除或修改；禁止为“统一”包一层只有单实现的 wrapper。

## 6. 冻结历史（非执行合同）

> 本节至第 10 节只保留首批集成的溯源快照，**禁止用于派单或继续实施**。后续执行只读 `/Users/ldh/Downloads/project/uitask/README.md`、`04-surface-ownership.md`、WP-01～WP-10 和对应 `evidence/<baseline-head>/progress-ledger.md`。

并发上限为主 Agent + 3 个 Subagent。文件所有权按下表冻结；跨包发现问题时发给 owner，不直接越界编辑。

| 工作包 | 唯一文件所有权 | 交付 |
|---|---|---|
| WP-A · Foundation | `src/app/globals.css`、`src/lib/design-tokens.ts`、`src/lib/theme-engine.ts`、`src/components/ui/**` 及对应测试 | 令牌消费收敛、基础控件状态、Overlay、深浅主题、reduced-motion；不新建第二套 primitives |
| WP-B · Shell | `src/components/shell/ShellLayout.tsx`、`Header.tsx`、`Sidebar.tsx`、`MainContent.tsx`、`RightPanel.tsx`、`Terminal.tsx`、`src/components/shell/sidebar/**`、Shell 布局 hooks/tests | Window Chrome、导航、面板、resize、自适应和焦点；不改 Workshop/Settings 域页 |
| WP-C · Agent Workflows | `src/components/assistant/**`、`src/components/jobs/**`、`src/app/ai/page.tsx`、`src/app/jobs/page.tsx` 及对应测试 | Transcript、Composer、Activity、审批、连接状态与 Jobs 完整流程 |
| WP-D · Product Surfaces | `src/components/files/**`、`capabilities/**`、`creative/**`、`settings/**`、`src/components/shell/WorkshopPage.tsx`、`shell/workshop/**`、`shell/SettingsPage.tsx`、对应路由与测试 | Files、Workshop、Capabilities、Settings、Modules/Store/Tools 的页面级统一 |
| WP-E · Menubar & A11y QA | `src/components/menubar/**`、`src/lib/useFocusTrap.ts`、Menubar/可访问性专项测试与证据文件 | Widget 视觉与状态、键盘走查、reduce-motion、对比度、截图矩阵；跨包缺陷交回 owner |
| 主 Agent | `src/i18n/zh/**`、`src/i18n/en/**`、`src/i18n/zh.ts`、`src/i18n/en.ts`、本文、`docs/README.md`、冲突整合与最终门禁 | 冻结契约、集中合并 i18n、防止共享文件冲突、统一验收与进度更新 |

WP-A 与 WP-B 不应同时编辑 `globals.css`；Shell 专用样式需求由 WP-B 提交清单，WP-A 统一落 token/style。WP-E 不在 QA 时顺手修改其它包文件。

## 7. 历史波次（禁止继续执行）

### Wave 0 · 基线与差距冻结

1. 在本文记录实施用集成 HEAD、Waku 参考状态和现有未提交文件。
2. 截取 dark/light × 1440×900/960×600 的主要表面 before 图。
3. 对视觉硬编码、Emoji、原生对话框、outline、动画、异步三态和 i18n 做静态盘点。
4. 每个页面列出最频繁主路径和最严重断点；禁止只凭首页截图排优先级。

### Wave 1 · Foundation

WP-A 先收敛现有令牌与通用原子，主 Agent 审核 ADR-0010、内容可读性、主题校验与 FOUC。此波只建立所有页面确实要消费的能力，不预建未来组件。

退出条件：基础控件状态矩阵、Overlay、Skeleton/Empty/Error 呈现、focus-visible、reduced-motion 和 dark/light 均可独立验收。

### Wave 2 · Shell

WP-B 在 Foundation 稳定后统一 Header/Sidebar/Main/RightPanel/Terminal，自适应只改变展示编排，不创建第二份业务状态。

退出条件：1440×900、1180×760、960×600 均无必要动作被裁；面板 resize/折叠/恢复和键盘焦点通过。

### Wave 3 · 业务面并行

并行执行 WP-C、WP-D、WP-E；主 Agent 集中处理 i18n。每个包至少交付一个贯穿真实数据、状态、键盘和最小窗口的 tracer path，然后完成包内所有列出的表面，不以 tracer path 代替完整范围。

历史合并顺序仅供溯源。新执行中 Subagent 不得提交；只由主 Agent 等待写入停止后串行创建聚焦提交。

### Wave 4 · 统一校准

1. 统一 Typography、行高、图标尺寸、间距、圆角、边框和阴影。
2. 消除页面私有灰色、重复 Card、重复 Toolbar 和重复 Overlay。
3. 校正中英文长文案、数据密度、空/错/部分/陈旧状态。
4. 只根据实测问题调整令牌；禁止为单页截图破坏全局语义。

### Wave 5 · 证据与发布门禁

1. 完成第 8 节视觉、交互、a11y 和性能矩阵。
2. 同设备、Release、同数据路径记录启动、交互 p95、动画 FPS、Bundle、CPU/RSS 前后数据。
3. 运行全局门禁和触及区域专项测试。
4. 把完成项、证据链接、剩余 blocker 和回滚点更新到执行包唯一进度台账；未满足不得标 complete。

## 8. 验收矩阵

### 8.1 Dark / Light × Viewport × State

下表对 Shell、Assistant、Jobs、Files、Workshop、Capabilities、Settings 强制执行；Menubar 使用 400×600 专项矩阵。`success` 表示真实正常数据，不是“操作成功”Toast。

| Theme | 1440×900 | 1180×760 | 960×600 |
|---|---|---|---|
| Dark · loading | 骨架/进度与最终布局同尺寸 | 无工具栏跳动 | 主操作仍可见 |
| Dark · success | 三栏层级完整、数据真实 | 次要面板按契约收敛 | 无水平裁切或遮挡 |
| Dark · empty | EmptyState 居中且有下一步 | 不留下死空白 | 文案和按钮不溢出 |
| Dark · error | 分类错误 + actionHint + 合法重试 | 不用 Toast 代替整页错误 | 错误不遮住退出/返回 |
| Dark · partial/stale | 覆盖范围、更新时间明确 | 状态不挤压标题 | 可继续使用已有真实数据 |
| Light · loading | 无白底闪烁，骨架对比清晰 | 无工具栏跳动 | 主操作仍可见 |
| Light · success | 语义层级与 Dark 一致 | 次要面板按契约收敛 | 无水平裁切或遮挡 |
| Light · empty | 空态与 Surface 可分 | 不留下死空白 | 长中英文不溢出 |
| Light · error | danger 可辨且非颜色唯一表达 | actionHint 清楚 | 焦点可到重试/返回 |
| Light · partial/stale | 状态标签 ≥3:1 且有文案 | 不伪装全量 | 可继续使用已有真实数据 |

Menubar 必须执行：dark/light × missing/partial/stale/error/success；每个状态同时验证首次显示、隐藏后静默、再次显示 reconcile、Escape/失焦只隐藏。

### 8.2 交互与可访问性

- 只用键盘完成：切换一级域、打开/关闭面板、Command Palette 搜索与执行、创建 Job、发送消息、处理权限、文件选择/预览、打开设置并切换主题。
- 所有 Modal：初始焦点合理、Tab 不逃逸、Escape 可按契约关闭、关闭后焦点归还触发器。
- hover 显示的操作在 focus-visible 时也显示；必要主操作永不只靠 hover。
- `prefers-reduced-motion: reduce` 下无装饰循环、位移入场或持续脉冲；进度仍以静态文本/图标表达。
- 200% 文本缩放、最长 zh/en 文案、长路径/模型名/项目名不遮住主操作。
- 状态、Diff、图表和选中项不以颜色为唯一信息载体。

### 8.3 性能与工程门禁

- 点击、输入、选择反馈 30 次 p95 ≤100ms；缓存页面切换 p95 ≤300ms。
- 单个主线程任务 ≤50ms，动画 ≥55 FPS；重 IO 100ms 内先展示加载反馈。
- `/layout + 当前入口页面` 初始 JS ≤350KB gzip；重型编辑器、图表和 Modal 延迟加载。
- 主窗口隐藏时暂停非必要 Renderer timer/animation；Widget 空闲 CPU ≤3%。
- 长列表超过 200 项使用分页/窗口化，不一次创建完整 DOM。
- 必跑：`rtk npm run typecheck`、`rtk npm run lint`、`rtk npm run test`、`rtk npm run perf:check`。
- 若触及 Rust/Host、协议、Extension Host，按 `AGENTS.md` 追加对应专项门禁。

## 9. 风险与回滚

| 风险 | 早期信号 | 控制 | 回滚点 |
|---|---|---|---|
| 全局 token/CSS 级联造成跨页回归 | 单页变好、其它页对比或间距突变 | 主 Agent 为 WP-01 创建独立原子提交；每批跑截图矩阵 | 回退对应 Foundation 提交，不回退业务逻辑 |
| 参考 Waku 时引入品牌冲突 | 珊瑚/蓝色成为普通选中或主操作色 | ADR-0010 review；Adopt/Adapt/Reject 检查 | 回退到既有中性语义映射 |
| 玻璃/阴影增加 GPU 与文本模糊 | FPS、GPU、文字边缘下降 | 玻璃仅 Shell/Navigation/Floating/Widget；实测 | 回退材质参数，保留布局与状态改进 |
| 响应式隐藏必要操作 | 960×600 无法完成主路径 | 每页定义唯一主操作；次要动作进菜单/Palette | 回退该页面布局提交 |
| 多 Agent 修改共享文件冲突 | `globals.css`、Shell、i18n 反复冲突 | 严格文件所有权；i18n 由主 Agent 集中 | 停止冲突 Agent，主 Agent 核对并只集成 owner 的授权变更 |
| 视觉统一破坏数据/安全语义 | 权限、来源、partial/stale 被弱化 | 产品/安全规范作为验收门 | 回退表现层，不回退真实状态和安全防线 |
| 动效/Blur 导致性能回退 | 长会话滚动、页面切换超预算 | CSS 优先、reduce-motion、隐藏门控、前后测量 | 关闭装饰动效/降低材质，不加性能豁免 |
| i18n 长文案溢出 | 英文按钮被截断、中文层级不自然 | 集中 i18n + 双语截图 | 回退局部布局，不删双语文案 |

每个可独立验收的切片由主 Agent 创建独立、可回滚的原子提交。禁止用 feature flag 或第二套主题长期并存来规避回滚。

## 10. 进度记录

以下是首批集成的冻结历史快照，不再更新。活动状态只维护在执行包的唯一进度台账：

| Wave | 状态 | 集成 commit | 证据 | Blocker |
|---|---|---|---|---|
| 0 · 基线 | completed | `1943c57` | Waku dark/light 参考截图 + Natives browser before/首批 after | 参考仓 dirty，证据以已核源码与截图为准 |
| 1 · Foundation | in progress | `1943c57` | 语义表面、阅读宽度、统一 motion；typecheck/colors/定向 lint 通过 | 通用 UI 原子仍待后续 Wave 覆盖；全量 lint 被既有 `embedded_prod` / `global_singleton` 清单债务阻断，新增债务为 0 |
| 2 · Shell | in progress | `1943c57` | 连续工作台、中性导航选中态、48px titlebar；1280×720 dark 浏览器实测可见 | 全页面最小窗口与 light 矩阵待后续 Goal 完成 |
| 3 · 业务面 | in progress | `1943c57` | Assistant Timeline/Composer + Home 标题层级已集成；相关类型与定向测试通过 | Files/Jobs/Workshop/Settings 等待 WP-C/D/E 后续执行；全量测试既有 fixture 失败 1 项（770/771 通过） |
| 4 · 统一校准 | pending | — | — | — |
| 5 · 验收 | pending | — | — | — |

状态只能依据同一集成 HEAD 的证据更新；Subagent 完成不等于 Wave 完成。

## 11. Goal 启动指令

新版完整提示词已迁移到 `/Users/ldh/Downloads/project/uitask/GOAL.md`。启动 Agent 时必须复制该文件全文，不能继续使用本文旧版简化提示词。

## 12. Workspace 画布组件专项整改（2026-08-23）

> 状态：ready for implementation。交互规范以 [`../standards/ui-ux/02-interaction.md`](../standards/ui-ux/02-interaction.md) §6 R-U15–R-U20 为准；本文只记录当前源码差距、实施顺序、文件边界和验收证据，不另建规范。
> 审计基线：`5ee9a7917deb`。审计时工作树已有 Apps/Rust/i18n 未提交改动，本专项 Agent 不得触碰或覆盖这些用户改动。
> 产品面：Hub / Personal Workspace；布局轨：Compact Grid + bounded DOM Free Canvas；Widget 仍为内置 React renderer，不是 Plugin Runtime。

### 12.1 当前问题与源码证据

| 用户问题 | 当前证据 | 根因方向 | 目标规则 |
|---|---|---|---|
| 缺少同步按钮 | `WorkspaceSessionProvider` 有 `reloadHost`/`refreshHostSnapshot`，`WorkspaceDataBroker` 有 `invalidate`，但正常态 Workspace Header/WidgetShell 没有同步入口、更新时间或 syncing 状态 | 已有数据能力未形成共享交互合同 | R-U15 |
| 缺少时间组件 | `DistributionChartWidget` 在组件内自管 7/30/90 天；usage adapter key 固定 `30d`；其他时间类 Widget 没有统一范围 | 时间范围被局部实现，未进入 Workspace 上下文和 adapter key | R-U16 |
| 样式不统一 | `WorkspaceCompositionPage`、Inspector、DataView、FreeCanvasView 大量使用局部 `gap/p/text-[…]`；`WidgetShell` 又混用 inline token 与独立 CSS 数值 | Header/Body/状态/控件没有统一 anatomy 与密度基线 | R-U17 |
| 编辑态无法稳定拖动 | Grid 仅 `.ws-drag-handle` 22×22px 可拖，Body 全部 cancel；Free Canvas node 与 stage 同时绑定 `beginGesture`，需验证冒泡/Pointer Capture；group member 的 y 位移当前误用 `dxWorld` | 命中区过小，手势所有权和事件互斥存在缺陷 | R-U19/R-U20 |
| 组件间距过大 | `GRID_MARGIN=[12,12]`、`GRID_PADDING=[16,16]`，外层还可能叠加 Widget/容器 padding | 多层 gutter 叠加，未共享 4px spacing rhythm | R-U18 |

以上是静态审计结论；“无法拖动”的最终根因必须先用真实 Grid 与 Canvas 路径复现并由最小交互测试锁定，禁止只扩大 CSS 把手后宣称修复。

### 12.2 目标交互模型

```text
Workspace Header
├─ Workspace identity
├─ TimeRangeControl（存在时间敏感 Widget 时）
├─ Sync（存在数据 Widget 时）+ last successful sync
└─ Edit layout / Done

Workspace Body
├─ Compact Grid：8px item gap / 12px outer gutter
└─ Free Canvas：8px snap / node header drag / body interaction cancel

WidgetShell
├─ 32px shared header：title / inherited range / status / local actions
├─ shared body density：12px；dense=8px
└─ shared loading / empty / error / stale / ready states
```

同步和时间范围属于数据上下文，不属于布局状态；进入编辑态、移动卡片、切换 Grid/Canvas 都不得触发数据刷新或重置时间范围。布局 pointer move 只写内存，时间范围切换与同步只影响 DataBroker key/cache，不写布局表。

### 12.3 实施切片与依赖

#### WSW-0 · 基线、复现与最小测试（P0）

**Owner 文件**：只新增/修改 Workspace 现有同目录测试；不改生产实现。

1. 记录 dark/light × 1440×900/960×600 的 Grid、Canvas 截图；测量当前 card gap、outer gutter、Header/Body padding。
2. 为 Grid 写一个最小交互测试：编辑态 Header 空白区域可启动拖拽，Header button 与 Body 不启动拖拽，stop 只持久化一次。
3. 为 Free Canvas 写一个最小交互测试：node drag 的 x/y 正确、group member 同步 x/y、Pointer Cancel 不留下 draft；锁定节点不移动。
4. 为 DataBroker 写计数测试：两个相同 key Widget 手动同步只调用一次 loader；失败保留 last good data。

**退出条件**：至少一个测试稳定复现当前拖拽问题；before 证据与测量值写入本节 12.6，不用截图主观判断代替失败测试。

#### WSW-1 · 数据上下文：同步与时间范围（P0，依赖 WSW-0）

**Owner 文件**：`src/lib/workspace/widgets/{types,data-broker}.ts`、`src/components/workspace/widgets/WidgetRenderer.tsx`、Workspace 数据上下文的一个现有模块及对应测试。

1. 在现有 DataBroker 上补最小的“按可见 key invalidate + refetch”入口和同步状态；不创建第二个 store/event bus。
2. 建立一个 Workspace 级 `TimeRange` 值（today/7d/30d/90d；Host 支持任意起止日期时再加 custom），进入 time-aware adapter key；非时间 Widget 不订阅。
3. 去掉“adapter 固定 30d、组件只做前端裁切”的权威路径；查询必须使用实际范围、时区和作用域。Host 只支持有限预设时只显示真实支持的预设，不伪造 custom 查询结果。
4. stale/error 继续使用现有 last-good cache；分类错误后提供重试。

**退出条件**：同 key 去重测试通过；时间范围变更只刷新 time-aware key；同步不改变 Workspace snapshot 的 layout/view state。

#### WSW-2 · 共享控制条与 Widget anatomy（P0，依赖 WSW-1）

**Owner 文件**：`WorkspaceCompositionPage.tsx`、`WidgetShell.tsx`、`widgets.css`、必要的现有 UI primitive 与定向测试。i18n 由集成 Agent 集中修改 `src/i18n/zh/app.ts` 与 `src/i18n/en/app.ts`，并先保留审计基线中已有的未提交内容。

1. 在现有 Workspace Header 中加入 TimeRangeControl、Sync、最近同步时间、Edit/Done；不新建第二条工具栏。
2. TimeRangeControl 复用现有 Button/Popover/Segmented control；自定义日期用原生 date input，不新增依赖。
3. WidgetShell 统一 32px Header、标题/范围/状态/action slots、Body density 和 state region；删除各 Widget 重复 shell/padding。
4. 窄窗口把时间预设收敛为单一 Popover；同步状态和 Edit/Done 永不被隐藏到不可达位置。

**退出条件**：仅键盘可完成同步、改范围、进入/退出编辑；zh/en 长文案不挤掉主操作；sync pending 不引发布局跳动。

#### WSW-3 · 拖拽根因修复（P0，可与 WSW-2 在文件不重叠时并行）

**Owner 文件**：`layout/CompactGrid.tsx`、`views/FreeCanvasView.tsx`、`canvas/**` 与对应测试；不得改 Widget 数据层。

1. Grid 继续复用 `react-grid-layout`；把完整 Widget Header 空白区域设为 handle，Header 控件和 Body 设为 cancel，不换拖拽库。
2. 扩大 Resize 实际命中区但不扩大视觉噪声；dragging 时只使用现有 elevation/edge token。
3. Free Canvas 明确 stage/node/resize handle 的手势优先级，修复事件冒泡、Pointer Capture/Cancel 和 group y 轴位移；禁止在 pointer move 写 Host。
4. 补方向键移动、Escape 取消/退出、焦点归还；键盘和指针走同一个 commit 函数。

**退出条件**：测试覆盖单节点、group、locked、resize、cancel；连续拖拽 30 次无丢失/跳回；pointer move Host 写入为 0，stop 为 1。

#### WSW-4 · 密度与首批 Widget 迁移（P1，依赖 WSW-2/3）

**Owner 文件**：`src/lib/workspace/views/types.ts`、`src/app/styles/widgets.css`、`src/components/workspace/widgets/**`；共享文件由集成 Agent串行落地。

1. 将 Grid margin 改为 8px、outer padding 改为 12px；最小窗口为 8px；Canvas snap 对齐 8px。
2. 按“数据/列表/图表”三类审计现有 Widget，只迁移实际存在的差异，不增加新 wrapper：
   - 数据指标：Today Usage、Token/Cost/Work Time；
   - 图表：Distribution；
   - 列表/状态：Recent Files、Apps、AI/Proxy/Tool、Storage；
   - 非数据：Greeting、Notes、Quick Links、Prompt Snippets。
3. 时间敏感类继承 Workspace range；确需局部覆盖的只在 Inspector 暴露。非数据类不显示同步/范围。
4. 删除 10px 正文、重复卡片边框、重复 padding 和无消费方的样式；保留 Surface Policy 差异。

**退出条件**：相邻卡片边缘 8px、工作区 gutter 12px；所有 Widget 的标题/正文/辅助信息和 loading/empty/error/stale 对齐；无新增视觉魔法数字。

#### WSW-5 · 集成与发布证据（P0，依赖全部）

**Owner**：集成 Agent；集中处理共享 i18n、冲突和最终提交。

1. 更新 12.6 台账，附 before/after、测试命令、实测请求数和失败场景。
2. 执行 dark/light × 1440×900/960×600 × ready/loading/empty/error/stale；验证 125%/150% 缩放和 reduced motion。
3. 性能对比：同步反馈 p95 ≤100ms；drag ≥55 FPS；pointer move 无 Host 写；一次全局同步请求数等于唯一可见 adapter key 数。
4. 运行 `rtk npm run typecheck`、`rtk npm run lint`、`rtk npm run test`、`rtk npm run perf:check`；触及 Host/协议时再跑对应专项门禁。

**退出条件**：五个用户问题全部有自动化或可复现证据；任何失败门禁与既有失败必须区分，不得用“视觉完成”代替工程完成。

### 12.4 Agent 文件所有权与合并顺序

| 工作包 | 可并行 | 独占范围 | 禁止事项 |
|---|---|---|---|
| WSW-1 Data Context | 与 WSW-0 结束后的基线文档工作 | `lib/workspace/widgets/data-*`、WidgetRenderer 数据状态 | 不改 layout/canvas CSS |
| WSW-2 Chrome | 与 WSW-3 | WorkspaceCompositionPage、WidgetShell、共享控件 | 不改 canvas engine |
| WSW-3 Interaction | 与 WSW-2 | CompactGrid、FreeCanvas、canvas math/tests | 不改 data broker/i18n |
| WSW-4 Widgets | WSW-2/3 合并后再分 Widget 文件并行 | 各具体 Widget；同一 Widget 只允许一个 owner | 不复制 WidgetShell/TimeRangeControl |
| WSW-5 Integration | 不并行写共享文件 | i18n、`views/types.ts`、`widgets.css`、本文与最终门禁 | 不顺手合入无关格式化/重命名 |

合并顺序固定为 WSW-0 → WSW-1 → WSW-2/WSW-3 → WSW-4 → WSW-5。每个切片保持可独立回滚；Agent 不提交用户当前 Apps/Rust/i18n 未提交文件。

### 12.5 明确非目标

- 不引入新状态库、日期库、拖拽库、图表库或 Canvas runtime。
- 不新增自动轮询；事件更新 + 手动同步已经覆盖数据新鲜度。
- 不做无限画布、Connector、CRDT、多人协作、Widget Plugin/Marketplace。
- 不以“全卡玻璃化”、大 glow 或更大卡片制造高级感。
- 不借本专项修改 Workspace SQLite 模型、Apps 域或 Legacy Agent/Daemon。

### 12.6 验收台账

| ID | 验收 | 状态 | 证据 |
|---|---|---|---|
| UX-WS-01 | 正常态有同步入口、最近同步时间；同 key 去重；失败保留旧数据 | pending | — |
| UX-WS-02 | today/7d/30d/90d 可选；支持 custom 时边界真实；时区/局部覆盖明确 | pending | — |
| UX-WS-03 | Header/Body/字体/padding/状态区域统一，dark/light 均清晰 | pending | — |
| UX-WS-04 | Grid 与 Canvas 指针/键盘可移动、缩放、取消；无 move 写库 | pending | — |
| UX-WS-05 | card gap=8px、outer gutter=12px，960×600 无双倍间距 | pending | — |
| UX-WS-06 | typecheck/lint/test/perf 与截图、性能、a11y 矩阵通过 | pending | — |

只有集成 Agent 可以更新状态；`completed` 必须附同一 HEAD 下的可复现证据路径或命令输出。
