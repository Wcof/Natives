# Legacy 删除证明

> 状态：2026-09-14 已执行。本文是当前删除结果，不是未来架构草案。

已从生产代码和活动规范移除：旧桌面工作台、通用 Agent/Daemon/Jobs/Plugin Runtime、模块发现目录、分块传输与模块安装事务、Seed/独立 Release 分发及其 GitHub Actions 工作流、独立卸载/回滚 API、扩展单独 ZIP/current 产物、Windows Web Store/IExpress 安装链、macOS 外部扩展模板，以及重复的扩展运行时。

当前生产入口只有：

- `extension/`：Chrome Extension 页面、静态资源和 AI 用量界面；
- `crates/native-file-host`：Files Host、产品清单校验、内置模块注册和用户数据管理；
- `crates/file-manager-core`：文件领域逻辑；
- `model-host`：单用途 Provider/OAuth/Keychain/loopback Host；
- `crates/app-runtime` / `crates/app-runtime-core`：统一 `natives-app-runtime` 可执行文件（ADR-0031）与官方内置模块编译期注册；
- `modules/`：官方 Built-in App（如 `modules/fund`）源码与 UI。

旧文件已移动到 [`docs/archive/`](../archive/README.md)，不参与活动索引和门禁。历史 ADR 保留原文，依靠 supersedes/历史标记记录决策链；不得从历史 ADR 恢复已删除的生产路径。

验收：`npm run standards:check` 检查旧入口不存在、活动 Standards 无旧架构标识、核心 Host 协议无旧分发方法；Rust、Extension、性能和安装门禁在最终发布前继续执行。
