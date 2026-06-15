# UI/UE 01 · 设计令牌与主题系统

> **版本**: 1.0.0 · **日期**: 2026-06-15
> **关联 ADR**: 无
> **关联源文件**: `src/lib/design-tokens.ts`（TS 常量）、`src/lib/theme-engine.ts`（三皮肤 + Zod 校验 + 应用）、`src/app/globals.css`、`docs/STYLE_GUIDE_AUDIT.md`（历史改造清单，已被本篇收口为规范）

---

## 一、本篇要约束什么

Natives 的视觉一致性靠「设计令牌」保证——颜色、间距、圆角、字号、阴影、过渡都从统一源头出发，三套皮肤通过覆盖语义令牌切换。本篇把令牌体系、三皮肤契约、字体绑定钉成规范。

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
- **为什么**：令牌是三皮肤与视觉一致性的唯一保证；魔法值会让换肤局部失效、让间距节奏失控。
- **检查方法**：见 `frontend/01` R-E4 的 grep 思路；新增魔法值时优先找现有令牌。

---

## 三、三皮肤契约

当前三套内置皮肤定义在 `theme-engine.ts` 的 `THEMES`：`terminal-volt`（默认暗）、`warm-archive`（暖亮）、`editorial`（粗野亮）。

#### R-U2 · 三皮肤必须共享同一套语义键
- **等级**:MUST
- **分类**：主题
- **规则**：每套皮肤**必须**提供**完全相同**的语义键集合（`bg` / `bg-2` / `bg-3` / `panel` / `border` / `text` / `text-dim` / `text-faint` / `accent` / `accent-ink` / `radius` / `font-display`）。新增语义键**必须**在所有皮肤补齐，**禁止**只在某套皮肤定义。
- **为什么**：组件用 `var(--xxx)` 引用，键缺失会让某皮肤下该处显示空值/默认值，破坏一致性。
- **检查方法**：`theme-engine.ts` 中三套皮肤对象键一致；新增键时三处同改。

#### R-U3 · 皮肤切换必须即时且经 Zod 校验
- **等级**：MUST
- **分类**：主题、数据
- **规则**：皮肤切换**必须**：经 `validateTheme()`（Zod）校验 → `applyTheme()` 设置 `data-theme` 属性 + 注入 CSS 变量 + 注入终端 ANSI 色 + 通知监听器。**禁止**绕过校验直接改 DOM。颜色值**必须**匹配 `^#[0-9a-fA-F]{6}$`。
- **为什么**：损坏的配置不应注入非法 CSS（承接 `technical/02` R-S11）。
- **检查方法**：所有主题切换入口都调 `applyTheme`。

#### R-U4 · 终端配色必须随皮肤联动
- **等级**：MUST
- **分类**：主题
- **规则**：`TERMINAL_THEMES` 中每套皮肤**必须**提供对应的终端 `background` / `foreground` / `cursor` / `selectionBackground`。切换皮肤时终端**必须**重新应用对应配色，**禁止**终端保持旧皮肤配色。
- **为什么**：终端是界面一大块，配色不联动会形成视觉割裂。
- **检查方法**：`Terminal.tsx` 监听 `onThemeChange` 并更新 xterm `theme`。

---

## 四、字体绑定

不同皮肤有不同的「展示字体」语义（terminal-volt 用等宽、warm-archive 用衬线、editorial 用极粗 sans）。

#### R-U5 · 标题/品牌/激活态消费 `--font-display`
- **等级**：SHOULD
- **分类**：主题、命名
- **规则**：侧栏品牌、主标题、面包屑激活态、卡片标题等「展示性」文字**应该**用 `font-family: var(--font-display)`，让字体随皮肤变化呈现不同语境。代码文件名、数值、终端数据**应该**用等宽（`var(--font-mono)` 或 `design-tokens` 中的 mono 栈）。正文 UI 用 `var(--font-ui)`。
- **为什么**：见 `STYLE_GUIDE_AUDIT.md` 的 typography 缺口分析——字体不随皮肤绑定会丧失 warm-archive 的出版物感与 editorial 的块状冲击力。
- **检查方法**：标题类元素核对字体来源。

---

## 五、本篇合规自检清单

- [ ] 我的视觉值都引用了令牌（TS 常量或 CSS 变量），没有魔法数字（R-U1）。
- [ ] 若我新增了语义键，已在三皮肤全部补齐（R-U2）。
- [ ] 主题切换走 `applyTheme` + Zod 校验（R-U3）。
- [ ] 终端配色随皮肤联动（R-U4）。
- [ ] 展示性文字消费 `--font-display`（R-U5）。
