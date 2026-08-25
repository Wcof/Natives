# ADR-0022: Appearance Preference、Workspace 交互与 Apps Web Surface 统一权威

- **状态**: 已接受（Accepted）
- **日期**: 2026-08-25
- **决策者**: 产品方（用户）与主集成 Agent
- **取代（范围）**:
  - 取代 ADR-0021 中关于 `workspaces.theme` 驱动主题的条款（主题 authority 收敛至全局 `AppearancePreference`）；
  - 取代 ADR-0010 与 standards/ui-ux/01 中“禁止橙色/彩色主导交互、仅限中性色板”与 Design System V2 双材质冲突的部分（明确暗黑流光与晶透液态语义 token 色阶为一等公民）；
  - 取代历史文档中对 `AppView` 根字段序列化与 `BrowserState` 实例管理的歧义描述。
- **保留**: ADR-0020 的 Host authority、Legacy death list、Secret 归 OS Keychain、领域边界；ADR-0021 的 Multi-Workspace、Structured/Free 双布局、Widget Framework 升级等全部基础决策。
- **关联**: `docs/standards/`、`docs/contracts/workspace-v2-contract.md`、`docs/contracts/appearance-preference-contract.md`、`docs/contracts/apps-web-surface-contract.md`。

---

## 一、上下文与问题

1. **主题双权威与 FOUC 冲突**：
   - 现行实现中，设置页写入 Host `settings:theme`，而 Workspace 加载/切换时又从 `workspaces.theme` 读取并执行 `applyTheme`。当用户在设置页切换为浅色主题后，返回工作空间或刷新页面时，历史 Workspace 的 `dark` 会覆盖全局设置。
   - 此外，Shell 在异步读取主题完成前提前显窗，导致首帧闪烁；现有 standards/ui-ux/01 强调完全中性黑白灰，与 Design System V2 及参考图中的珊瑚橙/琥珀金交互色存在标准冲突。
2. **Workspace 交互手势与状态回滚缺失**：
   - Structured Grid 中，`.grid-content` 同时作为容器与 RGL cancel ancestor，吞噬了卡片 Header 的拖动手柄，且 resize 只支持单向；Free Canvas 中存在 821 行巨型组件、Stage 与 Node 竞争 pointer gesture、8px resize handle 远低于 24px 命中区合同、pointermove 期间存在非受控状态抖动。
3. **Apps Wire Contract 与 WebView 运行时分裂**：
   - Rust 后端 `AppView` 未配置 `#[serde(rename_all = "camelCase")]`，字段以 snake_case 序列化，而前端读取 camelCase 字段，导致 `appId` 解析为 `undefined`，出现 React key 告警与无法打开 Web 应用。
   - `commands/apps.rs` 的 `service()` 函数每次构造新的局部 `BrowserState`，与 Tauri managed 状态分离，导致打开、预算、隐藏、关闭不在同一运行时权威。

---

## 二、决策

### 1. 全局 AppearancePreference 单一持久权威（取代 ADR-0021 对应主题段）

- **单一持久源**: Host SQLite 中的 `settings:theme` 为全应用唯一主题持久权威；键值词表严格限制为 `dark | light`。
- **派生消费模型**: Renderer `AppearanceCoordinator` 是前端唯一协调层，负责向 Host 发起原子读写、Zod 边界校验、并发序列化、广播监听与 DOM/CSS 变量注入。React Context、Terminal、Monaco、Workshop 等均为派生只读消费者。
- **Workspace Theme 降级与 v30 迁移**:
  - `workspaces.theme` 从生产 Snapshot、Create/Update DTO、Inspector 与运行时读写中彻底移除。
  - 增量迁移 **v30** 按照 `settings:theme（若合法） > 活跃 workspace legacy theme > 'dark'` 进行单向迁移并锁定。`workspaces.theme` 物理列保留作迁移审计证据，不立即 drop。
- **首帧与多窗口 FOUC 防护**:
  - 窗口启动保持隐藏（`visible: false`），由 `RootClient` 执行 `get_theme` → 校验 → 注入 `html[data-theme]` 与全量 CSS 变量 → 触发 `theme_ready_signal` → Host 显示窗口。若 Host 读取失败，使用受控 `dark` fallback 显窗并呈现分类错误及重试入口。

