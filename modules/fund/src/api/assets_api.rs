//! 已记录投资资产与目标/配置 API（P1/P2，方案 01 F2/F3）。
//!
//! 服务端与页面执行相同校验（方案 04 旅程 B）；错误返回稳定 code 与
//! 可执行的修正提示，不泄漏 SQL/路径。金额一律定点字符串（人民币分位）。

use super::{api_error, json_str};
use crate::assets::{self, summary_json, ManualAssetInput};
use crate::planning::{self, AllocationTargetRow};
use crate::storage::Store;
use app_runtime_core::http::HttpRequest;
use rusqlite::OptionalExtension;
use serde_json::Value;

fn body_json(body: &str) -> Result<Value, (u16, String)> {
    serde_json::from_str(body).map_err(|e| api_error(400, "ASSET_INVALID", &e.to_string()))
}

fn get_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, (u16, String)> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| api_error(400, "ASSET_INVALID", &format!("{key} 缺失")))
}

fn get_bool(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn manual_input_from(value: &Value) -> Result<ManualAssetInput, (u16, String)> {
    Ok(ManualAssetInput {
        account_name: get_str(value, "account")?.to_string(),
        product_type: get_str(value, "productType")?.to_string(),
        market: value
            .get("market")
            .and_then(Value::as_str)
            .map(str::to_string),
        code: value
            .get("code")
            .and_then(Value::as_str)
            .map(str::to_string),
        name: get_str(value, "name")?.to_string(),
        risk_class: value
            .get("riskClass")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        market_value_text: get_str(value, "marketValue")?.to_string(),
        valued_at: get_str(value, "valuedAt")?.to_string(),
        confirm_distinct: get_bool(value, "confirmDistinct"),
    })
}

fn asset_error(e: assets::AssetError) -> (u16, String) {
    let status = match e {
        assets::AssetError::NotFound(_) => 404,
        assets::AssetError::Duplicate(_) | assets::AssetError::OverlapReview { .. } => 409,
        assets::AssetError::Invalid(_) => 400,
        assets::AssetError::Overflow(_) => 500,
        assets::AssetError::Db(_) => 500,
    };
    api_error(status, e.code(), &e.message())
}

fn plan_error(e: planning::PlanError) -> (u16, String) {
    let status = match e {
        planning::PlanError::NotFound(_) => 404,
        planning::PlanError::Invalid(_) => 400,
        planning::PlanError::Overflow(_) => 500,
        planning::PlanError::Db(_) => 500,
    };
    api_error(status, e.code(), &e.message())
}

// ---------------------------------------------------------------------------
// GET /api/assets：资产页完整负载（同一次读快照）
// ---------------------------------------------------------------------------

pub fn api_assets_list(store: &Store) -> Result<String, (u16, String)> {
    let today = super::today_iso();
    let (summary, revision) = store
        .with_snapshot(|conn| -> Result<_, assets::AssetError> {
            Ok((
                assets::assets_summary(conn, &today)?,
                Store::data_revision(conn)?,
            ))
        })
        .map_err(asset_error)?;
    let funds = summary
        .funds
        .iter()
        .map(|f| {
            format!(
                "{{\"accountId\":{acc},\"account\":{acc_name},\"fundId\":{fid},\"code\":{code},\"name\":{name},\
                 \"quantity\":{qty},\"cost\":{cost},\"costUnknown\":{cost_unknown},\"holdingAsOf\":{as_of},\
                 \"nav\":{nav},\"navDate\":{nav_date},\"navSource\":{nav_source},\
                 \"navCurrency\":{cur},\"riskClass\":{risk},\
                 \"value\":{value},\"valueGap\":{gap},\"stale\":{stale}}}",
                acc = f.account_id,
                acc_name = json_str(&f.account_name),
                fid = f.fund_id,
                code = json_str(&f.fund_code),
                name = json_str(&f.fund_name),
                qty = json_str(&crate::fixed::Fixed::nav(f.quantity_raw).to_string_value()),
                cost = json_str(&crate::fixed::Fixed::amount(f.cost_raw).to_string_value()),
                cost_unknown = f.cost_unknown,
                as_of = json_str(&f.holding_as_of),
                nav = f.nav_raw.map(|raw| json_str(&crate::fixed::Fixed::nav(raw).to_string_value())).unwrap_or_else(|| "null".into()),
                nav_date = f.nav_date.as_ref().map(|d| json_str(d)).unwrap_or_else(|| "null".into()),
                nav_source = f.nav_source.as_ref().map(|s| json_str(s)).unwrap_or_else(|| "null".into()),
                cur = json_str(&f.nav_currency),
                risk = json_str(&f.risk_class),
                value = f.value_minor.map(|v| json_str(&crate::fixed::Fixed::amount(v).to_string_value())).unwrap_or_else(|| "null".into()),
                gap = f.value_gap.as_ref().map(|g| json_str(g)).unwrap_or_else(|| "null".into()),
                stale = f.stale,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let manual = summary
        .manual
        .iter()
        .map(|m| {
            format!(
                "{{\"id\":{id},\"accountId\":{acc},\"account\":{acc_name},\"productType\":{pt},\
                 \"instrumentKey\":{key},\"name\":{name},\"riskClass\":{risk},\
                 \"marketValue\":{value},\"valuedAt\":{date},\"stale\":{stale},\
                 \"overlapReview\":{review},\"legacyOverlapDistinct\":{distinct}}}",
                id = m.id,
                acc = m.account_id,
                acc_name = json_str(&m.account_name),
                pt = json_str(&m.product_type),
                key = json_str(&m.instrument_key),
                name = json_str(&m.name),
                risk = json_str(&m.risk_class),
                value =
                    json_str(&crate::fixed::Fixed::amount(m.market_value_minor).to_string_value()),
                date = json_str(&m.valued_at),
                stale = m.stale,
                review = m.overlap_review,
                distinct = m.legacy_overlap_distinct,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        "{{\"today\":{today},\"dataRevision\":{revision},\"summary\":{summary},\"funds\":[{funds}],\"manual\":[{manual}]}}",
        today = json_str(&today),
        revision = json_str(&revision),
        summary = summary_json(&summary),
        funds = funds,
        manual = manual,
    ))
}

// ---------------------------------------------------------------------------
// 人工资产写路径
// ---------------------------------------------------------------------------

pub fn api_create_asset(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let input = manual_input_from(&value)?;
    let today = super::today_iso();
    let id = assets::create_asset(store, &input, &today).map_err(asset_error)?;
    Ok(format!("{{\"id\":{id}}}"))
}

pub fn api_update_asset(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = value
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| api_error(400, "ASSET_INVALID", "id 缺失"))?;
    let input = manual_input_from(&value)?;
    let today = super::today_iso();
    versioned_asset_change(store, id, &value, Some(&input), &today)?;
    Ok("{\"ok\":true}".into())
}

pub fn api_delete_asset(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let id = value
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| api_error(400, "ASSET_INVALID", "id 缺失"))?;
    versioned_asset_change(store, id, &value, None, &super::today_iso())?;
    Ok("{\"ok\":true}".into())
}

fn versioned_asset_change(
    store: &Store,
    id: i64,
    value: &Value,
    input: Option<&ManualAssetInput>,
    today: &str,
) -> Result<(), (u16, String)> {
    let expected = value["expectedVersion"]
        .as_i64()
        .filter(|n| *n > 0)
        .ok_or_else(|| api_error(400, "ASSET_INVALID", "expectedVersion 缺失"))?;
    store.with_write(|c|{
        let old=c.query_row("SELECT version,deleted_at,converted_ref,market_value_minor,valued_at,name FROM manual_assets WHERE id=?1",[id],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,i64>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?))).optional()?.ok_or_else(||assets::AssetError::NotFound("资产".into()))?;
        if old.0!=expected||old.1.is_some()||old.2.is_some(){return Err(assets::AssetError::Duplicate("记录已变化、归档或转换，请刷新".into()));}
        if let Some(input)=input{assets::update_manual_asset(c,id,input,today)?;}else{c.execute("UPDATE manual_assets SET deleted_at=?2 WHERE id=?1",rusqlite::params![id,today])?;}
        c.execute("UPDATE manual_assets SET version=version+1 WHERE id=?1",[id])?;
        c.execute("INSERT INTO legacy_asset_history(asset_id,payload_json,created_at) VALUES(?1,?2,?3)",rusqlite::params![id,serde_json::json!({"previous":old,"operation":if input.is_some(){"update"}else{"delete"},"input":value}).to_string(),crate::finance::now_secs()])?;
        Ok(())
    }).map_err(asset_error)
}

/// 基金净值币种确认 / 主类别确认（P1/P2 共用；来源固定为用户确认）。
pub fn api_set_fund_metadata(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let fund_id = value
        .get("fundId")
        .and_then(Value::as_i64)
        .ok_or_else(|| api_error(400, "ASSET_INVALID", "fundId 缺失"))?;
    let nav_currency = value
        .get("navCurrency")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let risk_class = value
        .get("riskClass")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let expected = value
        .get("dataRevision")
        .map(|v| {
            v.as_str()
                .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                .ok_or_else(|| api_error(400, "ASSET_INVALID", "dataRevision 无效"))
        })
        .transpose()?;
    store
        .with_write(|conn| -> Result<(), assets::AssetError> {
            if let Some(revision) = expected {
                if Store::data_revision(conn)? != revision {
                    return Err(assets::AssetError::Duplicate(
                        "记录已变化，请刷新后确认币种".into(),
                    ));
                }
            }
            assets::set_fund_metadata(conn, fund_id, nav_currency, risk_class)?;
            Ok(())
        })
        .map_err(asset_error)?;
    Ok("{\"ok\":true}".into())
}

// ---------------------------------------------------------------------------
// 目标与配置（P2）
// ---------------------------------------------------------------------------

pub fn api_goal_get(store: &Store) -> Result<String, (u16, String)> {
    let today = super::today_iso();
    let goal = store
        .with_read(|conn| planning::get_goal(conn, &today))
        .map_err(|e| api_error(500, "PLAN_DB", &e.to_string()))?;
    let Some(goal) = goal else {
        return Ok(format!(
            "{{\"today\":{today},\"goal\":null}}",
            today = json_str(&today)
        ));
    };
    let summary = assets::summary(store, &today).map_err(asset_error)?;
    let progress = planning::goal_progress(&summary, &goal);
    let progress_json = format!(
        "{{\"computable\":{computable},\"reasonCodes\":[{reasons}],\"ratioPercent\":{ratio},\
         \"currentValue\":{value}}}",
        computable = progress.computable,
        reasons = progress
            .reason_codes
            .iter()
            .map(|c| json_str(c))
            .collect::<Vec<_>>()
            .join(","),
        ratio = progress
            .ratio_tenths
            .map(|t| json_str(&format!("{:.1}", t as f64 / 10.0)))
            .unwrap_or_else(|| "null".into()),
        value = progress
            .current_value_minor
            .map(|v| format!(
                "{{\"amount\":{},\"currency\":\"CNY\"}}",
                json_str(&crate::fixed::Fixed::amount(v).to_string_value())
            ))
            .unwrap_or_else(|| "null".into()),
    );
    Ok(format!(
        "{{\"today\":{today},\"goal\":{{\"targetAmount\":{amount},\"targetDate\":{date},\"expired\":{expired}}},\"progress\":{progress_json}}}",
        today = json_str(&today),
        amount = format!(
            "{{\"amount\":{},\"currency\":\"CNY\"}}",
            json_str(&crate::fixed::Fixed::amount(goal.target_minor).to_string_value())
        ),
        date = json_str(&goal.target_date),
        expired = goal.expired,
        progress_json = progress_json,
    ))
}

pub fn api_goal_save(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let target = get_str(&value, "target")?;
    let date = value
        .get("targetDate")
        .and_then(Value::as_str)
        .unwrap_or("");
    let keep = get_bool(&value, "keepExistingDate");
    let today = super::today_iso();
    planning::save_goal_tx(store, target, date, keep, &today).map_err(plan_error)?;
    Ok("{\"ok\":true}".into())
}

pub fn api_goal_clear(store: &Store) -> Result<String, (u16, String)> {
    planning::clear_goal_tx(store).map_err(plan_error)?;
    Ok("{\"ok\":true}".into())
}

pub fn api_allocation_get(store: &Store) -> Result<String, (u16, String)> {
    let today = super::today_iso();
    let check = planning::allocation_check(store, &today).map_err(plan_error)?;
    let classes = check
        .classes
        .iter()
        .map(|c| {
            format!(
                "{{\"riskClass\":{rc},\"targetTenths\":{t},\"lowerTenths\":{lo},\"upperTenths\":{up},\
                 \"actualTenths\":{actual},\"amount\":{amount},\"status\":{status},\"deltaTenths\":{delta}}}",
                rc = json_str(&c.risk_class),
                t = c.target_tenths,
                lo = c.lower_tenths,
                up = c.upper_tenths,
                actual = c.actual_tenths.map(|v| v.to_string()).unwrap_or_else(|| "null".into()),
                amount = c.amount_minor.map(|v| json_str(&crate::fixed::Fixed::amount(v).to_string_value())).unwrap_or_else(|| "null".into()),
                status = c.status.map(|s| json_str(s)).unwrap_or_else(|| "null".into()),
                delta = c.delta_tenths.map(|v| v.to_string()).unwrap_or_else(|| "null".into()),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let out_of_range = check
        .out_of_range
        .iter()
        .map(|o| {
            format!(
                "{{\"riskClass\":{rc},\"direction\":{dir},\"actualTenths\":{actual}}}",
                rc = json_str(&o.risk_class),
                dir = json_str(o.direction),
                actual = o.actual_tenths,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    // 未配置策略时也返回已保存目标行（可能是空集）。
    let targets = store
        .with_read(planning::get_allocation)
        .map_err(|e| api_error(500, "PLAN_DB", &e.to_string()))?;
    let targets_json = targets
        .iter()
        .map(|t| {
            format!(
                "{{\"riskClass\":{rc},\"targetTenths\":{t},\"lowerTenths\":{lo},\"upperTenths\":{up}}}",
                rc = json_str(&t.risk_class),
                t = t.target_tenths,
                lo = t.lower_tenths,
                up = t.upper_tenths,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        "{{\"configured\":{configured},\"computable\":{computable},\"reasonCodes\":[{reasons}],\
         \"unknownClassCount\":{unknown_cls},\"totalValue\":{total},\
         \"classes\":[{classes}],\"outOfRange\":[{out_of_range}],\"targets\":[{targets_json}]}}",
        configured = check.configured,
        computable = check.computable,
        reasons = check
            .reason_codes
            .iter()
            .map(|c| json_str(c))
            .collect::<Vec<_>>()
            .join(","),
        unknown_cls = check.unknown_class_count,
        total = check
            .total_minor
            .map(|v| json_str(&crate::fixed::Fixed::amount(v).to_string_value()))
            .unwrap_or_else(|| "null".into()),
        classes = classes,
        out_of_range = out_of_range,
        targets_json = targets_json,
    ))
}

pub fn api_allocation_save(store: &Store, body: &str) -> Result<String, (u16, String)> {
    let value = body_json(body)?;
    let rows_value = value
        .get("targets")
        .and_then(Value::as_array)
        .ok_or_else(|| api_error(400, "PLAN_INVALID", "targets 缺失"))?;
    let mut rows = Vec::with_capacity(rows_value.len());
    for row in rows_value {
        rows.push(AllocationTargetRow {
            risk_class: row
                .get("riskClass")
                .and_then(Value::as_str)
                .ok_or_else(|| api_error(400, "PLAN_INVALID", "riskClass 缺失"))?
                .to_string(),
            target_tenths: row
                .get("targetTenths")
                .and_then(Value::as_i64)
                .ok_or_else(|| api_error(400, "PLAN_INVALID", "targetTenths 缺失"))?,
            lower_tenths: row
                .get("lowerTenths")
                .and_then(Value::as_i64)
                .ok_or_else(|| api_error(400, "PLAN_INVALID", "lowerTenths 缺失"))?,
            upper_tenths: row
                .get("upperTenths")
                .and_then(Value::as_i64)
                .ok_or_else(|| api_error(400, "PLAN_INVALID", "upperTenths 缺失"))?,
        });
    }
    planning::save_allocation_tx(store, &rows).map_err(plan_error)?;
    Ok("{\"ok\":true}".into())
}

/// 账户列表（资产表单账户选择复用）。
pub fn api_asset_accounts(store: &Store) -> Result<String, (u16, String)> {
    let names: Vec<String> = store
        .with_read(|conn| -> rusqlite::Result<Vec<String>> {
            let mut stmt = conn.prepare("SELECT name FROM accounts ORDER BY id")?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(|e| api_error(500, "LEDGER_DB", &e.to_string()))?;
    let items = names
        .iter()
        .map(|n| json_str(n))
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!("{{\"accounts\":[{items}]}}"))
}

/// 基金主档（资产页币种确认入口需要 fundId）。
pub fn api_fund_by_code(store: &Store, request: &HttpRequest) -> Result<String, (u16, String)> {
    let code = request
        .path
        .split_once('?')
        .and_then(|(_, q)| q.split('&').find_map(|p| p.strip_prefix("code=")))
        .map(crate::market::url_decode)
        .ok_or_else(|| api_error(400, "ASSET_INVALID", "code 缺失"))?;
    let row: Option<(i64, String, String, Option<i64>, Option<String>)> = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT id, code, name, latest_nav_raw, latest_nav_date FROM funds WHERE code = ?1",
                rusqlite::params![code],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()
        })
        .map_err(|e| api_error(500, "ASSET_DB", &e.to_string()))?;
    let Some((id, code, name, nav_raw, nav_date)) = row else {
        return Err(api_error(404, "ASSET_NOT_FOUND", "基金不存在"));
    };
    Ok(format!(
        "{{\"id\":{id},\"code\":{code},\"name\":{name},\"latestNav\":{nav},\"latestNavDate\":{date}}}",
        code = json_str(&code),
        name = json_str(&name),
        nav = nav_raw.map(|v| json_str(&crate::fixed::Fixed::nav(v).to_string_value())).unwrap_or_else(|| "null".into()),
        date = nav_date.map(|d| json_str(&d)).unwrap_or_else(|| "null".into()),
    ))
}
