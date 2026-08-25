# UI/UE 02 · 交互模式

> **版本**: 1.1.0 · **日期**: 2026-08-23
> **关联 ADR**: [ADR-0021](../../adr/0021-multi-workspace-design-system-v2.md)
> **关联源文件**: `src/lib/notification-ui.ts`（toast / 通知中心 / 错误页）、`src/components/shell/CommandPalette.tsx`、`src/components/ui/EmptyState.tsx`、`src/components/workspace/`、`src/lib/workspace/widgets/`

---

## 一、本篇要约束什么

交互规范的缺失会让每个组件各搞一套：这个用 toast、那个用弹窗、又一个直接 `alert`。本篇钉死**反馈渠道的选用**、**空/加载态**、**快捷键体系**三件事，保证用户体验一致、可预期。

---

## 二、反馈渠道：toast / 通知中心 / 模态 / 内联

四类反馈渠道，各有适用场景，**不可混用**：

| 渠道 | 实现 | 适用 | 不适用 |
|------|------|------|--------|
| **Toast** | `showToast()` / `showErrorToast()`（`notification-ui.ts`） | 短暂的、无需回顾的操作反馈（保存成功、复制成功、瞬时错误） | 需要用户决策、需要历史回顾的消息 |
| **通知中心** | `NotificationPanel` + `notifications` 表 | 来自插件/系统的、可累积、可标记已读的消息 | 短暂操作反馈（用 toast） |
| **模态/对话框** | 居中 overlay（`OVERLAY_STYLE`） | 需要用户**立即决策**且阻塞主流程（确认删除、发布向导、标注编辑） | 非阻塞的提示（用 toast） |
| **内联状态** | 组件内 `loading` / `error` / `empty` 分支 | 当前组件自己数据的状态展示 | 跨组件的全局事件（用 toast/通知） |

#### R-U6 · 反馈渠道必须按场景选用，禁止 alert/prompt/confirm
- **等级**：MUST
- **分类**：交互
- **规则**：面向用户的反馈**必须**按上表选用合适的渠道。**禁止**使用浏览器原生 `alert()` / `prompt()` / `confirm()`（样式无法主题化、阻塞、不专业）。
- **正例**：复制成功 → `showToast(t('copied'))`；删除确认 → 模态对话框；模块消息 → 通知中心。
- **反例**：`if (!confirm('确定?')) return` → 违反；`alert('保存成功')` → 违反（应用 toast）。
- **为什么**：原生对话框破坏主题一致性（违背用户主权 R-P4）与跨平台体验。
- **检查方法**：`grep -rn "alert(\|prompt(\|confirm(" src/components src/app` 应为空。

#### R-U7 · Toast 必须自动消失且不打断用户
- **等级**：SHOULD
- **分类**：交互、反馈
- **规则**：Toast **应该**自动消失（当前约 2.2s），**应该**不阻塞操作（`pointer-events` 受控）、不抢焦点。错误级 toast **可以**视觉更强（红色边）。同时多个 toast **应该**纵向堆叠不重叠。
- **为什么**：toast 的本质是「不打断的轻反馈」；抢焦点或常驻就退化成模态。
- **检查方法**：复用 `notification-ui.ts` 的 `showToast`，不自造 toast。

#### R-U8 · 需要回顾的消息进通知中心，不进 toast
- **等级**：SHOULD
- **分类**：交互
- **规则**：来自插件的通知、重要的系统事件（模块崩溃、安装完成等用户**可能想事后查看**的消息）**应该**进通知中心（持久化到 `notifications` 表），而非只用 toast 一闪而过。
- **为什么**：toast 是易失的；用户没盯着就会错过。
- **检查方法**：插件 `notification.send` 走通知中心；瞬时操作反馈走 toast。

---

## 三、空态、加载态、错误态

承接 `product/02`（无假数据）与 `frontend/02`（三态）的前端落点。

