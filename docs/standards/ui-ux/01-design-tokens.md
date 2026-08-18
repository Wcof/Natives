# UI/UE 01 · 设计令牌与主题系统

> **版本**: 1.2.1 · **日期**: 2026-08-12
> **关联 ADR**: [ADR 0009](../../adr/0009-monochrome-dashboard-theme.md)、[ADR 0010](../../adr/0010-global-neutral-spectrum.md)
> **关联源文件**: `src/lib/design-tokens.ts`（TS 常量）、`src/app/styles/tokens.css`（CSS 语义与原始色板）、`src/lib/theme-engine.ts`（深浅主题 + Zod 校验 + 应用）、`src/app/globals.css`、`docs/STYLE_GUIDE_AUDIT.md`（历史改造清单，已被本篇收口为规范）

---

## 一、本篇要约束什么

Natives 的视觉一致性靠「设计令牌」保证——颜色、间距、圆角、字号、阴影、过渡都从统一源头出发，深浅主题通过覆盖语义令牌切换。本篇把全局中性色阶、主题契约、数据可视化、Vibe 材质层与字体绑定钉成规范。

> `STYLE_GUIDE_AUDIT.md` 是一次性改造清单（历史），**其约束性已被本篇取代**；本篇是权威。

---

## 二、两套令牌：TS 常量 + CSS 变量

Natives 同时维护两套令牌，各有用途：

| 令牌集 | 位置 | 用途 | 是否随主题变 |
|--------|------|------|-------------|
| **TS 常量** | `src/lib/design-tokens.ts`（`SPACING` / `FONT_SIZE` / `BORDER_RADIUS` / `TRANSITION` / `LAYOUT` / `SHADOW`） | 给 inline style / 计算用，**不随主题变** | 否（结构性数值） |
| **CSS 语义变量** | `tokens.css` 的固定色板与派生角色 + `theme-engine.ts` 的 `THEMES` 核心主题值 | 给样式表 / `var(--x)` 引用，**随主题变** | 是（主题相关） |

#### R-U1 · 视觉值必须引用令牌，禁止魔法数字
- **等级**：MUST
- **分类**：主题、命名
- **规则**：组件中的所有视觉值**必须**引用令牌：
  - 主题相关（颜色等）→ CSS 语义变量 `var(--background)` / `var(--surface)` / `var(--primary)`。
  - 结构性（间距/圆角/字号/阴影/过渡）→ `design-tokens.ts` 的常量 `SPACING.md` / `BORDER_RADIUS.lg` / `TRANSITION.fast`。
  - **禁止**硬编码 `#1a1a2e`、`padding: 13px`、`0.2s ease` 等魔法值。
- **正例**：`padding: SPACING.md`、`color: 'var(--primary)'`、`transition: TRANSITION.fast`。
- **反例**：`background: '#0b0c0a'`（硬编码，换皮肤失效）；`border-radius: 8px`（应 `BORDER_RADIUS.lg`）。
- **为什么**：令牌是深浅主题与视觉一致性的唯一保证；魔法值会让换肤局部失效、让间距节奏失控。
- **检查方法**：见 `frontend/01` R-E4 的 grep 思路；新增魔法值时优先找现有令牌。

---

## 三、深浅主题契约、全局中性色阶与 Liquid Glass 边界

当前 Natives 主题定义分为两层：`theme-engine.ts` 中的常规 `dark` / `light` 主题，以及 `globals.css` 中的 `--vibe-*` / `--liquid-*` / `--glass-*` 材质层。历史主题 ID 只可作为兼容别名，新增规范与组件必须使用 `dark` / `light` 语义。

参考 CodePilot 的 macOS visual profile 后，Natives 的结论是：高级磨砂玻璃不能靠到处叠 `backdrop-filter` 实现。它必须先有透明窗口与透明根节点作为底座，再由 shell / navigation / floating control 分层承载玻璃材质，内容阅读层保持稳定和可读。真正的 Liquid Glass 只用于少数高价值容器或控件，不作为全局背景滤镜滥用。

