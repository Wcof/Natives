# Appearance Preference 契约与首帧状态机（ADR-0022）

> 状态：**已冻结**（2026-08-25）。全应用唯一主题持久与协调契约。
> 关联：[ADR-0022](../adr/0022-appearance-workspace-and-app-surface-authority.md)、`docs/standards/frontend/02-state-and-data.md`、`docs/standards/technical/02-security.md`。

---

## 1. 总体权威与模型

- **单一持久权威**: Host SQLite `settings:theme`。
- **内部主题词表**: 严格限制为 `'dark' | 'light'`（`ThemeId`）。历史别名（`terminal-volt` → `dark`, `frosted-jasmine` → `light`）仅在 Host 读取时单向归一化，运行时不产生新别名。
- **协调层**: Renderer 端 `AppearanceCoordinator` 是唯一与 Host 通信并管理 DOM 注入的权威，React Context / Terminal / Monaco / Workshop 均为只读消费者。

```text
Host settings:theme + revision
        ↓ typed read/write + validated event
AppearanceCoordinator
        ├─ html[data-theme] + CSS variables (DOM Sink)
        ├─ ThemeContext (React Read Model)
        ├─ Terminal / Monaco / Ghostty Consumers
        └─ Settings / Command Palette / Status Bar
```

---

## 2. 接口与 DTO 契约

```ts
export type ThemeId = 'dark' | 'light';

export interface AppearancePreference {
  theme: ThemeId;
  revision: number;
}

export interface AppearanceCoordinator {
  /** 初始化并应用主题，返回当前生效的偏好 */
  bootstrap(): Promise<AppearancePreference>;
  /** 切换主题并持久化到 Host，返回新状态 */
  select(theme: ThemeId): Promise<AppearancePreference>;
  /** 订阅主题变更事件 */
  subscribe(listener: (value: AppearancePreference) => void): () => void;
  /** 获取当前同步读模型 */
  getSnapshot(): AppearancePreference;
}
```

---

## 3. 首帧与多窗口 FOUC 状态机

```text
[Window Launch: visible = false]
       │
       ▼
[RootClient: get_theme IPC]
       │
       ├─► 成功: Zod validate -> apply html[data-theme] & tokens.css variables -> ready
       │
       └─► 失败/超时: apply safe 'dark' fallback -> mark ready with classified error
       │
       ▼
[Invoke 'theme_ready_signal']
       │
       ▼
[Host: show window & make visible]
```

- **错误与降级**: 若 Host 读取失败，界面以受控 `dark` 显窗，并在 UI 呈现可重试的分类错误（`classifyError`），**禁止**静默 console.error 或抛出裸异常。
- **多窗口/设置联动**: 主题变更通过 Host 发送 `db-state-changed`（channel: `"theme"`, payload: `{ "theme": "dark" | "light", "revision": number }`），所有活跃窗口的 `AppearanceCoordinator` 监听并同步更新 DOM 与 CSS 变量。

---

## 4. 迁移与兼容性矩阵（v30 增量迁移）

1. **优先级**:
   `settings:theme`（合法值且非空） > 活跃 workspace legacy `theme` > `'dark'`。
2. **幂等性**:
   迁移仅在 `settings:theme` 缺失或非法时执行补齐；一旦设置，Workspace 切换或重载不得覆盖。
3. **Legacy 列存根**:
   `workspaces.theme` 物理列保留在 SQLite 中作为迁移审计证据，生产 DTO、查询与更新逻辑中彻底移除。
