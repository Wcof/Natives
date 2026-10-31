# ADR-0049 · 文件与投资模块视觉修订：Surface Ladder v1 与强调预算

- 日期：2026-10-29
- 状态：已接受（用户要求优化「文件」「投资」两模块视觉；空间与其余模块不得改动）
- 依赖：ADR-0021（Design System V2）、ADR-0048（DSH 第三入口 D5.2）、`docs/standards/ui-ux/01`、`02`
- 范围：`extension/files.*`、`modules/fund/ui/**`。**不含** `space.*`、`apps.*`、`tokenusage`、`app.*`。

## 背景

两个模块"看起来没做完"，根因不在配色，而在层级通道缺失：

1. `extension/theme.css` 只有 4 个表面名与 1 个 `--natives-shadow`，ADR-0021 要求的 raised/floating/inset 没有对应 token，实现者只能用「1px 边框 + 纯色面」表达层级。
2. 实测 `--natives-bg` 与 `--natives-surface` 在 volt 下同为 `#0b0c0a`（1.00:1），卡片与画布不可分辨；`--natives-border` 对画布 1.33:1，是唯一分隔通道。
3. fund 用 10 处 `:root:not([data-theme="archive"])` 表达"深色"，把深色等同于"非 archive"，在 ADR-0048 D5.2-8 的宿主色板下语义失真。

## 决策

### D1 · 新增派生角色（Surface Ladder v1）

`extension/theme.css` 在四个既有调色板块内**纯增量**追加，不改任何既有角色。深色用 `text-primary` 把画布抬亮（保留绿调：色相留在 60–80°，避免"两种纸"），浅色直接用既有 surface 值——**浅色不能用 `mix(text)` 派生 raised**，否则卡片会比画布更暗，读作凹陷。

| token | archive | volt | standalone-light | standalone-dark |
|---|---|---|---|---|
| `--natives-surface-raised` | `var(--natives-surface)` #ffffff | `color-mix(in srgb, var(--natives-text-primary) 13%, var(--natives-surface))` = **#292a27** | `var(--natives-surface)` #ffffff | `9%` 混色 = **#2f2f30** |
| `--natives-surface-chrome` | `var(--natives-surface-2)` #f8f9fb | `8%` 混色 = **#1d1e1c** | `var(--natives-surface-2)` #f4f5f7 | `5%` 混色 = **#262627** |
| `--natives-surface-well`（**任何调色板都不许用 mix**） | `var(--natives-surface-3)` #eef1f5 | `var(--natives-surface)` #0b0c0a | `var(--natives-surface-3)` #ebeef2 | `var(--natives-surface)` #1b1b1c |
| `--natives-edge`（基准 = raised） | `color-mix(in srgb, var(--natives-text-primary) 12%, transparent)` | 9% | 11% | 8% |
| `--natives-accent-edge` | `var(--natives-accent)` | `color-mix(in srgb, var(--natives-accent) 78%, var(--natives-surface-raised))` = #a9c643 | `var(--natives-accent)` | 同式 = #cdcdce |
| `--natives-elev-1` | `0 1px 2px rgba(0,0,0,.05), 0 1px 1px rgba(0,0,0,.03)` | `0 1px 2px rgba(0,0,0,.35)` | `0 1px 2px rgba(0,0,0,.04), 0 2px 6px rgba(0,0,0,.04)` | `0 1px 2px rgba(0,0,0,.35)` |
| `--natives-elev-2` | `0 2px 4px rgba(0,0,0,.04), 0 6px 16px rgba(0,0,0,.06)` | `0 4px 14px rgba(0,0,0,.45)` | 同 archive | `0 6px 18px rgba(0,0,0,.40)` |
| `--natives-elev-3` | `0 12px 36px rgba(0,0,0,.12)` | `0 12px 36px rgba(0,0,0,.60)` | 同 archive | `0 12px 36px rgba(0,0,0,.55)` |
| `--natives-rim` | `inset 0 1px 0 rgba(255,255,255,.75)` | `inset 0 1px 0 rgba(255,255,255,.07)` | `inset 0 1px 0 rgba(255,255,255,.60)` | `inset 0 1px 0 rgba(255,255,255,.06)` |
| `--natives-well-inset`（凹陷的唯一线索） | `inset 0 1px 2px rgba(0,0,0,.10)`（边缘对 well 1.25） | `none`（45% 黑压在 #0b0c0a 上只有 1.03，等于没画；深色凹陷由 raised→well 的 1.358 面差完成） | 同 archive | `none` |