#### R-U2 · 深浅主题必须共享语义键并使用全局中性色板
- **等级**: MUST
- **分类**：主题
- **规则**：每套常规主题**必须**提供完全相同的语义键集合，至少覆盖 `background` / `surface` / `surface-hover` / `sidebar` / `border` / `border-subtle` / `text` / `text-body` / `text-secondary` / `text-disabled` / `primary` / `primary-hover` / `primary-soft` / `primary-dark`。磨砂材质可以增加 `--vibe-*`，但不得建立另一套颜色权威。
- **全局单色规则**：页面、侧栏、弹窗、控件、图表和第一方模块必须以 R-U2.5 的中性色板为品牌基底，不再由绿色或橙色主导。彩色仅允许用于 danger、warning、success、info、Diff、终端 ANSI、用户内容和第三方嵌入内容。
- **为什么**：全局共享同一色板和语义角色，既能保持黑白灰品牌一致性，也能用足够的光度层次表达界面结构和数据体量。

#### R-U2.1 · 玻璃效果必须遵守材质分层矩阵
- **等级**：MUST
- **分类**：主题、视觉效果、可访问性
- **规则**：所有磨砂玻璃、液态玻璃、透明背景效果**必须**按下表分层使用，禁止把玻璃效果无差别铺到所有容器：

| 层级 | 允许材质 | 典型组件 | 红线 |
|------|----------|----------|------|
| Window / Root | 透明窗口 + 透明根节点 | Tauri 主窗口、`html` / `body` / `#root` / `#app` | 不得出现不透明白底或黑底遮住桌面/背景 |
| Shell | 半透明底色 + `backdrop-filter` + 轻阴影 | `.shell`、顶栏、应用外壳 | 不得用纯色大面积盖住窗口底材 |
| Navigation | 半透明 tint + blur + 边缘高光 | Sidebar、RightPanel、导航列表 | 不得完全透明到失去面板边界 |
| Floating Control | CSS 材质模拟 + 强边界 | Command Palette、Popover、Context Menu、Toast、Tooltip | 不得宣称为原生 vibrancy；必须保持文本对比度 |
| Content / Reading | 稳定、不透明或近不透明表面 | 正文、设置表单、代码、diff、终端正文、iframe 内容 | 禁止为了玻璃感降低可读性或 artifact 保真 |
| Widget / Control Hub | Polished Crystal / Liquid Glass | `.main-card`、桌面悬浮小组件 | 必须隐藏应用 Chrome，只保留单一高质感容器 |

- **正例**：Sidebar 使用 `--vibe-sidebar-bg` + `backdrop-filter`；正文列表卡片使用 `--vibe-content-bg` 时必须保持足够 alpha 和文字对比；小组件主卡使用统一 Liquid Glass 参数。
- **反例**：给所有 `.card`、代码块、终端正文、设置表单统一加 `blur(40px) saturate(200%)`。
- **为什么**：玻璃材质是为了让 shell 更轻、层次更清楚，而不是牺牲阅读。CodePilot 的有效经验也是“外壳和控件玻璃化，内容层保持稳定”。
- **检查方法**：review 新增 `backdrop-filter` 的 selector，确认它属于 Shell / Navigation / Floating / Widget 层；若作用于 Content 层，必须说明可读性理由并提供截图验证。

#### R-U2.2 · Liquid Glass 必须由令牌和组件共同约束
- **等级**：MUST
- **分类**：主题、组件
- **规则**：Liquid Glass 不是普通磨砂玻璃的同义词。实现时必须同时满足：
  - **底座**：Tauri window `transparent: true`，且 `html` / `body` / `#root` / `#app` 保持透明，避免遮住系统或应用背景。
  - **材质令牌**：使用 `--liquid-blur-factor` / `--liquid-saturation-factor` / `--liquid-radius-factor` 作为调参入口，再派生 `--glass-*`、`--vibe-*`、`--main-card-*`，禁止组件自造另一套玻璃参数。
  - **视觉资产组合**：至少包含半透明 tint、backdrop blur、饱和度提升、细边缘高光、内阴影/外阴影、必要时的低透明噪点纹理。
  - **物理畸变**：只有 Widget / Control Hub 或特别指定的 hero control 才能使用 `liquid-glass-react` 这类位移/折射组件；普通列表、表单、代码阅读区不得使用物理畸变。
