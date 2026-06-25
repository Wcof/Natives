# UI/UE 01 · 设计令牌与主题系统

> **版本**: 1.0.0 · **日期**: 2026-06-15
> **关联 ADR**: 无
> **关联源文件**: `src/lib/design-tokens.ts`（TS 常量）、`src/lib/theme-engine.ts`（二皮肤 + Zod 校验 + 应用）、`src/app/globals.css`、`docs/STYLE_GUIDE_AUDIT.md`（历史改造清单，已被本篇收口为规范）

---

## 一、本篇要约束什么

Natives 的视觉一致性靠「设计令牌」保证——颜色、间距、圆角、字号、阴影、过渡都从统一源头出发，二套皮肤通过覆盖语义令牌切换。本篇把令牌体系、二皮肤契约、Vibe 材质层、字体绑定钉成规范。

> `STYLE_GUIDE_AUDIT.md` 是一次性改造清单（历史），**其约束性已被本篇取代**；本篇是权威。

---

## 二、两套令牌：TS 常量 + CSS 变量

Natives 同时维护两套令牌，各有用途：

| 令牌集 | 位置 | 用途 | 是否随主题变 |
|--------|------|------|-------------|
| **TS 常量** | `src/lib/design-tokens.ts`（`SPACING` / `FONT_SIZE` / `BORDER_RADIUS` / `TRANSITION` / `SHADOW`） | 给 inline style / 计算用，**不随主题变** | 否（结构性数值） |
| **CSS 语义变量** | `theme-engine.ts` 的 `THEMES` → 注入为 `--bg` / `--text` / `--accent` 等 | 给样式表 / `var(--x)` 引用，**随主题变** | 是（皮肤相关） |

#### R-U1 · 视觉值必须引用令牌，禁止魔法数字
- **等级**：MUST
- **分类**：主题、命名
- **规则**：组件中的所有视觉值**必须**引用令牌：
  - 皮肤相关（颜色等）→ CSS 变量 `var(--bg)` / `var(--accent)`，或 `THEME_TOKENS.bg`。
  - 结构性（间距/圆角/字号/阴影/过渡）→ `design-tokens.ts` 的常量 `SPACING.md` / `BORDER_RADIUS.lg` / `TRANSITION.fast`。
  - **禁止**硬编码 `#1a1a2e`、`padding: 13px`、`0.2s ease` 等魔法值。
- **正例**：`padding: SPACING.md`、`color: 'var(--accent)'`、`transition: TRANSITION.fast`。
- **反例**：`background: '#0b0c0a'`（硬编码，换皮肤失效）；`border-radius: 8px`（应 `BORDER_RADIUS.lg`）。
- **为什么**：令牌是二皮肤与视觉一致性的唯一保证；魔法值会让换肤局部失效、让间距节奏失控。
- **检查方法**：见 `frontend/01` R-E4 的 grep 思路；新增魔法值时优先找现有令牌。

---

## 三、二皮肤契约、Vibe 材质层与 Liquid Glass 边界

当前 Natives 皮肤定义分为两层：常规二皮肤（定义在 `theme-engine.ts` 的 `THEMES`：`terminal-volt`（深色）、`frosted-jasmine`（浅色磨砂茉莉））与 Vibe 材质层（定义在 `globals.css` 的 `--vibe-*` / `--liquid-*` / `--glass-*` 变量）。CSS `data-theme` 属性值直接使用主题 ID：`terminal-volt`、`frosted-jasmine`。

参考 CodePilot 的 macOS visual profile 后，Natives 的结论是：高级磨砂玻璃不能靠到处叠 `backdrop-filter` 实现。它必须先有透明窗口与透明根节点作为底座，再由 shell / navigation / floating control 分层承载玻璃材质，内容阅读层保持稳定和可读。真正的 Liquid Glass 只用于少数高价值容器或控件，不作为全局背景滤镜滥用。

