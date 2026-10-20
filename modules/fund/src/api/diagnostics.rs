//! Test-only SQL counter endpoint. Enabled only by NATIVES_SQL_TRACE=1.
use app_runtime_core::http::HttpRequest;

pub(super) fn handle(request: &HttpRequest) -> Result<String, (u16, String)> {
    if !crate::sql_diag::enabled() {
        return Err((404, "FIN_DIAGNOSTICS_DISABLED".into()));
    }
    if request.method == "POST" {
        crate::sql_diag::reset();
    }
    serde_json::to_string(&crate::sql_diag::snapshot())
        .map_err(|_| (500, "FIN_DIAGNOSTICS_UNAVAILABLE".into()))
}