#### R-U9 · 列表/面板必须有统一的空态
- **等级**：MUST
- **分类**：交互、无假数据
- **规则**：所有展示集合数据的组件（列表、面板、看板）**必须**在数据为空时渲染 `EmptyState`（`components/ui/EmptyState.tsx`），含标题 + 可选引导文案/行动按钮。空态文案**必须**走 i18n。**禁止**用假数据填充空列表。
- **正例**：`<EmptyState title={t('notifications.empty')} hint={t('notifications.emptyHint')} />`
- **反例**：空列表时什么都不渲染（留一块空白）→ 用户以为加载未完成。
- **为什么**：空态是诚实信号 + 引导机会（R-F4）。
- **检查方法**：每个集合组件核对空分支。

#### R-U10 · 加载态必须有视觉指示
- **等级**：MUST
- **分类**：交互
- **规则**：异步取数时**必须**显示加载指示（骨架屏 / spinner / 文案），**禁止**在加载期间显示空数据态（会被误认为「就是空的」）或假数据。
- **为什么**：加载与空是两种状态，混为一谈会让用户误判。
- **检查方法**：loading 分支与 empty 分支分开。

---

## 四、快捷键体系

#### R-U11 · 全局快捷键用 Cmd/Ctrl 修饰，跨平台兼容
- **等级**：SHOULD
- **分类**：交互、可访问性
- **规则**：快捷键**应该**用 `Cmd`（mac）/ `Ctrl`（Win/Linux）修饰，判断时用 `event.metaKey || event.ctrlKey`。全局快捷键（如 `Cmd+K` 命令面板）**应该**在 `ShellLayout` 层统一注册，避免重复绑定。**禁止**劫持浏览器/系统级快捷键。
- **为什么**：跨平台一致性 + 不与系统冲突。
- **检查方法**：新增快捷键时核对修饰键与是否已有绑定。

#### R-U12 · 命令面板（Cmd+K）是全局入口
- **等级**：SHOULD
- **分类**：交互
- **规则**：全局搜索/快速跳转**应该**统一收敛到 `CommandPalette`（`Cmd+K`），而非各自造搜索框。命令面板支持模块跳转、文件搜索（含 `content:` 全文）、功能入口。
- **为什么**：单一入口降低认知负担（符合用户主权 R-P4）。
- **检查方法**：新功能搜索入口优先并入命令面板。

---

---

## 五、无边框窗口拖拽交互规范

由于 Natives Control Hub 小组件移除了所有的 OS 原生标题栏与窗口控制 Chrome，窗口移动需要借助页面内容本身进行承载：

#### R-U12.1 · 水晶卡片主体必须支持拖动且排斥交互控件
- **等级**：MUST
- **分类**：交互、桌面集成
- **规则**：
  - 小组件的卡片背景或主体面板**必须**声明 `-webkit-app-region: drag;`，使用户在桌面上可以按住任意无交互背景直接拖动窗口。
  - 卡片内部的所有交互式控件（包括但不限于按钮 `button`、输入框 `input`、滑块 `switch`、超链接 `a` 等）**必须**强制声明 `-webkit-app-region: no-drag;`。
- **为什么**：如果不进行交互排斥，拖拽属性会劫持底层操作系统的鼠标单击、聚焦以及文本选中事件，使得用户无法点击按钮或操作输入框。

---

## 五-2、Workspace Free Canvas 交互规范

承接 ADR-0021 双布局决策：Home 同时提供 Structured Canvas（历史名 Compact Grid；PWSV2 2026-08-23 词表统一为 `structured | free`，structured 为默认）与 Free Canvas，Free Canvas 是自研轻量 DOM 画布，只服务 Workspace 自由布局。

