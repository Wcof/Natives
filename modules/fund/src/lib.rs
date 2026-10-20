//! Fund 官方内置应用模块库（appId=fund，ADR-0031）。
//!
//! 内部模块划分：portfolio（持仓）、ledger（交易流水）、
//! nav（净值）、import（导入）、storage（私有数据访问）、migration（schema 迁移）。
//! fixed 是跨模块的定点精度基建（实施方案 §7.2）。

mod accounts;
mod analysis_output;
pub mod api;
pub mod app;
pub mod assets;
pub mod cards;
pub mod channel_protocols;
pub mod channels;
mod chart_drawings;
mod config_receipts;
mod config_templates;
pub mod corrections;
pub mod decisions;
mod evidence;
pub mod finance;
pub mod fixed;
pub mod goals;
pub mod group_plan;
mod group_projection;
pub mod home;
mod home_period;
mod home_urgent;
pub mod import;
pub mod information;
mod information_decisions;
pub mod ledger;
mod legacy_assets;
pub mod market;
pub mod migration;
mod migration_late;
pub mod nav;
mod network;
mod notification_send;
mod notification_subjects;
mod notification_tasks;
pub mod offline;
mod offline_settlement;
mod opening_ops;
mod opportunities;
mod opportunity_discovery;
mod opportunity_history_source;
mod opportunity_observations;
mod opportunity_records;
mod opportunity_refresh;
pub mod otc_holding;
pub mod otc_ops;
pub mod otc_plans;
mod overview;
pub mod planning;
pub mod portfolio;
mod portfolio_attribution;
mod portfolio_benchmark;
mod report_feedback;
pub mod returns;
pub mod rules;
mod rules_eval;
mod securities_income;
pub mod settings;
pub mod simulate;
pub mod sources;
mod sql_diag;
pub mod storage;
mod strategies;
mod surface_tasks;
mod template_donor;
pub mod trends;
pub mod ui;
pub mod valuation;
mod watchlists;

pub use app::FundModule;

mod trade_cash;
mod transfers;

#[cfg(test)]
#[path = "n04_valuation_tests.rs"]
mod n04_valuation_tests;
mod wire;
