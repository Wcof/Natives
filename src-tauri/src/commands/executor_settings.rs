// 执行引擎设置遗留数据模型（MIG-002：写入口已退役）
//
// 只保留迁移读源：Execution Engine Settings V2（`execution_engine:settings:v2`）
// 是唯一持久化权威。旧 `executor:settings` 键不再可写 — `executor_get_settings` /
// `executor_save_settings` 已从 invoke_handler 物理注销，前端经
// `executionEngine.saveSettings` 写 V2。本文件仅保留迁移所需的类型与键常量。

use serde::{Deserialize, Serialize};

pub const EXECUTOR_KEY: &str = "executor:settings";

/// 持久化的执行引擎配置。前端 Settings ↔ Daemon-native run start 共用。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExecutorSettings {
    /// 工具名 → 是否启用。缺省键视为关闭。
    #[serde(default)]
    pub enabled_tools: std::collections::HashMap<String, bool>,
    /// 自愈上限（PRD 3.4：≤3 次自愈，第 4 次熔断）。
    #[serde(default = "default_max_self_heal")]
    pub max_self_heal: u32,
    /// 步数上限（Q17 S1：默认 50，可在设置页配置）。
    #[serde(default)]
    pub max_steps: Option<u32>,
}

fn default_max_self_heal() -> u32 {
    3
}
