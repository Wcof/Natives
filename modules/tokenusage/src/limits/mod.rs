//! limits: AI 工具与模型提供方额度（Limits）监控。
//! 严格遵循“零假数据”原则：未配置或离线时明确标注状态，不填充假百分比或假零值。

use crate::storage::Store;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LimitWindow {
    pub provider_id: String,
    pub account_id: String,
    pub window_kind: String, // 'session', 'daily', 'weekly', 'billing', 'credits'
    pub label: String,
    pub used_percent: Option<f64>,
    pub remaining_percent: Option<f64>,
    pub used_units: Option<f64>,
    pub total_units: Option<f64>,
    pub unit_type: String, // 'percent', 'tokens', 'usd', 'requests'
    pub resets_at: Option<String>,
    pub fetched_at: String,
    pub status: String, // 'ok', 'offline', 'not_configured', 'error'
}

pub const KNOWN_LIMIT_PROVIDERS: &[(&str, &str)] = &[
    ("claude", "Claude Code"),
    ("codex", "Codex"),
    ("cursor", "Cursor"),
    ("openrouter", "OpenRouter"),
    ("deepseek", "DeepSeek"),
    ("minimax", "Minimax"),
    ("volcengine", "Volcengine"),
    ("alibaba", "Alibaba Cloud"),
];

pub fn resolve_model_host_state_path() -> Option<std::path::PathBuf> {
    if let Ok(override_dir) = std::env::var("NATIVES_MODEL_HOST_CONFIG_DIR") {
        if override_dir == "none" || override_dir.is_empty() {
            return None;
        }
        let p = std::path::PathBuf::from(override_dir).join("state.json");
        if p.exists() {
            return Some(p);
        }
        return None;
    }
    // 测试运行环境（cargo test 注入环境特征）默认隔离本机实际配置，保证测试独立性
    if std::env::var_os("RUST_TEST_THREADS").is_some()
        || std::env::var_os("CARGO_TARGET_TMPDIR").is_some()
    {
        return None;
    }

    if let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) {
        let p1 = home.join("Library/Application Support/Natives/model-host/state.json");
        if p1.exists() {
            return Some(p1);
        }
        let p2 = home.join(".config/natives/model-host/state.json");
        if p2.exists() {
            return Some(p2);
        }
    }
    None
}

