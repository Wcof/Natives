# 当前性能规则

以 `docs/standards/technical/04-performance.md` 和
`docs/architecture/application-performance-remediation.md` 为唯一性能依据。

- 先测量，再优化；记录同一设备、构建和数据集的 before/after。
- 保持 Extension、Native Host 和内置模块的体积、RSS、CPU、p95、超时与 EOF 预算。
- 所有 Port、Watcher、监听器、计时器、Blob URL 和临时文件必须成对释放。
- 性能检查使用 `rtk npm run perf:check` 与 `rtk npm run perf:files`；失败时保留实际证据。
