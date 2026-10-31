# UI/UX 01 · Extension 视觉与空间样式

> 版本：4.0.0 · 日期：2026-09-14
> 当前 Surface：`extension/space.css`、`files.css`、`apps.css`、`app.css` 及对应页面样式模块

#### R-U1 · 主题值只有 `archive | volt`

- **等级**：MUST
- 产品外观通过 `html[data-theme]` 投影；持久偏好非法时回退 `archive`。
- 两套主题提供相同语义角色，切换不改变布局、数据或功能状态。
- 主题监听每个 document 只注册一次。
- 被 DSH 嵌入时（`html.standalone`）可在 `theme.css` 内重映射基础角色以贴合宿主色板；
  不新增主题值、不改布局与行为，宿主外层（Chrome 扩展、本机 Web）保持 `archive`／`volt` 原方向。

#### R-U2 · 组件消费语义角色

- **等级**：MUST
- 页面组件使用 background/surface/border/text/primary/danger/warning/success 等已有 CSS custom properties。
- 原始色值只在主题定义、文件类型色或受审计 fallback 中出现。
- 对比度按对象分三档判定：
  - **文字** ≥4.5:1（≥18.66px 粗体或 ≥24px 视为大字，≥3:1）；
  - **可交互控件的可见边界与状态**（input/button 边框、焦点环、选中指示、开关）与**图表数据线/图例/图标** ≥3:1；
  - **结构分隔线、卡片外框、区域分隔**不计入 3:1，但必须伴随第二个通道：`edge` 对相邻面 ≥1.20:1，**且**（表面差 ≥1.20:1 或一层 elevation/rim）。比值必须实测记录在 `extension/theme.css` 的派生角色表里，改值必须同步改表。

#### R-U2a · 层级不得只靠边界

- **等级**：MUST
- 本条约束**跨层级**表面：raised 浮于 canvas、浮层浮于 raised、well 凹于 raised。这些关系必须满足「表面差 ≥1.20:1」或「一层 elevation」之一；1px 边框只做收口，不能是唯一区分手段。
- **同层并列分区**（工具栏/侧栏/状态栏与内容区、列表与预览）之间的分隔属结构线，按 R-U2 第三档判定，不受本条约束——浅色下这类分区本来就只能靠那条 1px `edge`。
- 反向同样成立：深色主题不得只靠阴影分层（近黑上阴影不可见），浅色主题不得只靠底色差分层（白面之间的差通常 <1.1:1）。

#### R-U2b · 层级只来自派生角色

- **等级**：MUST
- 表面、边界与 elevation 只能来自 `--natives-surface-{raised,chrome,well}`、`--natives-edge`、`--natives-elev-{1,2,3}`、`--natives-rim`、`--natives-accent-edge`、`--natives-well-inset`。
- 强调色的**强度档**同样只能来自角色：实心强调底与「强调色当文字/图标」用 `--natives-accent-ink` / `--natives-ok-ink`（ADR-0049 D10），`--natives-accent` 留给 1px 描边、下划线、选中竖条等非实心用法。禁止在组件层写 `color-mix()` 现算强调色档位。
- 组件层禁止出现 `[data-theme=…]` 选择器，也禁止用 `color-mix()` 现算层级；需要新层级时先加派生角色。
- 已知例外（ADR-0049 D2 收口记录）：`extension/files.css` 的 `--display-font`/`--radius`（主题级字体与形状选择）、`positions.css` 的涨跌色降饱和（需按 `data-color-mode × 调色板` 矩阵取值，属调色板性格）。
- 新增基础角色必须同步 `scripts/modules/theme-compose.mjs` 的 `BASE_THEME_ROLES`，否则「模块不得重复声明基础角色」的护栏对新角色失效。
- `--natives-surface-well` 不得用 `color-mix` 派生：浅色取 `--natives-surface-3` 原值、深色取 `--natives-surface` 原值。深色 well 与 raised 的面差已有 1.358，**不需要 in-set 阴影**（近黑上的内阴影实测只有 1.03:1，等于没画）；浅色才用 `--natives-well-inset`（边缘 1.25）。混入文本色会让浅色 well 比现状更弱（1.133 → 1.119），是负收益。

#### R-U3 · 空间样式权威高于页面样式

- **等级**：MUST
- Space 内每个 Widget 的字体、颜色、背景、边框、圆角和强调色来自 Workspace/Widget display 配置解析后的 `--space-widget-*` 局部角色。
- AI 效能、时间、图表和列表不得回读 Files、Apps 或全局 Host Surface 的具体颜色/字号。
- Widget Shadow DOM/根容器必须接收同一组局部角色；切换空间风格后内容保持可读。

