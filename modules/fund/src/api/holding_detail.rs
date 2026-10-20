//! Holding detail and evidence-bound information decision routes.
use super::{api_error, nav_api, positions_history};
use crate::storage::Store;
use app_runtime_core::http::HttpRequest;

pub(super) fn handle(
    store: &Store,
    request: &HttpRequest,
    route: &str,
) -> Result<String, (u16, String)> {
    if route == "/api/fund/classification" && request.method == "GET" {
        let code = super::query_param(&request.path, "code")
            .filter(|s| s.len() == 6 && s.bytes().all(|b| b.is_ascii_digit()))
            .ok_or_else(|| api_error(400, "NAV_BAD_PAYLOAD", "基金代码无效"))?;
        return crate::nav::classification::fetch(&code)
            .map_err(|e| api_error(502, e.code(), &e.message()));
    }
    if route == "/api/fund/nav-history" && request.method == "GET" {
        return nav_api::api_nav_detail_history(&request.path);
    }
    if route == "/api/information/decisions" {
        let error = |e: crate::rules::RuleError| {
            if matches!(&e, crate::rules::RuleError::Db(_)) {
                return api_error(500, "FIN_DB", "资讯决定读写失败");
            }
            api_error(
                match e {
                    crate::rules::RuleError::Conflict(_) => 409,
                    crate::rules::RuleError::NotFound(_) => 404,
                    crate::rules::RuleError::Db(_) => 500,
                    _ => 400,
                },
                "FIN_INVALID",
                &e.message(),
            )
        };
        return match request.method.as_str() {
            "GET" => crate::information_decisions::list(store)
                .map(|v| v.to_string())
                .map_err(error),
            "POST" => {
                let input =
                    serde_json::from_slice::<crate::information_decisions::Input>(&request.body)
                        .map_err(|_| api_error(400, "FIN_INVALID", "资讯决定参数无效"))?;
                crate::information_decisions::save(store, &input)
                    .map(|id| serde_json::json!({"ok":true,"id":id}).to_string())
                    .map_err(error)
            }
            _ => Err(api_error(405, "APP_METHOD_NOT_ALLOWED", "请求方法不支持")),
        };
    }
    if route == "/api/holding/income-history" && request.method == "GET" {
        return positions_history::income_history(store, &request.path);
    }
    Err(api_error(
        405,
        "APP_METHOD_NOT_ALLOWED",
        "Unsupported request method",
    ))
}
