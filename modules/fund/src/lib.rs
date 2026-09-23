//! Fund 官方内置应用模块库（appId=fund，ADR-0031）。
//!
//! 内部模块划分：portfolio（持仓）、ledger（交易流水）、
//! nav（净值）、import（导入）、storage（私有数据访问）、migration（schema 迁移）。
//! fixed 是跨模块的定点精度基建（实施方案 §7.2）。

pub mod api;
pub mod app;
pub mod fixed;
pub mod import;
pub mod ledger;
pub mod market;
pub mod migration;
pub mod nav;
pub mod portfolio;
pub mod storage;
pub mod ui;

pub use app::FundModule;