- **为什么**：液态玻璃的高级感来自“折射、透明、边缘、阴影、噪点、背景穿透”的组合，单独加 blur 只会变成廉价毛玻璃。
- **检查方法**：新增 Liquid Glass 效果时，检查是否复用上述变量和组件参数；检查是否影响输入、阅读、拖拽与点击。

#### R-U2.3 · 背景磨砂必须先保证透明链路
- **等级**：MUST
- **分类**：主题、桌面集成
- **规则**：应用背景磨砂玻璃必须建立完整透明链路：
  - `src-tauri/tauri.conf.json` 主窗口保持 `transparent: true`、`decorations: false`、`visible: false`。
  - Renderer 根节点（`html`, `body`, `#root`, `#app`, `main`）保持 `background: transparent`。
  - Shell 层再叠加半透明材质，而不是由根节点提供实色背景。
  - macOS 原生 vibrancy 若后续接入，必须作为窗口级能力处理，不得在 DOM 文档中声称每个 Popover 都是原生 vibrancy。
- **为什么**：只要透明链路中任一祖先变成不透明，底层桌面/窗口材质就完全无法穿透，`backdrop-filter` 只能模糊应用内部像素。
- **检查方法**：用 DevTools 检查根节点 computed background；启动 Tauri 窗口检查是否有白底/黑底闪烁；FOUC 仍按 `technical/02` 的 `theme_ready_signal` 约束执行。

#### R-U2.4 · 内容层玻璃化必须以可读性为上限
- **等级**：MUST
- **分类**：可访问性、视觉效果
- **规则**：Content / Reading 层如果使用 `--vibe-content-bg`，必须更像“高质量半透明纸面”，而不是强折射玻璃。正文、代码、diff、终端、iframe、设置表单中的文字对比度优先级高于材质感，禁止使用强 blur、色差、位移、动态扭曲。
- **正例**：内容卡片使用轻透明背景、细边框和低强度阴影，但文本区域本身保持清晰。
- **反例**：终端正文后方持续透出高饱和背景并叠加 200px blur，导致字符边缘发糊。
- **为什么**：Natives 是生产力桌面应用，内容可信度和长时间阅读舒适度高于视觉炫技。
- **检查方法**：深色/浅色两套主题下查看代码、表单、终端、长列表；若文字边缘变糊或背景抢占注意力，必须降低玻璃强度。

#### R-U2.5 · 全局原始中性色板必须固定且不可按页面改写

- **等级**：MUST
- **分类**：主题、数据可视化、命名
- **规则**：系统必须提供以下 15 级 `--neutral-*` 原始色阶。该色阶在深浅主题中保持同值；主题切换只改变语义令牌映射，禁止反向重定义原始色值。

| 原始令牌 | HEX | RGB | 基准用途 |
|---|---:|---:|---|
| `--neutral-0` | `#010101` | `1, 1, 1` | 最深 Canvas、选中亮色控件上的文字 |
| `--neutral-100` | `#111113` | `17, 17, 19` | 深色 Shell 次级背景 |
| `--neutral-150` | `#18181A` | `24, 24, 26` | 深色内容块、浅色选中控件 |
| `--neutral-200` | `#202024` | `32, 32, 36` | 深色低层级填充、零值热力格 |
| `--neutral-250` | `#27262B` | `39, 38, 43` | 深色未选控件 |
| `--neutral-300` | `#323137` | `50, 49, 55` | 深色 Hover、强边框 |
| `--neutral-400` | `#48474D` | `72, 71, 77` | 低强调图形、浅色正文 |
| `--neutral-500` | `#646268` | `100, 98, 104` | 浅色次要文字、图表中低量级 |
| `--neutral-600` | `#7F7D83` | `127, 125, 131` | 中性中点、图表中量级 |
| `--neutral-700` | `#9B999E` | `155, 153, 158` | 深色未选文字、辅助信息 |
| `--neutral-800` | `#B7B5BA` | `183, 181, 186` | 图表中高量级、浅色弱边界 |
| `--neutral-850` | `#D4D3D7` | `212, 211, 215` | 深色正文、浅色 Hover |
| `--neutral-900` | `#E8E7EA` | `232, 231, 234` | 浅色未选控件、深色高量级图形 |
| `--neutral-950` | `#FAFAFC` | `250, 250, 252` | 深色主要文字和选中控件、浅色 Canvas |
| `--neutral-1000` | `#FFFFFF` | `255, 255, 255` | 浅色内容块；不得大面积替代 Canvas |

