# 应用中心 V2 · AI 可执行需求基线

> 状态：`ready-for-solution-design`
> 日期：2026-08-23
> 适用平台：macOS（P0）
> 产物用途：供后续 Agent 进行交互方案、技术方案、任务拆分与验收设计
> 权威说明：本文是产品需求输入；若与 `docs/standards/` 或已接受 ADR 冲突，必须先修订 ADR/Standards，禁止在实现中静默绕过。

## 1. 一句话定义

应用中心是 Natives 内统一管理和快速切换常用 Web 应用与 macOS 应用的个人应用工作台：Web 应用在 Natives 内容区内真实呈现；macOS 应用由 Natives 观察、唤起并将其主窗口一次性归位到内容区位置，但不伪装成跨进程嵌入。

## 2. 需求来源与事实边界

### 2.1 已确认的用户决策

| ID | 决策 | 来源 |
|---|---|---|
| D-01 | 应用中心只支持 `Web 应用` 与 `macOS 应用` 两类，不再提供本地项目入口。 | 用户确认 |
| D-02 | 进入应用中心默认看到已注册应用列表、真实状态和基础 CRUD。 | 用户确认 |
| D-03 | 左侧侧边栏展示用户在应用中心选择“显示在侧边栏”的应用，可点击切换。 | 用户确认 |
| D-04 | Web 应用在 Natives 内容区内显示，不跳到系统浏览器，不以浏览器 Tab 作为主交互。 | 用户确认 |
| D-05 | macOS 应用不做伪 iframe；首次点击时启动/激活，并把主窗口一次性移动、缩放到 Natives 内容区。 | 用户确认 |
| D-06 | 一次归位后不持续跟随 Natives 窗口移动或缩放；需要时由用户主动“重新归位”。 | 用户确认 |
| D-07 | macOS 应用后续只提供观察、激活、隐藏、正常终止和重新归位，不扩展成远程控制或窗口管理器。 | 用户确认 + 产品收敛 |

### 2.2 当前代码事实

| ID | 当前事实 | 证据 |
|---|---|---|
| F-01 | Apps 已有 App Registry、System/Web/Local 三类模型、CRUD、状态能力和侧边栏投影。 | `src-tauri/src/apps/`、`src/components/apps/` |
| F-02 | 当前侧边栏点击 `apps:item:<id>` 只调用 `appsApi.open()`，没有把 Shell 的 `activeView` 切换为应用呈现态。 | `src/components/shell/ShellLayout.tsx` |
| F-03 | 当前 Web 应用使用 Tauri child WebView，但默认固定 bounds，不绑定 Shell 实际内容区。 | `src-tauri/src/apps/web.rs` |
| F-04 | 当前 Web 注册、打开和错误反馈没有形成完整用户闭环；部分错误只写 `console.error`。 | `AppsPage.tsx`、`WebApplicationForm.tsx` |
| F-05 | 当前 macOS driver 已具备发现、启动/激活、观察和终止能力；“主窗口归位”尚未纳入产品契约。 | `src-tauri/src/apps/system/` |
| F-06 | 当前 WebView 资源预算已有软上限 6、硬上限 10，应复用，不另建资源管理体系。 | `src-tauri/src/apps/`、既有 Apps 架构文档 |

### 2.3 必须守住的技术真实性

- `Surface` 只表示由 Natives 拥有的呈现面。Web child WebView 可以是 Surface；外部 macOS App 的窗口不是 Natives Surface。
- macOS App 归位是对另一个进程窗口的移动/缩放，不是真正嵌入、重父化或 iframe。
- 不使用 ScreenCaptureKit 伪造可交互页面，不做画面采集 + 输入转发。
- 辅助功能权限缺失、窗口不可调整、应用全屏或应用不支持标准窗口属性时，必须显示真实的 `permission_required` / `unsupported` / `failed`，并降级为启动或激活。
- Web 注册成功与 Web 页面打开成功是两个状态。加载失败不得回滚或伪装成注册失败。

## 3. 产品目标与非目标

### 3.1 P0 目标

