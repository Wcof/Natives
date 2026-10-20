//! Types for the built-in module registry.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct App {
    pub app_id: String,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub show_in_sidebar: bool,
    pub sidebar_order: i64,
    pub updated_at: i64,
    pub revision: i64,
    pub product_generation: Option<u64>,
    pub capabilities: Vec<String>,
    pub requires: Vec<String>,
    pub views: Vec<String>,
}

#[derive(Debug)]
pub enum AppError {
    Busy,
    NotFound(String),
    InvalidState(String),
    PackageInvalid(String),
    Conflict(String),
    Sql(rusqlite::Error),
    Io(std::io::Error),
}

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Busy => "APP_BUSY",
            Self::NotFound(_) => "APP_NOT_FOUND",
            Self::InvalidState(_) => "APP_INVALID_STATE",
            Self::PackageInvalid(_) => "APP_PACKAGE_INVALID",
            Self::Conflict(_) => "APP_CONFLICT",
            Self::Sql(_) => "APP_INTERNAL",
            Self::Io(_) => "APP_IO",
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy => write!(f, "module is in use"),
            Self::NotFound(target) => write!(f, "app not found: {target}"),
            Self::InvalidState(message) => write!(f, "invalid app state: {message}"),
            Self::PackageInvalid(message) => write!(f, "invalid product content: {message}"),
            Self::Conflict(message) => write!(f, "conflicting app state: {message}"),
            Self::Sql(error) => write!(f, "sql error: {error}"),
            Self::Io(error) => write!(f, "io error: {error}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<rusqlite::Error> for AppError {
    fn from(error: rusqlite::Error) -> Self {
        match error {
            rusqlite::Error::QueryReturnedNoRows => Self::NotFound("row not found".into()),
            other => Self::Sql(other),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