- **正例**：在主题定义中将 `surface` 映射到 `--neutral-150`，图表组件消费 `--chart-volume-4`。
- **反例**：某个页面自行新增 `#707075`，或在浅色主题中把 `--neutral-150` 改成另一颜色。
- **为什么**：固定色板让背景、边框、文字和图表共享同一光度语言，避免“同为灰色但每页不同”。

#### R-U2.6 · 深浅主题必须按固定语义映射消费色板

- **等级**：MUST
- **分类**：主题、可访问性
- **规则**：以下核心角色必须采用固定映射；组件只能消费角色令牌，不得直接消费 `--neutral-*`，数据可视化令牌除外。

| 语义角色 | 深色主题 | 浅色主题 |
|---|---|---|
| `--background` | `--neutral-0` (`#010101`) | `--neutral-950` (`#FAFAFC`) |
| `--surface` | `--neutral-150` (`#18181A`) | `--neutral-1000` (`#FFFFFF`) |
| `--canvas` | `--background` | `--background` |
| `--raised` | `--surface-hover` | `--surface` |
| `--inset` | `--neutral-100` (`#111113`) | `--surface-hover` |
| `--composer` | `--surface-hover` | `--surface` |
| `--surface-hover` | `--neutral-200` (`#202024`) | `--neutral-900` (`#E8E7EA`) |
| `--sidebar` | `--neutral-100` (`#111113`) | `--neutral-900` (`#E8E7EA`) |
| `--border-subtle` | `--neutral-200` (`#202024`) | `--neutral-900` (`#E8E7EA`) |
| `--border` | `--neutral-300` (`#323137`) | `--neutral-850` (`#D4D3D7`) |
| `--border-strong` | `--neutral-400` (`#48474D`) | `--neutral-800` (`#B7B5BA`) |
| `--text` | `--neutral-950` (`#FAFAFC`) | `--neutral-150` (`#18181A`) |
| `--text-body` | `--neutral-850` (`#D4D3D7`) | `--neutral-400` (`#48474D`) |
| `--text-secondary` | `--neutral-700` (`#9B999E`) | `--neutral-500` (`#646268`) |
| `--text-disabled` | `--neutral-500` (`#646268`) | `--neutral-700` (`#9B999E`) |
| `--text-tertiary` | `--neutral-600` (`#7F7D83`) | `--neutral-600` (`#7F7D83`) |
| `--text-ghost` | `--text-disabled` | `--text-disabled` |
| `--selection` | 24% `--neutral-950` | 18% `--neutral-150` |
| `--primary` | `--neutral-950` (`#FAFAFC`) | `--neutral-150` (`#18181A`) |
| `--primary-hover` | `--neutral-850` (`#D4D3D7`) | `--neutral-300` (`#323137`) |
| `--primary-soft` | `--neutral-250` (`#27262B`) | `--neutral-900` (`#E8E7EA`) |
| `--primary-dark` | `--neutral-0` (`#010101`) | `--neutral-0` (`#010101`) |
| `--control-bg` | `--neutral-250` (`#27262B`) | `--neutral-900` (`#E8E7EA`) |
| `--control-bg-hover` | `--neutral-300` (`#323137`) | `--neutral-850` (`#D4D3D7`) |
| `--control-fg` | `--neutral-700` (`#9B999E`) | `--neutral-500` (`#646268`) |
| `--control-selected-bg` | `--neutral-950` (`#FAFAFC`) | `--neutral-150` (`#18181A`) |
| `--control-selected-bg-hover` | `--neutral-850` (`#D4D3D7`) | `--neutral-300` (`#323137`) |
| `--control-selected-fg` | `--neutral-0` (`#010101`) | `--neutral-950` (`#FAFAFC`) |