**实测比值（WCAG 相对亮度，2026-10-29 独立复算）**。表内每列必须写「分母角色 = 合成 hex + 比值」，只写比值会在后续调色板微调时对不上账（本轮出现过 8.83 vs 8.64 的分母漂移）。

| 对 | archive | volt | sa-light | sa-dark |
|---|---|---|---|---|
| raised / bg | 1.081 | **1.358** | 1.045 | **1.364** |
| chrome / bg | 1.026 | 1.171 | 1.044 | 1.206 |
| raised / chrome | 1.053 | 1.159 | 1.091 | 1.130 |
| well / raised | 1.133 | 1.358 | 1.164 | 1.287 |
| edge / raised（基准 = raised） | 1.260 | **1.300** | 1.260 | **1.271** |
| muted / raised | 5.81 | 5.48 | 5.80 | 4.90 |

深色分层阈值分两档写，避免有人拿"下限"当默认：**目标值 `raised/bg ≥1.35`**、`raised/chrome ≥1.10`、`well/raised ≥1.20`、`edge/raised ≥1.25`；**回滚下限 ≥1.20，但必须同时保留 `edge ≥1.25` 与 rim**，低于 1.20 不接受。浅色不靠面差分（1.05–1.08），由 `edge`（≥1.24）+ `elev-1` 承担——这是 Liquid Crystal 与 Dark Glow 的真实分工。

若真机复核发现 1.358 过亮（卡片像灰板），只能**整体**降到 1.24 并同步更新本表，不得逐块调。派生角色不进 `themeColor()/themeRgba()` 取样面。

**不新增** `--natives-text-subtle`：实测 `muted 78%` 只有 3.58–3.94:1，推到 94% 才合规但两档灰差仅 1.09:1，肉眼不可辨、增益为零。第三档层级改用尺寸（见 D6-5）。

### D2 · 组件层禁止主题名分支

- 删除**本次页面**里用主题名判断层级的 7 处：`home.css:64/446/455/537/641/642`、`investment.css:190`。
- **第一轮保留**共 10 处（后已收口，见下）：`fund-detail.css:71/131/222`（基金详情页卡片面）、`investment.css:8/19/38`、`positions.css:139/140/141`（对比度修正，属配色语义，需要 `--natives-accent-ink`／`--natives-ok-ink` 才能消除）。
- `extension/files.css:16` 保留品牌字体 `--display-font` 与 `--radius`（主题级字体/形状选择，不是颜色与层级，作为 R-U2b 的已知例外记录）；但**不得整体删掉该覆盖**——files.css 有 25 处 `var(--radius)`，删掉会让 volt 的圆角从 4px 静默变成基础值。处置：基础 `--radius: 9px → 8px`（归档入闭集），volt 覆盖保持 4px（4 已在闭集内），两套主题各自不变形。
- 主题定义本体（`fund/ui/src/styles/theme.css:86/117`）不动。

**收口结果（第二轮，2026-10-29 深夜）**：`fund-detail.css` 三处改成 `surface-raised` / `chrome`（D1 已按调色板分档，不需要分支）；`investment.css` 三处由 D10 的 `--natives-accent-ink` / `--natives-ok-ink` 取代并整体删除；`positions.css` 的 `.pos-tab` 一处同上，另两处 `.up`/`.down` 保留（见下）。
全仓现在只剩 2 类主题名选择器：①调色板定义本体（`extension/theme.css`、`fund/ui/src/styles/theme.css`）；②`files.css` 的 `--display-font/--radius` 与 `positions.css:139/140` 的涨跌色降饱和。后者是**调色板性格**而非层级：涨跌色需要按 `data-color-mode`（cn/intl）× 调色板四象限取值，硬套墨色档会让 DSH 单色板下的红绿一起褪成灰，故保留为显式分支并在 `positions.css` 注明。

### D3 · 强调预算

每屏实心 accent 元素 ≤1 处；实心面积 ≤ 所在卡片 6%；>1200px² 的 accent 面积必须是 `15%→0%` 透明渐变（水位取 `12%→0%`）。1px accent 描边不算实心。保留的实心：`.primary`、`nav.main-nav` 2px 下划线、`.range-edge` 选中日。

### D4 · 阶梯闭集

字号 `11/12/13/14/17/20/22/28`（11 仅非正文微标签）；间距 `0/2/4/8/12/16/20/24/32`；圆角 `0/4/6/8/12/999`；字重 `400/600/650`（700 仅 hero）；行高正文与说明 1.5、数字 1.2–1.3、17px 标题 1.35。

