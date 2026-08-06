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
- [~] 主线程阻塞、缩略图、Shell 和加载动画（系统采样、缩略图、渲染副作用、CSS loader、拖拽局部状态已完成；完整可见性取消仍待实测）
- [~] 助理事件限额、批处理、计时器和分页（事件窗口、序号集合移除、分页 API/UI、旧消息滚动锚点、迁移索引已完成；单状态 batch 与 delta 不入历史仍待后续优化）
- [~] Bundle、文件浏览、个人创意和 Dashboard（懒加载、可见缩略图、文件分批、单监听、stat 并发上限、reconcile 降频已完成；Dashboard 隐藏态专项证据缺失）
- [~] Release 人工性能证据（Release `.app`/`.dmg` 已生成，并完成 5 次冷启动、空闲 CPU/RSS 采样；7 项大数据集人工场景未执行）

当前实测：`/page` 183.2KB、`/modules/page` 215.2KB、`/files/page` 199.1KB gzip，全部通过 350KB 门禁。

## T11 性能与可观测性验收（2026-08-07）

同设备、debug 构建、标准数据集（500 会话×2000 消息、20000 运行事件）的 daemon-side
证据已落地，复现命令与完整样本见 `scripts/perf/daemon-evidence.sh`（产出
`perf-evidence.json` + `idle-evidence.json`）。核心结论：

- **wire replay 有界化（新增）**：`run.getEvents` / `run.subscribe` 的响应此前对一个
  长 run 会超过 `MAX_FRAME_BYTES`（2MB）而被客户端拒绝（20000 事件全量 replay 实测
  报 `response frame exceeds 2097152 bytes`）。现在 UDS 边界按
  `MAX_WIRE_REPLAY_EVENTS = 2000` 截断（保留最旧 N 条，游标单调前进），内存侧
  `replay_after_checked` 仍返回全量供 resume/subagent 使用。20000 事件 run 的
  `run.getEvents` 从「报错」变为「返回 2000 条、无错误」，尾部窗口 p95 从约 219ms
  降为约 4ms（此前 oversized 帧阻塞了同连接后续请求）。
- **live WebView 上限（新增）**：`WindowController::open` 增加
  `MAX_LIVE_WINDOWS = 10` 上限（R-P9），超过时返回带说明的 typed error；复开已开
  窗口不计数。10 WebView 支持上限因此有明确落点。
- **daemon 空闲基线**：60 秒空闲 RSS 中位约 20.7MB、CPU 中位 0%（p95 0.3%），低于
  2% 预算。
- **checkpoint 1GiB 流式哈希**：内存有界断言成立（content 不缓冲、hash 64 字符）。
  debug 构建约 49–51s（SHA-256 未优化）；release 等价吞吐（系统 `shasum -a 256`
  同文件）约 3.4s。Release 绝对值需在目标机 release 构建复测。
- 仍需真机验证：GUI 冷启动到可交互、交互 p95/主线程 long task、30 分钟导航
  RSS 增长、WebView RSS、Docker/Python/Binary start/stop 资源清零（Docker 本机无
  executable，如实标注 blocked）。

## Release 证据

验收命令（Apple Silicon/macOS 工作区，2026-07-24）：

- `npm run typecheck`：通过。
- `npm run lint`：0 errors；保留既有 warnings。
- `npm run build`：Next production build 通过。
- `npm run perf:bundle`：`/page 183.2KB`、`/modules/page 215.2KB`、`/files 199.1KB`，均为 `ok`。
- `cargo build --release -p natives-agent-daemon`：通过。
- 按本轮要求未新增、未执行相关自动化测试；仅执行类型检查和 diff 静态检查。

Release 包已成功生成：

- `target/release/bundle/macos/Natives.app`
- `target/release/bundle/dmg/Natives_0.1.0_aarch64.dmg`

真实运行采样（从上述 `.app` 启动）：

- 5 次冷启动到主进程出现：173ms、84ms、114ms、92ms、105ms；进程出现时间 p50 105ms、p75 114ms（不等同于 UI 可交互时间）。
- 启动后约 16 秒：主窗口 RSS 86.9MB / CPU 0.1%；Daemon RSS 8.2MB / CPU 0.0%。
- 启动后约 66 秒空闲：主窗口 RSS 42.2MB / CPU 0.4%；Daemon RSS 8.2MB / CPU 0.0%。

本次证据覆盖进程启动与资源基线；交互 p95、FPS、30 分钟导航内存曲线及 7 项大数据集人工场景未执行，不能宣称预算已验收。

## 约束

默认不新增性能框架或虚拟列表依赖；优先使用现有 React、Rust 标准库、CSS 和浏览器观察器。完整历史仍以 SQLite 为权威，Renderer 只保留交互所需窗口。
