# 技术 07 · 官方内置应用开发规范

> 版本：2.0.0 · 日期：2026-10-04
> 依据：ADR-0031、ADR-0039、`docs/standards/technical/06-built-in-modules.md`、`docs/contracts/managed-app-contract.md`

本规范是 Natives 官方内置应用（Built-in App）的开发与接入标准。新增官方应用模块必须严格遵守本规范。

---

## 1. 目录结构规范

每个内置应用源码位于主仓库的 `modules/<appId>/` 目录下。下图表示职责，
不强制现有领域文件搬成固定目录；平铺的领域文件与明确子目录均可，依赖与
公开接口按[技术 08](08-module-boundaries-and-contracts.md)核验，不创建空壳凑目录：

```text
modules/<appId>/
├── Cargo.toml          # Rust Library crate（严禁包含 [[bin]] 或生成独立 host）
├── module.json         # 模块静态元数据描述清单
├── src/                # 业务源码
│   ├── lib.rs          # 导出 BuiltInAppModule 实现与工厂函数
│   ├── api/            # Runtime HTTP／Native 业务适配
│   ├── domain/         # 领域规则／用例；纯计算与有明确 owner 的副作用分开
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
    "zh_CN": "投资",
    "en": "Invest"
  },
  "description": {
    "zh_CN": "投资应用",
    "en": "Investment application"
  },
  "entryRoute": "app.html?app=fund",
  "moduleApiVersion": 1,
  "dataSchemaVersion": 2,
  "capabilityVersion": 1,
  "schemaVersion": 2,
  "views": ["home"],
  "permissions": ["local-data"],
  "capabilities": ["local-data"],
  "requires": []
}
```

> 示例只说明字段形态，不声明 Fund 的当前版本／全部能力。实际值以模块
> `module.json`、编译描述、registry 与受验签产品清单的一致性检查为准；不能
> 复制示例版本绕过真实迁移或权限。能力／views 沿 ADR-0037／0038 校验。

---

## 3. 模块 Rust 契约接口

内置应用在 Rust 侧必须作为 Library 实现统一契约。当前接口定义位于
`crates/app-runtime-core/src/module.rs`，生命周期与请求服务分离；以下为相同
签名的接口节选，`ModuleService::card_snapshot` 保留源码中的默认不支持实现：

```rust
use app_runtime_core::http::HttpRequest;
use app_runtime_core::module::{AppHealth, ModuleContext, ModuleDescriptor, ModuleHttpResponse, RequestContext};
use app_runtime_core::protocol::{CardSnapshotResult, DataStatusResult};
use std::sync::Arc;

pub trait ModuleService: Send + Sync {
    fn handle_ui(&self, path: &str) -> Result<Option<ModuleHttpResponse>, String>;
    fn handle_api(
        &self,
        req: &HttpRequest,
        route: &str,
    ) -> Result<ModuleHttpResponse, (u16, String)>;
    fn handle_authenticated_api(
        &self,
        _context: &RequestContext,
        _req: &HttpRequest,
        _route: &str,
    ) -> Result<ModuleHttpResponse, (u16, String)> {
        Err((403, "APP_REQUEST_CONTEXT_REQUIRED".into()))
    }
    fn release_surface(&self, _surface_id: &str) {}
    fn protected_task_count(&self) -> usize {
        0
    }
    fn card_snapshot(&self) -> Result<CardSnapshotResult, (u16, String)> {
        Err((501, "APP_CARD_SNAPSHOT_UNSUPPORTED".into()))
    }
}

pub trait BuiltInAppModule: Send + Sync {
    fn descriptor(&self) -> ModuleDescriptor;
    fn initialize(&mut self, context: &ModuleContext) -> Result<(), String>;
    fn start(&mut self) -> Result<Arc<dyn ModuleService>, String>;
    fn data_status(&self) -> Result<DataStatusResult, String>;
    fn health(&self) -> Result<AppHealth, String>;
    fn shutdown(&mut self) -> Result<(), String>;
}
```

这里列接口用于理解契约；生产实现导入 `app_runtime_core::module` 的现有 trait，
不能复制声明成第二套生产类型。ModuleService 与生命周期 trait／所有实现／
契约示例同时变更并检查。Runtime 提供 listener／鉴权后调用服务，模块不自行
创建另一服务；HTTP 不持生命周期 Mutex 执行业务。

