# 设置页独立外壳整改设计

日期：2026-07-14

## 目标

把设置改造成类似 Codex 的独立工作区：用户点击主侧栏底部的“设置”后，左侧整体切换为设置菜单，右侧切换为设置正文；设置侧栏顶部提供“返回主页”，返回后进入 Dashboard。

本次允许重排设置的信息架构与正文布局，但不新增设置能力。环境变量是旧版服务商 URL/API Key 配置的重复入口，确认当前没有需要迁移的数据，因此直接移除该入口，不做迁移或兼容读取。

## 范围

### 包含

- 设置模式的独立侧栏、正文和返回主页交互。
- 设置菜单重排为通用、外观、服务商、执行引擎、插件五页。
- 设置正文统一为单列、低层级、少卡片的 Codex 式布局。
- 进入设置时隐藏全局 Header、底部终端和右侧面板，离开设置时恢复仍适用于 Dashboard 的原状态。
- 修复 `settings:*` 被错误解析为普通模块的问题。
- 删除设置页中的环境变量入口、页面、前端状态、加载逻辑和专属文案。
- 移除旧 Onboarding 中使用环境变量保存 AI Key 的路径，使服务商成为 AI URL、API Key 和默认模型的唯一配置入口。

### 不包含

- 新增设置项。
- 重写服务商、执行引擎或插件的业务逻辑。
- 数据迁移；当前确认没有环境变量凭据数据。
- 删除终端使用的 Shell 环境配置能力。
- 删除 `env_manager` 的共享加密基础设施；现有旧凭据读取和终端环境注入仍依赖它。

## 方案选择

采用“扁平五页”。

评估过的其他方案：

- 分组五页：增加“基础 / AI / 扩展”分组，未来扩展更方便，但当前只有五项，分组会增加视觉层级。
- 单页滚动：所有设置在一张长页面，左侧使用锚点；服务商和执行引擎内容较长，会让页面过重并增加滚动定位复杂度。

扁平五页最接近 Codex 的设置结构，定位直接，也能复用当前页面能力，改动最小。

## 信息架构

设置路由使用以下内部视图标识：

| 视图 | 菜单名称 | 正文内容 |
| --- | --- | --- |
| `settings:general` | 通用 | 语言设置 |
| `settings:appearance` | 外观 | 现有主题选择 |
| `settings:providers` | 服务商 | 服务商 URL、API Key、默认模型管理 |
| `settings:runtime` | 执行引擎 | 现有 Runtime、工具与保护设置 |
| `settings:plugins` | 插件 | 现有插件刷新、启停和卸载 |

`settings` 和无效的 `settings:*` 均回退到 `settings:general`。设置视图不写入 URL，也不需要兼容旧的 `settings:theme`、`settings:env`、`settings:executor` 内部状态，因为当前 Shell 视图没有持久化。

## 布局

### 设置侧栏

- 复用主侧栏现有宽度、窗口控制区、拖拽区、折叠与缩放能力。
- 设置模式下不显示品牌、搜索、快捷访问、助理、模块、通知、设置和创意工坊入口。
- 窗口控制区下方固定“← 返回主页”按钮。
- 返回按钮下方显示“设置”标题，再按顺序显示五个设置菜单项。
- 当前项使用现有选中态 token；其他项使用普通 hover 状态，不为每一项常驻填充背景。
- 菜单项使用真实 `button`，当前项设置 `aria-current="page"`，保留键盘焦点样式。

### 设置正文

- 设置模式不显示全局 Header；页面标题由设置正文自己提供。
- 正文使用单列布局，最大宽度约 `760px`，在可用空间内水平居中，并让所有页面共享同一条左侧内容基线。
- 每页顶部包含标题和一句简短说明。
- 普通设置采用“左侧名称与说明、右侧控件”的设置行，并用分隔线区分设置组。
- 减少重复卡片和嵌套边框；仅服务商密钥列表、执行引擎状态和插件列表等复杂内容保留容器。
- 内容区独立滚动，侧栏保持固定。

## 导航与状态流

1. 用户点击主侧栏“设置”。
2. Shell 将 `activeView` 设置为 `settings:general`。
3. `isSettingsView` 由 `activeView === 'settings' || activeView.startsWith('settings:')` 得出。
4. Sidebar 根据 `isSettingsView` 渲染设置菜单；MainContent 根据合法 section 渲染对应正文。
5. Header 在设置模式不渲染。
6. Terminal 和 RightPanel 在设置模式只做视觉隐藏，不卸载、不清空父级状态，避免终端会话、文件预览和通知状态丢失。
7. 主内容容器在设置模式不保留终端间距。
8. 用户点击“返回主页”，`activeView` 变为 `dashboard`；终端和仍适用于 Dashboard 的右侧面板恢复显示。
9. `module-details` 只对模块页有效，返回 Dashboard 时关闭，避免显示无上下文的空面板；通知和文件预览可以恢复。