#### R-U2 · 皮肤系统必须共享或对齐语义键并满足色系一致性
- **等级**: MUST
- **分类**：主题
- **规则**：每套常规皮肤**必须**提供**完全相同**的语义键集合（`bg` / `bg-2` / `bg-3` / `panel` / `border` / `text` / `text-dim` / `text-faint` / `accent` / `accent-ink` / `radius` / `font-display`）。磨砂玻璃皮肤在常规键之外，引入布局令牌（`--vibe-*`）。
- **色系隔离红线**：
  - **Terminal Volt (深色模式)**：必须由**亮绿色/极光绿**（`#00ff9c`）主导，严禁混入任何暖橙色。
  - **Frosted Jasmine (浅色模式)**：必须由**暖橙色/桃红色**（`#ff793f` / `#f05b3f`）主导，作为 hover 态、激活选中态等主色调，严禁混入任何极光绿。
- **色彩同步参数**：
  - `terminal-volt` (深色): `--vibe-sidebar-bg` 为 `rgba(20, 24, 30, 0.68)`，`--vibe-canvas-bg` 为暗色渐变，`--vibe-btn-bg` 为 `rgba(255, 255, 255, 0.06)`，强调色为 `#00ff9c`（极光绿）。
  - `frosted-jasmine` (浅色): `--vibe-sidebar-bg` 为 `rgba(255, 255, 255, 0.52)`，`--vibe-canvas-bg` 为暖白渐变，`--vibe-btn-bg` 为 `rgba(255, 255, 255, 0.4)`，强调色为 `#ff793f`（暖橙色）。
- **为什么**：保证二皮肤切换时所有布局组件（侧栏、顶栏、按钮、状态度量）的颜色和玻璃质感同步变化，且绝对不发生风格漂移。

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

#### 关键代码片段 (Theme Code Snippets)

##### 1. 皮肤引擎配置契约 (`src/lib/theme-engine.ts`)
```typescript
export const THEMES: Record<string, Theme> = {
  'frosted-jasmine': {
    bg: '#fffdfa',
    'bg-2': '#ffffff',
    'bg-3': '#fff8f2',
    panel: '#fffdfa',
    border: '#ecdccf',
    text: '#2d1f14',
    'text-dim': '#7a6b5a',
    'text-faint': '#b8a594',
    accent: '#ff793f',
    'accent-ink': '#ffffff',
    radius: '12px',
    'font-display': '"Noto Serif SC", Georgia, serif',
    danger: '#f05b3f',
    'danger-soft': '#f05b3f20',
    warning: '#ffa466',
    info: '#be88ed',
    'diff-add': '#ff9856',
    'diff-del': '#f05b3f',
    'diff-mod': '#be88ed',
  },
  'terminal-volt': {
    bg: '#0d0f12',
    'bg-2': '#15181d',
    'bg-3': '#1c2027',
    panel: '#0d0f12',
    border: '#2a2f38',
    text: '#dcdfe6',
    'text-dim': '#8b90a0',
    'text-faint': '#555a66',
    accent: '#00ff9c',
    'accent-ink': '#0d0f12',
    radius: '4px',
    'font-display': '"JetBrains Mono", "Fira Code", monospace',
    danger: '#ff3a4d',
    'danger-soft': '#ff3a4d15',
    warning: '#ffb545',
    info: '#45b5ff',
    'diff-add': '#00ff9c',
    'diff-del': '#ff3a4d',
    'diff-mod': '#45b5ff',
  },
};
```

