# UI/UE 03 · 反馈、动效与可访问性

> **版本**: 1.1.0 · **日期**: 2026-08-12
> **关联 ADR**: 无
> **关联源文件**: `src/lib/design-tokens.ts`（`TRANSITION` / `SPINNER_EASING`）、`src/app/styles/tokens.css`（CSS motion 变量）、`src/app/globals.css`、`src/lib/chime.ts`（提示音）、`src/hooks/useFocusTrap.ts`

---

## 一、本篇要约束什么

动效与反馈是「质感」的来源，但也最易被滥用——过多动效拖慢操作，过少则生硬。本篇约束**动效曲线与时长**、**键盘焦点**、**提示音**三件事，把 `STYLE_GUIDE_AUDIT.md` 提出的 macOS 原生质感方向固化为规则。

---

## 二、动效

#### R-U13 · 动效必须使用统一缓动曲线
- **等级**：MUST
- **分类**：反馈
- **规则**：所有过渡/动画**必须**引用 `design-tokens.ts` 的 `TRANSITION`（`fast` / `normal` / `slow`，统一用 `cubic-bezier(0.16, 1, 0.3, 1)`）或对应 CSS 变量。**禁止**自定义其它缓动曲线或写死 `ease` / `linear`。持续旋转的加载 spinner 是唯一 `linear` 例外，必须引用 `SPINNER_EASING` / `--spinner-easing`，不得把该例外扩展到普通入场或状态切换。
- **正例**：`transition: TRANSITION.fast`、`animation: edIn var(--transition-normal)`。
- **反例**：`transition: all 0.5s ease` → 曲线不统一，整体节奏混乱。
- **为什么**：统一曲线 = 统一「物理感」，是 macOS 级质感的基础（见 `STYLE_GUIDE_AUDIT.md` C 节）。
- **检查方法**：`grep -rn "transition\|animation" src` 核对曲线来源。

#### R-U14 · 动效时长分级，禁止长动效阻塞操作
- **等级**：SHOULD
- **分类**：反馈、性能
- **规则**：动效时长**应该**按 `TRANSITION` 三档：微交互 fast（`120ms`）、常规 normal（`200ms`）、入场/面板 slow（`300ms`）。**禁止**超过约 `400ms` 的非循环动效（会让人感觉卡顿）。
- **为什么**：长动效在频繁操作时是负担。
- **检查方法**：review 动效时长。

#### R-U15 · 面板/iframe 切换有入场动效
- **等级**：SHOULD
- **分类**：反馈
- **规则**：面板切换、iframe 打开、命令面板唤起**应该**有 `edIn`（淡入+轻微上移）等入场动效，用 `cubic-bezier(0.16,1,0.3,1)`。
- **为什么**：硬切会显得「网页套壳」，入场动效是原生感的细节（`STYLE_GUIDE_AUDIT.md` C 节）。
- **检查方法**：主要切换入口核对入场动效。

#### R-U15.1 · 装饰动效必须优先 transform / opacity 并响应减弱动效
- **等级**：MUST
- **分类**：反馈、性能、可访问性
- **规则**：不改变信息布局的入场、退出、hover 与状态切换必须只动画 `transform` / `opacity`；禁止用 `top` / `left` / `width` / `height` / `filter` / `box-shadow` 制作可由合成层完成的装饰动效。用户直接拖拽面板、确定型进度条和确需重排的布局变化属于例外，但必须避免与输入竞争。
- **减弱动效**：新增动效必须提供 `@media (prefers-reduced-motion: reduce)` 或等价运行时分支，移除装饰位移并将过渡即时化或缩短至不可感知。持续 spinner 在减弱动效下必须停止旋转或替换为静态图标 + 加载文案，且任何状态不得只靠运动表达。
- **为什么**：`transform` / `opacity` 通常不触发布局和重绘；尊重系统减弱动效可避免眩晕并保持状态可理解。
- **检查方法**：DevTools 检查动画属性；开启系统“减弱动态效果”验证面板、Toast、Popover 与 loading 状态。

---

## 三、键盘焦点与可访问性

#### R-U16 · 用 :focus-visible 而非 :focus 提供焦点环
- **等级**：MUST
- **分类**：可访问性、反馈
- **规则**：焦点环样式**必须**用 `:focus-visible`（仅键盘焦点时显示精致的 `--accent` 边框），**禁止**用 `:focus`（鼠标点击也显示生硬焦点环）或 `outline: none` 全局去除焦点。模态/陷阱焦点**必须**用 `useFocusTrap`。
- **为什么**：见 `STYLE_GUIDE_AUDIT.md` 第 4 点——键盘焦点与鼠标焦点混杂不符合高品质应用的直觉；同时焦点环是键盘用户的导航生命线，不可全局去除。
- **检查方法**：`globals.css` 含 `:focus-visible` 规则；无全局 `outline: none`。

#### R-U17 · 交互元素必须有可达的键盘路径
- **等级**：SHOULD
- **分类**：可访问性
- **规则**：所有交互元素（按钮、链接、输入、可点击项）**应该**能用 Tab 到达、Enter/Space 触发。自定义可点击的 `<div>` **应该**加 `role="button"` / `tabIndex={0}` / `onKeyDown` 处理。
- **为什么**：键盘用户与辅助技术依赖语义化与可达性。
- **检查方法**：review 自定义交互元素的可访问属性。

---

## 四、提示音

#### R-U18 · 提示音是可选的、有节制的、可关闭的
- **等级**：SHOULD
- **分类**：反馈
- **规则**：提示音（`chime.ts`）**应该**仅用于真正需要用户注意的事件（如新截图、长任务完成），**禁止**泛滥用于每次点击/成功。提示音**应该**可在设置中关闭。**禁止**自动播放长音频。
- **为什么**：声音打扰度高；滥用会逼用户静音整个应用。
- **检查方法**：新增提示音前自问「这事值得响吗」；设置中可关。

---

## 五、本篇合规自检清单

- [ ] 我的动效用 `TRANSITION` 与统一曲线，没有 `ease`/`linear`（R-U13）。
- [ ] 动效时长不超过约 0.4s（R-U14）。
- [ ] 装饰动效只使用 transform/opacity，并在 reduced-motion 下取消位移与持续旋转（R-U15.1）。
- [ ] 焦点环用 `:focus-visible`，没有全局 `outline: none`（R-U16）。
- [ ] 自定义交互元素有键盘可达路径与 ARIA 属性（R-U17）。
- [ ] 提示音节制且可关闭（R-U18）。
