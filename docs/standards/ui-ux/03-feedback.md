# UI/UX 03 · 动效、焦点与可访问性

> 版本：4.0.0 · 日期：2026-09-14

#### R-U18 · 动效时长统一

- **等级**：MUST
- 微交互约 120ms，常规切换约 200ms，面板入场不超过 300ms；非循环装饰动效不超过 400ms。
- 使用现有 CSS motion custom properties；同一 Surface 不自造另一套曲线。
- spinner 可以 linear，其他状态变化优先统一 easing。

#### R-U19 · 动画优先 transform/opacity

- **等级**：MUST
- 装饰动画不使用可由 transform/opacity 代替的 top/left/width/height/filter/box-shadow 动画。
- 直接拖拽和确定型进度条可以更新布局，但不能与输入竞争。
- 不为每个 Widget 建永久 animation frame。

#### R-U20 · 减弱动效完整

- **等级**：MUST
- `prefers-reduced-motion: reduce` 下移除装饰位移，过渡即时化或缩短。
- 持续 spinner 停止或替换为静态图标和加载文案。
- 状态不能只靠运动表达。

#### R-U21 · 焦点可见且可恢复

- **等级**：MUST
- 使用 `:focus-visible`；禁止全局 `outline: none`。
- `<dialog>` 打开后聚焦有效控件，关闭后恢复触发点。
- iframe/新页面切换后提供可预测初始焦点。

#### R-U22 · 交互元素使用原生语义

- **等级**：MUST
- 优先 button、a、input、select、dialog；自定义控件补 role、tabindex、键盘事件、aria 状态。
- Enter/Space 激活按钮类控件；Escape 关闭可取消浮层；方向键只用于符合平台预期的复合控件。
- 点击目标与视觉目标满足 `01-design-tokens.md` 的命中区。

#### R-U23 · 状态不只靠颜色

- **等级**：MUST
- error/warning/success、选择、连接和运行状态同时使用文字、图标或形状。
- 图表提供标签、值和必要的可访问名称；Tooltip 不能是唯一信息源。

#### R-U24 · 动态反馈不打断用户

- **等级**：MUST
- 普通 toast 不抢焦点；必要的状态更新使用合适的 `aria-live` 强度。
- 高频进度合并更新，避免每个 token/event 都触发屏幕阅读器播报。
- 自动消失消息不得承载唯一修复信息。

## 合规自检

- [ ] 动效时长和属性有界。
- [ ] reduced-motion 下无持续装饰运动。
- [ ] 焦点可见、模态可进出、触发点可恢复。
- [ ] 控件有原生或完整自定义语义。
- [ ] 状态不只靠颜色，动态播报不过载。