##### 2. CSS 关联颜色联动系统 (`src/app/globals.css`)
```css
/* Terminal Volt 联动色 */
[data-theme="terminal-volt"] {
  --accent: #00ff9c;
  --accent-soft: rgba(0, 255, 156, 0.12);
  --accent-border: rgba(0, 255, 156, 0.35);
  --accent-ink: #0c0e12;
  --vibe-btn-hover-bg: rgba(0, 255, 156, 0.08);
  --vibe-btn-hover-color: #00ff9c;
  --vibe-btn-text: rgba(255, 255, 255, 0.65);
  --vibe-active-bg: rgba(0, 255, 156, 0.12);
  --vibe-active-color: #00ff9c;
  --vibe-accent-color: #00ff9c;
  --vibe-folder-bg: linear-gradient(180deg, #50ffbe 0%, #00ff9c 100%);
  --vibe-folder-flap-bg: linear-gradient(180deg, #78ffd1 0%, #2bffa6 100%);
  --vibe-progress-bg: linear-gradient(to right, #00ff9c, #00ddff);
}

/* Frosted Jasmine 联动色 */
[data-theme="frosted-jasmine"] {
  --accent: #ff793f;
  --accent-soft: rgba(255, 121, 63, 0.15);
  --accent-border: rgba(255, 121, 63, 0.40);
  --accent-ink: #ffffff;
  --vibe-btn-hover-bg: rgba(255, 121, 63, 0.1);
  --vibe-btn-hover-color: #e05a1d;
  --vibe-btn-text: #8e8e93;
  --vibe-active-bg: rgba(255, 121, 63, 0.15);
  --vibe-active-color: #ff793f;
  --vibe-accent-color: #ff793f;
  --vibe-folder-bg: linear-gradient(180deg, #ff9856 0%, #ff793f 100%);
  --vibe-folder-flap-bg: linear-gradient(180deg, #ffa466 0%, #ff8548 100%);
  --vibe-progress-bg: linear-gradient(to right, #ff9856, #f05b3f);
}
```


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
- **参考代码**：[DashboardPage (src/app/page.tsx)](file:///Users/ldh/Downloads/project/AiNative/Natives/src/app/page.tsx) 是本项目小组件玻璃效果的官方标准实现。
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

不同皮肤有不同的「展示字体」语义（`terminal-volt` 用等宽、`frosted-jasmine` 用衬线）。

#### R-U5 · 标题/品牌/激活态消费 `--font-display`
- **等级**：SHOULD
- **分类**：主题、命名
- **规则**：侧栏品牌、主标题、面包屑激活态、卡片标题等「展示性」文字**应该**用 `font-family: var(--font-display);`，让字体随皮肤变化呈现不同语境。代码文件名、数值、终端数据**应该**用等宽（`var(--font-mono)` 或 `design-tokens` 中的 mono 栈）。正文 UI 用 `var(--font-ui)`。
- **为什么**：见 `STYLE_GUIDE_AUDIT.md` 的 typography 缺口分析——字体不随皮肤绑定会丧失 frosted-jasmine 的雅致衬线感与 terminal-volt 的终端等宽感。
- **检查方法**：标题类元素核对字体来源。

#### R-U6 · 统一使用专业 SVG 图标，禁止使用 UI 字符 Emoji
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
- [ ] 若我新增了语义键，已在二皮肤全部补齐（R-U2）。
- [ ] 我的玻璃效果符合材质分层矩阵，没有把内容阅读层强行玻璃化（R-U2.1, R-U2.4）。
- [ ] 我的 Liquid Glass 使用统一 `--liquid-*` / `--vibe-*` / `--main-card-*` 令牌，且没有自造参数体系（R-U2.2）。
- [ ] 应用背景磨砂的透明链路完整，没有根节点或 wrapper 变成不透明底色（R-U2.3）。
- [ ] 主题切换走 `applyTheme` + Zod 校验（R-U3）。
- [ ] 终端配色随皮肤联动（R-U4）。
- [ ] Widget / Control Hub 模式隐藏应用 Chrome，并只保留高质量单卡玻璃容器（R-U5.1-R-U5.3）。
- [ ] 展示性文字消费 `--font-display`（R-U5）。
- [ ] 所有的 UI 图标均使用 `lucide-react` 的 SVG 图标，无任何 Emoji 字符用作 UI 图标（R-U6）。
