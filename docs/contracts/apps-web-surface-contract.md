# Apps Web Surface 契约与运行时生命周期（ADR-0022）

> 状态：**已冻结**（2026-08-25）。Apps 域 DTO、Wire Contract 与 Web Surface 生命周期契约。
> 关联：[ADR-0022](../adr/0022-appearance-workspace-and-app-surface-authority.md)、`docs/standards/technical/01-layering.md`、`docs/standards/technical/02-security.md`、`docs/standards/technical/05-backend.md`。

---

## 1. DTO 与 Wire 契约权威

Rust 后端 `src-tauri/src/apps/model.rs` 为 DTO 单一 Source of Truth，必须显式配置 `#[serde(rename_all = "camelCase")]` 与 `#[ts(export)]`：

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
#[serde(rename_all = "camelCase")]
pub struct AppView {
    pub app_id: String,
    pub title: String,
    /// snake_case: local_project | system_application | web_application
    pub kind: String,
    /// snake_case: manual | local_scan | system_discovery | legacy_internal | legacy_github | migration
    pub registration_origin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub show_in_sidebar: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidebar_order: Option<i64>,
    pub capabilities: AppCapabilities,
    pub runtime_state: AppRuntimeState,
    pub updated_at: String,
}
```

- **生成输出**: `src/types/generated/AppView.ts` 由 `ts-rs` 自动生成，**严禁手写或手动修改**。
- **主键唯一性**: `appId` 必须非空且唯一，React 渲染列表必须以 `app.appId` 为 key，**严禁**使用 index 或 title 进行 fallback 掩盖主键缺失。

---

## 2. Managed BrowserState 与运行时生命周期

- **托管单例**: Tauri Host 使用单一 `BrowserStateHandle`（`Arc<Mutex<BrowserState>>`）进行状态托管。
- **状态流转**:
  - `apps_open`: 校验 `appId` → 查找/分配 live child WebView → 设置有效 bounds → show。
  - `apps_web_hide` / 导航离开: 保留实例但隐藏视图（hide）。
  - `apps_web_close`: 显式销毁 WebView 实例并释放预算配额。
  - `apps_remove`: 先关闭关联的 Web Surface，再从 SQLite Registry 中删除记录。

```text
[Registered in SQLite] ──(apps_open)──► [Child WebView Allocated]
                                               │
                                 ┌─────────────┴─────────────┐
                           (navigate away)              (apps_web_close)
                                 │                           │
                                 ▼                           ▼
                           [View Hidden]               [Destroyed & Budget Freed]
```

---

## 3. 预算与 LRU 休眠（Hibernate）

- **活跃上限**: 同一时刻最多保持 **6** 个活跃 Child WebView。
- **总实例上限**: 最多分配 **10** 个实例。
- **LRU 回收**: 超出上限时，按照 `last_active_at` 顺序自动触发 `Hibernate`，销毁底层 WebKit 进程并将 `runtime_state` 标记为 `hibernated`。

---

## 4. 呈现与 Bounds 时序

- `AppPresentationHost` 必须监听 Shell 内容区域的 `ResizeObserver`；
- 仅在获取到宽度 > 0 且高度 > 0 的有效 `DOMRect` 后，才向 Host 发送坐标更新指令；
- Host 负责对 bounds 进行 clamp 和安全校验，高频 resize 采用防抖（debounce），防止每帧 IPC。

---

## 5. 安全与能力隔离红线

- Child WebView 处于独立沙箱信任域；
- **严禁**注入主窗口 Tauri IPC、本地文件读写、Shell、Secret 或 Workshop Session Token；
- 目标 URL 必须通过 Scheme 与 Host 白名单过滤。