#### R-U4 · 空间组件结构统一

- **等级**：MUST

```text
Widget
├─ Header：标题 / 状态 / 当前组件动作
├─ Summary（可选）：主指标与单位
├─ Body：真实内容 / loading / empty / error
└─ Footer（可选）：来源 / 更新时间 / 下一步
```

- 缺少内容的层不渲染空占位。
- 标题栏、Body padding、状态容器和编辑控件复用现有 Space shell，不由各 Widget 复制。

#### R-U5 · 密度与命中区稳定

- **等级**：MUST
- 普通正文不小于 12px；主指标 20–24px；行高：正文与说明 1.4–1.5（新写或改动处 ≥1.45），单行数值 1.2–1.3，被行数或固定高度钳制的容器（`-webkit-line-clamp`、固定 `max-height` 的多行文本）、单行截断（`white-space:nowrap` + `text-overflow:ellipsis`）的元信息与图表标注可用 1.3–1.5。
- 页面级 hero 数值每屏最多 1 处，可取 20–28px，并须同时满足：与相邻同级数值差 ≥6px 或字重差 ≥100；同视口内不得有第二个 ≥24px 的数值；≤760px 时降到 22–24px。
- 图标按钮可见图标 14–16px，命中区至少 32×32px；文本按钮命中区至少 28×28px；`@media (pointer:coarse)` 下可点元素至少 36×36px，但**固定高度的 chrome（状态栏、列表头等）内的控件占满该 chrome 整高即可（≥32）**，不得为了凑 36 撑破固定行高；缩放/拖拽命中区至少 24×24px。
- Grid 间距基线 8px，工作区边缘 12px；窄屏可收敛到 8px。
- 必要信息不得使用 disabled 色或只靠透明度表达；次要信息的层级用字号、字重（或既有 `text-muted`）表达，不得用 `opacity < 1` 区分层级（禁用态除外）。**不新增第三档灰**：muted 再混 surface 到合规（≥4.5:1）时与 muted 只差约 1.1，肉眼不可辨，属无效角色。

#### R-U5a · 阶梯是闭集

- **等级**：MUST
- 适用范围：**新增或修改的**字号/间距/圆角/字重必须取自闭集；存量差异列入迁移清单，不得在业务改动中顺手放宽，也不要求一次性清零（现有代码里闭集外约 150 处，属迁移债）。
- 字号只允许 11/12/13/14/17/20/22/28（11 仅限非正文微标签）；间距只允许 0/2/4/8/12/16/20/24/32；圆角只允许 0/4/6/8/12/999；字重只允许 400/600/650（700 仅 hero 数字）。
- 禁止 15/16/18/21/23/24/26/27/33/34 这类近邻值——近邻值无法被读者分辨，只会制造噪音。
- 列表行距与 Grid 间距是两件事：列表行可以紧凑（0–4），Grid 才用 8 的倍数。

#### R-U6 · 材质服从可读性和性能

- **等级**：MUST
- Shell/导航/浮层可以使用轻量半透明、blur 和阴影；正文、表格、代码、表单和图表保持稳定清晰。
- 禁止把高成本 filter、drop-shadow 或折射效果铺到所有 Widget。
- 视觉效果必须通过性能标准和 `prefers-reduced-motion`。
- 当卡片/浮层的边界依赖 elevation 时，必须提供 `@media (forced-colors: active)` 的 `border: 1px solid CanvasText` 退化；依赖 blur 的浮层必须提供 `@media (prefers-reduced-transparency: reduce)` 的去 blur 退化。系统高对比模式会移除 `box-shadow`，没有这条退化等于卡片分组消失。

#### R-U7 · 数据图表表达真实语义

- **等级**：MUST
- 数量强度使用有序层级；零值与缺失数据分离。
- 分类图同时提供标签和真实值，不只靠颜色；默认最多展示前 6 类，其余合并“其他”。
- danger/warning/success 只表达明确语义，不作普通装饰系列色。
- 数值使用 tabular numbers；成本显示币种，估算显示估算标记。

#### R-U8 · 图标使用受控 SVG

- **等级**：MUST
- 交互、文件类型和状态图标使用仓库内受控 SVG/sprite/CSS mask。
- 不为图标引入 UI 框架；不使用 Emoji 代替产品图标。
- 图标必须有文本、title 或 aria-label，不能单靠图形传达危险操作。

## 合规自检

- [ ] 主题只有 archive/volt 且角色一致。
- [ ] Space Widget 使用 `--space-widget-*` 局部角色。
- [ ] 字体、密度、对比度和命中区合格。
- [ ] 图表区分零值、缺失和语义状态。
- [ ] 材质没有牺牲可读性或性能。