- **规则补充**：主要正文对比度必须达到 WCAG AA 4.5:1；大号文字、图标、图表关键边界至少达到 3:1。禁用状态可以低于正文标准，但不得承担必要信息。
- **为什么**：语义反转比数学反色更可控，能保持两套主题相同的信息层级。

#### R-U2.7 · 图表体量必须使用 0 + 8 级顺序色阶

- **等级**：MUST
- **分类**：数据可视化、无假数据、可访问性
- **规则**：热力图、分布条、密度图和其他“数值越大视觉越强”的图表必须使用 `--chart-volume-0` 至 `--chart-volume-8`。`0` 仅代表真实零值或无活动；缺失数据必须使用独立的空态/纹理，禁止伪装成零值。

| 图表令牌 | 深色主题 | 浅色主题 |
|---|---|---|
| `--chart-volume-0` | `#202024` | `#E8E7EA` |
| `--chart-volume-1` | `#323137` | `#D4D3D7` |
| `--chart-volume-2` | `#48474D` | `#B7B5BA` |
| `--chart-volume-3` | `#646268` | `#9B999E` |
| `--chart-volume-4` | `#7F7D83` | `#7F7D83` |
| `--chart-volume-5` | `#9B999E` | `#646268` |
| `--chart-volume-6` | `#B7B5BA` | `#48474D` |
| `--chart-volume-7` | `#D4D3D7` | `#323137` |
| `--chart-volume-8` | `#FAFAFC` | `#18181A` |

- **映射公式**：默认使用图表当前可见数据域内的线性映射。`value = 0` 使用 level 0；非零值使用 `max(1, ceil(value / visibleMax * 8))`。当 `visibleMax = 0` 时全部使用 level 0。
- **长尾例外**：仅在数据明显长尾时允许平方根或对数映射；必须在图例中标明“平方根刻度”或“对数刻度”，Tooltip 仍显示真实值。
- **禁止**：用临时 `opacity`、任意 rgba、品牌色或语义状态色表达普通数据体量。
- **为什么**：同一顺序色阶能让用户快速判断数量级，同时避免深浅主题中“越亮/越暗”的含义相反。

#### R-U2.8 · 分类图表不得只依赖相邻灰色区分类别

- **等级**：MUST
- **分类**：数据可视化、可访问性
- **规则**：分类分布必须按数值降序展示并同时提供标签与真实值。默认最多展示前 6 类，第 7 类起合并为“其他”；禁止仅靠颜色识别类别。
- **折线规则**：最多同时展示 3 条主线，分别使用主题下的高、中、低对比中性色，并辅以实线、虚线、点线或数据点形状。超过 3 条时改用筛选器，不继续堆叠相近灰线。
- **饼图规则**：只有类别不超过 6 且标签可同时显示时才可使用；否则优先使用排序横向条形图。
- **为什么**：灰阶适合表达顺序和体量，不适合无限扩展类别；标签和线型能避免色觉、屏幕质量与相邻灰度导致的歧义。

#### R-U2.9 · 语义彩色不得被图表灰阶吞并或滥用

- **等级**：MUST
- **分类**：主题、数据可视化、可访问性
- **规则**：danger、warning、success、info、Diff 与终端 ANSI 保留独立色相。它们只表达明确语义，不得作为普通系列色或装饰强调色；普通增长/下降数据若不代表成功或故障，仍使用中性色并配合 `+` / `−`、箭头和文字说明。
- **为什么**：限制彩色出现频率，既保持全局黑白灰品牌，也让真正重要的异常和状态更醒目。

