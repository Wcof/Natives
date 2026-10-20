//! HTTP API 路由与业务请求处理（拆分自 main.rs 以满足 R-B9 与模块边界规范）。

mod account_ops;
mod assets_api;
pub mod corrections_api;
mod diagnostics;
mod drawings;
mod holding_detail;
pub mod home_api;
mod import_api;
pub mod information_api;
mod nav_api;
pub(crate) mod notify_api;
pub mod offline_api;
mod otc_api;
pub(crate) mod portfolio_benchmark_api;
mod positions;
mod positions_detail;
mod positions_history;
mod positions_income;
pub mod reports_api;
mod rules_api;
mod sources_api;
mod transfer_api;
mod watchlists;

use crate::storage::Store;
use app_runtime_core::http::HttpRequest;

pub(crate) use crate::assets::today_iso;
use assets_api::{
    api_allocation_get, api_allocation_save, api_asset_accounts, api_assets_list, api_create_asset,
    api_delete_asset, api_fund_by_code, api_goal_clear, api_goal_get, api_goal_save,
    api_set_fund_metadata, api_update_asset,
};
use drawings::{api_delete_drawing, api_drawings_list, api_save_drawing};
use home_api::{
    api_finance_cash, api_finance_event_create, api_finance_events, api_finance_reservation_create,
    api_finance_reservation_settle, api_finance_reservations, api_finance_reserve_save,
    api_finance_returns, api_finance_snapshot_save, api_goal_archive, api_goal_create,
    api_goal_restore, api_goal_set_primary, api_goal_update, api_goals_list, api_home_snapshot,
};
use import_api::{api_import_commit, api_import_preview};
use nav_api::{api_nav_list, api_nav_sync};
use positions::{
    api_accounts, api_confirm_transaction, api_create_account, api_history, api_positions,
    api_positions_detail, api_record_opening, api_record_transaction, api_transactions,
};
use rules_api::{
    api_check_run, api_decision_create, api_decisions_list, api_event_ack, api_event_resolve,
    api_event_snooze, api_events_list, api_rule_archive, api_rule_create, api_rule_set_enabled,
    api_rule_update, api_rules_list, api_simulate_buy,
};
use sources_api::{
    api_source_archive, api_source_create, api_source_flags, api_source_update, api_sources_list,
    api_theses_list, api_thesis_archive, api_thesis_create, api_thesis_restore,
};
use transfer_api::{api_transfer_commit, api_transfer_preview};
use watchlists::{
    api_add_watchlist_item, api_create_watchlist, api_manage_watchlist, api_remove_watchlist_item,
    api_watchlists,
};

