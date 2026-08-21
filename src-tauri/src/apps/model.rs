//! Apps 目标边界（ADR-0020 §4 / 05-MODULE-REMEDIATION-PLAN §3）
//!
//! 收敛后的 Apps 域模型：App / RuntimeSpec / RuntimeInstance / Surface，
//! 运行（RuntimeInstance）与呈现（Surface）分离。
//!
//! - `App`            —— 应用定义（`applications` 表 read-through）
//! - `RuntimeSpec`    —— 怎么启动（已验证的 `LaunchPlan`，即 `startup_plans.plan_json`）
//! - `RuntimeInstance`—— 这一次正在运行的实例（`runtime_instances` 表）
//! - `Surface`        —— 怎么呈现（`application_surfaces` 表，复用成熟类型）
//!
//! 复用 > 新建：App / RuntimeSpec / Surface 直接复用 `creative_app` 成熟类型；
//! 仅 `RuntimeInstance` 补一个表行结构（此前无统一实体）。

use serde::{Deserialize, Serialize};

/// 运行与呈现分离：Surface 关闭不等同于 Runtime 停止（05 §3 关键边界）。
pub use crate::creative_app::model::LaunchPlan as RuntimeSpec;
pub use crate::creative_app::model_runtime::ApplicationSurface as Surface;

/// 应用定义 —— `applications` 表行（read-through，不做第二套 CRUD）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct App {
    pub id: String,
    pub source: String,
    pub source_id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub version: String,
    pub created_at: String,
    pub updated_at: String,
}

/// 运行实例 —— `runtime_instances` 表行（App 的一次运行态）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInstance {
    pub id: String,
    pub application_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleanup_status: Option<String>,
    pub owner_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pgid: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_port: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
