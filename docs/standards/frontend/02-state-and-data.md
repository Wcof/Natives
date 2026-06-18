# 前端架构 02 · 状态、数据流与错误处理

> **版本**: 1.0.0 · **日期**: 2026-06-15
> **关联 ADR**: [ADR-0003](../../adr/0003-plugin-ipc-main-process-relay.md)（插件 IPC 中转）、[ADR-0004](../../adr/0004-terminal-env-injection-new-sessions-only.md)（注入仅对新会话）
> **关联源文件**: `src/components/shell/ShellLayout.tsx`、`src/lib/follow-mode.ts`、`electron/preload.ts`、`src/main/bridge-host.ts`

---

## 一、本篇要约束什么

前端状态管理是 Natives 最易出乱的地方：ShellLayout 持有大量状态，多个面板需要同步。本篇约束三件事：**状态怎么放**、**数据怎么取（IPC 模式）**、**错误怎么处理**。无假数据红线见 `product/02`，本篇聚焦前端落点。

---

## 二、状态归属

#### R-E7 · 全局 Shell 状态集中在 ShellLayout
- **等级**：SHOULD
- **分类**：状态
- **规则**：跨域的全局状态（侧栏宽度、面板模式、终端高度、当前视图、主题/locale）**应该**集中在 `ShellLayout`（或其拆出的自定义 Hook），通过 props 下发。**禁止**在多个组件各自用 `useState` 重复持有同一份全局状态。
- **为什么**：单一真相源避免「侧栏在 A 组件折叠了，B 组件还以为展开」的不一致。
- **检查方法**：新增全局可见状态时，确认它是否已在 ShellLayout 管理。

#### R-E8 · 局部状态就近，跨组件复用提为 Hook
- **等级**：SHOULD
- **分类**：状态
- **规则**：只在一个组件内用的状态**应该**就地 `useState`。当同一段状态逻辑在 2+ 组件复用时，**应该**提取到 `src/hooks/`（如现有的 `useFocusTrap`、`useFollowMode`、`use-file-drop`）。**禁止**为复用而把局部状态硬塞进全局。
- **为什么**：复用与隔离的平衡；过早全局化比重复更危险。
- **检查方法**：第二次写相同状态逻辑时，提取 Hook。

---

## 三、数据获取（IPC 模式）

#### R-E9 · Renderer 取数只走 `window.nativesAPI`
- **等级**：MUST
- **分类**：状态、安全
- **规则**：Renderer 获取任何 Main 侧数据**必须**经 preload 暴露的 `window.nativesAPI.{domain}.{action}()` → `ipcRenderer.invoke('domain:action')` → Main handler。**禁止**直接 `fetch` 本地 HTTP 服务（那是给插件的通道）、**禁止**绕过 preload 自造 IPC。
- **为什么**：preload 是 `contextIsolation` 下的唯一受控通道；绕过它等于破坏隔离。
- **检查方法**：Renderer 中无 `ipcRenderer` 直接 import（应由 preload 封装）；无对本地 HTTP 端口的 `fetch`。

#### R-E10 · 异步取数必须有加载/错误/成功三态
- **等级**：MUST
- **分类**：状态、无假数据
- **规则**：任何从 Main/IPC 取数的 UI **必须**处理三个状态：`loading`（加载态）、`error`（错误态，走 `classifyError`）、`success`（数据）。**禁止**假设取数永远成功、**禁止**在 loading 时显示假数据占位。
- **正例**：`if (loading) return <Spinner/>; if (error) return <ErrorState .../>; return <Data/>;`
- **反例**：`const data = await fetch(...); return <List items={data}/>` —— 没有处理 pending 与 rejected。
- **为什么**：IPC 会失败（Main 忙、DB 锁、模块缺失）；不处理就崩在用户脸上。
- **检查方法**：每个异步取数组件核对三态覆盖。

#### R-E11 · 监听广播更新状态，不轮询
- **等级**：SHOULD
- **分类**：状态、性能
- **规则**：需要实时性的数据（模块列表、通知、设置变更）**应该**监听 Main 的 `db-state-changed` 等广播 channel 主动同步，**禁止**用 `setInterval` 高频轮询 IPC。
- **为什么**：广播是单向总线（防线 4）的设计姿态；轮询浪费 CPU 与 IPC 带宽。
- **检查方法**：搜 `setInterval` 配合 IPC 调用的组合。

---

## 四、错误处理

#### R-E12 · 捕获的错误必须经 classifyError 后再展示
- **等级**：MUST
- **分类**：状态、无假数据
- **规则**：所有面向用户展示的错误**必须**经 `classifyError()`（见 `product/02` R-F5）。组件内 `try/catch` 捕获后**应该**用 `showErrorToast(err)` 或渲染 `ErrorState`，**禁止** `console.error` 后静默吞掉、**禁止**弹原始异常。
- **为什么**：统一错误体验 + 不泄露内部细节 + 不丢错误。
- **检查方法**：搜空 `catch` 与 `console.error` 后无用户反馈的路径。

#### R-E13 · 异步副作用要有清理
- **等级**：SHOULD
- **分类**：状态、性能
- **规则**：`useEffect` 中订阅的事件、广播、定时器**应该**在 cleanup 中取消，避免组件卸载后更新状态（报错 / 内存泄漏）。
- **为什么**：React 严格模式下未清理的副作用会告警并可能泄漏。
- **检查方法**：每个 `useEffect` 检查返回的 cleanup 函数。

---

## 五、本篇合规自检清单

- [ ] 我没有重复持有全局状态，全局状态在 ShellLayout（R-E7）。
- [ ] 我的取数走 `window.nativesAPI`，没有绕过 preload（R-E9）。
- [ ] 我的异步 UI 覆盖了 loading/error/success 三态（R-E10）。
- [ ] 实时数据用广播同步而非轮询（R-E11）。
- [ ] 我的错误经 `classifyError` 后展示，没有静默吞掉或弹原始异常（R-E12）。
- [ ] 我的 `useEffect` 都有 cleanup（R-E13）。
