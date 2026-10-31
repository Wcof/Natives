# 空间卡片与 space-kit 组件边界

> 维护日期：2026-10-31
> 适用代码：`extension/space-kit/`、`extension/plugins/widgets/`、`extension/space-*.js`

## 1. 目标与来源边界

空间模块原有 37 张卡片各自实现渲染、设置和样式，缺少统一的卡片外壳、元数据与可复用原语。本次参考
`joeseesun/qiaomu-home-dsh` 的个人空间交互（卡片目录、卡壳、待办/习惯/时间进度/世界时钟等），只借鉴
**交互模型与信息结构**，代码按本项目原生 DOM + `configJson` 契约重新实现。

参考仓库为 GPL-3.0-only；Natives 未声明兼容许可证。因此：

- 不复制其源码、样式文本、文案或图片资源；
- 不引入 React / localStorage 全局 store / 瀑布流布局；
- 新增代码全部为原创实现，引用仓库仅作为行为对照。

## 2. 目录与职责

```
extension/space-kit/            与产品壳无关的卡片基础设施
  card-meta.js                  卡片 key、分类、展示名、默认位置、limit 的唯一前端来源
  card-data.js                  日期键、七日窗口、时间进度、每日稳定选择、时区
  card-icons.js                 内联 SVG 图标（Shadow DOM 不依赖页面 sprite）
  card-shell.js                 卡壳：标题/图标/计数/动作/页脚
  card-ui.js                    空态、行内表单、列表行、进度行、文本域、防抖提交
  card-styles.js                卡壳与通用原语样式，由 Dashboard 只注入一次
  index.js                      公共出口

extension/plugins/widgets/      卡片实现，只依赖 space-kit 与自身数据
  daily-focus.js                今日重点
  quick-note.js                 快速记录
  scratchpad.js                 便签
  habit.js                      习惯打卡
  time-progress.js              时间进度
  world-clock.js                世界时钟
  daily-question.js             每日一问

extension/space-*.js            产品壳：快照、位置、Inspector、目录、写入队列
```

边界规则：

| 层 | 可以做 | 不可以做 |
|---|---|---|
| `space-kit` | 纯 DOM 结构、纯函数、通用样式 | 读 workspace 快照、调用 Native Host、依赖 Dashboard/Inspector |
| 卡片 `plugins/widgets/*` | 读 `data`、调用 `onDataChange`、声明 `renderSettings` | 直接写快照、直接 `nativeCall`、改全局 CSS |
| `space-dashboard.js` | 挂载/复用/销毁卡片、样式注入、写入编排 | 解释具体卡片业务 |
| `space-catalog.js` / `space-widget-settings.js` | 按元数据生成目录与通用设置 | 维护第二份卡片 key/分类清单 |

`space-kit.test.mjs` 会扫描源码，阻止 kit 反向依赖 Dashboard/Inspector，也阻止新卡片绕过 kit 直接依赖产品壳。

## 3. 与现有模块的集成方式

1. **注册表**：`card-meta.js` 的 `CARD_META` 是前端 key 唯一来源；`plugins/sanitizer.js` re-export
   `WIDGET_KEYS`，`space-plugins.js` 的 `pluginName()` 优先读取元数据里的中英展示名。
2. **目录**：`space-catalog.js` 按 `CARD_CATEGORIES` + `cardsInCategory()` 生成分组；`hidden: true`
   只保留给历史 `widget/aiPerformance`。新增卡片不需要再改目录数组。
3. **设置**：元数据声明 `limit: { def, max }` 时，`space-widget-settings.js` 自动渲染「显示数量」；
   卡片只读 `config.limit`，无需重复实现输入控件。
4. **位置**：`space-inspector.js` 添加卡片时使用元数据 `defaultPosition`，缺省仍是 `middleCentre`。
   九宫格与自由画布语义不变。
