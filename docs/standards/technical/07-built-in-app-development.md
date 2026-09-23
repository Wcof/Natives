# 技术 07 · 官方内置应用开发规范

> 版本：1.0.0 · 日期：2026-09-14
> 依据：ADR-0031、`docs/standards/technical/06-built-in-modules.md`、`docs/contracts/managed-app-contract.md`

本规范是 Natives 官方内置应用（Built-in App）的开发与接入标准。新增官方应用模块必须严格遵守本规范。

---

## 1. 目录结构规范

每个内置应用源码位于主仓库的 `modules/<appId>/` 目录下：

```text
modules/<appId>/
├── Cargo.toml          # Rust Library crate（严禁包含 [[bin]] 或生成独立 host）
├── module.json         # 模块静态元数据描述清单
├── src/                # 业务源码
│   ├── lib.rs          # 导出 BuiltInAppModule 实现与工厂函数
│   ├── api/            # HTTP 业务路由与 RPC 接口
│   ├── domain/         # 领域核心逻辑（纯业务，无外部 side-effect）
│   ├── storage/        # 模块专属 SQLite 存储抽象
│   └── migration/      # 幂等 schema 迁移与备份恢复逻辑
├── ui/                 # 前端界面源码
│   ├── src/            # 前端工程源码（遵守 Natives Design Tokens）
│   └── dist/           # 构建生成的静态资源（随产品包签名交付）
└── tests/              # 模块单元测试与集成测试
```

---

## 2. 静态元数据声明（module.json）

每个模块根目录必须包含 `module.json`，声明如下核心元数据：

```json
{
  "appId": "fund",
  "displayName": {
    "zh_CN": "基金",
    "en": "Fund"
  },
  "description": {
    "zh_CN": "本地基金持仓记账：账户、交易流水、净值与 CSV 导入。",
    "en": "Local fund portfolio bookkeeping: accounts, transactions, NAV and CSV import."
  },
  "entryRoute": "app.html?app=fund",
  "moduleApiVersion": 1,
  "dataSchemaVersion": 1,
  "capabilityVersion": 1,
  "permissions": ["local-data", "external-nav-source", "keychain"]
}
```

---

## 3. 模块 Rust 契约接口

内置应用在 Rust 侧必须作为 Library 实现统一的 `BuiltInAppModule` 契约：

```rust
pub trait BuiltInAppModule: Send + Sync {
    /// 模块描述符
    fn descriptor(&self) -> ModuleDescriptor;

    /// 使用 Runtime 注入的受控上下文完成初始化
    fn initialize(&mut self, context: ModuleContext) -> Result<(), ModuleError>;

    /// 启动模块（启动 loopback HTTP 路由并绑定业务 API）
    fn start(&mut self, router: &mut ModuleRouter) -> Result<(), ModuleError>;

    /// 数据状态查询（用于诊断、备份和数据统计）
    fn data_status(&self) -> Result<ModuleDataStatus, ModuleError>;

    /// 健康检查
    fn health(&self) -> Result<ModuleHealth, ModuleError>;

    /// 退出与彻底资源清理
    fn shutdown(&mut self) -> Result<(), ModuleError>;
}
```

### ModuleContext 注入要求

Runtime 向模块注入 `ModuleContext`，包括：
- `app_id`：模块稳定标识；
- `product_version`：Natives 产品版本；
- `module_data_root`：模块独占的数据目录（如 `~/.natives/apps/<appId>/data/`）；
- `imports_root`：导入暂存目录；
- `cache_root`：模块缓存目录；
- `logs_root`：模块日志目录；
- `keychain_namespace`：Keychain 命名空间（`com.natives.app.<appId>`）；
- `activation_generation`：当前激活代际；
- `cancellation_token`：全局取消信号；
- `runtime_limits`：内存与并发限额。

---

## 4. 强制禁令（Red Lines）

官方内置应用必须遵守以下严格禁令：

1. **MUST 使用 Runtime 注入目录**：模块严禁调用 `std::env::home_dir()`、读取环境变量推导产品路径或访问其他模块目录；所有路径必须使用 `ModuleContext` 传入的根路径。
2. **MUST 物理隔离数据库**：模块业务表必须完全存储在自身的数据目录中（如 `data/<appId>.db`），严禁跨模块写数据库，严禁向 Core 数据库（`natives.db`）写入任何业务表。
3. **MUST 实现优雅关闭（shutdown）**：在收到取消信号或 Native Messaging 关闭时，必须在 2 秒内停止监听、取消后台协程/线程、刷新并关闭 SQLite 连接，释放运行时锁。
4. **MUST 支持 Cancellation**：长耗时操作（如导入解析、历史重放、网络同步）必须检查并响应 `cancellation_token`。
5. **MUST 无常驻后台任务与全局 Daemon**：页面关闭后进程立即退出，禁止后台守护进程、系统开机自启或驻留 Timer。
6. **MUST 无独立可执行程序（No Binary）**：模块在编译期静态链接入 `natives-app-runtime`，严禁在 `Cargo.toml` 中配置 `[[bin]]` 产出独立可执行文件。
7. **MUST 无独立 Native Messaging Manifest**：模块不向操作系统注册独立的 `com.natives.app.<appId>`，统一使用 `com.natives.app_runtime`。
8. **MUST 无独立 Release / 独立版本**：模块版本完全跟随 Natives 产品主版本，无独立发布流程。
9. **MUST 无动态代码加载**：严禁使用 `dlopen`、`.dylib`、WASM 运行时、eval 或下载远程脚本/DSL。
10. **MUST 无任意 Shell 执行**：模块不得暴露或调用系统 Shell 执行任意命令。
11. **MUST 无任意 Files Host 能力**：模块不得作为通用文件管理器代理，不得绕过授权读写任意系统文件。

---

## 5. UI 与 Design Token 规范

1. **统一 Design Token**：模块 UI 必须遵循 Natives 全局样式规范（Typography、Spacing、Border-radius、Colors、Z-index），禁止自带完全不同的设计体系。
2. **外观与主题同步**：模块 iframe 加载时接收父级页面同步的 `appearance`、`language`、`density`，通过 `data-theme` 属性应用样式，禁止模块独立持久化一套相冲突的外观偏好。
3. **安全沙箱环境**：模块 UI 运行在受限 sandbox iframe（`allow-scripts allow-forms`）中，无 `allow-same-origin`，与外部通信仅通过带 Bearer Token 的 127.0.0.1 动态端口 loopback API。