所有设置入口，包括主侧栏、命令面板和业务内跳转，都统一发送 `settings:<section>`。Shell 的导航解析必须先处理 `settings`/`settings:*`，再处理普通模块，禁止为设置视图添加 `module:` 前缀。

## 环境变量清理边界

环境变量旧设置曾承担服务商 URL/API Key 配置，现由服务商完整覆盖。整改时：

- 从 Sidebar 删除环境变量菜单。
- 从 SettingsPage 删除 `env` section、配置档案列表、变量列表、创建配置和相关加载状态。
- 删除仅服务于该页面的中英文文案。
- 删除旧 Onboarding 中通过 `env.listProfiles`、`env.createProfile`、`env.setVariable` 保存 AI Key 的路径；后续 AI 凭据配置只进入服务商流程。
- 不增加一次性迁移、不读取旧环境凭据、不自动删除数据库数据。

终端当前通过环境配置档案向 Shell 会话注入变量。它不是服务商凭据入口，因此本次保留终端选择器、Tauri 环境命令、数据库表和 `env_manager` 加密函数。共享加密函数也仍被部分旧凭据读取代码使用，不能按文件整体删除。

## 组件改动

### `ShellLayout`

- 增加统一的设置视图判断。
- 导航处理器直接接受 `settings:*`。
- 设置模式隐藏全局 Header，并视觉隐藏 Terminal 与 RightPanel。
- 返回 Dashboard 时只清理失去上下文的 `module-details`。

### `Sidebar`

- 设置模式改为五项扁平菜单。
- 返回按钮发送 `__dashboard__`。
- 删除环境变量项，更新选中态和无障碍属性。

### `MainContent`

- 规范化设置 section；缺失或非法值回退到 `general`。
- 把合法 section 传给 SettingsPage。

### `SettingsPage`

- section 改为 `general | appearance | providers | runtime | plugins`。
- 删除环境变量状态、请求和 JSX。
- 将语言与外观拆为两页。
- 复用 ProviderDetail、RuntimePanel 和现有插件处理函数。
- 只渲染当前 section 的正文，保持统一页面标题、说明、间距和错误状态。

### 旧 Onboarding

- 当前 `OnboardingWizard` 没有调用方，直接删除该死组件，从而一并移除把 AI Key 写入环境变量配置档案的旧路径，不另建替代抽象。

## 反馈与错误处理

- 设置页数据加载失败使用当前 section 内联错误，不让整页白屏。
- 保存、测试和刷新结果继续使用现有 Toast。
- 删除服务商、Key 或插件继续使用确认对话框，不使用浏览器原生确认框。
- 非法设置 section 静默回退到“通用”，不显示错误页。
- 服务商凭据继续只在 Rust 侧解密；前端只接收掩码值。

## 验收与测试

新增一个最小导航回归测试，覆盖：

- `settings` 和非法设置 section 回退到 `settings:general`。
- `settings:providers` 保持原值，不变成 `module:settings:providers`。
- 普通模块仍解析为 `module:<id>`。
- 设置 section 列表不包含环境变量。

人工交互验收：

1. 从 Dashboard、模块页和文件页进入设置，左侧与正文同时切换。
2. 默认打开“通用”，五个菜单均能正确切换且选中态唯一。
3. 设置模式不显示全局 Header、终端和右侧面板。
4. 返回按钮进入 Dashboard；原终端会话仍在，通知或文件预览状态可恢复，模块详情不会成为空面板。
5. 界面中不再出现用户可见的环境变量设置入口。
6. 旧 Onboarding 不再通过环境变量保存 AI Key。
7. 服务商的 URL、API Key、默认模型创建、测试、选择和删除保持可用。
8. 浅色、深色和窄窗口下检查正文滚动、侧栏缩放、焦点与文本截断。

自动验证命令：

```bash
rtk npm test
rtk npm run typecheck
rtk npm run i18n:check
rtk npm run lint
```

## 完成标准

- 设置拥有独立外壳和五页扁平信息架构。
- 返回主页、面板隐藏/恢复和设置项切换行为符合本设计。
- 服务商是 AI URL、API Key 和默认模型的唯一用户入口。
- 环境变量旧设置入口及 AI Key 写入路径已移除，终端 Shell 环境能力未受影响。
- 导航回归测试与项目验证命令通过。
