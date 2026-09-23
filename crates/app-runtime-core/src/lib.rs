//! 官方托管应用运行支持库（ADR-0027 / 托管应用契约 v1）。
//!
//! 由标准样例与各官方应用共用：Native framing、origin 校验、运行锁、
//! 会话鉴权、App v1 协议类型与错误码。不依赖 native-file-host 二进制
//! 或 file-manager-core 文件能力；协议 schema/fixture 从本库单一来源导出。

pub mod activation;
pub mod cancellation;
pub mod error;
pub mod framing;
pub mod http;
pub mod layout;
pub mod limits;
pub mod lock;
pub mod module;
pub mod origin;
pub mod protocol;
pub mod session;

pub use activation::{verify_activation, verify_activation_with_generation, ActivationRecord};
pub use cancellation::CancellationToken;
pub use error::AppErrorCode;
pub use limits::RuntimeLimits;
pub use module::{
    AppHealth, BuiltInAppModule, ModuleContext, ModuleDescriptor, ModuleFactory,
    ModuleHttpResponse, ModuleRegistry,
};
pub use protocol::{
    APP_PROTOCOL_VERSION, APP_PROTOCOL_VERSION_V2, MAX_FRAME_BYTES, PRODUCT_PROTOCOL_VERSION,
    PRODUCT_PROTOCOL_VERSION_V5,
};
pub use session::SessionManager;

/// 计算字节序列的 SHA-256 小写十六进制摘要。
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}
