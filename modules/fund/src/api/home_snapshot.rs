//! Snapshot reads, opportunity generation and explicit derived valuation observation.
use super::home_api::err;
use crate::{finance::FinanceError, storage::Store};
use app_runtime_core::http::HttpRequest;

pub(crate) fn write_snapshot(
    store: &Store,
    request: &HttpRequest,
    route: &str,
) -> Result<String, (u16, String)> {
    if !request.body.is_empty()
        && !serde_json::from_slice::<serde_json::Value>(&request.body)
            .is_ok_and(|v| v.as_object().is_some_and(|o| o.is_empty()))
    {
        return Err(super::api_error(
            400,
            "FIN_INVALID",
            "估值请求不接受数据字段",
        ));
    }
    if route == "/api/home/valuation/observe" {
        api_home_observe(store, &request.path)
    } else {
        api_home_generate(store, &request.path)
    }
}

pub fn api_home_snapshot(store: &Store, path: &str) -> Result<String, (u16, String)> {
    home_snapshot(store, path, false, false)
}
pub(crate) fn api_home_generate(store: &Store, path: &str) -> Result<String, (u16, String)> {
    home_snapshot(store, path, true, true)
}
pub(crate) fn api_home_observe(store: &Store, path: &str) -> Result<String, (u16, String)> {
    home_snapshot(store, path, false, true)
}
fn home_snapshot(
    store: &Store,
    path: &str,
    generate: bool,
    observe: bool,
) -> Result<String, (u16, String)> {
    let range = crate::market::query_param(path, "range").unwrap_or_else(|| "all".into());
    let valid = matches!(
        range.as_str(),
        "today" | "month" | "year" | "all" | "custom"
    );
    if !valid {
        return Err(super::api_error(400, "FIN_INVALID", "收益区间无效"));
    }
    let from = crate::market::query_param(path, "from");
    let to = crate::market::query_param(path, "to");
    crate::home::checked_range(
        &range,
        from.as_deref(),
        to.as_deref(),
        &crate::finance::today_cn(),
    )
    .map_err(|e| err(400, &e))?;
    let refresh = crate::market::query_param(path, "refreshEstimates").as_deref() == Some("1");
    let _budget = crate::network::limit_duration(std::time::Duration::from_secs(20));
    if refresh {
        let today = crate::finance::today_cn();
        let rows = store
            .with_read(|conn| crate::assets::collect(conn, &today))
            .map_err(|e| super::api_error(500, e.code(), &e.message()))?;
        crate::nav::warm_estimates(&rows.funds, &today);
        if generate {
            crate::home::build_snapshot_range(store, &range, from.as_deref(), to.as_deref())
                .map_err(|e| err(400, &e))?;
        }
    }
    let mut snapshot = if generate {
        crate::opportunity_records::generate_home(store, &range, from.as_deref(), to.as_deref())
    } else if !refresh {
        crate::home::cached_snapshot_range(store, &range, from.as_deref(), to.as_deref())
    } else {
        crate::home::build_snapshot_range(store, &range, from.as_deref(), to.as_deref())
    }
    .map_err(|e| {
        err(
            match e {
                FinanceError::Conflict(_) => 409,
                FinanceError::Db(_) => 500,
                _ => 400,
            },
            &e,
        )
    })?;
    if observe {
        crate::valuation_history::observe(store, &mut snapshot).map_err(|e| {
            err(
                match e {
                    FinanceError::Conflict(_) => 409,
                    FinanceError::Db(_) => 500,
                    _ => 400,
                },
                &e,
            )
        })?;
    }
    Ok(snapshot.to_string())
}
