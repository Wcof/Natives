//! Apps 目标域（ADR-0020 / 05-MODULE-REMEDIATION-PLAN §3）
//!
//! 收敛为 App / RuntimeSpec / RuntimeInstance / Surface 四元边界，运行与呈现分离。
//! 资产复用 `creative_app` 成熟实现；本模块只提供目标类型边界与只读 facade，
//! 不复制 CRUD / 进程 / 端口 / 健康探测逻辑（复用 > 新建）。

pub mod facade;
pub mod model;

pub use model::{App, RuntimeInstance, RuntimeSpec, Surface};