#### R-U13 · Free Canvas 是 bounded DOM 画布，禁止无限画布与协同类实现
- **等级**：MUST
- **分类**：交互、Workspace 双布局
- **规则**：Free Canvas **必须**是 bounded DOM 画布：固定视口、zoom 有 clamp 上下限、空画布 fit 有确定结果（确定性收敛的初始视口）。只实现 Workspace 所需能力：Drag / Resize / Pan / Zoom / Snap / Multi-select / Group / Frame / Z-order。**禁止**无限画布、Yjs/CRDT、AFFiNE 式画布、多人实时协作、Plugin Runtime。
- **反例**：zoom 无上限、拖到边界外可无限平移的画布；为协同引入 CRDT 同步层。
- **为什么**：ADR-0021 §2 明确不做绘图工具、Connector、CRDT、BlockSuite/Yjs、多人协同与通用建模器；任何「扩展成绘图/协同/建模平台」的诉求需新 ADR。

#### R-U14 · 画布指针手势的持久化只在停止/flush 时发生
- **等级**：MUST
- **分类**：交互、性能
- **规则**：Free Canvas 的 drag / resize / pan / zoom 期间指针移动**必须**只更新内存态；持久化**必须**在 Pointer Stop、Resize Stop 或 debounce/flush 时执行，move 过程中对布局数据的写库次数必须为 0。
- **为什么**：与 product/02 R-F6、ADR-0021 §2（stop/debounce/flush 才持久化，指针移动只写内存）一致，避免拖拽期间写库放大 IO。

---

## 六、Workspace 画布组件交互合同

本节约束个人空间中 Compact Grid 与 Free Canvas 共用的 Widget 体验。目标是让刷新、时间范围、编辑、拖拽和视觉密度在两种布局中保持同一语义；实现必须复用 WidgetShell、WorkspaceDataBroker、现有设计令牌和已安装的 `react-grid-layout`，不得再造第二套组件外壳、数据总线或拖拽引擎。

### 6.1 组件结构与操作层级

一个数据 Widget 只允许以下四层，缺少对应内容时整层不渲染，不保留空占位：

```text
WidgetShell
├─ Header：标题 / 范围摘要 / 状态 / 组件级动作
├─ Summary（可选）：主指标与单位
├─ Body：图表、列表或真实状态
└─ Footer（可选）：更新时间、来源或下一步
```

- Workspace 工具栏承载影响多个 Widget 的动作：刷新全部、共享时间范围、布局编辑。
- Widget Header 只承载当前 Widget 独有的动作；同一动作不得同时常驻在 Workspace 工具栏和每张卡片。
- 删除、拖拽、调整大小仅在编辑态出现；查看态不得用编辑 Chrome 挤压内容。
- Header、Body、空态、错误态必须通过同一个 `WidgetShell` 呈现；禁止各 Widget 自己复制标题栏、卡片 padding 或状态容器。

#### R-U15 · Workspace 必须提供可见、可解释的同步入口
- **等级**：MUST
- **分类**：交互、状态、无假数据
- **规则**：只要当前视图存在远端或 Host 数据 Widget，Workspace 工具栏必须显示一个带 `aria-label`/Tooltip 的“同步数据”按钮，并显示最近一次成功更新时间或“尚未同步”。触发后必须按当前可见 Widget 的 adapter key 去重失效并重新取数；禁止刷新页面、重置布局、重建 Workspace 或并发重复请求。
- **状态**：`idle → syncing → success | error`。同步中按钮保持原尺寸、禁用重复触发并显示进度；成功更新“最近同步”；失败保留旧的真实数据并显示内联 stale/error 状态与重试，不能清空成假零值。事件推送仍是默认更新路径，手动同步是用户兜底，不得演变为高频轮询。
- **组件级例外**：只有当某 Widget 独立失败或使用独立数据源时，Header 才可以显示组件级重试/同步；它必须只刷新自己的 adapter key。
- **检查方法**：同一 adapter key 被多个 Widget 使用时，点击一次只产生一次 loader 请求；同步不得改变 Widget 坐标、尺寸、选择或滚动位置。

