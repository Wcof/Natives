//! AI Resources 目标模块（ADR-0020 §4）。
//!
//! 资产复用现有 Provider 域成熟实现；本模块只提供目标类型边界与只读
//! read-through facade（Provider / Connection / Credential / Model），
//! 不复制 CRUD / OAuth / 密钥管理逻辑（复用 > 新建）。

pub mod facade;
pub mod model;

pub use model::{Connection, Credential, Model, Provider};