pub fn refresh_limits(store: &Store) -> Result<usize, String> {
    let now = chrono_now();
    let mut updated = 0;

    store.with_write(|conn| {
        let mut stmt = conn
            .prepare(
                "INSERT INTO limits_cache (
                provider_id, account_id, window_kind, label, used_percent, remaining_percent,
                used_units, total_units, unit_type, resets_at, fetched_at, status
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ON CONFLICT(provider_id, account_id, window_kind) DO UPDATE SET
                label = excluded.label,
                used_percent = excluded.used_percent,
                remaining_percent = excluded.remaining_percent,
                used_units = excluded.used_units,
                total_units = excluded.total_units,
                unit_type = excluded.unit_type,
                resets_at = excluded.resets_at,
                fetched_at = excluded.fetched_at,
                status = excluded.status",
            )
            .map_err(|e| e.to_string())?;

        let mut configured_providers = std::collections::HashSet::new();

        // 1. 接入路由核心模块 proxy (model-host / CLIProxyAPI)
        if let Some(state_path) = resolve_model_host_state_path() {
            if let Ok(bytes) = std::fs::read(&state_path) {
                if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    // 1.1 读取 proxy 认证账号
                    if let Some(accounts) = val.get("accounts").and_then(|v| v.as_array()) {
                        for acc in accounts {
                            let id = acc.get("id").and_then(|v| v.as_str()).unwrap_or("default");
                            let provider = acc
                                .get("provider")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            let label = acc
                                .get("label")
                                .and_then(|v| v.as_str())
                                .unwrap_or(provider);
                            let enabled =
                                acc.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
                            let status = if enabled { "ok" } else { "disabled" };
                            // 零假数据：启停状态是真实信息，额度未知时不写占位百分比，
                            // 真实窗口余量由上游请求或 host 同步写入
                            let rem_pct: Option<f64> = None;
                            let used_pct: Option<f64> = None;

                            configured_providers.insert(provider.to_string());

                            stmt.execute(rusqlite::params![
                                provider,
                                id,
                                "session",
                                format!("{label} ({provider})"),
                                used_pct,
                                rem_pct,
                                None::<f64>,
                                None::<f64>,
                                "percent",
                                None::<String>,
                                now,
                                status
                            ])
                            .map_err(|e| e.to_string())?;
                            updated += 1;
                        }
                    }

                    // 1.2 读取 proxy 网关运行状态
                    if let Some(gateway) = val.get("gateway") {
                        let state_str = gateway
                            .get("state")
                            .and_then(|v| v.as_str())
                            .unwrap_or("stopped");
                        let is_running = state_str == "running";
                        let status = if is_running { "ok" } else { "offline" };
                        // 零假数据：网关运行状态真实，剩余额度未知时不写占位百分比
                        let rem_pct: Option<f64> = None;
                        let port = gateway
                            .get("port")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(52567);

                        configured_providers.insert("local-proxy".to_string());

                        stmt.execute(rusqlite::params![
                            "local-proxy",
                            "gateway",
                            "status",
                            format!("Local Gateway (:{port})"),
                            None::<f64>,
                            rem_pct,
                            None::<f64>,
                            None::<f64>,
                            "percent",
                            None::<String>,
                            now,
                            status
                        ])
                        .map_err(|e| e.to_string())?;
                        updated += 1;
                    }
                }
            }
        }

        // 2. 探测未在 proxy 中配置的已知提供商，如实标注 'not_configured'
        for &(provider_id, label) in KNOWN_LIMIT_PROVIDERS {
            if !configured_providers.contains(provider_id) {
                let status = "not_configured";
                let used_pct: Option<f64> = None;
                let rem_pct: Option<f64> = None;
                let resets: Option<String> = None;

                stmt.execute(rusqlite::params![
                    provider_id,
                    "default",
                    "session",
                    label,
                    used_pct,
                    rem_pct,
                    None::<f64>,
                    None::<f64>,
                    "percent",
                    resets,
                    now,
                    status
                ])
                .map_err(|e| e.to_string())?;

                updated += 1;
            }
        }

        Ok(())
    })?;

    Ok(updated)
}

pub fn record_limit_window(store: &Store, window: &LimitWindow) -> Result<(), String> {
    store.with_write(|conn| {
        conn.execute(
            "INSERT INTO limits_cache (
                provider_id, account_id, window_kind, label, used_percent, remaining_percent,
                used_units, total_units, unit_type, resets_at, fetched_at, status
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ON CONFLICT(provider_id, account_id, window_kind) DO UPDATE SET
                label = excluded.label,
                used_percent = excluded.used_percent,
                remaining_percent = excluded.remaining_percent,
                used_units = excluded.used_units,
                total_units = excluded.total_units,
                unit_type = excluded.unit_type,
                resets_at = excluded.resets_at,
                fetched_at = excluded.fetched_at,
                status = excluded.status",
            rusqlite::params![
                window.provider_id,
                window.account_id,
                window.window_kind,
                window.label,
                window.used_percent,
                window.remaining_percent,
                window.used_units,
                window.total_units,
                window.unit_type,
                window.resets_at,
                window.fetched_at,
                window.status
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}

fn chrono_now() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = dur / 86400;
    let secs_of_day = dur % 86400;
    let (y, m, d) = days_to_ymd(days);
    let h = secs_of_day / 3600;
    let min = (secs_of_day % 3600) / 60;
    let s = secs_of_day % 60;
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, h, min, s)
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let yr = if m <= 2 { y + 1 } else { y };
    (yr, m, d)
}
