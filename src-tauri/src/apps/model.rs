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
//!
//! Phase B（APP-012 / APP-013，v28 schema 之上）新增强类型契约：
//! - `AppKind` / `RegistrationOrigin`：kind 与 origin 分离（APP-004），snake_case。
//! - `AppRuntimeState` / `AppCapabilities` / `AppView`：动作矩阵派生的视图 DTO。
//! - 类型专属注册/编辑输入 DTO（`Register*Input` / `Update*Input`）。
//!
//! 兼容说明（spec 05 / APP-006 禁止）：v28 不删除 `source`/`source_id`，进入兼容
//! 迁移期，kind 逐步成为权威。因此本轮 `App` / `RuntimeInstance` 保持现有读库字段
//! 不变（`facade` 读库契约不动），新的强类型身份经 `AppView` 视图承载，
//! `kind`/`registration_origin`/sidebar 与 runtime `ownership_mode`/`external_identity`
//! 已在 v28 落库（见 `db::migration_v28`），由后续 C 阶段 repository 投影进 `AppView`。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 运行与呈现分离：Surface 关闭不等同于 Runtime 停止（05 §3 关键边界）。
pub use crate::creative_app::model::LaunchPlan as RuntimeSpec;
pub use crate::creative_app::model_runtime::ApplicationSurface as Surface;

/// 应用类型 —— 固定三值（APP-004 / 02 §AppKind）。
///
/// 序列化命名遵循 03 规格：snake_case（`local_project` / `system_application` /
/// `web_application`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
#[serde(rename_all = "snake_case")]
pub enum AppKind {
    LocalProject,
    SystemApplication,
    WebApplication,
}

impl AppKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalProject => "local_project",
            Self::SystemApplication => "system_application",
            Self::WebApplication => "web_application",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "local_project" => Self::LocalProject,
            "system_application" => Self::SystemApplication,
            "web_application" => Self::WebApplication,
            _ => return None,
        })
    }
}

/// 注册来源 —— 固定六值（APP-004 / 02 §RegistrationOrigin）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
#[serde(rename_all = "snake_case")]
pub enum RegistrationOrigin {
    Manual,
    LocalScan,
    SystemDiscovery,
    LegacyInternal,
    LegacyGithub,
    Migration,
}

impl RegistrationOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::LocalScan => "local_scan",
            Self::SystemDiscovery => "system_discovery",
            Self::LegacyInternal => "legacy_internal",
            Self::LegacyGithub => "legacy_github",
            Self::Migration => "migration",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "manual" => Self::Manual,
            "local_scan" => Self::LocalScan,
            "system_discovery" => Self::SystemDiscovery,
            "legacy_internal" => Self::LegacyInternal,
            "legacy_github" => Self::LegacyGithub,
            "migration" => Self::Migration,
            _ => return None,
        })
    }
}

/// App 的运行态投影（由 runtime_instances.status 派生；Web 无 RuntimeInstance）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
#[serde(rename_all = "snake_case")]
pub enum AppRuntimeState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
    Orphaned,
    /// Web 专用：窗口已休眠（LRU/hibernation，v28 window_instances.hibernated_at）。
    Hibernated,
}

impl AppRuntimeState {
    /// 由 runtime_instances.status 派生；无活动实例时为 `Stopped`。
    pub fn from_instance_status(status: Option<&str>) -> Self {
        match status {
            Some("starting") => Self::Starting,
            Some("running") => Self::Running,
            Some("stopping") => Self::Stopping,
            Some("failed") => Self::Failed,
            Some("orphaned") => Self::Orphaned,
            _ => Self::Stopped,
        }
    }
}

/// 能力位（APP-003 冻结动作矩阵的后端投影）。
///
/// Web 永远 `can_stop=false` / `can_restart=false`；风险等级 0/1/2 由能力推导。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
#[serde(rename_all = "camelCase")]
pub struct AppCapabilities {
    pub can_start: bool,
    pub can_stop: bool,
    pub can_restart: bool,
    pub can_open: bool,
    pub can_edit: bool,
    pub can_remove: bool,
    pub can_sidebar: bool,
    /// 最高风险等级（0 = 无副作用，1 = 可回滚，2 = 高风险需确认）。
    pub risk_level: u8,
}

