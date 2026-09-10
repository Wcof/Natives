//! 官方托管应用运行支持库（ADR-0027 / 托管应用契约 v1）。
//!
//! 由标准样例与各官方应用共用：Native framing、origin 校验、运行锁、
//! 会话鉴权、App v1 协议类型与错误码。不依赖 native-file-host 二进制
//! 或 file-manager-core 文件能力；协议 schema/fixture 从本库单一来源导出。

pub mod error;
pub mod lock;
pub mod protocol;
pub mod session;

pub use error::AppErrorCode;
pub use protocol::{
    APP_PROTOCOL_VERSION, CATALOG_VERSION_V3, CORE_APPS_PROTOCOL_V4, MAX_FRAME_BYTES,
};
pub use session::SessionManager;