1. 用户可以稳定注册并保存一个 Web URL，或从已安装应用中注册一个 macOS App。
2. 应用中心首先呈现应用列表，而不是创建器、空白页或运行时详情页。
3. 用户可完成新增、查看、编辑、移除注册和侧边栏显示管理。
4. 用户从左侧侧边栏单击应用即可切换到对应目标，不需要管理浏览器 Tab。
5. Web 应用保留登录态和页面上下文，并在内容区内显示。
6. macOS 应用可被真实观察、激活、隐藏、正常终止，并在首次打开或用户要求时归位到内容区。
7. 所有 loading、empty、error、unsupported 和 permission 状态都可区分、可恢复。

### 3.2 明确非目标

- 不注册或运行本地项目。
- 不把 macOS App 真正嵌入 Natives 进程或 WebView。
- 不做多窗口管理；P0 只控制一个“当前主窗口”。
- 不持续追踪、锁定或强制纠正用户移动后的外部窗口。
- 不做屏幕采集、键鼠注入、自动化脚本或远程控制。
- 不做强制终止；P0 只请求应用正常退出。
- 不做通用浏览器的 Tab、地址栏、书签、扩展市场或下载管理器。
- 不做 Windows/Linux 的外部应用窗口控制。
- 不做应用商店、团队共享、云同步或插件运行时。

## 4. 竞品研究与可采用结论

| 产品 | 已验证做法 | 对 Natives 的启发 | 不照搬的部分 |
|---|---|---|---|
| Arc | Pinned Tabs/Favorites 固定在侧栏；不同 Space 保存不同上下文；固定项更像“应用 + 书签”。 | 侧栏应用应稳定、可排序、可恢复上次上下文；点击是直接切换，不先进入详情页。 | 不引入 Tab/Space 浏览器模型。 |
| Wavebox | Web 应用集中在纵向栏；保持多账号登录；闲置应用可睡眠，点击后恢复并明确显示睡眠状态。 | 每个 Web 应用使用独立持久会话；保留既有 6/10 WebView 预算与 LRU 休眠；侧栏必须显示休眠/错误状态。 | 不建设 Chromium 浏览器、扩展生态、统一通知或多账号平台。 |
| Raycast | 可以搜索、启动、切换隐藏状态；窗口移动/缩放需要 macOS Accessibility 权限，个别 App 会忽略窗口命令。 | macOS App 的核心能力应是 launch/activate/hide/terminate；窗口归位按能力执行并提供权限引导和诚实降级。 | 不建设全局 Launcher、热键平台或任意窗口布局系统。 |
| Rectangle | 通过快捷动作将当前窗口移动、缩放到明确区域。 | “重新归位”应是一次性、确定性的窗口动作，尊重用户之后的手工移动。 | 不提供通用吸附区域、复杂布局和快捷键配置。 |
| macOS Stage Manager | 左侧最近应用 + 中央焦点窗口支持快速切换；窗口仍保持独立，可由系统排列。 | “单一当前目标 + 左侧切换”是可理解的桌面心智；Natives 应承认外部窗口独立存在。 | 不复制系统级分组、Space 与全桌面窗口编排。 |

竞品来源（均为官方资料）：

