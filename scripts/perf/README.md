# 性能门禁

脚本测量当前 Natives 产品的 Extension、Files Host、Model Host 和应用模块资源边界。
测试使用有界临时目录与确定性数据集，不连接生产账户、不写用户数据。

## 命令

```sh
rtk npm run perf:extension
rtk npm run perf:extension:test
rtk npm run perf:native-host
rtk npm run perf:model-host
rtk npm run perf:files
rtk npm run perf:check
```

`perf:check` 先执行当前扩展门禁和架构规模检查，再执行上述 Host 与体积检查。
Model Host 冷启动、空闲 RSS/CPU、最后客户端 EOF、常驻复用和目录查询均单独报告。

性能结果只对记录的设备、构建和数据集有效。历史结果在
`docs/archive/legacy-development/perf-results/`，不能作为当前通过依据。