#### R-U2.10 · 阅读工作台必须共享结构令牌

- **等级**：MUST
- **分类**：布局、排版、组件
- **规则**：会话、编辑器、设置等生产力工作台必须引用共享结构令牌，不得按页面复制同义尺寸：
  - `--reading-width` / `LAYOUT.readingWidth`：正文与 Composer 的最大阅读宽度，基线 `760px`。
  - `--titlebar-height` / `LAYOUT.titlebarHeight`：主窗口标题栏高度，基线 `48px`。
  - `--control-compact` / `LAYOUT.controlCompact`：紧凑桌面控件的可见高度，基线 `32px`；命中区不得因图标较小而同步缩小。
  - `--turn-gap` / `LAYOUT.turnGap`：会话新回合的垂直分隔，基线 `40px`；普通同回合内容使用既有间距令牌。
- **规则补充**：正文、代码、表单必须优先消费稳定的 `--canvas` / `--raised` / `--inset` / `--composer` 表面角色；浮层和选中态分别消费 `--border-strong` / `--selection`，禁止用品牌色制造普通结构层级。
- **为什么**：统一阅读宽度、标题栏与回合节奏，能让 Shell、Assistant 和设置页共享同一视觉骨架，同时避免为参考设计复制第二套色板。


#### R-U3 · 皮肤切换必须即时且经 Zod 校验
- **等级**：MUST
- **分类**：主题、数据
- **规则**：皮肤切换**必须**：经 `validateTheme()`（Zod）校验（对于常规皮肤）或经 `globals.css` 级主题选择器响应，最终将 `data-theme` 注入 `html` 节点。常规 `THEMES` 颜色值**必须**匹配 `^#[0-9a-fA-F]{6}$`；透明/rgba/gradient 材质值必须留在 CSS 变量层，禁止塞进 `THEMES` 绕过校验。
- **为什么**：损坏的配置不应注入非法 CSS。

#### R-U4 · 终端配色必须随皮肤联动
- **等级**：MUST
- **分类**：主题
- **规则**：常规皮肤及玻璃皮肤**必须**提供匹配的终端 `background` / `foreground` / `cursor` / `selectionBackground`。切换皮肤时终端**必须**重新应用对应配色。
- **为什么**：终端是界面一大块，配色不联动会形成视觉割裂。

---

## 四、Natives Control Hub 与 Popup/Widget 专有设计规范

Natives 引入了专为桌面小组件 (Widget) 模式设计的 "Polished Crystal Glass" 界面规范，该模式下彻底消除应用 Chrome，使之悬浮于桌面上：

#### R-U5.1 · 桌面小组件模式布局与 100% 透明度红线
- **等级**：MUST
- **分类**：布局、主题
- **规则**：当应用在 Widget/Control Hub 模式下运行时，**必须**隐藏 Sidebar、Header、RightPanel、Terminal 等全部应用 Chrome（通过 `display: none !important` 抑制），且 `html`, `body`, `#root`, `#app`, `.shell`, `.content-area` 等所有包裹层**必须**强制设为 `background: transparent !important`。
- **为什么**：防止任何残留的白色/纯色矩形背景阻挡桌面壁纸，破坏“水晶悬浮”感觉。

#### R-U5.2 · 水晶玻璃容器 (.main-card) 规格
- **等级**：MUST
- **分类**：视觉效果
- **规则**：小组件主体容器（`.main-card`）**必须**符合以下视觉资产组合规范：
  - **iOS 连续曲率**：`border-radius: 28px`，结合 `0.5px` 半透明微边缘（`rgba(255, 255, 255, 0.22)`）以消减锯齿。
  - **色彩蒸馏基质**：`background` 为 `linear-gradient(135deg, rgba(255, 255, 255, 0.24) 0%, rgba(255, 255, 255, 0.06) 100%)`。
  - **高饱和壁纸穿透**：`backdrop-filter: blur(200px) saturate(280%) contrast(110%)`。
  - **多层微影叠合**：使用带上高光、下遮蔽和多重扩散阴影的 HIG 阴影矩阵（`box-shadow: inset 0 1.5px 0 0 rgba(255,255,255,0.75), inset 0 0 0 0.5px ...`）。
  - **微米噪点纹理**：应用 `::after` 遮罩叠加透明度为 `0.025` 的 SVG 碎银分形噪声滤镜。
  - **统一调参入口**：上述强度必须通过 `--main-card-blur` / `--main-card-saturation` / `--main-card-radius` 或 `--liquid-*` 因子派生，避免单点硬编码。
