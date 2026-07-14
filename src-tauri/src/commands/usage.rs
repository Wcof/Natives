// ── Tauri Command: usage_refresh ──
// Entry point for Usage Dashboard IPC. Delegates to the usage module.

use crate::usage::{
    self, build_dashboard_response, validate_request, cache_key,
    UsageCache, UsageDashboardRequest, UsageDashboardResponse,
};
use crate::{Error, Result};
use serde_json::Value as JsonValue;
use tauri::State;

use crate::AppState;

/// Refresh usage data for the given range.
/// If `force` is false and a cached response exists (TTL: 30s), returns cached.
#[tauri::command]
pub async fn usage_refresh(
    state: State<'_, AppState>,
    request: UsageDashboardRequest,
) -> Result<UsageDashboardResponse> {
    // Validate request
    validate_request(&request).map_err(|e| Error::InvalidInput(e.to_string()))?;

    let key = cache_key(request.start_ms, request.end_ms, request.include_comparison, &request.time_zone);

    // Check response cache (only if not forced)
    if !request.force {
        if let Some(cached) = state.usage_cache.get(&key) {
            return Ok(cached);
        }
    }

    // Build fresh response
    let response = build_dashboard_response(&request).await;

    // Cache the response
    state.usage_cache.set(&key, response.clone());

    Ok(response)
}

/// Expose UsageCache to AppState.
impl UsageCache {
    /// Initialize a new UsageCache.
    pub fn new_arc() -> std::sync::Arc<UsageCache> {
        std::sync::Arc::new(UsageCache::new())
    }
}