ADR-0042 的生产 HTTP adapter 校验 bearer 后，构造 `RequestContext` 并调用
`handle_authenticated_api`；不能退回没有身份的 `handle_api`。Context 的
principal、surface_id、app_id、worker_epoch、generation 与 capabilities 来自
Runtime 受信会话，不能使用页面自报字段。任务所属会话在模块内部核对；
`release_surface` 只取消该会话的任务，`protected_task_count` 报告尚未结束的
任务，用于最后租约释放后的保护判断。


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
- `secrets`：受控模块 Keychain 能力，明文不进入 UI／响应；
- `cancellation`：当前取消信号；
- `limits`：当前 `RuntimeLimits`。

字段名和类型以当前源码定义与一致性检查为准；`limits` 不等于已实现任意堆
内存分配硬限制，RSS／增长仍需按性能规范实际测量。

---

## 4. 强制禁令（Red Lines）

官方内置应用必须遵守以下严格禁令：

1. **MUST 使用 Runtime 注入目录**：模块严禁调用 `std::env::home_dir()`、读取环境变量推导产品路径或访问其他模块目录；所有路径必须使用 `ModuleContext` 传入的根路径。
2. **MUST 物理隔离数据库**：模块业务表必须完全存储在自身的数据目录中（如 `data/<appId>.db`），严禁跨模块写数据库，严禁向 Core 数据库（`natives.db`）写入任何业务表。
3. **MUST 实现优雅关闭（shutdown）**：在收到产品关闭信号时，必须在 2 秒内停止监听、取消后台协程/线程、刷新并关闭 SQLite 连接，释放运行时锁。ADR-0042 下单个 Surface 关闭仅释放自身；最后 owner 释放且无受保护任务后，按 60 秒宽限期启动此关闭流程。
4. **MUST 支持 Cancellation**：长耗时操作（如导入解析、历史重放、网络同步）必须检查并响应 `ModuleContext.cancellation`。
5. **MUST 无常驻后台任务与全局 Daemon**：模块不自行保活、开机自启或建立守护进程；仅允许 ADR-0042 的产品 owner 聚合租约及有界宽限回收，未使用模块不留专属 worker。
6. **MUST 无独立可执行程序（No Binary）**：模块在编译期静态链接入 `natives-app-runtime`，严禁在 `Cargo.toml` 中配置 `[[bin]]` 产出独立可执行文件。
7. **MUST 无独立 Native Messaging Manifest**：模块不向操作系统注册独立的 `com.natives.app.<appId>`。ADR-0042 的页面通过 Core Relay 取得共享 Runtime 会话；`com.natives.app_runtime` 是产品现有协议兼容入口，不允许页面另起同 appId 的 writer。
8. **MUST 无独立 Release / 独立版本**：模块版本完全跟随 Natives 产品主版本，无独立发布流程。
9. **MUST 无动态代码加载**：严禁使用 `dlopen`、`.dylib`、WASM 运行时、eval 或下载远程脚本/DSL。
10. **MUST 无任意 Shell 执行**：模块不得暴露或调用系统 Shell 执行任意命令。
11. **MUST 无任意 Files Host 能力**：模块不得作为通用文件管理器代理，不得绕过授权读写任意系统文件。

---

## 5. UI 与 Design Token 规范

1. **统一 Design Token**：模块 UI 必须遵循 Natives 全局样式规范（Typography、Spacing、Border-radius、Colors、Z-index），禁止自带完全不同的设计体系。
2. **外观与主题同步**：模块 iframe 加载时接收父级页面同步的 `appearance`、`language`、`density`，通过 `data-theme` 属性应用样式，禁止模块独立持久化一套相冲突的外观偏好。
3. **安全沙箱环境**：模块 UI 运行在受限 sandbox iframe（`allow-scripts allow-forms`）中，无 `allow-same-origin`；业务 API 使用带 Bearer Token 的 127.0.0.1 动态端口 loopback。ADR-0037／0038 的推理、外链和 Secret 配置只经受校验的 Shell 能力代理，不新增任意通信或暴露凭据。

## 6. 开发与扩展入口

新应用接入顺序：appId／数据 owner → Library 与 UI 源码 → 现行 trait → Cargo
静态依赖／registry → 元数据／能力／views／受验签清单 → 首次初始化、权限、
失败、迁移、关闭和完整候选验收。不在通用 Shell／Runtime 加具体业务分支。

应用内部新增职责沿包内业务接口，不增 appId／卡片／数据库 authority。
公共状态、动作装配、SDK 适配和边界检查遵守技术 08；UI 同时遵守前端 01～03。
