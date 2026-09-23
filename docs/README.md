# Natives 文档入口

> 版本：v4.0 · 2026-09-14

Natives 是单一用户产品：Chrome Extension 提供界面，Rust native-file-host 提供 Files 与产品配置，Go Model Host 提供 AI Provider/OAuth/Keychain/loopback，Rust natives-app-runtime 提供内置应用通用运行时，Fund 等功能作为 Monorepo 内置应用模块随同一个安装包交付。

## 当前架构

- 唯一界面：`extension/`；Service Worker 无状态，页面按需连接 Native Messaging。
- 文件与产品 Host：`crates/native-file-host`；文件领域逻辑：`crates/file-manager-core`。
- AI Host：`model-host`；不得扩展为通用 Agent、Jobs、Plugin Runtime 或第二数据库权威。
- 内置模块与统一运行时：所有官方内置应用归入 `modules/` 并编译进统一 `natives-app-runtime`，系统 Native Messaging 注册单一 `com.natives.app_runtime`。打开时按需启动独立 Runtime 进程实例，关闭时 2 秒内彻底退出回收全部资源；没有模块下载、独立可执行文件、独立 Native Host 注册、独立安装或独立 Release。Runtime loopback 默认绑定 `127.0.0.1:8765`（占用回退动态端口，实际端口经 `app:start` 上报），见 ADR-0032 与[端口注册表](standards/technical/05-port-registry.md)。
- 性能、内存回收、安全、数据和可访问性规则继续有效，分别见 `standards/technical/04-performance.md`、`02-security.md`、`03-data.md`、`06-built-in-modules.md`、`07-built-in-app-development.md` 和 `ui-ux/`。

## 阅读顺序

1. [Standards](standards/README.md)
2. 适用 [ADR](adr/)
3. [Contracts](contracts/)
4. 当前架构文档和领域实现
5. [Legacy archive](archive/README.md) 只用于追溯

## 任务速查

| 任务 | 先读 |
|---|---|
| 单一产品、内置 Fund 和 App Center | [托管应用契约](contracts/managed-app-contract.md)、[内置模块标准](standards/technical/06-built-in-modules.md)、[应用中心实施方案](development/app-center-fund-implementation-plan.md) |
| 安装、引导、打包和发布 | [应用中心实施方案](development/app-center-fund-implementation-plan.md)、[当前架构](architecture/ARCHITECTURE.md) |
| Files | [Files 审计](architecture/FILE_MANAGER_AUDIT.md) |
| AI 用量、成本、会话和提醒 | [AI 效能总方案](development/ai-efficiency-components-plan.md)、[整改方案](development/ai-efficiency-remediation-implementation-plan.md)、ADR-0030 |
| 性能和内存回收 | [性能整改方案](architecture/application-performance-remediation.md)、[性能标准](standards/technical/04-performance.md) |
| Provider、OAuth、Secret | ADR-0020、[Security](standards/technical/02-security.md)、[Data](standards/technical/03-data.md) |
| 本地 loopback 端口申请与登记 | [端口注册表](standards/technical/05-port-registry.md)、ADR-0032 |
| 旧方案处置 | [Legacy 删除证明](architecture/legacy-death-list.md)、[历史归档](archive/README.md) |

## 变更纪律

活动目录中的文档必须描述当前代码和当前决策。废弃方案移入 `docs/archive/`；ADR 保留历史正文但必须有 supersedes 关系。放宽 MUST 或改变产品边界先更新 ADR，再同步 Standards、Contracts 和门禁。完成声明必须区分本地验证、发布验证和未验证项。