### 2. Design System V2 语义 Token 色阶（规范对齐）

- 明确 **晶透液态（Liquid Crystal / 浅色）** 与 **暗黑流光（Dark Glow / 深色）** 为一等支持主题，共享同一套语义 key：
  - `canvas`: 浅色 `#F5F6F8`~`#F8FAFC`；深色 `#0B0F14`~`#0D1117`。
  - `surface`: 浅色 `#FFFFFF`（72% 白玉玻璃）；深色 `#121820`（88% 黑晶表面）。
  - `inset`: 浅色 `#F1F3F5`；深色 `#18202B`。
  - `interactive-accent`: 浅色 `#EA580C`~`#F97316`（珊瑚橙）；深色 `#F59E0B`~`#FB923C`（琥珀金）。
  - `chart lines`: 浅色 紫/蓝；深色 亮紫/天蓝。
- 业务组件禁止私有 hex 色值，所有视觉值经 CSS 语义变量或 TS 设计令牌消费。

### 3. Workspace 交互与生命周期状态机

- **布局权威链**: `Host SQLite -> Workspace Domain Service -> Typed IPC -> Renderer Snapshot`。
- **手势生命周期**: 统一为 `idle → draft (dragging/resizing) → commit | rollback`。
  - `pointermove` 期间 IPC / SQLite 写入必须为 **0**；
  - 仅在有效的 `drag-stop` / `resize-stop` / `keyboard` 事件触发时向 Host 提交最多 **1** 次原子事务；
  - `Escape`、`pointercancel`、网络失败或 Host 冲突时，必须回滚到上一次确认的 canonical snapshot。
- **Structured Grid**:
  - Header 非控件空白区为唯一拖动手柄（`handle: .ws-shell-header`）；
  - Body、按钮、输入框、图表交互区严格为取消区（`cancel`）；
  - 支持 n/e/s/w/ne/nw/se/sw 八向缩放，可见区域 ≥16px，命中区域 ≥24px；
  - 键盘方向键移动 1 unit，Shift 步进 2 units。
- **Free Canvas**:
  - Stage 独占手势控制器（`gesture controller`），按职责拆分为视图容器、手势控制器、节点表面、屏幕坐标八向缩放覆盖层（Screen-space Resize Overlay）与工具栏；
  - 缩放手柄保持屏幕像素恒定（≥24px 命中区），不随画布 world zoom 缩小；
  - 8px 网格吸附；键盘方向键 8px，Shift+方向键 1px；
  - 新增 Widget 必须由 Host 原子分配真实 `workspace_widgets.id` 并落库，禁止 Renderer 生成幽灵节点。

### 4. Apps Wire Contract 与 Managed Web Surface

- **DTO 序列化单一权威**: Rust `AppView` 根字段使用 `#[serde(rename_all = "camelCase")]`，TS 生成绑定为前端唯一契约源。禁止手写影子类型，禁止使用 index 或 title 替代 `appId` 作为 React key。
- **单一 Managed BrowserState**:
  - `src-tauri` 使用统一的 Tauri managed `BrowserStateHandle`（`Arc<Mutex<BrowserState>>`）；
  - `apps_open`、`apps_web_close`、`apps_web_hide`、预算管理（LRU/Hibernate）全部由该单例托管；
  - `AppPresentationHost` 必须在测量到有效 `contentRect` 后再发起呈现，Host 负责对 bounds 进行安全校验与 clamp。
- **独立 Trust Domain**: Child WebView 严禁注入主窗口 capability、Tauri IPC、本地文件读写、Shell、Secret 或 Workshop Session Token。

---

## 三、后果与约束

1. **架构收益**:
   - 彻底消除了主题多端不一致、FOUC 闪烁和返回覆盖问题；
   - 恢复了 Grid 和 Canvas 完整的拖拽与八向缩放体验，手势零无效持久化；
   - 修复了应用中心的 wire 契约和 WebView 生命周期，确保多 App 预览和预算管理受控。
2. **红线与约束**:
   - 禁止新增任何第三方状态库（Redux/Zustand等）、额外拖拽引擎或 Canvas 运行时；
   - 禁止为解决 key 告警而在前端加 fallback 补丁；
   - 源码修改必须在 P0 提交完全生效后由各任务按文件租约执行。