- **正例**：Natives Control Hub 的主体 card 样式。
- **反例**：直接使用 CSS filter shadow，或采用普通纯色半透明，显得“网页塑料感”。

#### R-U5.3 · 桌面小组件液态玻璃实现参考 (Reference Implementation)
- **等级**：MUST
- **分类**：视觉效果、组件
- **规则**：小组件主体容器若采用液态玻璃效果，**必须**引用 `liquid-glass-react` 组件并配置相应物理畸变参数，保证交互及渲染品质。严禁硬编码不一致的玻璃参数或 ad-hoc 阴影导致视觉漂移。
  - **核心参数基准**：
    - `displacementScale`: 64 （边缘折射强度）
    - `blurAmount`: 0.40 （磨砂模糊）
    - `saturation`: 135% （色彩饱和度）
    - `aberrationIntensity`: 2 （色差分离）
    - `elasticity`: 0 （默认静止，禁用 hover 拉伸以防视疲劳，支持按需微调）
    - `cornerRadius`: 28 （对齐 HIG 圆角）
  - **无边框窗口拖拽规范**：主体卡片元素配置 `WebkitAppRegion: 'drag'`（拖拽移动），且内部所有按钮、滑块、标签页切换器等交互控件**必须**强制设为 `WebkitAppRegion: 'no-drag'`，以排除交互事件劫持（符合 R-U13）。
- **验收落点**：[MenubarSurface](../../../src/components/menubar/MenubarSurface.tsx) 与 [MenubarOverview.module.css](../../../src/components/menubar/MenubarOverview.module.css) 是当前 Widget 生产表面；实现是否达标以本节规则和真机证据为准，不将普通 Dashboard 当作 Liquid Glass 基线。
- **为什么**：液态玻璃视觉涉及复杂的 WebGL 着色器和 SVG 滤镜混合，统一参数能防止不同开发阶段风格发生偏移，并确保在 Tauri v2 桌面框架中无边框拖拽和控件响应完美互斥。

---

## 五、实施建议：从 Frosted Glass 到 Liquid Glass

#### R-U5.4 · 背景玻璃升级必须分阶段落地
- **等级**：SHOULD
- **分类**：主题、实施
- **规则**：从现有 Frosted Glass 升级到更接近 Liquid Glass 的效果时，应该按以下顺序推进：
  1. **透明链路验收**：先确认 Tauri 透明窗口、根节点透明、FOUC guard 没有白底闪烁。
  2. **材质矩阵收敛**：先调整 `--vibe-sidebar-*`、`--vibe-toolbar-*`、`--vibe-content-*` 的 alpha / blur / shadow，不改业务组件结构。
  3. **内容层降噪**：确保代码、终端、设置表单、长文本卡片不被强玻璃化。
  4. **Widget 单点液态化**：只在 `.main-card` 或少数高价值控件接入 `liquid-glass-react`，验证性能、拖拽和控件点击。
  5. **截图回归**：深色/浅色各截取主工作台、设置页、终端、Widget，确认无白底、无文字发糊、无控件重叠。
- **为什么**：Liquid Glass 是系统工程，不是单个 CSS 参数。分阶段能避免“越调越糊”和性能回退。

---

## 六、字体与图标绑定

字体系统与颜色系统解耦；深浅主题切换不得改变同一信息层级的字体角色。