数字分档：**28 = 单屏唯一 hero（管理资产）｜20 = 账本其余三格与收益区首数｜17 = 水位三指标｜14 = 正文**。

### D5 · 边界不是唯一层级通道

1px 结构边不计入 3:1 判定（见 R-U2），但必须满足：`edge/raised ≥1.20` **且**（表面差 ≥1.20 **或** 存在 elev/rim）。比值记录在 D1 表中，改值必须同步改表。

### D6 · 文件模块

1. 表面：`.layout` = raised；`#app-sidebar/.toolbar/#status-bar` = chrome；`#preview` = **raised**（承载编辑器/表格/代码，R-U6 要求正文面稳定）；输入/`pre`/编辑器 = well + `--natives-well-inset`（浅色 well 与 raised 只差 1.13，凹陷只能靠它）。**well 只用于小面积、本身有边界的控件**（输入框、代码块、编辑器），建议渲染面积 ≤480×320；超过这个量级不得用 well 表达内容区层级——浅色下那层凹陷读不出来，越用越假。
2. 控件：统一 32px 高、圆角 8；静止 `border-color:var(--natives-edge)`、hover `--natives-control` + `--hover` 底；`[aria-pressed=true]` 与段控选中项 = `surface-raised` + `1px solid var(--natives-accent-edge)` + 600；`:disabled` 用 `color: color-mix(muted 45%, transparent)` + `not-allowed`，**不得只留 opacity**。
3. 强调归属：home 态 `createEntry()` 因 `!session.currentPath` 静默 return，`#new-menu` 在该态**隐藏**（`body:has(.home-welcome-view) #new-menu{display:none}`），唯一实心 accent 归空态的「搜索文件 ⌘K」；进入目录后 `#new-menu` 恢复并成为唯一实心，空态按钮不再出现在该态。
4. 空态：图标盒 64→56、中性面 + edge + rim，图标本身保留 `--accent`；`.home-welcome-view` 限宽 460、`padding 40px 20px 64px`。
5. 列表：行 40，**行距保持 2（不是 12）**，网格 gap 12；选中 = selected 浅底 + `inset 2px 0 0 accent`，网格选中 = `inset 0 0 0 1.5px accent`。网格里 size/modified 两行用 **12px / 11px** 区分（取代 `opacity:.85/.75`），不新增灰阶。
6. 状态栏 32px；文本按钮 28、图标按钮 32；`pointer:coarse` 下 32/36；Host 状态 = 文字 + 6px 圆点。
7. 尺寸统一用一个 `--files-chrome-h` 收敛 4 处 `calc(100vh - …)`，避免 toolbar/status 改高后预览多出滚动条。
8. 网格三档按钮补 `aria-label`（现在读屏念 `·/▪/◼`）；home 态 `#entries` 去掉 `role="listbox"`。

### D7 · 投资模块