/// 应用中心列表/详情视图 DTO（APP-012 step 5 / APP-020 step 6）。
///
/// `kind` / `registration_origin` 为 snake_case 字符串（03 规格，权威身份）；
/// `capabilities` 与 `runtime_state` 是后端派生值，不依赖前端手写 source 猜测。
///
/// 序列化命名遵循 03 规格 wire 约定：根字段全量 camelCase
/// （`appId` / `showInSidebar` / `sidebarOrder` / `runtimeState` / `updatedAt`），
/// 枚举 wire value 保持 snake_case（`kind` / `registration_origin` 为 String 字段，
/// 值域由 `AppKind` / `RegistrationOrigin` 保证，serde 不重命名字符串内容）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
#[serde(rename_all = "camelCase")]
pub struct AppView {
    pub app_id: String,
    pub title: String,
    /// snake_case：`local_project` / `system_application` / `web_application`。
    pub kind: String,
    /// snake_case：`manual` / `local_scan` / `system_discovery` /
    /// `legacy_internal` / `legacy_github` / `migration`。
    pub registration_origin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
    pub show_in_sidebar: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub sidebar_order: Option<i64>,
    pub capabilities: AppCapabilities,
    pub runtime_state: AppRuntimeState,
    pub updated_at: String,
}

/// 应用定义 —— `applications` 表行（read-through，不做第二套 CRUD）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
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

/// 类型专属注册/编辑输入 DTO（APP-013）。
///
/// 后端先行校验路径/URL/枚举（Phase C 在 service 层落地）；这些输入不再暴露
/// 任意 `source`/`sourceId` create 作为新产品 API（APP-013 验收）。

/// 注册本地项目（kind=local_project，origin=manual / local_scan）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterLocalProjectInput {
    pub title: String,
    pub project_root: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
}

/// 注册系统应用（kind=system_application，origin=system_discovery / manual）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterSystemApplicationInput {
    pub title: String,
    /// `.app` 绝对路径（必填，spec 05 system_application_specs.application_path NOT NULL）。
    pub application_path: String,
    #[serde(default)]
    pub bundle_identifier: Option<String>,
    /// 平台：`macos` / `windows` / `linux`（NOT NULL）。
    pub platform: String,
    /// 启动策略（缺省 `activate_existing`）。
    #[serde(default)]
    pub launch_policy: Option<String>,
}

/// 注册 Web 应用（kind=web_application，origin=manual）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterWebApplicationInput {
    pub title: String,
    /// http/https URL（必填，spec 05 web_application_specs.url NOT NULL）。
    pub url: String,
    /// 信任域：允许的 origin 集合（remote 必填非空，Phase C 校验）。
    #[serde(default)]
    pub approved_origins: Vec<String>,
    /// 打开行为（缺省 `native_webview`）。
    #[serde(default)]
    pub open_behavior: Option<String>,
    /// keep-alive（缺省 false / 0）。
    #[serde(default)]
    pub keep_alive: bool,
}

/// 更新应用通用 metadata（APP-013 step 4）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAppMetadataInput {
    pub app_id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub show_in_sidebar: Option<bool>,
    #[serde(default)]
    pub sidebar_order: Option<i64>,
}

