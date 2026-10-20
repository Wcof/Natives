//! Model Host 查询错误类型。

use std::fmt;

#[derive(Debug, Clone)]
pub enum ModelQueryError {
    Unavailable(String),
    Status(u16, String),
    Parse(String),
}

impl fmt::Display for ModelQueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(msg) => write!(f, "Model Host 8766 服务不可达: {msg}"),
            Self::Status(code, msg) => write!(f, "Model Host 8766 响应异常 ({code}): {msg}"),
            Self::Parse(msg) => write!(f, "Model Host 8766 响应反序列化失败: {msg}"),
        }
    }
}

impl std::error::Error for ModelQueryError {}
