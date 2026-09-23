# UI/UX 01 · Extension 视觉与空间样式

> 版本：4.0.0 · 日期：2026-09-14
> 当前 Surface：`extension/space.css`、`files.css`、`apps.css`、`app.css` 及对应页面样式模块

#### R-U1 · 主题值只有 `archive | volt`

- **等级**：MUST
- 产品外观通过 `html[data-theme]` 投影；持久偏好非法时回退 `archive`。
- 两套主题提供相同语义角色，切换不改变布局、数据或功能状态。
- 主题监听每个 document 只注册一次。

#### R-U2 · 组件消费语义角色

- **等级**：MUST
- 页面组件使用 background/surface/border/text/primary/danger/warning/success 等已有 CSS custom properties。
- 原始色值只在主题定义、文件类型色或受审计 fallback 中出现。
- 正文对比度 ≥4.5:1；大号文字、图标和关键图形 ≥3:1。

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
- 普通正文不小于 12px；主指标 20–24px；行高 1.4–1.5。
- 图标按钮可见图标 14–16px，命中区至少 32×32px；缩放/拖拽命中区至少 24×24px。
- Grid 间距基线 8px，工作区边缘 12px；窄屏可收敛到 8px。
- 必要信息不得使用 disabled 色或只靠透明度表达。

#### R-U6 · 材质服从可读性和性能

- **等级**：MUST
- Shell/导航/浮层可以使用轻量半透明、blur 和阴影；正文、表格、代码、表单和图表保持稳定清晰。
- 禁止把高成本 filter、drop-shadow 或折射效果铺到所有 Widget。
- 视觉效果必须通过性能标准和 `prefers-reduced-motion`。

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
