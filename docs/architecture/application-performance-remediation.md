# Natives 应用性能整改记录

## 基线

基线来自 2026-07-24 的 Release Next 构建检查：Dashboard 初始入口约 580KB gzip，个人创意约 465KB，文件页约 489KB；TypeScript 通过，生产构建被 6 个 ESLint 错误阻断。完整基准设备、数据集和预算见 `docs/standards/technical/04-performance.md`。

## 已确认热点

- `src-tauri/src/commands/disk.rs` 的系统指标命令每次创建全量 `System` 并同步休眠 200ms。
- `src-tauri/src/commands/thumbnail.rs` 同步执行缩略图生成；前端图片卡片挂载即请求，缓存清理会重复扫描目录。
- `src/components/ui/MathCurveLoader.tsx` 每个实例运行永久 rAF、120 段曲线和粒子计算。
- 助理事件状态逐事件复制数组和序列集合，长任务会持续扩大 Renderer 工作量。
- 会话和消息读取无界；文件内容块、运行事件和文件目录的 UI 也缺少统一窗口限制。
- `useCreativeAppCatalog` 的 5 秒 reconcile 与后端本地看门狗重复。

## 整改状态（2026-07-24）

- [x] 规范、Agent 入口和 Bundle 门禁（门禁脚本/CI 已加入；预算仍会阻断超标入口）
- [~] 主线程阻塞、缩略图、Shell 和加载动画（系统采样、缩略图、渲染副作用、CSS loader 已完成；拖拽 rAF 尚未完成）
- [~] 助理事件限额、批处理、计时器和分页（事件窗口、计时器、分页 API/UI、迁移索引已完成；单状态 batch 仍待后续优化）
- [x] Bundle、文件浏览、个人创意和 Dashboard（懒加载、可见缩略图、文件分批、reconcile 降频已完成）
- [~] Release 人工性能证据（生产构建、Bundle、Daemon Release 和自动回归已采集；Instruments/真实 30 分钟人工会话仍需在验收机执行）

当前实测：`/page` 183.2KB、`/modules/page` 215.2KB、`/files/page` 199.1KB gzip，全部通过 350KB 门禁。

## Release 证据

验收命令（Apple Silicon/macOS 工作区，2026-07-24）：

- `npm run typecheck`：通过。
- `npm run lint`：0 errors；保留既有 warnings。
- `npm run build`：Next production build 通过。
- `npm run perf:bundle`：`/page 183.2KB`、`/modules/page 215.2KB`、`/files 199.1KB`，均为 `ok`。
- `cargo build --release -p natives-agent-daemon`：通过。
- Daemon conversation pagination 单测：通过。
- Daemon adapter、workspace reducer 测试：25 项通过；10,000 事件窗口保持 2,000 条上限。

Release 包已成功生成：

- `target/release/bundle/macos/Natives.app`
- `target/release/bundle/dmg/Natives_0.1.0_aarch64.dmg`

真实运行采样（从上述 `.app` 启动）：

- 冷启动到主进程出现：约 98ms（单次样本，进程出现时间，不等同于 UI 可交互时间）。
- 启动后约 16 秒：主窗口 RSS 86.9MB / CPU 0.1%；Daemon RSS 8.2MB / CPU 0.0%。
- 启动后约 66 秒空闲：主窗口 RSS 42.2MB / CPU 0.4%；Daemon RSS 8.2MB / CPU 0.0%。

仍需在最终验收机上使用 Web Inspector/Instruments 补录冷启动 p75、交互 p95、FPS 和 30 分钟导航内存曲线；这些运行时指标不能由单次进程采样代替。

## 约束

默认不新增性能框架或虚拟列表依赖；优先使用现有 React、Rust 标准库、CSS 和浏览器观察器。完整历史仍以 SQLite 为权威，Renderer 只保留交互所需窗口。