1. 卡片统一 `surface-raised + edge + rim + elev-1`；`--fund-card-radius 14→12`、`--fund-card-padding 22→20`（改变量，不逐个选择器）。
2. 头部：`.brand-title` 16→13px，**保持 `--natives-text-primary` + 600**（嵌入时 `.app-shell-head` 被隐藏，它是模块唯一身份）；删 `@media(max-width:380px)` 的 `display:none`，改 11px 保底。`header.top-header` 68→56 且 `nav.main-nav button` padding 23→17；`#home-overview-title` 27→22。
3. 水位：删 `.goal-scale.right`；刻度只留 0/25/50/75/100、**12px（与正文地板一致）+ `white-space:nowrap`**（删 ≤760 的 9px），刻度槽宽 `--goal-tick-col` 桌面 60 / ≤760 52（给「100%」+ 13px 主刻度线留位）；**水位与刻度共用一个 `--goal-band` 高的仪表带**（桌面 120 / ≤760 96），两者纵向 `inset` 完全重合，百分比不再做像素重映射（见 D8）；填充 `12%→0%` 渐变并删 `:455` 分支；水位线 **70% accent / 1px**（archive 3.60:1、volt ≥3:1），中段为水波留透明；百分比 28/650 无框、**默认 2 位小数，仅当 2 位会把非零进度抹成 0 时才展开到 4 位**（`0.001%` 不得塌缩成 `0.00%`，`29.5885%` 显示为 `29.59%`），`aria-valuetext` 保留 4 位；指标 17；**水位卡不自设 padding**，沿用 `.main-column .card{--fund-card-padding}`，刻度槽在仪表带内部切出。
4. **需动 `home.js` 约 3–5 行**（CSS 做不到）：`role="progressbar"` 与 `aria-value*` 从 `.goal-water-content` 移到 `.goal-water`；hero 文案用「≥2 位、≤4 位去尾零」，`aria-valuetext` 继续 4 位。必须与 CSS 同一提交，否则读屏读不到三个指标；断言 selector 同步改成 `.goal-water`（`home-usability-browser.mjs`、`home-native-browser.test.mjs` 各 2 处）。
5. 账本：`28/20/12` 三档；列宽 `1.5fr 1fr 1fr 1fr`；四格 `align-content:space-between` 让「详情」贴底；`hc-detail` 灰 + 下划线，hover 才 accent，`:focus-visible` 保留 accent 描边。
6. 收益区：`repeat(3,1fr) + 1.25fr`（第 4 卡更宽）；**禁止 `order` 重排**（WCAG 1.3.2），保持 DOM 顺序，靠 12px 标签 + 20px 数值建立层级；同时删除 `.goal-water-metrics` 既有三条 `order`（`investment.css:282-284`）。
7. 强调：`.group-tab.active` 实心 accent → `surface-raised + 1px var(--natives-accent-edge) + 600`；`.home-ai-panel` 顶边 3px→2px 45% **且同批补 `edge` 或 `elev-1`**（否则其余三边消失）；图表线 85% 混色。
8. 表格：`th` 用 chrome 面 + edge 下边；`tr.selected` = selected 底 + `inset 2px 0 0 accent`，去掉 `!important`。已知限制：`tr` 无 tabindex，键盘不可达，本轮不扩范围。

### D8 · 水位与尺寸耦合

**旧契约（已废）**：`home.js` 的 `calc(${lineTop}% + ${((level/100)*2-1)*16}px)` 把 `[0,100%]` 重映射到 `investment.css` 里 `.goal-scale{top/bottom:16px}`、`.goal-water-fill{bottom:16px}` 的内缩轴，前提是 `.goal-water` 铺满整卡（`inset:0`）。这个三点绑定一旦 `.goal-water` 改成弹性块就立刻失效：水位线按带高取百分比、刻度按卡高取百分比，两套基准，实测 29.59%（应落在 70.41%）的线跑到 50% 刻度附近，且刻度一直画到卡片底边、穿进指标行。

**新契约（单一基准）**：

1. `#home-goal-body` 内插 `.goal-gauge`（`position:relative; height:var(--goal-band); flex:0 0 auto`），只占位、不绘制。
2. `.goal-gauge > .goal-water` 与 `.goal-gauge > .goal-scale` 均为 `position:absolute` 且纵向 `inset` 完全重合（同一个 `--goal-axis-pad`，取 12px 刻度标签的半行高 6px，否则端点标签会各超出 6px、被挤出仪表带压到标题行或指标带）；水位线、填充、水波、刻度百分比全部相对这一条带取值，`0/50/100` 三点必须与刻度对齐（实测偏差 ≤1px）。
3. `home.js` 只输出 `--line-top:${lineTop}%`，**不得再做像素加减**；`investment.css` 的 `.goal-scale{top/bottom:16px}` 与 `.goal-water-fill{bottom:16px}` 两条内缩**删除**——内缩会重新制造第二套基准。
4. `--goal-band`、`--goal-tick-col`、`--goal-axis-pad` 是这条轴唯一的三个尺寸来源，改带宽只改一个变量。**刻度槽必须在仪表带内部切**（`.goal-scale` 占 `0..--goal-tick-col`、`.goal-water` 从 `--goal-tick-col` 起）——一旦改成「卡内边距 = 槽宽 + 负边距」，就会被 `investment.css` 的 `.main-column .card{padding:var(--fund-card-padding)}`（(0,2,0)，20px）静默覆盖，刻度直接跑出卡外被 `overflow:hidden` 裁掉（实测 `x=-15`）。指标带在仪表带之下（`#home-goal-body` gap 12 + `.goal-water-metrics` margin-top 24），水位线永不再横穿「目标／当前覆盖／缺口」三行数字。
5. 无值（隐私模式、不可计算、未建目标）**仍渲染仪表带**：`.goal-water.is-blank` 隐藏填充/线/波且不带 `role="progressbar"`，避免切换显示模式时卡片高度跳动，也让「空罐 + 完整刻度」本身成为无数据的表达。

### D9 · 文件模块尺寸同步点