#### R-U5 · 标题/品牌/激活态消费 `--font-display`
- **等级**：SHOULD
- **分类**：主题、命名
- **规则**：侧栏品牌、主标题、面包屑激活态、卡片标题等「展示性」文字**应该**用 `font-family: var(--font-display);`。代码文件名、数值、终端数据**应该**用等宽（`var(--font-mono)` 或 `design-tokens` 中的 mono 栈）。正文 UI 用 `var(--font-ui)`。
- **为什么**：固定字体角色可避免主题切换同时改变颜色和排版，降低视觉漂移。
- **检查方法**：标题类元素核对字体来源。

#### R-U5.5 · 统一使用专业 SVG 图标，禁止使用 UI 字符 Emoji
- **等级**：MUST
- **分类**：主题、组件
- **规则**：项目中所有 UI 交互元素、文件类型标识、状态提示、操作按钮处的图标，**必须**统一使用来自 `lucide-react` 的专业 SVG 图标组件，**绝对禁止**使用原生 Emoji 字符（如 `📁`、`⚙️`、`⚠️`、`❌`、`✅` 等）作为 UI 图标。
- **正例**：
  - ActionButton 使用 `<Folder size={14} />` 代替 `"📁"`
  - Toast 使用 `<XCircle size={14} />` 代替 `"❌ "`
  - EmptyState 的 icon 属性默认值为 `<Inbox size={32} />`
- **反例**：`<button>⚙️ 设置</button>`（不仅在不同操作系统下渲染不一致，且破坏专业精致感）。
- **为什么**：原生 Emoji 字符在不同系统平台（macOS、Windows、Linux）中外观差异极大，且与专业的现代应用界面（如 Steam 风格或 Warm 风格）不搭，会极大地拉低应用的视觉档次与一致性。使用 Lucide 矢量图标能保证跨平台的高清、风格一致及完美的对齐方式。
- **检查方法**：核对组件 UI 定义及状态标识，确保没有任何裸 Emoji 用作操作按钮或图标显示。

---

## 七、本篇合规自检清单

- [ ] 我的视觉值都引用了令牌（TS 常量或 CSS 变量），没有魔法数字（R-U1）。
- [ ] 若我新增了语义键，已在深浅主题全部补齐（R-U2）。
- [ ] 我的第一方 UI 颜色来自固定的 15 级中性色板，没有页面私有灰色（R-U2.5）。
- [ ] 我的深浅主题按统一语义映射消费色板，正文与关键图形达到对比度要求（R-U2.6）。
- [ ] 我的阅读列、标题栏、紧凑控件和回合间距使用共享结构令牌（R-U2.10）。
- [ ] 我的体量图表使用 `--chart-volume-0..8`，零值、缺失值和非零值没有混淆（R-U2.7）。
- [ ] 我的分类图表同时提供标签/数值，没有只靠相邻灰色区分类别（R-U2.8）。
- [ ] 我的彩色只用于语义状态、Diff、终端 ANSI 或用户内容，没有作为普通图表装饰色（R-U2.9）。
- [ ] 我的玻璃效果符合材质分层矩阵，没有把内容阅读层强行玻璃化（R-U2.1, R-U2.4）。
- [ ] 我的 Liquid Glass 使用统一 `--liquid-*` / `--vibe-*` / `--main-card-*` 令牌，且没有自造参数体系（R-U2.2）。
- [ ] 应用背景磨砂的透明链路完整，没有根节点或 wrapper 变成不透明底色（R-U2.3）。
- [ ] 主题切换走 `applyTheme` + Zod 校验（R-U3）。
- [ ] 终端配色随皮肤联动（R-U4）。
- [ ] Widget / Control Hub 模式隐藏应用 Chrome，并只保留高质量单卡玻璃容器（R-U5.1-R-U5.3）。
- [ ] 展示性文字消费 `--font-display`（R-U5）。
- [ ] 所有的 UI 图标均使用 `lucide-react` 的 SVG 图标，无任何 Emoji 字符用作 UI 图标（R-U5.5）。