- [Arc Pinned Tabs](https://resources.arc.net/hc/en-us/articles/19231060187159-Pinned-Tabs-Tabs-you-want-to-stick-around)
- [Arc Spaces](https://resources.arc.net/hc/en-us/articles/19228064149143-Spaces-Distinct-Browsing-Areas)
- [Wavebox App Sleep](https://kb.wavebox.io/kb/sleep-apps-to-save-memory/)
- [Wavebox Workspaces](https://wavebox.io/appinfo/wavebox_workspaces.html)
- [Raycast Applications Settings](https://manual.raycast.com/settings)
- [Raycast Window Management](https://manual.raycast.com/window-management)
- [Rectangle](https://rectangleapp.com/)
- [Apple Stage Manager](https://support.apple.com/guide/mac-help/use-stage-manager-mchl534ba392/mac)

### 4.1 竞品分析后的产品修正

1. 不再把所有类型都塞进“运行/停止”二元状态；Web 与 macOS App 使用各自真实状态。
2. 不把 Web 打开失败和注册失败混为一谈；Registry 是稳定资产，呈现是可恢复会话。
3. 不默认持续吸附外部窗口；一次归位 + 显式重新归位更符合用户控制权。
4. 不在启动时索要 Accessibility 权限；仅在首次执行窗口归位时解释并请求。
5. 不承诺任意 macOS App 都可归位；启动/激活是基础能力，归位是 capability-gated 增强能力。

## 5. 信息架构

```text
左侧主导航
└── 应用中心
    ├── 用户选定的应用 A
    ├── 用户选定的应用 B
    └── 用户选定的应用 C

主内容区
├── 应用中心控制页
│   ├── 应用列表
│   └── 所选应用详情/操作
├── Web 应用呈现态（Natives-owned WebView）
└── macOS 应用承载态（外部窗口归位覆盖内容区；Natives 仅保留控制壳）
```

### 5.1 应用中心控制页

- 默认页必须是应用列表。
- 列表最少展示：图标、名称、类型、真实状态、是否显示在侧边栏、最后活跃时间、更多操作。
- 支持按名称搜索，按“全部 / Web / macOS”和状态筛选。
- 点击列表项打开详情；点击“打开”才进入应用呈现态。
- 空态提供两个主入口：“添加 Web 应用”“添加 macOS 应用”。

### 5.2 左侧侧边栏

- 仅投影 `showInSidebar = true` 的已注册应用。
- 支持拖拽排序；排序只更新 `sidebarOrder`，不改变应用 Registry 身份。
- 当前目标必须有选中态；状态点只表达真实状态，不用装饰性绿色。
- 单击直接切换；右键或更多菜单提供“从侧边栏移除”，不等于删除注册。
- “应用中心”固定保留为管理入口，不被应用列表挤掉。

## 6. 核心对象与状态

### 6.1 对象模型

```ts
type AppKind = 'web_application' | 'system_application';

type AppRegistration = {
  appId: string;
  kind: AppKind;
  title: string;
  icon?: string;
  showInSidebar: boolean;
  sidebarOrder?: number;
  createdAt: string;
  updatedAt: string;
};

type WebSpec = {
  appId: string;
  url: string;
  approvedOrigins: string[];
  keepAlive: boolean;
  browserProfileId: string;
};

type SystemAppSpec = {
  appId: string;
  applicationPath: string;
  bundleIdentifier: string;
};
```

要求：优先收敛既有类型与表，不建立平行 Registry；字段名称可在技术设计阶段适配现有 schema。

### 6.2 Web 状态

| 状态 | 用户文案 | 含义 |
|---|---|---|
| `closed` | 已关闭 | 无活跃 WebView，会话数据仍保留。 |
| `loading` | 正在打开 | WebView 已创建，页面尚未 ready。 |
| `ready` | 已打开 | 页面可见或可立即显示。 |
| `hidden` | 后台 | WebView 存活但不可见。 |
| `hibernated` | 已休眠 | 为释放资源销毁活跃 WebView，下次点击恢复 URL/会话。 |
| `error` | 打开失败 | 注册仍存在；用户可重试、编辑 URL 或外部打开。 |
| `unsupported` | 不兼容 | 页面明确阻止或 WebKit 无法满足关键能力。 |

### 6.3 macOS App 状态

| 状态 | 用户文案 | 含义 |
|---|---|---|
| `not_installed` | 未找到 | 注册路径或 Bundle ID 已失效。 |
| `stopped` | 未运行 | 系统未观察到该应用进程。 |
| `launching` | 正在启动 | 已发出启动请求，等待应用可观察。 |
| `running` | 运行中 | 进程存在，但不一定前台。 |
| `hidden` | 已隐藏 | 进程存在且由系统报告为隐藏。 |
| `active` | 使用中 | 应用为前台目标。 |
| `unobservable` | 状态不可用 | 系统查询失败，不得显示为“已停止”。 |

### 6.4 归位状态

| 状态 | 用户文案 | 行为 |
|---|---|---|
| `never_docked` | 尚未归位 | 首次点击尝试归位。 |
| `docked` | 已归位 | 最近一次窗口设置成功，不代表持续绑定。 |
| `needs_redock` | 可重新归位 | Natives 内容区改变或用户移动了窗口；不自动纠正。 |
| `permission_required` | 需要辅助功能权限 | 保留启动/激活能力，显示授权说明与系统设置入口。 |
| `unsupported` | 此窗口无法归位 | 保留启动/激活/隐藏/终止能力。 |
| `failed` | 归位失败 | 显示可重试错误，不伪装成功。 |

## 7. 功能需求

### FR-01 应用列表与 CRUD（P0）

- 首次进入 `/apps` 必须读取真实 Registry 并展示列表。
- Create：仅提供 Web 与 macOS App 两种添加入口。
- Read：可查看类型专属配置、状态和当前可用动作。
- Update：可修改名称、图标、类型专属配置、侧边栏可见性和顺序；不可原地切换 AppKind。
- Delete：只删除 Natives 注册，不卸载 macOS App，不删除 Web 会话数据；清除 Web 数据必须是独立高风险动作。
- 任一 mutation 成功后列表、详情和侧边栏必须基于同一事件源更新。
- 列表加载失败显示内联错误和重试，不得退化为空态。

### FR-02 Web 注册（P0）

- URL 缺少 scheme 时自动补 `https://`，并在提交前展示规范化结果。
- 仅接受 `http`/`https`；公网地址默认要求 `https`，loopback 开发地址允许 `http`。
- 标题为空时可从 hostname 生成，用户仍可编辑。
- `approvedOrigins` 默认从 URL 自动推导；普通用户界面不要求手填，扩展域名放入“高级设置”。
- 点击保存时先持久化 Registry；保存成功后返回新 `appId` 并选中该记录。
- 页面加载属于独立动作；加载失败不删除刚保存的注册。
- 重复 URL 允许注册，但必须由独立 BrowserProfile 隔离会话，名称不得为空。
- 所有校验错误在字段下显示；后端错误显示可读信息与重试入口，禁止只写 console。

### FR-03 Web 呈现与切换（P0）

- 每个已打开 Web 应用最多一个长期 child WebView，以稳定 label 复用。
- WebView bounds 必须来自 Natives 当前内容区实际矩形，不使用固定坐标。
- 点击 Web 侧边栏项：激活 Natives → 隐藏上一目标 → 创建或显示目标 WebView → 更新选中态。
- 切换离开时默认隐藏，不销毁；达到资源预算时按既有 LRU 规则休眠。
- 重新显示必须保留 cookie、localStorage、登录态和可恢复页面上下文；休眠后允许页面重载。
- 内容区提供最小控制：后退、前进、刷新、在系统浏览器打开、关闭呈现。
- “关闭”只关闭当前 WebView，不删除注册、不清除会话数据。
- 弹窗、OAuth、外部协议和下载必须沿用现有 Web trust-domain 安全策略；不可为了兼容网站放宽 Tauri IPC/Bridge。

### FR-04 macOS App 注册（P0）

- 从真实已安装 App 列表中搜索并选择，保存 `applicationPath + bundleIdentifier`。
- 已注册同一 Bundle ID 时阻止重复保存，并引导用户打开现有记录。
- 应用被移动或升级后，优先用 Bundle ID 重定位；找不到时显示“未找到”并允许重新选择。
- 注册不触发启动，不提前请求 Accessibility 权限。

### FR-05 macOS App 打开与一次归位（P0）

首次或未运行时：

1. 发出启动请求并立即显示 `launching`。
2. 等待系统观察到运行实例；超时则显示可重试错误。
3. 激活/取消隐藏应用。
4. 选择前台的标准主窗口；P0 不处理第二窗口、浮层、偏好设置或弹窗。
5. 计算 Natives 当前屏幕上的内容区矩形，保留 8px 安全间隙并避开侧边栏。
6. 有 Accessibility 权限且窗口支持 position/size 时，一次性移动与缩放。
7. 成功后记录 `docked`；失败按真实原因记录，并保留启动/激活能力。

已运行时：激活/取消隐藏 → 如从未归位则尝试归位；已归位则不反复设置窗口位置。用户点击“重新归位”时才再次执行第 4–7 步。

补充规则：

- 目标显示器以 Natives 主窗口所在显示器为准。
- Natives 主窗口后续移动/缩放只将状态更新为 `needs_redock`，不得拖着外部 App 持续移动。
- 用户手工移动外部窗口后，Natives 不抢回控制权。
- 全屏窗口、非标准窗口、不可缩放窗口、其他 Space 与 Stage Manager 特殊布局在 P0 允许返回明确 unsupported，不做隐式破坏。

### FR-06 macOS App 控制动作（P0）

| 动作 | 前置条件 | 结果 |
|---|---|---|
| 观察 | 已注册 | 从 NSWorkspace/NSRunningApplication 获取真实状态；失败为 Unknown。 |
| 激活 | 已安装 | 未运行则启动，已隐藏则取消隐藏，然后激活。 |
| 隐藏 | 正在运行 | 请求系统隐藏；失败保留原状态并提示。 |
| 正常终止 | 正在运行 | 强确认后请求正常退出；超时显示“终止未完成”。 |
| 重新归位 | 正在运行且有主窗口 | 再执行一次移动/缩放；不持久跟随。 |

- 不提供 Force Quit P0。
- 终止前必须说明应用可能有未保存内容；删除注册与终止应用是两个独立动作。
- 切换离开 macOS App 时，默认只隐藏“由 Natives 当前切换流程激活的上一应用”；绝不自动终止。

### FR-07 单一当前目标切换（P0）

- Shell 维护唯一 `activeAppId` 和 `activeTargetKind`，不把 app item 当成一次性命令。
- Web → Web：隐藏旧 WebView，显示新 WebView。
- Web → macOS：隐藏旧 WebView，启动/激活目标 macOS App，按规则归位。
- macOS → Web：隐藏由 Natives 管理的上一 macOS App，激活 Natives，显示 WebView。
- macOS → macOS：隐藏上一受管 App，激活目标 App；不得终止上一 App。
- 点击“应用中心”返回控制页；当前 WebView 隐藏，当前受管 macOS App 隐藏，但其进程保持运行。
- 任一步失败时，侧边栏选中态不得假装切换成功；保留可恢复的上一个目标或进入明确错误态。

### FR-08 权限与隐私（P0）

- 首次执行“归位”时才检查并请求 Accessibility 权限。
- 权限说明必须明确：仅用于读取主窗口并设置位置/大小，不读取键盘输入、不录屏。
- 拒绝授权后仍可注册、观察进程、启动、激活、隐藏和正常终止；“归位”显示不可用。
- 提供“打开系统设置”和“重新检测”动作。
- Web Surface 保持远程 trust domain：无任意 Tauri command、文件系统、Shell、Secret 或 Host IPC 权限。

### FR-09 旧 Local Project 迁移（P0）

- 新 UI、侧边栏和新增入口不再展示 `local_project`。
- 不得直接删除用户已有 Registry、运行记录或本地目录。
- 技术方案必须先统计现有 `local_project` 数据，再选择：一次性只读迁移提示，或在确认无用户数据后删除旧类型。
- 迁移完成后清理生产入口与调用；禁止长期保留隐藏的双生产路径。
- 若处置改变既有 Apps 目标或放宽 MUST，先补 ADR/Standards，再实现。

## 8. 关键交互流程

### 8.1 添加 Web 应用

```text
应用中心 → 添加 Web 应用 → 输入 URL
→ 前端规范化与校验
→ 保存 Registry
├─ 成功：进入新应用详情，可选择“打开”
└─ 失败：保留输入，字段/表单内显示原因与重试

打开
→ 创建或复用 WebView
├─ ready：内容区显示，侧边栏选中
├─ error：注册保留，显示重试/编辑 URL/外部打开
└─ unsupported：解释兼容性边界，允许外部打开
```

### 8.2 首次打开 macOS App

```text
点击侧边栏应用
→ 已安装？
├─ 否：未找到 + 重新选择应用
└─ 是：启动/激活
   → 找到标准主窗口？
   ├─ 否：应用已激活 + 归位不可用
   └─ 是：有 Accessibility 权限？
      ├─ 否：解释权限；用户授权或继续仅激活
      └─ 是：一次性移动/缩放到内容区
         ├─ 成功：docked
         └─ 失败：真实错误 + 重试/仅激活
```

## 9. 验收标准

### AC-01 应用中心

- Given Registry 有 Web 和 macOS App，When 打开应用中心，Then 首屏为真实列表，类型和状态正确，loading/empty/error 分离。
- Given 用户增删改应用或切换侧边栏显示，When mutation 成功，Then 列表、详情和侧边栏在同一事件后保持一致。
- Given 删除 macOS App 注册，When 用户确认，Then 只删除 Natives 记录，不卸载应用、不结束进程。

### AC-02 Web 注册

- Given 输入 `chatgpt.com`，When 保存，Then 系统按 `https://chatgpt.com` 校验并持久化，返回可重新查询的 `appId`。
- Given 合法 URL 保存成功但页面无法加载，When 返回应用中心，Then 注册仍存在且显示“打开失败”，不是“保存失败”。
- Given 非 http(s) URL，When 保存，Then 不调用后端 mutation，并在字段下显示原因。
- Given 后端保存错误，When 失败，Then 表单保留用户输入并显示可读错误，不只记录 console。

### AC-03 Web 切换

- Given 两个已登录 Web 应用，When 连续从侧边栏切换，Then 内容区只显示当前 WebView，两个应用的会话相互隔离且登录态保留。
- Given 当前 WebView 被关闭，When 再次点击，Then 可重新创建并恢复持久会话。
- Given 活跃 WebView 超过既有预算，When LRU 触发，Then 非 keep-alive 应用进入可见的 hibernated 状态，硬上限不超过 10。

### AC-04 macOS App 控制

- Given App 未运行，When 点击侧边栏项，Then 状态经历 launching → running/active，不以固定延时伪造成功。
- Given App 已隐藏，When 点击，Then App 被取消隐藏并激活。
- Given 用户选择隐藏，When 系统成功，Then 状态更新为 hidden，进程仍运行。
- Given 用户确认正常终止，When App 接受退出，Then 状态最终为 stopped；拒绝或超时则显示失败，不显示 stopped。

### AC-05 一次归位

- Given 有权限且 App 主窗口支持移动/缩放，When 首次点击，Then 主窗口被放到 Natives 当前屏幕内容区内，且不遮挡侧边栏。
- Given 用户随后移动 Natives 或外部 App 窗口，When 操作结束，Then 外部窗口不被自动拉回，并出现可用的“重新归位”。
- Given 无 Accessibility 权限，When 点击 App，Then App 仍可启动/激活，归位显示 permission_required，并提供设置入口。
- Given App 窗口不可调整或处于全屏，When 尝试归位，Then 显示 unsupported，禁止伪报成功或持续重试。

### AC-06 安全与回归

- Web 远程页面无法调用未授权 Tauri/Host IPC、读取本地文件或 Secret。
- 权限拒绝、App 退出、WebView 崩溃、Natives 重启后无孤儿 WebView、无无限轮询、无状态假绿。
- `src/i18n/zh.ts` / `src/i18n/en.ts` 对新增用户文案保持同步。
- 实现完成必须通过项目规定的 typecheck、lint、test、perf 和触及区域的 Rust/protocol 检查。

## 10. 性能与体验指标

| 指标 | P0 目标 | 口径 |
|---|---|---|
| Web 注册成功率 | ≥ 99%（合法 URL、本地持久化可用时） | 保存成功 / 合法提交 |
| 暖态 Web 切换可见反馈 | p95 ≤ 300ms | 点击到目标 WebView 可见或 loading 明确出现 |
| macOS 激活反馈 | p95 ≤ 500ms | 点击到 launching/active 反馈，不等同应用完全启动 |
| WebView 数量 | soft 6 / hard 10 | 复用既有预算 |
| 状态正确性 | 0 个 error-as-empty / unknown-as-stopped | 自动化 + 手工异常验收 |

指标是方案与验收基线；如现有基准不同，实施前记录同设备基线并按性能规范给出前后证据，不得只凭体感宣称优化。

## 11. Agent 执行约束

后续方案设计 Agent 必须：

1. 先读 `docs/README.md`、相关 Standards、ADR-0020 和既有 Apps 架构文档。
2. 从现有 `AppRepository`、`AppsService`、System/Web driver、BrowserProfile、Surface store、Sidebar 投影和 6/10 WebView 预算继续演进，不新建第二套 Apps 系统。
3. 先画清 `active target` 状态机、Web Surface 生命周期和 macOS docking capability，再拆实现任务。
4. 把“Web 注册失败”的真实根因复现并定位到共享校验/持久化链路；不得只在表单吞错或增加假成功提示。
5. 把外部 macOS Window 与 Natives-owned Surface 分开建模；禁止用同一状态假装跨进程嵌入。
6. 将 Local Project 处置作为迁移切片，先审计数据，再移除入口；不得删除用户目录。
7. 所有能力从后端真实 capability/state 派生，前端不得按 AppKind 猜测成功。
8. 方案必须列出权限拒绝、无主窗口、不可缩放、全屏、其他 Space、多显示器、App 自行退出、Web 登录/OAuth、WebView 崩溃的异常路径。

## 12. 方案设计前的决策门

以下不是重新向用户追问，而是后续技术方案必须用 Spike 或源码证据收口的工程决策：

| ID | 待证实项 | 默认产品决策 | 通过条件 |
|---|---|---|---|
| G-01 | 主窗口识别是否覆盖目标应用 | 优先 focused/standard main window，只管一个 | 钉钉、微信、ChatGPT 三个样本均能稳定识别；失败有 typed unsupported |
| G-02 | Natives 内容区屏幕坐标换算 | 以主窗口当前显示器和真实 content rect 为准 | Retina、多显示器、侧栏展开/折叠下无明显偏移 |
| G-03 | Natives 与外部 App 的窗口层级切换 | 采用“激活目标 App，窗口不覆盖侧栏”的视觉组合 | 侧栏再次点击可可靠切换，允许短暂系统级焦点变化但不能卡死 |
| G-04 | WebKit 对目标站点兼容性 | 支持即内嵌，不支持则 honest fallback | ChatGPT、钉钉 Web 等目标站点完成登录、OAuth、弹窗 smoke matrix |
| G-05 | Local Project 既有数据规模 | 默认隐藏新增入口、保留数据待迁移 | 输出数据审计、迁移与 rollback/death proof |

任一 Gate 失败，只能缩小 capability 或明确 unsupported；不得用假 iframe、录屏或持续窗口强控绕过。

## 13. 追溯矩阵

| 用户诉求 | 对应需求 | 验收 |
|---|---|---|
| Web URL 无法创建保存 | FR-02 | AC-02 |
| 应用中心首先显示应用列表与状态 | FR-01 | AC-01 |
| 提供基础 CRUD | FR-01、FR-04 | AC-01、AC-02 |
| 侧边栏显示选定应用并点击切换 | FR-07 | AC-03、AC-04 |
| Web 在 Natives 内直接使用 | FR-03 | AC-03、AC-06 |
| macOS App 不额外做假页面 | 2.3、FR-05 | AC-05 |
| 首次将外部 App 窗口吸附一次 | FR-05 | AC-05 |
| 后续观察、激活、隐藏、终止 | FR-06 | AC-04 |
| 只保留 Web 与 App 两类 | D-01、FR-09 | AC-01、AC-06 |

## 14. 平台依据

- [Apple NSRunningApplication](https://developer.apple.com/documentation/appkit/nsrunningapplication)：观察、激活、隐藏/取消隐藏、正常终止应用实例。
- [Apple NSWorkspace](https://developer.apple.com/documentation/appkit/nsworkspace)：发现、启动应用并接收运行状态通知。
- [Apple AXIsProcessTrustedWithOptions](https://developer.apple.com/documentation/applicationservices/1459186-axisprocesstrustedwithoptions)：检查并请求辅助功能信任。
- [Apple AXUIElementSetAttributeValue](https://developer.apple.com/documentation/applicationservices/1460434-axuielementsetattributevalue)：设置窗口 position/size，并返回 unsupported/cannot-complete 等真实错误。
- [Tauri WebviewWindow API](https://v2.tauri.app/reference/javascript/api/namespacewebviewwindow/)：唯一 label 的 child WebView、位置/尺寸、show/hide 等能力。