toolbar 48 / status 32 必须同步：`body` 的 `grid-template-rows`（基础与 1180 断点）、`.toolbar` padding、四个 `calc(100vh - …)`、`.entry/.list-head` 列模板（4 处）。任一处漏改会造成错行或多余滚动条。

### D10 · 强调墨色档（ink）

`:root` 声明一次，四套调色板共用：

```css
--natives-accent-ink: color-mix(in srgb, var(--natives-accent) 70%, var(--natives-text-primary));
--natives-ok-ink:     color-mix(in srgb, var(--natives-ok) 70%, var(--natives-text-primary));
```

**为什么一套公式够**：`text-primary` 永远是所在面色的反相（亮面近黑、暗面近白），所以「往正文方向压 30%」在亮底是加深、在暗底是提亮，正是逐主题分支想表达的同一件事。自定义属性在计算值阶段就完成 `var()` 替换，声明在 `:root` 也会按当时胜出的 `--natives-accent` / `--natives-text-primary` 解析，因此不必逐调色板复制。

**实测（混比 30% 档 / 原色，WCAG 相对亮度）**

| 用途 | archive | volt | standalone-light | standalone-dark |
| --- | --- | --- | --- | --- |
| `on-accent` 压在实心强调底上 | 9.28 / 7.44 | 15.78 / 15.31 | 18.90 / 18.90 | 17.45 / 17.45 |
| `ok` 墨色压在 chrome 面上 | 7.59 / 5.69 | 13.42 / 13.01 | **5.15 / 3.02** | 8.21 / 6.62 |
| `accent` 墨色压在 chrome 面上 | 8.81 / 7.07 | 13.34 / 13.01 | 17.32 / 17.32 | 14.44 / 14.44 |

DSH 浅色板的 `ok` 落在 chrome 上原本只有 **3.02:1**（低于 4.5），这是这一档存在的硬理由；其余三套是顺带把余量从 5.7–13 提到 7.6–13.4。

**使用约束**

1. 实心强调底（`.btn-accent`、页面级主按钮）与「强调色当文字/图标」都用 `*-ink`；`--natives-accent` 保留给 1px 描边、下划线、选中竖条这类**非实心**用法（D3）。
2. 任何组件层不得再写 `:root[data-theme="archive"]` 去压深强调色。
3. 与 `--natives-accent-edge` 的分工：edge 是**描边**在深底上的可辨档（78% + raised），ink 是**填充与文字**的强度档（70% + text-primary），两者不可互换。

### D11 · 第三轮收口：行键盘、缺口标注、涨跌色四象限

1. **可选中表格行必须键盘可达**。资产/板块/自选表里带 `clickable` 的行加 `tabIndex=0`，`keydown` 里只处理 `Enter` / `空格`（`e.target !== tr` 时放行，避免抢行内按钮与复选框的键），走 `tr.click()` 复用同一套选中逻辑，不另写分支；选中态同步 `aria-current="true"`（`aria-selected` 只对 grid/row 合法，这里保持表格语义，用 `aria-current` 表达「当前行」）。焦点环 `outline: 2px solid accent; outline-offset: -2px`（inset，避免被滚动容器裁掉）。
2. **水位缺口标注不引入新 DOM**。`--line-top` 同时是「水面上沿」，所以用 `repeating-linear-gradient` + `background-size: 1px var(--line-top)` 在罐体右缘画一条虚线柱，长度恰好是从水面到罐顶（100%）的缺口；罐顶再补一条 1px 虚线（`::before`）作为目标线。无值态（`is-blank`）不画缺口——空罐不是「差满」。之所以不加文字标签：缺口金额已在水位三指标里，而文字要走 i18n，用伪元素写死会破坏中英切换。
3. **涨跌色按「嵌入态 × 明暗 × cn/intl」四象限取值**。`html.standalone[data-theme]`(0,2,1) 与 `[data-theme][data-color-mode="intl"]`(0,3,1) 稳压模块里的 `:root[data-theme="volt"]`(0,2,0)；深色用 `#f25a5a / #22c55e`（surface 5.23 / 7.55、chrome 4.60 / 6.64），浅色沿用 `#b13642 / #217052`（6.04 / 5.99）。此前 DSH 深色会落到 archive 档，实测只有 **2.85 / 2.87**，低于 3:1。




## 验证

**tokenusage / space 零变化的四道闸**

