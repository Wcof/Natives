# Reference Provenance 记录模板（M-007）

> 每个借鉴点必须可追踪且**无源码复制**。Midday / Plane / AFFiNE / Twenty 只吸收
> 产品、设计、布局、交互**思想**，禁止复制、移植或逐行改写其源码（含 JSX/TS/CSS/SQL/Schema）。

## 硬规则

1. 不复制参考项目 JSX/TS/TSX/CSS/SQL/Schema。
2. 不对参考源代码做逐行翻译或重命名搬运。
3. 不从 AGPL/Enterprise 区域复制实现细节（Midday/Plane/Twenty 主仓库为 AGPLv3，AFFiNE 部分 MIT/其他许可）。
4. 不引入对方内部 package 以规避重写。
5. 每个借鉴项先写成「行为需求 / 工程原则」，再用 AiNative 自身类型与现有依赖（React/Framer/RGL/Recharts/Zod）重建。
6. 新文件命名、数据模型、API 以 AiNative 语义为准。
7. 提交说明至少标记：`Inspired concept: <project>/<concept>; implementation: native AiNative design`。
8. 若对某段实现是否过度相似存在疑问：停用该段，按需求重新设计。

## 记录模板（每波 handoff 与 commit message 中使用）

```text
## Reference Provenance

- Inspired concept: Midday/metric-chart-primitives
  Behavior: 独立 Metric/Chart primitive + 统一 container/tooltip
  Implementation: AiNative 自研 MetricBlock/ChartFrame（Framer + Recharts + V2 tokens）——NO SOURCE COPIED
- Inspired concept: AFFiNE/edgeless-geometry-selection
  Behavior: viewport/camera 与 document model 分离、selection 独立管理
  Implementation: AiNative 轻量 DOM Canvas（Camera/Geometry/Selection/Snap）——NO SOURCE COPIED
- ...
```

## 借鉴点速查（03-REFERENCE-ABSORPTION-MATRIX 摘要）

| 维度 | 参考 | AiNative 实现 | 禁止 |
|---|---|---|---|
| Dark Glow / Metric / Chart | Midday | Token V2 + Metric/Chart primitives | 抄 CSS/组件 |
| Multi-Workspace / View State | Plane | SQLite Workspace domain + session tabs | SaaS/Issue 模型 |
| Compact Grid | AiNative 现状 | 继续 react-grid-layout | 重写成熟网格 |
| Free Canvas | AFFiNE | 轻量 DOM camera/geometry/selection/frame | BlockSuite/Yjs/CRDT |
| Widget Extensibility | Twenty | code registry: Renderer/Inspector/Adapter | Plugin Runtime/No-Code |
| Inspector | Twenty | 泛化 ResizableRightPanel | 复制 SidePanel |
| Data Views | Plane + Twenty | 受控 List/Table/Board/Calendar | 通用元数据平台 |
| Local-First | AiNative + AFFiNE | SQLite/Files authoritative + memory hot state | IndexedDB authority |
| Motion | Midday | Framer Motion + CSS transitions | RGL transform 冲突 |
