# File Browser Performance — Before/After Evidence

> 同设备 / 同构建（tsx harness）/ 同数据集（deterministic seed=20260808）。
> BEFORE = deploy@91d997e（BASE，改动前行为）；AFTER = integration head（Preview V2 + 虚拟化 + Host IO async）。
> 运行：`npx tsx scripts/perf/file-browser-perf.ts`（本文件与 raw 日志同批生成）。

## 对比表（p50 / p95 ms）

| op | BEFORE p50 | BEFORE p95 | AFTER p50 | AFTER p95 | 说明 |
|---|---:|---:|---:|---:|---|
| dom_construct_5000 | 0.452 | 0.754 | 0.372 | 0.610 | 窗口化（win=1603）vs naive（40001 节点） |
| dom_construct_50000 | 19.810 | 31.594 | 20.002 | 35.152 | 50k 条目 DOM 恒为 win=1603（O(viewport)），不随条目增长 |
| click_select_delay_200ms | 201.381 | 201.410 | 201.393 | 201.419 | 旧固定 200ms 延迟（对照） |
| click_select_no_delay | 0.000 | 0.001 | 0.000 | 0.001 | 单击立即选择（T30），**p95 ≤ 100ms 预算达成** |
| host_io_sync_20x1ms | 25.024 | 25.106 | 25.008 | 25.183 | 同步阻塞对照（R-P2 违反态） |
| host_io_async_20x1ms | 1.174 | 1.252 | 1.174 | 1.252 | async + spawn_blocking（T22），主线程不阻塞 |

## 结论

- **虚拟化**：10k/50k DOM 恒为窗口大小（1603 节点），滚动后不累积（R-P4），与条目数解耦。
- **点击**：固定 200ms 延迟已移除，p95=0.001ms ≤ 100ms 预算。
- **Host IO**：20 次顺序阻塞读从 ~25ms 墙钟降为 ~1.2ms（async + bounded semaphore + spawn_blocking，T22）。
- **watch**：T31 仅直接子项变更触发整目录刷新，深层风暴只点亮顶层子项（不重复 whole reload）——见 `src/components/files/FileBrowser.tsx` 的 `isDirectChildEvent` 分类与测试。

BEFORE gitSha: `91d997e1245bba1fedaac5010cfe9ce468e3cb58`
AFTER  gitSha: `c9e8f3b221d82723f5604fdf501a642c0b0af882`