1. 静态：10 个新名字在 `space*.css|space*.html|apps*|app.css|modules/tokenusage` 命中 = 0。
2. 动态：改前改后对 `space.html`、`app.html?app=tokenusage` × {archive,volt} 采样 ≥40 个元素的 `getComputedStyle`（color/background/border/box-shadow/radius/font/padding/margin），JSON 必须全等。
3. 视觉：同 viewport、`deviceScaleFactor:1`、`reducedMotion:reduce`、屏蔽动态文本后截图 sha256 相等。
4. 产物：`git diff --stat modules/tokenusage` 只允许 `ui/dist/styles/theme.css` 追加。

**选中态验收不使用"跨调色板的视觉轻重"**：判据只有两条——① 描边对控件所在面（`surface-raised`）≥3:1（图形门槛，实测 archive 7.44 / volt 7.46 / sa-light 18.90 / sa-dark 8.42）；② 描边对未选中边框 `--natives-edge` ≥4.5:1（信息对比，实测 5.91 / 5.73 / 15.00 / 6.62）。只有在控件确实位于 well 内时才改用 `surface-well` 作分母（6.57 / 10.12 / 16.24 / 10.83）。standalone-light 的近黑描边强度是 Archive 的 2.5 倍，这是 DSH 单色强调（#0f1115）的性格差，不是缺陷，不得据此回调数值。

**其余验收**：`modules:ui:check`、`extension:check` 全绿；字号/间距/圆角闭集 grep 命中 0；层级类 `:root:not([data-theme` 归零；每屏实心 accent ≤1；水位线上文字 ≥4.5:1；`progressbar` 只包水位；`forced-colors: active` 下卡片仍有边界；六档宽度（1280/1180/1024/760/560/390）× 2 主题截图无错行。**水位轴同基准（D8）**：`--line-top` 换算出的线与刻度 `0/50/100` 三点实测偏差 ≤1px，且线不进入 `.goal-water-metrics` 的纵向区间——`getBoundingClientRect` 断言，不靠截图目测。

## 后果

- 正面：层级由 token 表达，四种调色板同一份规则；深色卡片终于可分；accent 恢复指向能力；WCAG 1.3.2 与 R-U9 的两处既有缺陷被修掉；第二轮收口后组件层不再有主题名分支，强调色只有「原色（描边）」与「墨色（填充/文字）」两档。
- 成本：volt 卡片从 `#0b0c0a` 抬到 `#292a27`，观感变化明显（更接近暖黑），需真机确认；10 个新角色加入 `BASE_THEME_ROLES` 后，未来模块重复声明会被 `check-ui` 拦住；第二轮把 `--natives-surface-2/3` 在文件与投资两模块内全量映射到 `chrome/well`（浅色是恒等映射，深色才换基准），因此深色下所有次级面整体抬了一档。

## 未做 / 已知例外（第三轮收口后）

四项欠账已在第三轮全部做掉（见 D10、D11）：`--natives-accent-ink`／`--natives-ok-ink` 落地并清掉组件层最后 3 处对比度分支；`--natives-surface-2/3` 在文件与投资两模块内引用数归零；`--natives-border` 整体并入 `--natives-edge` 后删除该角色（含 canvas `themeColor()` 与 SVG 属性里的 6 处字符串引用）；水位缺口标注与表格行键盘也补上了。剩下的是两处**验证**缺口，不是实现缺口：

1. **表格行键盘只做了静态与构图级验证**：`tabIndex` / `keydown(Enter, Space)` / `aria-current` / inset 焦点环四条都实现了，但真实走查需要「自选总览」或「板块」里有行——当前隔离 state（`.tmp/sidebar-state`）没有任何自选与板块数据，三次探测都拿到 0 行。补验条件：把开发库指向有自选/持仓的账本，或给这两个视图做夹具 harness（`installHomeFixture` 只覆盖首页）。
2. **首页降级态夹具是 2026-09 的快照结构**：`home-browser.test.mjs` 里那块合成快照当前会被 renderer 判为加载失败（缺新字段），所以它覆盖的是「加载失败/空态」而不是「合成快照的 incomplete 状态」。断言改成了对失败态也成立的「未知不编造 0.00」，并把这段的定位写进注释；把合成快照迁到当前 schema 属于另一件事。

另外一条**已知例外**保留：`--natives-border` 删除后，空间与 Token 统计页的分隔线从字面色值变成 `--natives-edge`（archive `#e5e9ef → #e4e5e6`、volt `#262920 → #1d1d1b`，浅色几乎不可辨，深色是稍冷的一档）。这两页此前「逐像素不变」的闸门因此不再适用，已重拍截图人工核对：侧栏、工具栏下边、状态栏上边、卡片边界均仍在。