/// 本地项目运行配置编辑（APP-027 plan version 语义）：保存新 plan_version 并
/// 切 active；`commit` 区分 save-only 与 save-and-restart（后者 risk 1）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateLocalProjectInput {
    /// 统一 application id（service 侧 resolve，允许 source id 由命令层解析）。
    pub app_id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub project_root: Option<String>,
    #[serde(default)]
    pub launch_mode: Option<crate::creative_app::model::LaunchMode>,
    /// 提供时 = 编辑 launch plan（必须通过 validate_launch_plan，AI/用户计划
    /// 永不直接执行）。
    #[serde(default)]
    pub launch_plan: Option<crate::creative_app::model::LaunchPlan>,
    #[serde(default)]
    pub env: Option<Vec<crate::creative_app::model::EnvPair>>,
    #[serde(default)]
    pub env_upsert: Option<Vec<crate::creative_app::model::EnvPair>>,
    #[serde(default)]
    pub env_remove_keys: Option<Vec<String>>,
    /// "save"（缺省）= 只保存；"restart" = 保存并重启（risk 1）。
    #[serde(default)]
    pub commit: Option<String>,
}

/// 更新系统应用 spec（APP-013 step 4）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSystemApplicationSpecInput {
    pub app_id: String,
    #[serde(default)]
    pub application_path: Option<String>,
    #[serde(default)]
    pub bundle_identifier: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub launch_policy: Option<String>,
}

/// 更新 Web 应用 spec（APP-013 step 4）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWebApplicationSpecInput {
    pub app_id: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub approved_origins: Option<Vec<String>>,
    #[serde(default)]
    pub open_behavior: Option<String>,
    #[serde(default)]
    pub keep_alive: Option<bool>,
}

#[cfg(test)]
mod model_tests {
    //! APP-012 验收：serde roundtrip 覆盖三类 kind + origin 枚举 + 命名约定。

    use super::*;
    use serde_json::{from_str, to_string};

    #[test]
    fn app_kind_serde_roundtrip_is_snake_case() {
        for (variant, wire) in [
            (AppKind::LocalProject, "local_project"),
            (AppKind::SystemApplication, "system_application"),
            (AppKind::WebApplication, "web_application"),
        ] {
            assert_eq!(to_string(&variant).unwrap(), format!("\"{wire}\""));
            assert_eq!(
                from_str::<AppKind>(&format!("\"{wire}\"")).unwrap(),
                variant
            );
            assert_eq!(variant.as_str(), wire);
            assert_eq!(AppKind::parse(wire), Some(variant));
        }
        assert_eq!(AppKind::parse("nope"), None);
    }

    #[test]
    fn registration_origin_serde_roundtrip_is_snake_case() {
        for (variant, wire) in [
            (RegistrationOrigin::Manual, "manual"),
            (RegistrationOrigin::LocalScan, "local_scan"),
            (RegistrationOrigin::SystemDiscovery, "system_discovery"),
            (RegistrationOrigin::LegacyInternal, "legacy_internal"),
            (RegistrationOrigin::LegacyGithub, "legacy_github"),
            (RegistrationOrigin::Migration, "migration"),
        ] {
            assert_eq!(to_string(&variant).unwrap(), format!("\"{wire}\""));
            assert_eq!(
                from_str::<RegistrationOrigin>(&format!("\"{wire}\"")).unwrap(),
                variant
            );
            assert_eq!(RegistrationOrigin::parse(wire), Some(variant));
        }
    }

