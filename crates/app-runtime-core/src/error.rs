//! 内置模块错误码（managed-app-contract v2 的单一来源）。
//! 扩展页面与 App Host 共用此清单；未知 code 一律映射为 APP_START_FAILED 展示。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppErrorCode {
    UnsupportedPlatform,
    Incompatible,
    SignatureInvalid,
    PackageInvalid,
    InstallationChanged,
    Busy,
    AlreadyRunning,
    RunningElsewhere,
    RuntimeLimit,
    StartFailed,
    ProtocolMismatch,
    SessionInvalid,
    MigrationFailed,
    DataSchemaIncompatible,
    KeychainLocked,
    ModuleNotFound,
    AlreadyBound,
    AlreadyStarted,
    NotInitialized,
    Stopping,
    Stopped,
    InvalidState,
}

impl AppErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedPlatform => "APP_UNSUPPORTED_PLATFORM",
            Self::Incompatible => "APP_INCOMPATIBLE",
            Self::SignatureInvalid => "APP_SIGNATURE_INVALID",
            Self::PackageInvalid => "APP_PACKAGE_INVALID",
            Self::InstallationChanged => "APP_INSTALLATION_CHANGED",
            Self::Busy => "APP_BUSY",
            Self::AlreadyRunning => "APP_ALREADY_RUNNING",
            Self::RunningElsewhere => "APP_RUNNING_ELSEWHERE",
            Self::RuntimeLimit => "APP_RUNTIME_LIMIT",
            Self::StartFailed => "APP_START_FAILED",
            Self::ProtocolMismatch => "APP_PROTOCOL_MISMATCH",
            Self::SessionInvalid => "APP_SESSION_INVALID",
            Self::MigrationFailed => "APP_MIGRATION_FAILED",
            Self::DataSchemaIncompatible => "APP_DATA_SCHEMA_INCOMPATIBLE",
            Self::KeychainLocked => "APP_KEYCHAIN_LOCKED",
            Self::ModuleNotFound => "APP_MODULE_NOT_FOUND",
            Self::AlreadyBound => "APP_RUNTIME_ALREADY_BOUND",
            Self::AlreadyStarted => "APP_RUNTIME_ALREADY_STARTED",
            Self::NotInitialized => "APP_RUNTIME_NOT_INITIALIZED",
            Self::Stopping => "APP_RUNTIME_STOPPING",
            Self::Stopped => "APP_RUNTIME_STOPPED",
            Self::InvalidState => "APP_RUNTIME_INVALID_STATE",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        // 嵌套 const 项不能捕获 Self，显式使用枚举路径。
        const ALL: &[AppErrorCode] = &[
            AppErrorCode::UnsupportedPlatform,
            AppErrorCode::Incompatible,
            AppErrorCode::SignatureInvalid,
            AppErrorCode::PackageInvalid,
            AppErrorCode::InstallationChanged,
            AppErrorCode::Busy,
            AppErrorCode::AlreadyRunning,
            AppErrorCode::RunningElsewhere,
            AppErrorCode::RuntimeLimit,
            AppErrorCode::StartFailed,
            AppErrorCode::ProtocolMismatch,
            AppErrorCode::SessionInvalid,
            AppErrorCode::MigrationFailed,
            AppErrorCode::DataSchemaIncompatible,
            AppErrorCode::KeychainLocked,
            AppErrorCode::ModuleNotFound,
            AppErrorCode::AlreadyBound,
            AppErrorCode::AlreadyStarted,
            AppErrorCode::NotInitialized,
            AppErrorCode::Stopping,
            AppErrorCode::Stopped,
            AppErrorCode::InvalidState,
        ];
        ALL.iter()
            .copied()
            .find(|candidate| candidate.as_str() == code)
    }
}

/// 协议错误结构：{code, message, retryable}。业务错误不输出堆栈/路径/Secret。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AppErrorBody {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl AppErrorBody {
    pub fn new(code: AppErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code: code.as_str().to_string(),
            message: message.into(),
            retryable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_round_trip() {
        for code in [
            AppErrorCode::UnsupportedPlatform,
            AppErrorCode::SessionInvalid,
            AppErrorCode::KeychainLocked,
            AppErrorCode::RunningElsewhere,
        ] {
            assert_eq!(AppErrorCode::from_code(code.as_str()), Some(code));
        }
        assert_eq!(AppErrorCode::from_code("APP_NOPE"), None);
    }
}
