//! Portable non-secret configuration: bounded preview, revision binding, atomic additive import.
use crate::{finance::FinanceError, storage::Store};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
const MAX_BYTES: usize = 200 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Template {
    format_version: u32,
    #[serde(default)]
    generated_at: String,
    #[serde(default)]
    redacted: bool,
    #[serde(default)]
    modules: Vec<String>,
    settings: Value,
    accounts: Vec<Account>,
    strategies: Vec<Strategy>,
    rules: Vec<Rule>,
    sources: Vec<Source>,
    channels: Vec<Channel>,
    watchlists: Vec<Watchlist>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Account {
    name: String,
    sort_order: i64,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Strategy {
    code: String,
    enabled: bool,
    weight_ppm: i64,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Rule {
    name: String,
    rule_type: String,
    object_key: String,
    params: Value,
    period: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Source {
    source_id: String,
    enabled: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Channel {
    name: String,
    kind: String,
    config: Value,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Watchlist {
    name: String,
    items: Vec<WatchItem>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WatchItem {
    symbol: String,
    name: String,
    asset_type: String,
}
fn invalid(message: &str) -> FinanceError {
    FinanceError::Invalid(message.into())
}
fn rule_input(r: &Rule) -> crate::rules::RuleInput {
    crate::rules::RuleInput {
        name: r.name.clone(),
        rule_type: r.rule_type.clone(),
        object_key: r.object_key.clone(),
        params: r.params.clone(),
        period: r.period.clone(),
        enabled: false,
    }
}
fn bounded_text(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.chars().count() <= max && !s.chars().any(char::is_control)
}
fn valid_security(item: &WatchItem) -> bool {
    let symbol = item.symbol.as_str();
    let digits = |s: &str, n| s.len() == n && s.bytes().all(|b| b.is_ascii_digit());
    if item.asset_type == "fund" {
        return digits(symbol, 6);
    }
    if let Some(code) = symbol
        .strip_prefix("sh")
        .or_else(|| symbol.strip_prefix("sz"))
        .or_else(|| symbol.strip_prefix("bj"))
    {
        return digits(code, 6);
    }
    if item.asset_type == "stock" {
        if let Some(code) = symbol.strip_prefix("hk") {
            return digits(code, 5);
        }
        if let Some(code) = symbol.strip_prefix("us") {
            return !code.is_empty()
                && code.len() <= 15
                && code.as_bytes()[0].is_ascii_alphabetic()
                && code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.');
        }
    }
    false
}
fn parse(value: &Value) -> Result<Template, FinanceError> {
    if value.to_string().len() > MAX_BYTES {
        return Err(invalid("配置模板超过 200 KiB"));
    }
    let data: Template =
        serde_json::from_value(value.clone()).map_err(|_| invalid("模板版本、字段或类型无效"))?;
    if data.format_version != 1 {
        return Err(invalid("模板版本不支持"));
    }
    if !data.settings.is_null() {
        crate::settings::validate(&data.settings)?;
    }
    if data.generated_at.len() > 64
        || data.modules.len() > 7
        || data.modules.iter().any(|m| {
            ![
                "settings",
                "accounts",
                "strategies",
                "rules",
                "sources",
                "channels",
                "watchlists",
            ]
            .contains(&m.as_str())
        })
    {
        return Err(invalid("模板模块或元数据无效"));
    }
    if !data.modules.is_empty() {
        for (key, present) in [
            ("settings", !data.settings.is_null()),
            ("accounts", !data.accounts.is_empty()),
            ("strategies", !data.strategies.is_empty()),
            ("rules", !data.rules.is_empty()),
            ("sources", !data.sources.is_empty()),
            ("channels", !data.channels.is_empty()),
            ("watchlists", !data.watchlists.is_empty()),
        ] {
            if present && !data.modules.iter().any(|m| m == key) {
                return Err(invalid("模板包含未选中的模块"));
            }
        }
    }
    if [
        data.accounts.len(),
        data.strategies.len(),
        data.rules.len(),
        data.sources.len(),
        data.channels.len(),
        data.watchlists.len(),
    ]
    .iter()
    .any(|n| *n > 100)
    {
        return Err(invalid("模板每类最多 100 条"));
    }
    for a in &data.accounts {
        if !bounded_text(&a.name, 64) || !(0..=10000).contains(&a.sort_order) {
            return Err(invalid("账户元数据无效"));
        }
    }
    let mut codes = std::collections::HashSet::new();
    for s in &data.strategies {
        if !bounded_text(&s.code, 64)
            || !codes.insert(&s.code)
            || !(100_000..=3_000_000).contains(&s.weight_ppm)
        {
            return Err(invalid("策略参数无效"));
        }
    }
    for r in &data.rules {
        crate::rules::validate_rule(&rule_input(r)).map_err(|e| invalid(&e.message()))?;
        if !r.params.as_object().is_some_and(|o| {
            o.keys().all(|k| {
                [
                    "threshold",
                    "direction",
                    "window",
                    "monthlyPlan",
                    "dueDate",
                    "condition",
                    "expiryDate",
                    "invalidated",
                ]
                .contains(&k.as_str())
            })
        }) {
            return Err(invalid("规则参数字段无效"));
        }
    }
    for s in &data.sources {
        if !crate::information::store::SOURCE_IDS.contains(&s.source_id.as_str()) {
            return Err(invalid("来源不在适配器名单内"));
        }
    }
    for c in &data.channels {
        if !bounded_text(&c.name, 64) {
            return Err(invalid("渠道名称无效"));
        }
        crate::channels::validate_kind(&c.kind)?;
        crate::channels::validate_config(&c.kind, &c.config)?;
    }
    let total: usize = data.watchlists.iter().map(|w| w.items.len()).sum();
    if total > 1000 {
        return Err(invalid("自选最多 1000 条"));
    }
    for w in &data.watchlists {
        if !bounded_text(&w.name, 64) {
            return Err(invalid("自选分组名称无效"));
        }
        for i in &w.items {
            if !bounded_text(&i.name, 128)
                || !valid_security(i)
                || !["stock", "etf", "fund"].contains(&i.asset_type.as_str())
            {
                return Err(invalid("自选证券身份无效"));
            }
        }
    }
    Ok(data)
}
fn read_template(conn: &Connection) -> Result<Value, FinanceError> {
    let mut settings = crate::settings::read(conn)?;
    settings
        .as_object_mut()
        .ok_or_else(|| invalid("配置损坏"))?
        .remove("version");
    let accounts=conn.prepare("SELECT name,sort_order FROM accounts WHERE archived_at IS NULL ORDER BY sort_order,id LIMIT 101")?.query_map([],|r|Ok(Account{name:r.get(0)?,sort_order:r.get(1)?}))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let strategies=conn.prepare("SELECT c.code,c.enabled,COALESCE(o.weight_ppm,c.default_weight_ppm) FROM strategy_catalog c LEFT JOIN strategy_weight_overrides o ON o.code=c.code ORDER BY c.code")?.query_map([],|r|Ok(Strategy{code:r.get(0)?,enabled:r.get(1)?,weight_ppm:r.get(2)?}))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let rules = crate::rules::list_rules(conn, false)
        .map_err(|e| invalid(&e.message()))?
        .into_iter()
        .map(|r| {
            Ok(Rule {
                name: r.name,
                rule_type: r.rule_type,
                object_key: r.object_key,
                params: serde_json::from_str(&r.params_json)
                    .map_err(|_| invalid("规则配置损坏"))?,
                period: r.period,
            })
        })
        .collect::<Result<Vec<_>, FinanceError>>()?;
    let sources = crate::information::store::source_states(conn)?
        .into_iter()
        .map(|s| Source {
            source_id: s.source_id,
            enabled: s.enabled,
        })
        .collect();
    let channels=conn.prepare("SELECT name,kind,config_json FROM notify_channels WHERE archived=0 ORDER BY id LIMIT 101")?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))?.map(|r|{let(name,kind,text)=r?;Ok(Channel{name,kind,config:serde_json::from_str(&text).map_err(|_|invalid("渠道配置损坏"))?})}).collect::<Result<Vec<_>,FinanceError>>()?;
    let groups = conn
        .prepare("SELECT id,name FROM watchlists ORDER BY sort_order,id LIMIT 101")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut watchlists = Vec::new();
    for (id, name) in groups {
        let items=conn.prepare("SELECT symbol,name,asset_type FROM watchlist_items WHERE watchlist_id=?1 ORDER BY sort_order,symbol LIMIT 1001")?.query_map([id],|r|Ok(WatchItem{symbol:r.get(0)?,name:r.get(1)?,asset_type:r.get(2)?}))?.collect::<rusqlite::Result<Vec<_>>>()?;
        watchlists.push(Watchlist { name, items });
    }
    let value = serde_json::to_value(Template {
        format_version: 1,
        generated_at: crate::finance::now_secs(),
        redacted: true,
        modules: [
            "settings",
            "accounts",
            "strategies",
            "rules",
            "sources",
            "channels",
            "watchlists",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        settings,
        accounts,
        strategies,
        rules,
        sources,
        channels,
        watchlists,
    })
    .map_err(|_| invalid("配置无法导出"))?;
    parse(&value)?;
    Ok(value)
}
pub fn export(store: &Store) -> Result<Value, FinanceError> {
    store.with_snapshot(read_template)
}
fn token(value: &Value, revision: &str) -> String {
    let digest = Sha256::digest(format!("{revision}:{}", value).as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}
fn preview_conn(conn: &Connection, value: &Value) -> Result<Value, FinanceError> {
    let data = parse(value)?;
    let revision = Store::data_revision(conn)?.to_string();
    let mut conflicts = Vec::new();
    for a in &data.accounts {
        if conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE name=?1)",
            [&a.name],
            |r| r.get::<_, bool>(0),
        )? {
            conflicts.push(json!({"module":"accounts","name":a.name,"resolution":"keep_existing"}));
        }
    }
    for r in &data.rules {
        if conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM watch_rules WHERE name=?1 AND archived=0)",
            [&r.name],
            |r| r.get::<_, bool>(0),
        )? {
            conflicts.push(json!({"module":"rules","name":r.name,"resolution":"keep_existing"}));
        }
    }
    for c in &data.channels {
        if conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM notify_channels WHERE name=?1 AND kind=?2 AND archived=0)",
            params![c.name, c.kind],
            |r| r.get::<_, bool>(0),
        )? {
            conflicts.push(json!({"module":"channels","name":c.name,"resolution":"keep_existing"}));
        }
    }
    for s in &data.strategies {
        if !crate::strategies::known_code(&s.code) {
            return Err(invalid("策略码不在当前目录内"));
        }
    }
    let channel_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM notify_channels", [], |r| r.get(0))?;
    if channel_count + data.channels.len() as i64 > 100 {
        return Err(invalid("导入后渠道超过 100 个"));
    }
    Ok(
        json!({"previewToken":token(value,&revision),"expectedRevision":revision,"conflicts":conflicts,"counts":{"accounts":data.accounts.len(),"strategies":data.strategies.len(),"rules":data.rules.len(),"sources":data.sources.len(),"channels":data.channels.len(),"watchlists":data.watchlists.len()},"requiresCredentials":data.channels.len(),"importedRulesDisabled":true,"importedChannelsDisabled":true,"ledgerExcluded":true,"displayAndStrategyWillChange":true}),
    )
}
pub fn preview(store: &Store, value: &Value) -> Result<Value, FinanceError> {
    let normalized = crate::template_donor::normalize(value)?;
    let mut result = store.with_snapshot(|c| preview_conn(c, &normalized))?;
    result["template"] = normalized;
    result["donorMapped"] = json!(value.get("formatVersion").is_none());
    Ok(result)
}
pub fn commit(store: &Store, input: &Value) -> Result<Value, FinanceError> {
    if !input.as_object().is_some_and(|o| {
        o.len() == 4
            && ["template", "previewToken", "expectedRevision", "requestId"]
                .iter()
                .all(|k| o.contains_key(*k))
    }) {
        return Err(invalid("导入提交字段无效"));
    }
    let data = parse(&input["template"])?;
    if input["requestId"].as_str().is_none() {
        return Err(invalid("缺少请求身份"));
    }
    crate::config_receipts::write(store, "template-import", input, |conn| {
        let preview = preview_conn(conn, &input["template"])?;
        if preview["previewToken"] != input["previewToken"]
            || preview["expectedRevision"] != input["expectedRevision"]
        {
            return Err(FinanceError::Conflict("配置预览已过期，请重新预览".into()));
        }
        let now = crate::finance::now_secs();
        crate::strategies::ensure_catalog(conn)?;
        let current = crate::settings::read(conn)?;
        let mut settings = data.settings.clone();
        if !settings.is_null() {
            settings["version"] = json!(current["version"]
                .as_i64()
                .ok_or_else(|| invalid("设置版本无效"))?
                .checked_add(1)
                .ok_or_else(|| FinanceError::Overflow("设置版本".into()))?);
            conn.execute("INSERT INTO app_meta(key,value) VALUES('financial_settings',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[settings.to_string()])?;
        }
        for a in &data.accounts {
            conn.execute("INSERT OR IGNORE INTO accounts(name,sort_order,created_at,updated_at) VALUES(?1,?2,?3,?3)",params![a.name,a.sort_order,now])?;
        }
        for s in &data.strategies {
            conn.execute(
                "UPDATE strategy_catalog SET enabled=?2,updated_at=?3 WHERE code=?1",
                params![s.code, s.enabled, now],
            )?;
            conn.execute("INSERT INTO strategy_weight_overrides(code,weight_ppm,updated_at) VALUES(?1,?2,?3) ON CONFLICT(code) DO UPDATE SET weight_ppm=excluded.weight_ppm,updated_at=excluded.updated_at",params![s.code,s.weight_ppm,now])?;
        }
        if !data.strategies.is_empty() {
            conn.execute(
                "UPDATE strategy_config SET revision=revision+1,updated_at=?1 WHERE id=1",
                [&now],
            )?;
        }
        for r in &data.rules {
            if !conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM watch_rules WHERE name=?1 AND archived=0)",
                [&r.name],
                |r| r.get::<_, bool>(0),
            )? {
                crate::rules::create_rule(conn, &rule_input(r))
                    .map_err(|e| invalid(&e.message()))?;
            }
        }
        for s in &data.sources {
            crate::information::store::set_source_enabled(conn, &s.source_id, s.enabled)?;
        }
        for c in &data.channels {
            if !conn.query_row("SELECT EXISTS(SELECT 1 FROM notify_channels WHERE name=?1 AND kind=?2 AND archived=0)",params![c.name,c.kind],|r|r.get::<_,bool>(0))?{
            conn.execute("INSERT INTO notify_channels(name,kind,config_json,secret_refs,enabled,is_default,created_at,updated_at) VALUES(?1,?2,?3,'{}',0,0,?4,?4)",params![c.name,c.kind,c.config.to_string(),now])?;
        }
        }
        for (index, w) in data.watchlists.iter().enumerate() {
            let existing: Option<String> = conn
                .query_row(
                    "SELECT id FROM watchlists WHERE name=?1 ORDER BY id LIMIT 1",
                    [&w.name],
                    |r| r.get(0),
                )
                .optional()?;
            let id = existing.unwrap_or_else(|| {
                format!(
                    "template-{}-{index}",
                    input["requestId"].as_str().unwrap_or_default()
                )
            });
            conn.execute("INSERT OR IGNORE INTO watchlists(id,name,sort_order,created_at) VALUES(?1,?2,?3,?4)",params![id,w.name,index as i64,now])?;
            for (i, item) in w.items.iter().enumerate() {
                conn.execute("INSERT OR IGNORE INTO watchlist_items(watchlist_id,symbol,name,asset_type,sort_order,created_at) VALUES(?1,?2,?3,?4,?5,?6)",params![id,item.symbol,item.name,item.asset_type,i as i64,now])?;
            }
        }
        Ok(
            json!({"ok":true,"conflicts":preview["conflicts"],"requiresCredentials":preview["requiresCredentials"]}),
        )
    })
}

#[cfg(test)]
#[path = "config_templates_tests.rs"]
mod tests;