    #[test]
    fn app_view_serializes_kind_as_snake_case_and_root_fields_in_camel() {
        let view = AppView {
            app_id: "app-1".into(),
            title: "T".into(),
            kind: AppKind::WebApplication.as_str().into(),
            registration_origin: RegistrationOrigin::Migration.as_str().into(),
            description: None,
            show_in_sidebar: true,
            sidebar_order: Some(2),
            capabilities: AppCapabilities {
                can_start: false,
                can_stop: false,
                can_restart: false,
                can_open: true,
                can_edit: true,
                can_remove: true,
                can_sidebar: true,
                risk_level: 0,
            },
            runtime_state: AppRuntimeState::Hibernated,
            updated_at: "2026-08-23T00:00:00Z".into(),
        };
        let json = to_string(&view).unwrap();

        // APP-01 / 03 规格：AppView 根字段 wire 全量 camelCase。
        for key in [
            "appId",
            "title",
            "kind",
            "registrationOrigin",
            "showInSidebar",
            "sidebarOrder",
            "capabilities",
            "runtimeState",
            "updatedAt",
        ] {
            assert!(
                json.contains(&format!("\"{key}\"")),
                "root field must be camelCase `{key}`: {json}"
            );
        }
        // 禁止残留 snake_case 根字段（wire 契约漂移即失败）。
        for key in [
            "app_id",
            "registration_origin",
            "show_in_sidebar",
            "sidebar_order",
            "runtime_state",
            "updated_at",
        ] {
            assert!(
                !json.contains(&format!("\"{key}\"")),
                "snake_case root field {key} must not appear on wire: {json}"
            );
        }

        // 枚举 wire value 保持 snake_case（03 规格，字符串字段不受 rename_all 影响）。
        assert!(
            json.contains("\"kind\":\"web_application\""),
            "kind must be snake_case: {json}"
        );
        assert!(
            json.contains("\"registrationOrigin\":\"migration\""),
            "registration_origin must keep snake_case value: {json}"
        );
        // capability 字段是 camelCase（crate 约定）。
        assert!(
            json.contains("\"canStop\":false"),
            "capability must be camelCase: {json}"
        );
        // Web 永远 can_stop/can_restart=false（APP-003）。
        assert!(!view.capabilities.can_stop);
        assert!(!view.capabilities.can_restart);
        // roundtrip 稳定。
        let back: AppView = from_str(&json).unwrap();
        assert_eq!(back, view);
    }

    #[test]
    fn app_runtime_state_derives_from_instance_status() {
        assert_eq!(
            AppRuntimeState::from_instance_status(Some("running")),
            AppRuntimeState::Running
        );
        assert_eq!(
            AppRuntimeState::from_instance_status(Some("starting")),
            AppRuntimeState::Starting
        );
        assert_eq!(
            AppRuntimeState::from_instance_status(Some("orphaned")),
            AppRuntimeState::Orphaned
        );
        assert_eq!(
            AppRuntimeState::from_instance_status(Some("stopped")),
            AppRuntimeState::Stopped
        );
        assert_eq!(
            AppRuntimeState::from_instance_status(None),
            AppRuntimeState::Stopped
        );
    }

    #[test]
    fn register_inputs_are_typed_per_kind() {
        // local — 无 web/system 专属字段。
        let l: RegisterLocalProjectInput = from_str(r#"{"title":"X","projectRoot":"/p"}"#).unwrap();
        assert_eq!(l.project_root, "/p");
        assert!(l.icon.is_none());

        // system — application_path + platform 必填，launch_policy 缺省。
        let s: RegisterSystemApplicationInput =
            from_str(r#"{"title":"S","applicationPath":"/Applications/X.app","platform":"macos"}"#)
                .unwrap();
        assert_eq!(s.application_path, "/Applications/X.app");
        assert_eq!(s.platform, "macos");
        assert_eq!(s.launch_policy, None);

        // web — url 必填，keep_alive 缺省 false，approved_origins 可缺省空。
        let w: RegisterWebApplicationInput = from_str(
            r#"{"title":"W","url":"https://a.example.com","approvedOrigins":["a.example.com"]}"#,
        )
        .unwrap();
        assert_eq!(w.url, "https://a.example.com");
        assert!(!w.keep_alive);
        assert_eq!(w.approved_origins, vec!["a.example.com".to_string()]);

        // update DTOs 全部字段可选（部分更新）。
        let u: UpdateAppMetadataInput = from_str(r#"{"appId":"app-1"}"#).unwrap();
        assert_eq!(u.app_id, "app-1");
        assert!(u.title.is_none());
        let us: UpdateSystemApplicationSpecInput = from_str(r#"{"appId":"app-2"}"#).unwrap();
        assert!(us.launch_policy.is_none());
        let uw: UpdateWebApplicationSpecInput = from_str(r#"{"appId":"app-3"}"#).unwrap();
        assert!(uw.keep_alive.is_none());
    }
}
