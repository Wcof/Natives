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
- [~] 助理事件限额、批处理、计时器和分页（事件窗口/计时器/迁移索引已完成；分页 API 与单状态 batch 尚未完成）
- [~] Bundle、文件浏览、个人创意和 Dashboard（懒加载、可见缩略图、文件分批、reconcile 降频已完成；预算仍超标）
- [ ] Release 人工性能证据

当前实测：`/page` 426.5KB、`/modules/page` 422.2KB、`/files/page` 444.5KB gzip；相比基线分别下降约 153.5KB、42.8KB、44.3KB。未达到 350KB 合入门禁，不能宣称整改完成。

## 约束

默认不新增性能框架或虚拟列表依赖；优先使用现有 React、Rust 标准库、CSS 和浏览器观察器。完整历史仍以 SQLite 为权威，Renderer 只保留交互所需窗口。