5. **样式**：`space-dashboard.js` 通过 `buildDashboardStyles({ cardStyles })` 一次性注入
   `SPACE_CARD_STYLES`；卡片 `styles` 只保留自身特有规则。
6. **数据**：仍走 `workspace_widget_upsert` → `configJson`。没有新增数据库表、没有新增 Port、
   没有改 `SCHEMA_VERSION`。跨天数据（今日重点、每日一问、习惯记录）由卡片在写入时归一化并裁剪。

## 4. 耦合点与冲突处理

| 耦合点 | 风险 | 当前处理 |
|---|---|---|
| Rust `workspace_store/schema.rs` 白名单 | 新 key 被 Host 拒绝，前端看似保存成功 | 已同步 7 个 key；`space-kit.test.mjs` 读取 Rust 源码逐个校验 |
| 三处旧卡片清单（sanitizer / catalog / plugins） | 增卡漏改导致目录或校验不一致 | 统一到 `card-meta.js`，其余位置只读派生 |
| Shadow DOM 内 `<use href="#i-...">` | 页面 sprite 在 Shadow DOM 中不可靠 | 新卡片使用 `card-icons.js` 内联 SVG；旧卡片暂不迁移 |
| 扩展打包 `scripts/extension-package.mjs` 的 `FILES` 白名单 | 新增目录不会进入发布包，生命周期检查报 `Missing production import` | 已把 `space-kit/` 加入白名单 |
| 保存回写触发整卡重渲染 | 多行输入时抢焦点/丢草稿 | `patchWidget` 对 `textarea:focus` 跳过重建；单行新增表单不跳过，保证提交后立即出现新条目 |
| 多个卡片各自 `setInterval` | 后台标签页耗电、节流不一致 | 复用 `plugins/widget-clock.js` 的文档级共享时钟 |
| `configJson` 64KB 上限 | 便签/记录/打卡无界增长 | 便签 20000 字、快速记录 100 条、习惯记录 30 天、答案 120 天、习惯 12 个 |
| 参考仓库 GPL-3.0 | 许可证冲突 | 只借鉴交互，不复制代码/文案/资源 |
| 参考仓库瀑布流布局 | 与现有 `displayJson.position` + 自由画布冲突 | 不引入 masonry；保持九宫格/自由画布为唯一位置权威 |
| 旧卡片外观 | 统一包卡壳会改变 37 张卡片的视觉与设置路径 | 卡壳按卡片 opt-in；新卡片使用，旧卡片逐步迁移 |

## 5. 新增一张卡片的检查清单

1. 在 `space-kit/card-meta.js` 增加 key、`category`、`icon`、可选 `limit` / `defaultPosition` / 中英名称。
2. 在 `plugins/widgets/` 新建实现，`render(container, data, display, { t, lang, onDataChange })` 返回 disposer。
3. 在 `plugins/widgets/index.js` 注册；`WIDGET_KEYS` 会自动更新。
4. 在 `crates/native-file-host/src/workspace_store/schema.rs` 的 `WIDGET_KEYS` 增加同一 key。
5. 在中英文 `messages.json` 增加卡片 UI 文案（目录名称/说明可只写在元数据）。
6. 运行：
   ```bash
   node extension/check-syntax.mjs
   node extension/space-kit.test.mjs
   node extension/space-widgets-structural.test.mjs
   node extension/space.test.mjs
   node extension/space-inspector.test.mjs
   cargo test -p native-file-host workspace_store
   ```

## 6. 未纳入本次迁移的能力

- **最近会话 / 全局搜索**：依赖 DSH 的 `sessions` / `workspaces` 服务与宿主路由，Natives 没有等价服务；
  强行迁移会引入新的跨模块契约。
- **瀑布流多页布局 / 每页独立编排**：现有空间以 `workspaceId + displayJson.position` 为权威，
  多页编排需要新的持久化模型与迁移，不属于组件抽离。
- **壁纸、天气、名言、倒计时**：现有卡片已覆盖，不重复实现。