pub fn handle_api_request(
    store: &Store,
    secrets: Option<&std::sync::Arc<app_runtime_core::secrets::SecretStore>>,
    request: &HttpRequest,
    route: &str,
) -> Result<String, (u16, String)> {
    if route == "/api/diagnostics/sql" {
        return diagnostics::handle(request);
    }
    if matches!(
        route,
        "/api/fund/nav-history"
            | "/api/fund/classification"
            | "/api/information/decisions"
            | "/api/holding/income-history"
    ) {
        return holding_detail::handle(store, request, route);
    }
    if let Some(result) = reports_api::feedback_request(store, request, route) {
        return result;
    }
    if matches!(
        route,
        "/api/opportunities/capture"
            | "/api/opportunities/feedback"
            | "/api/opportunities/snapshots"
            | "/api/opportunities/stats"
            | "/api/opportunities/feedback/history"
            | "/api/opportunities/observations/refresh"
            | "/api/strategies"
            | "/api/strategies/update"
    ) {
        let result = match (request.method.as_str(), route) {
            ("POST", "/api/opportunities/capture" | "/api/opportunities/feedback") => {
                let value: serde_json::Value = serde_json::from_slice(&request.body)
                    .map_err(|_| api_error(400, "FIN_INVALID", "JSON 无效"))?;
                if route.ends_with("capture") {
                    crate::opportunity_records::capture(store, &value)
                } else {
                    crate::opportunity_records::feedback(store, &value)
                }
            }
            ("POST", "/api/opportunities/observations/refresh") => {
                let value: serde_json::Value = serde_json::from_slice(&request.body)
                    .map_err(|_| api_error(400, "FIN_INVALID", "JSON 无效"))?;
                crate::opportunity_observations::refresh(store, &value)
            }
            ("GET", "/api/opportunities/snapshots") => {
                let page = crate::market::query_param(&request.path, "page")
                    .unwrap_or_else(|| "1".into())
                    .parse::<i64>()
                    .map_err(|_| api_error(400, "FIN_INVALID", "分页无效"))?;
                crate::opportunity_records::snapshots(store, page)
            }
            ("GET", "/api/opportunities/feedback/history") => {
                let sid =
                    crate::market::query_param(&request.path, "snapshotId").unwrap_or_default();
                let cid =
                    crate::market::query_param(&request.path, "candidateId").unwrap_or_default();
                let page = crate::market::query_param(&request.path, "page")
                    .unwrap_or_else(|| "1".into())
                    .parse::<i64>()
                    .map_err(|_| api_error(400, "FIN_INVALID", "分页无效"))?;
                crate::opportunity_records::feedback_history(store, &sid, &cid, page)
            }
            ("GET", "/api/opportunities/stats") => crate::opportunity_records::stats(store),
            ("GET", "/api/strategies") => crate::strategies::list(store),
            ("POST", "/api/strategies/update") => {
                let value: serde_json::Value = serde_json::from_slice(&request.body)
                    .map_err(|_| api_error(400, "FIN_INVALID", "JSON 无效"))?;
                crate::strategies::update(store, &value)
            }
            _ => return Err(api_error(405, "APP_METHOD_INVALID", "方法无效")),
        };
        return result.map(|v| v.to_string()).map_err(|e| {
            home_api::err(
                match e {
                    crate::finance::FinanceError::Conflict(_) => 409,
                    crate::finance::FinanceError::NotFound(_) => 404,
                    crate::finance::FinanceError::Db(_) => 500,
                    _ => 400,
                },
                &e,
            )
        });
    }
    if route.starts_with("/api/trends") {
        return crate::trends::handle_trends_request(request, route);
    }
    if route.starts_with("/api/rules")
        || route.starts_with("/api/events")
        || route == "/api/check/run"
        || route.starts_with("/api/simulate")
        || route == "/api/decisions"
    {
        let body_text = String::from_utf8_lossy(&request.body).to_string();
        return match request.method.as_str() {
            "GET" => match route {
                "/api/rules" => api_rules_list(store, request.path.as_str()),
                "/api/decisions" => api_decisions_list(store),
                "/api/events" => api_events_list(store, request.path.as_str()),
                _ => Err((404, "{\"error\":\"not found\"}".into())),
            },
            "POST" => match route {
                "/api/rules" => api_rule_create(store, &body_text),
                "/api/rules/update" => api_rule_update(store, &body_text),
                "/api/rules/enabled" => api_rule_set_enabled(store, &body_text),
                "/api/rules/archive" => api_rule_archive(store, &body_text),
                "/api/check/run" => api_check_run(store),
                "/api/events/resolve" => api_event_resolve(store, &body_text),
                "/api/events/ack" => api_event_ack(store, &body_text),
                "/api/events/snooze" => api_event_snooze(store, &body_text),
                "/api/simulate/buy" => api_simulate_buy(store, &body_text),
                "/api/decisions" => api_decision_create(store, &body_text),
                _ => Err((404, "{\"error\":\"not found\"}".into())),
            },
            _ => Err((404, "{\"error\":\"not found\"}".into())),
        };
    }
    if route == "/api/home/notifications/claim" && request.method == "POST" {
        return home_api::api_notification_claim(store);
    }
    if route == "/api/home/notifications/result" && request.method == "POST" {
        return home_api::api_notification_result(store, &String::from_utf8_lossy(&request.body));
    }
    if route == "/api/opening/list" && request.method == "GET" {
        return positions::api_opening_list(store, &request.path);
    }
    if route == "/api/holding/valuation" && request.method == "POST" {
        return positions::api_holding_valuation(store, &String::from_utf8_lossy(&request.body));
    }
    if route == "/api/holding/records" && request.method == "GET" {
        return positions::api_holding_records(store, &request.path);
    }
    if route == "/api/opening/history" && request.method == "GET" {
        return positions::api_opening_history(store, &request.path);
    }
    if route == "/api/opening/change" && request.method == "POST" {
        return positions::api_opening_change(store, &String::from_utf8_lossy(&request.body));
    }
    if route == "/api/legacy-assets" && request.method == "GET" {
        let page = crate::market::query_param(&request.path, "page")
            .unwrap_or_else(|| "1".into())
            .parse::<i64>()
            .map_err(|_| api_error(400, "ASSET_INVALID", "分页无效"))?;
        let kind = crate::market::query_param(&request.path, "kind").unwrap_or_default();
        return crate::legacy_assets::list_page(store, page, &kind)
            .map(|v| v.to_string())
            .map_err(|e| {
                api_error(
                    if matches!(e, crate::ledger::LedgerError::Invalid(_)) {
                        400
                    } else {
                        500
                    },
                    e.code(),
                    &e.message(),
                )
            });
    }
    if route == "/api/legacy-assets/change" && request.method == "POST" {
        let v: serde_json::Value = serde_json::from_slice(&request.body)
            .map_err(|_| api_error(400, "ASSET_INVALID", "JSON 无效"))?;
        return crate::legacy_assets::change(store, &v)
            .map(|v| v.to_string())
            .map_err(|e| {
                api_error(
                    if matches!(e, crate::ledger::LedgerError::Conflict(_)) {
                        409
                    } else {
                        400
                    },
                    e.code(),
                    &e.message(),
                )
            });
    }
    if route == "/api/home/evidence" && request.method == "POST" {
        return home_api::api_home_evidence(store, &String::from_utf8_lossy(&request.body));
    }
    // 通知渠道域（R5，S02～S04/S15）。
    if route == "/api/settings" && request.method == "GET" {
        return crate::settings::get(store)
            .map(|v| v.to_string())
            .map_err(|e| api_error(500, e.code(), &e.message()));
    }
    if route == "/api/settings" && request.method == "POST" {
        let value: serde_json::Value = serde_json::from_slice(&request.body)
            .map_err(|_| api_error(400, "FIN_INVALID", "JSON 无效"))?;
        return crate::settings::save(store, &value)
            .map(|v| v.to_string())
            .map_err(|e| {
                api_error(
                    if matches!(e, crate::finance::FinanceError::Conflict(_)) {
                        409
                    } else {
                        400
                    },
                    e.code(),
                    &e.message(),
                )
            });
    }
    if route.starts_with("/api/settings/")
        && matches!(
            route,
            "/api/settings/export" | "/api/settings/import/preview" | "/api/settings/import/commit"
        )
    {
        let result = match (request.method.as_str(), route) {
            ("GET", "/api/settings/export") => crate::config_templates::export(store),
            ("POST", "/api/settings/import/preview" | "/api/settings/import/commit") => {
                let value: serde_json::Value = serde_json::from_slice(&request.body)
                    .map_err(|_| api_error(400, "FIN_INVALID", "JSON 无效"))?;
                if route.ends_with("preview") {
                    crate::config_templates::preview(store, &value)
                } else {
                    crate::config_templates::commit(store, &value)
                }
            }
            _ => return Err(api_error(405, "APP_METHOD_INVALID", "方法无效")),
        };
        return result.map(|v| v.to_string()).map_err(|e| {
            api_error(
                if matches!(e, crate::finance::FinanceError::Conflict(_)) {
                    409
                } else {
                    400
                },
                e.code(),
                &e.message(),
            )
        });
    }
    if route == "/api/settings/health" && request.method == "GET" {
        return crate::settings::health(store)
            .map(|v| v.to_string())
            .map_err(|e| api_error(500, e.code(), &e.message()));
    }
    if route == "/api/notify/channels" && request.method == "GET" {
        return notify_api::api_notify_channels(store);
    }
    if route == "/api/notify/preview" && request.method == "POST" {
        let input: serde_json::Value =
            serde_json::from_slice(&request.body).map_err(|_| (400, "FIN_INVALID".to_string()))?;
        if !input.as_object().is_some_and(|o| {
            o.keys().all(|k| {
                ["id", "subject", "subjectId", "snapshotId", "language"].contains(&k.as_str())
            })
        }) {
            return Err((400, "FIN_INVALID".into()));
        }
        return crate::notification_subjects::preview(store, &input)
            .map(|v| v.to_string())
            .map_err(|e| home_api::err(400, &e));
    }
    if route == "/api/notify/channels/save" && request.method == "POST" {
        return notify_api::api_notify_channels_save(
            store,
            &String::from_utf8_lossy(&request.body),
        );
    }
    if route == "/api/notify/channels/secret" && request.method == "POST" {
        return notify_api::api_notify_channels_secret(
            store,
            secrets,
            &String::from_utf8_lossy(&request.body),
        );
    }
    if route == "/api/notify/channels/state" && request.method == "POST" {
        return notify_api::api_notify_channels_state(
            store,
            &String::from_utf8_lossy(&request.body),
        );
    }
    if route == "/api/notify/channels/delete" && request.method == "POST" {
        return notify_api::api_notify_channels_delete(
            store,
            &String::from_utf8_lossy(&request.body),
        );
    }
    if route == "/api/notify/settings" && request.method == "GET" {
        return notify_api::api_notify_settings_get(store);
    }
    if route == "/api/notify/settings" && request.method == "POST" {
        return notify_api::api_notify_settings_save(
            store,
            &String::from_utf8_lossy(&request.body),
        );
    }
    if route.starts_with("/api/notify/deliveries") && request.method == "GET" {
        return notify_api::api_notify_deliveries(store, request.path.as_str());
    }
    if route == "/api/analysis/validate" && request.method == "POST" {
        return reports_api::api_analysis_validate(&String::from_utf8_lossy(&request.body));
    }
    if route == "/api/analysis/reports" && request.method == "GET" {
        return reports_api::api_analysis_reports(store, request.path.as_str());
    }
    if route == "/api/analysis/reports/save" && request.method == "POST" {
        return reports_api::api_analysis_report_save(
            store,
            &String::from_utf8_lossy(&request.body),
        );
    }
    if route == "/api/analysis/reports/compare" && request.method == "GET" {
        return reports_api::api_analysis_report_compare(store, request.path.as_str());
    }
    if route == "/api/analysis/reports/restore" && request.method == "POST" {
        return reports_api::api_analysis_report_restore(
            store,
            &String::from_utf8_lossy(&request.body),
        );
    }
    if route == "/api/analysis/reports/delete" && request.method == "POST" {
        return reports_api::api_analysis_report_delete(
            store,
            &String::from_utf8_lossy(&request.body),
        );
    }
    if route == "/api/information/cache" {
        let result = match request.method.as_str() {
            "GET" => crate::information::cache::stats(store),
            "POST" => {
                let input: serde_json::Value = serde_json::from_slice(&request.body)
                    .map_err(|_| api_error(400, "FIN_INVALID", "JSON 无效"))?;
                crate::information::cache::clear(store, &input)
            }
            _ => return Err(api_error(405, "APP_METHOD_INVALID", "方法无效")),
        };
        return result.map(|v| v.to_string()).map_err(|e| {
            api_error(
                if matches!(e, crate::finance::FinanceError::Conflict(_)) {
                    409
                } else {
                    400
                },
                e.code(),
                &e.message(),
            )
        });
    }
    if route == "/api/information/items" && request.method == "GET" {
        return information_api::api_information_items(store, request.path.as_str());
    }
    if route == "/api/information/item" && request.method == "GET" {
        return information_api::api_information_item(store, request.path.as_str());
    }
    // Network tasks are intercepted by FundService's session-owned Tasks.
    if route == "/api/information/sources" && request.method == "GET" {
        return information_api::api_information_sources(store);
    }
    if route == "/api/information/sources/enabled" && request.method == "POST" {
        return information_api::api_information_source_enabled(
            store,
            &String::from_utf8_lossy(&request.body),
        );
    }
    if route == "/api/information/flags" && request.method == "POST" {
        return information_api::api_information_flags(
            store,
            &String::from_utf8_lossy(&request.body),
        );
    }
    if route == "/api/information/archive" && request.method == "POST" {
        return information_api::api_information_archive(
            store,
            &String::from_utf8_lossy(&request.body),
        );
    }
    // 资讯复核提醒（R02；方案 05 §8）：查看依据与处理状态。
    if route == "/api/information/alerts" && request.method == "GET" {
        return information_api::api_information_alerts(store);
    }
    if route == "/api/information/alerts/action" && request.method == "POST" {
        return information_api::api_information_alert_action(
            store,
            &String::from_utf8_lossy(&request.body),
        );
    }
    if route == "/api/home/preferences" {
        return home_api::api_home_preferences(store, request);
    }
    if route == "/api/home/group-plan" {
        return match request.method.as_str() {
            "GET" => home_api::api_group_plan_get(store),
            "PUT" => home_api::api_group_plan_save(store, &String::from_utf8_lossy(&request.body)),
            _ => Err((404, "{\"error\":\"not found\"}".into())),
        };
    }
    if route == "/api/home/group-plan/import-preview" && request.method == "GET" {
        return home_api::api_group_plan_import_preview(store);
    }
    if route == "/api/home/snapshot/generate" && request.method == "POST" {
        if !request.body.is_empty()
            && !serde_json::from_slice::<serde_json::Value>(&request.body)
                .is_ok_and(|v| v.as_object().is_some_and(|o| o.is_empty()))
        {
            return Err(api_error(400, "FIN_INVALID", "生成请求不接受数据字段"));
        }
        return home_api::api_home_generate(store, &request.path);
    }
    if route == "/api/home/snapshot" && request.method == "GET" {
        return api_home_snapshot(store, request.path.as_str());
    }
    if route == "/api/portfolio/benchmark" && request.method == "GET" {
        return portfolio_benchmark_api::api_portfolio_benchmark(store, request.path.as_str());
    }
    if route.starts_with("/api/finance") {
        return match request.method.as_str() {
            "GET" => match route {
                "/api/finance/trades" => home_api::api_trade_cash_list(store),
                "/api/finance/cash" => api_finance_cash(store),
                "/api/finance/reservations" => api_finance_reservations(store),
                "/api/finance/events" => api_finance_events(store, request.path.as_str()),
                "/api/finance/returns" => api_finance_returns(store, request.path.as_str()),
                _ => Err((404, "{\"error\":\"not found\"}".into())),
            },
            "POST" => {
                let body_text = String::from_utf8_lossy(&request.body).to_string();
                match route {
                    "/api/finance/trades/settle" => {
                        home_api::api_trade_cash_settle(store, &body_text)
                    }
                    "/api/finance/reserve" => api_finance_reserve_save(store, &body_text),
                    "/api/finance/events" => api_finance_event_create(store, &body_text),
                    "/api/finance/snapshot/save" => api_finance_snapshot_save(store, &body_text),
                    "/api/finance/reservations" => {
                        api_finance_reservation_create(store, &body_text)
                    }
                    "/api/finance/reservations/settle" => {
                        api_finance_reservation_settle(store, &body_text)
                    }
                    _ => Err((404, "{\"error\":\"not found\"}".into())),
                }
            }
            _ => Err((404, "{\"error\":\"not found\"}".into())),
        };
    }
    if route.starts_with("/api/sources") || route.starts_with("/api/theses") {
        let body_text = String::from_utf8_lossy(&request.body).to_string();
        return match request.method.as_str() {
            "GET" => match route {
                "/api/sources" => api_sources_list(store, request.path.as_str()),
                "/api/theses" => api_theses_list(store, &request.path),
                _ => Err((404, "{\"error\":\"not found\"}".into())),
            },
            "POST" => match route {
                "/api/sources" => api_source_create(store, &body_text),
                "/api/sources/update" => api_source_update(store, &body_text),
                "/api/sources/archive" => api_source_archive(store, &body_text),
                "/api/sources/flags" => api_source_flags(store, &body_text),
                "/api/theses" => api_thesis_create(store, &body_text),
                "/api/theses/archive" => api_thesis_archive(store, &body_text),
                "/api/theses/restore" => api_thesis_restore(store, &body_text),
                _ => Err((404, "{\"error\":\"not found\"}".into())),
            },
            _ => Err((404, "{\"error\":\"not found\"}".into())),
        };
    }
    if route.starts_with("/api/goals") {
        let body_text = String::from_utf8_lossy(&request.body).to_string();
        return match request.method.as_str() {
            "GET" => api_goals_list(store, request.path.as_str()),
            "POST" => match route {
                "/api/goals" => api_goal_create(store, &body_text),
                "/api/goals/update" => api_goal_update(store, &body_text),
                "/api/goals/primary" => api_goal_set_primary(store, &body_text),
                "/api/goals/archive" => api_goal_archive(store, &body_text),
                "/api/goals/restore" => api_goal_restore(store, &body_text),
                _ => Err((404, "{\"error\":\"not found\"}".into())),
            },
            _ => Err((404, "{\"error\":\"not found\"}".into())),
        };
    }

    let body_text = String::from_utf8_lossy(&request.body).to_string();
    let method = request.method.as_str();
    match (method, route) {
        ("GET", "/api/positions") => api_positions(store),
        ("GET", "/api/positions/detail") => api_positions_detail(store, request.path.as_str()),
        ("GET", "/api/transactions") => api_transactions(store, request.path.as_str()),
        ("POST", "/api/transactions") => api_record_transaction(store, &body_text),
        ("POST", "/api/transactions/confirm") => api_confirm_transaction(store, &body_text),
        // 期初登记（方案 01 §5.1）：账户+标的唯一，不扣现金。
        ("POST", "/api/openings") => api_record_opening(store, &body_text),
        // 交易更正/删除/恢复（方案 01 §5.2；预览无副作用，提交带 request_id 幂等）。
        ("POST", "/api/transactions/delete/preview") => {
            crate::api::corrections_api::api_preview_delete(store, &body_text)
        }
        ("POST", "/api/transactions/delete") => {
            crate::api::corrections_api::api_delete_transaction(store, &body_text)
        }
        ("POST", "/api/transactions/restore") => {
            crate::api::corrections_api::api_restore_transaction(store, &body_text)
        }
        ("POST", "/api/transactions/correction/preview") => {
            crate::api::corrections_api::api_preview_correction(store, &body_text)
        }
        ("POST", "/api/transactions/correction") => {
            crate::api::corrections_api::api_submit_correction(store, &body_text)
        }
        // 线下投资资产（方案 01 P05；O01～O05）。
        ("GET", "/api/offline/assets/exits") => {
            offline_api::api_offline_exits(store, &request.path)
        }
        ("POST", "/api/offline/assets/settle") => {
            offline_api::api_offline_settle(store, &body_text)
        }
        ("GET", "/api/offline/assets") => {
            crate::api::offline_api::api_offline_list(store, request.path.as_str())
        }
        ("POST", "/api/offline/assets") => {
            crate::api::offline_api::api_offline_create(store, &body_text)
        }
        ("POST", "/api/offline/assets/update") => {
            crate::api::offline_api::api_offline_update(store, &body_text)
        }
        ("POST", "/api/offline/assets/exit") => {
            crate::api::offline_api::api_offline_exit(store, &body_text)
        }
        ("POST", "/api/offline/assets/delete") => {
            crate::api::offline_api::api_offline_delete(store, &body_text)
        }
        ("POST", "/api/offline/assets/restore") => {
            crate::api::offline_api::api_offline_restore(store, &body_text)
        }
        ("GET", "/api/offline/assets/history") => {
            crate::api::offline_api::api_offline_history(store, request.path.as_str())
        }
        // 场外基金操作（N09）：订单状态机、持仓校准、定投计划。
        ("POST", "/api/otc/holding/preview") => otc_api::api_holding_preview(store, &body_text),
        ("POST", "/api/otc/holding") => otc_api::api_holding_create(store, &body_text),
        ("POST", "/api/otc/holding/edit/preview") => {
            otc_api::api_holding_edit_preview(store, &body_text)
        }
        ("POST", "/api/otc/holding/edit") => otc_api::api_holding_edit(store, &body_text),
        ("GET", "/api/otc/orders") => otc_api::api_orders_list(store, request.path.as_str()),
        ("POST", "/api/otc/orders") => otc_api::api_order_create(store, &body_text),
        ("POST", "/api/otc/orders/confirm") => otc_api::api_order_confirm(store, &body_text),
        ("POST", "/api/otc/orders/cancel") => otc_api::api_order_cancel(store, &body_text),
        ("POST", "/api/otc/orders/arrival") => otc_api::api_order_arrival(store, &body_text),
        ("GET", "/api/otc/plans") => otc_api::api_plans_list(store, request.path.as_str()),
        ("POST", "/api/otc/plans") => otc_api::api_plan_create(store, &body_text),
        ("POST", "/api/otc/plans/update") => otc_api::api_plan_update(store, &body_text),
        ("GET", "/api/otc/plans/runs") => otc_api::api_plan_runs(store, request.path.as_str()),
        ("POST", "/api/otc/plans/runs/register") => {
            otc_api::api_plan_run_register(store, &body_text)
        }
        ("GET", "/api/otc/fund") => otc_api::api_fund_exists(store, request.path.as_str()),
        ("GET", "/api/nav") => api_nav_list(store, request.path.as_str()),
        ("POST", "/api/nav/sync") => api_nav_sync(store, &body_text),
        ("GET", "/api/fund/quarterly-positions") => {
            nav_api::api_quarterly_positions(request.path.as_str())
        }
        ("GET", "/api/history") => api_history(store, request.path.as_str()),
        ("POST", "/api/import/preview") => api_import_preview(&body_text),
        ("POST", "/api/import/commit") => api_import_commit(store, &body_text),
        ("GET", "/api/accounts") => api_accounts(store),
        ("POST", "/api/accounts") => api_create_account(store, &body_text),
        // 结构化账户命令（R2 T03 / 方案 03）：乐观锁 + requestId 幂等。
        ("GET", "/api/accounts/detail") => {
            account_ops::api_accounts_detail(store, request.path.as_str())
        }
        ("POST", "/api/accounts/update") => account_ops::api_account_update(store, &body_text),
        ("POST", "/api/accounts/order") => account_ops::api_account_order(store, &body_text),
        ("POST", "/api/accounts/archive") => account_ops::api_account_archive(store, &body_text),
        ("POST", "/api/accounts/restore") => account_ops::api_account_restore(store, &body_text),
        // 持仓转移（双边事务；预览只读并回传 dataRevision）。
        ("POST", "/api/positions/transfer/preview") => api_transfer_preview(store, &body_text),
        ("POST", "/api/positions/transfer") => api_transfer_commit(store, &body_text),
        // 已记录投资资产（P1）与目标/配置（P2）。
        ("GET", "/api/assets") => api_assets_list(store),
        ("POST", "/api/assets") => api_create_asset(store, &body_text),
        ("POST", "/api/assets/update") => api_update_asset(store, &body_text),
        ("POST", "/api/assets/delete") => api_delete_asset(store, &body_text),
        ("POST", "/api/fund-asset-metadata") => api_set_fund_metadata(store, &body_text),
        ("GET", "/api/fund") => api_fund_by_code(store, request),
        ("GET", "/api/goal") => api_goal_get(store),
        ("POST", "/api/goal") => api_goal_save(store, &body_text),
        ("POST", "/api/goal/clear") => api_goal_clear(store),
        ("GET", "/api/allocation") => api_allocation_get(store),
        ("POST", "/api/allocation") => api_allocation_save(store, &body_text),
        ("GET", "/api/asset-accounts") => api_asset_accounts(store),
        // 首页「行情」：真实上游代理（东财/腾讯），无 store 依赖。
        ("GET", "/api/market/indices") => crate::market::api_market_indices(request),
        ("GET", "/api/market/stocks") => crate::market::api_market_stocks(request),
        ("GET", "/api/market/etfs") => crate::market::api_market_etfs(request),
        ("GET", "/api/market/sectors") => crate::market::api_market_sectors(request),
        ("GET", "/api/market/sector/stocks") => crate::market::api_market_sector_stocks(request),
        ("GET", "/api/market/themes") => crate::market::api_market_themes(),
        ("GET", "/api/market/theme/etfs") => crate::market::api_market_quotes(request),
        ("GET", "/api/market/quotes") => crate::market::api_market_quotes(request),
        ("GET", "/api/market/detail") => crate::market::api_market_detail(request),
        ("GET", "/api/market/suggest") => crate::market::api_market_suggest(request),
        ("GET", "/api/market/minute") => crate::market::api_market_minute(request),
        ("GET", "/api/market/kline") => crate::market::api_market_kline(request),
        // 自选管理
        ("GET", "/api/watchlists") => api_watchlists(store),
        ("POST", "/api/watchlists") => api_create_watchlist(store, &body_text),
        ("POST", "/api/watchlists/manage") => api_manage_watchlist(store, &body_text),
        ("POST", "/api/watchlists/items") => api_add_watchlist_item(store, &body_text),
        ("DELETE", "/api/watchlists/items") => api_remove_watchlist_item(store, request),
        // 原生 Canvas 划线数据持久化
        ("GET", "/api/drawings") => api_drawings_list(store, request),
        ("POST", "/api/drawings") => api_save_drawing(store, &body_text),
        ("DELETE", "/api/drawings") => api_delete_drawing(store, request),
        _ => Err((404, "{\"error\":\"not found\"}".into())),
    }
}

pub(crate) use crate::wire::json_str;

pub(crate) use crate::wire::api_error;

fn query_param<'a>(path: &'a str, key: &str) -> Option<String> {
    let query = path.split_once('?')?.1;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=')?;
        if k == key {
            return Some(crate::market::url_decode(v));
        }
    }
    None
}

fn query_param_from_path<'a>(path: &'a str, key: &str) -> Option<String> {
    let query = path.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| crate::market::url_decode(v))
    })
}
