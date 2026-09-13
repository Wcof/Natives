//! ManagedApp 内置应用接口（单 Runtime 整改 P1/P2，ADR-0029 决策 3）。
//!
//! Core 与 builtin 子应用（如 fund-core）共享的唯一 trait 定义。
//! 子应用实现本 trait 并在 Core 的 ManagedAppRegistry 中注册；
//! Core 负责 存在/已安装/启用/权限/会话/方法/载荷/审计 检查链，
//! 子应用只收到通过检查的 `handle` 调用（R-SUBAPP-NATIVE-01：
//! builtin 子应用不携带独立 executable，不 spawn 独立进程）。

use serde_json::Value;

/// 单次 app invoke 载荷上限（契约 §4.0 wire frame 预算）。
pub const MAX_INVOKE_PAYLOAD_BYTES: usize = 512 * 1024;

/// builtin 子应用业务上下文。数据路径由 Core 提供
/// （`~/.natives/apps/<appId>/data/`），子应用禁止自行推测 HOME/cwd。
pub struct AppContext<'a> {
    pub app_id: &'a str,
    pub app_data_dir: std::path::PathBuf,
}

pub struct AppHealth {
    pub ok: bool,
    pub detail: String,
}

pub struct DataStatus {
    pub current_schema: u32,
    pub migration_state: String,
    pub has_committed_new_writes: bool,
}

/// builtin 托管子应用实现（P1-1）。方法名沿用 `<domain>.<action>` 风格，
/// 例如 `ledger.create`。
pub trait ManagedApp: Send + Sync {
    fn app_id(&self) -> &'static str;

    fn runtime_api_version(&self) -> u32 {
        1
    }

    /// 每次 invoke 都要求已授予的权限；Core 逐项核对 app_permissions。
    fn required_permissions(&self) -> &[&str] {
        &[]
    }

    fn health(&self, ctx: &AppContext) -> Result<AppHealth, String>;

    fn handle(&self, ctx: &AppContext, method: &str, params: &Value) -> Result<Value, String>;

    fn inspect_data(&self, ctx: &AppContext) -> Result<DataStatus, String>;
}