#### R-U16 · 时间范围必须是共享上下文，局部覆盖必须显式
- **等级**：MUST
- **分类**：交互、数据、可访问性
- **规则**：当前视图只要存在时间敏感 Widget，Workspace 工具栏必须显示统一 `TimeRangeControl`，预设至少包含“今天 / 7 天 / 30 天 / 90 天”。真实查询 API 支持任意起止日期时可以提供“自定义”，并优先使用原生 `<input type="date">`；不得为日期选择新增依赖，也不得展示后端不支持的伪自定义范围。
- **语义**：默认范围为 30 天；按用户本地时区计算并显示时区提示。结束日期包含当日；未来日期、开始晚于结束和超出真实数据保留期必须在控件边界阻止或给出可执行错误。切换范围必须更新 adapter key，由 DataBroker 去重取数；不能只在图表前端裁切后假装已查询该周期。
- **继承**：时间敏感 Widget 默认继承 Workspace 范围。确有独立分析需要时可以在 Inspector 设置局部覆盖，Header 必须显示“自定义范围”摘要与“恢复跟随”；非时间敏感 Widget 必须忽略该上下文，不显示无效控件。
- **响应式**：空间足够时显示分段预设；窄窗口收敛为一个带当前值的按钮/Popover，主操作仍可见。键盘可完成打开、选择、确认与取消。

#### R-U17 · Widget 排版和内边距必须使用统一密度
- **等级**：MUST
- **分类**：布局、排版、组件
- **规则**：结构值必须引用 `SPACING` / `FONT_SIZE` / `BORDER_RADIUS` / `LAYOUT`，并遵守以下基线，不得在单个 Widget 中用任意 Tailwind 数值另起节奏：

| 项目 | 基线 | 说明 |
|---|---:|---|
| Header 高度 | `LAYOUT.controlCompact`（32px） | 横向 padding `SPACING.md`（12px），操作间距 `SPACING.xs`（4px） |
| Body padding | `SPACING.md`（12px） | 密集表格/热力图可用 `SPACING.sm`（8px），必须由 Widget 类型声明 |
| 区块垂直间距 | `SPACING.sm`（8px） | 大段说明才可用 `SPACING.md`（12px） |
| 标题 | `FONT_SIZE.micro`（12px）/ 600 | 单行省略，使用 UI/Display 字体角色 |
| 正文/标签 | `FONT_SIZE.xs`（13px）/ 400–500 | 行高 1.4–1.5，禁止用 10px 正文换密度 |
| 辅助信息 | `FONT_SIZE.micro`（12px） | 使用 `--text-secondary/tertiary`，必要信息不得用 disabled 色 |
| 主指标 | 20–24px / 600 | 数值使用 mono/tabular-nums，单位与数值分层 |
| 卡片圆角 | `BORDER_RADIUS.md`（12px） | Widget 内部禁止再套同等强度完整卡片 |

- 标题、图标和正文基线必须对齐；图标按钮视觉图标 14–16px、命中区至少 32×32px。
- Surface 由 Widget Definition 的 Surface Policy 决定；普通列表、正文和图表不得每层都加边框、阴影或玻璃。
- loading / empty / error 必须占用与 ready 内容相同的 Body 区域，避免同步时卡片跳高。

#### R-U18 · Grid 与 Canvas 必须使用紧凑且可预期的空间节奏
- **等级**：MUST
- **分类**：布局、Workspace 双布局
- **规则**：Compact Grid 默认卡片间距为 `SPACING.sm`（8px），画布/网格内容外边距为 `SPACING.md`（12px）；最小窗口可收敛为 8px。WidgetShell 自身不得再添加外边距。Free Canvas 的默认 snap 步长为 8px，组件新建或自动排列也必须落在该节奏上。
- **禁止**：同时叠加容器 padding、grid margin、article margin 与 WidgetShell margin；用透明占位项制造间距；通过扩大卡片最小尺寸掩盖内容布局问题。
- **检查方法**：相邻卡片可见边缘间距为 8px；首张卡片到工作区边缘为 12px；1440×900 与 960×600 下均无双倍 gutter。

#### R-U19 · 编辑态必须提供可靠的拖拽、缩放与键盘替代
- **等级**：MUST
- **分类**：交互、可访问性、性能
- **规则**：进入编辑态后，Widget 完整 Header 的非控件空白区域必须成为拖拽命中区，并显示 Grip 与 `grab/grabbing` 光标；不得只提供一个小于 32×32px 的孤立把手。Header 内 button/input/a/menu 以及 Widget Body 必须是拖拽 cancel 区，确保点击、选择文本和滚动不触发移动。
- **Grid**：继续由 `react-grid-layout` 独占 item transform；拖拽阈值为 3px；边界、碰撞与响应式布局必须可预测。Resize handle 只在编辑态显示，视觉命中区至少 16×16px、实际命中区至少 24×24px。
- **Free Canvas**：Pointer Down 必须从实际命中的 node 建立手势并 capture pointer；移动中只写 draft，Pointer Up/Cancel 统一提交或回滚。选中、拖拽、缩放、Pan 的事件必须互斥；交互子元素必须停止冒泡或登记为 cancel 区。锁定节点不得移动，并必须以 SVG 图标 + 文案/Tooltip 表达。
- **反馈**：拖拽开始时提升 dragging elevation/边界，移动中保持 55 FPS，停止后在 120ms 内稳定到落点；写入失败必须恢复最后成功布局并显示可重试错误。
- **键盘**：编辑态中聚焦 Widget 后，Grid 方向键移动一个网格单位、Shift+方向键移动两个网格单位；Free Canvas 方向键移动 8px、Shift+方向键细调 1px。Delete/Backspace 删除前遵循破坏性操作规则；所有操作只在 stop/flush 持久化。

#### R-U20 · 查看态与编辑态必须可辨、可退出且不破坏数据操作
- **等级**：MUST
- **分类**：交互、状态
- **规则**：Workspace 工具栏必须提供单一“编辑布局/完成”切换。编辑态用选中边界、Grip、Resize handle 和轻量提示表达，不给所有卡片铺强 glow；Escape 取消当前手势/选择，第二次 Escape 退出编辑态。退出时必须 flush 已停止的布局更改，并将焦点归还切换按钮。
- **规则补充**：同步、时间范围、图表 Tooltip、链接和表单在查看态可用；编辑态下仍可通过明确控件操作，但 Widget Body 不作为拖拽起点。切换模式不得重新取数、重置时间范围或丢失滚动位置。

---

## 七、本篇合规自检清单

- [ ] 我的反馈选对了渠道，没有用 `alert/prompt/confirm`（R-U6）。
- [ ] toast 自动消失、不抢焦点（R-U7）。
- [ ] 需要回顾的消息进了通知中心（R-U8）。
- [ ] 我的空列表用 `EmptyState` + i18n 文案，没有假数据（R-U9）。
- [ ] 加载态有视觉指示，不与空态混淆（R-U10）。
- [ ] 快捷键跨平台兼容，全局入口走命令面板（R-U11, R-U12）。
- [ ] 无边框窗口的主体卡片支持 `-webkit-app-region: drag`，且所有内部交互按键排斥拖拽（`-webkit-app-region: no-drag`）（R-U12.1）。
- [ ] Free Canvas 是 bounded DOM 画布，无无限画布/CRDT/AFFiNE 式画布/多人协作/Plugin Runtime（R-U13）；拖拽/缩放期间只写内存，停止或 debounce/flush 才持久化（R-U14）。
- [ ] 数据 Workspace 有去重同步入口、最近同步时间与 stale/error 反馈，刷新不改变布局（R-U15）。
- [ ] 时间敏感 Widget 继承共享时间范围，局部覆盖可见且可恢复，边界与时区明确（R-U16）。
- [ ] Widget 结构、字体、内边距、图标命中区与 Surface Policy 统一（R-U17）。
- [ ] Grid 间距 8px、外边距 12px，无双倍 gutter；Canvas snap 使用同一节奏（R-U18）。
- [ ] 编辑态 Header 可可靠拖拽，交互控件与 Body 不误触，键盘路径和失败回滚完整（R-U19/R-U20）。
