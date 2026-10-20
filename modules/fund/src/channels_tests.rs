//! 渠道域隔离回归（R5，S02～S04）：配置校验／SSRF 守卫／CRUD 版本冲突／
//! 九渠道官方协议请求 fixture（逐字段）／静默・重试・去重・投递事实。
//! 不触网（执行器注入）、不写真实 Keychain（Secret 能力传 None 验证拒绝路径）。

use super::*;
use serde_json::json;

fn recording_executor(
    responses: Vec<Result<(u16, String), String>>,
) -> (
    impl Fn(&OutboundRequest) -> ExecuteResult,
    std::rc::Rc<std::cell::RefCell<Vec<OutboundRequest>>>,
) {
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let responses = std::cell::RefCell::new(responses.into_iter());
    let seen_for_closure = seen.clone();
    (
        move |request: &OutboundRequest| {
            seen_for_closure.borrow_mut().push(OutboundRequest {
                url: request.url.clone(),
                headers: request.headers.clone(),
                body: request.body.clone(),
                expect: request.expect,
            });
            responses
                .borrow_mut()
                .next()
                .unwrap_or(Ok((200, "{}".into())))
        },
        seen,
    )
}

#[test]
fn config_validation_rejects_unknown_fields_and_ssrf_urls() {
    let store = Store::open_in_memory().unwrap();
    // 未知类型拒绝。
    assert!(save(&store, &json!({"kind":"slack","name":"x"})).is_err());
    // 配置白名单：telegram 只允许 chat_id。
    assert!(save(
        &store,
        &json!({"kind":"telegram","name":"t","config":{"proxy":"http://x"}})
    )
    .is_err());
    let created = save(
        &store,
        &json!({"kind":"telegram","name":"T","config":{"chat_id":"123"}}),
    )
    .unwrap();
    let id = created["id"].as_i64().unwrap();
    // 更新版本冲突。
    assert!(save(
        &store,
        &json!({"id":id,"kind":"telegram","name":"T2","expectedVersion":99})
    )
    .is_err());
    // 非法 URL：非 https、私网、localhost、metadata。
    for url in [
        "http://api.day.app/x",
        "https://192.168.1.1/push",
        "https://localhost:8766/x",
        "https://169.254.169.254/latest/meta-data",
    ] {
        assert!(
            save(
                &store,
                &json!({"kind":"bark","name":"B","config":{"server_url":url}})
            )
            .is_err(),
            "{url} must be rejected"
        );
    }
    assert!(host_allowed("https://api.day.app/abc"));
    assert!(!host_allowed("https://127.0.0.1/x"));
    assert!(!host_allowed(
        "https://metadata.google.internal/computeMetadata/v1"
    ));
    // 钉钉手机号必须是数字。
    assert!(save(
        &store,
        &json!({"kind":"dingtalk","name":"D","config":{"phones":"138a"}})
    )
    .is_err());
    assert!(save(
        &store,
        &json!({"kind":"dingtalk","name":"D","config":{"phones":"13800138000"}})
    )
    .is_ok());
    assert_eq!(id, 1);
}

#[test]
fn channel_crud_state_versioning_and_default_uniqueness() {
    let store = Store::open_in_memory().unwrap();
    let a = save(&store, &json!({"kind":"wecom","name":"Wecom"})).unwrap();
    let b = save(&store, &json!({"kind":"feishu","name":"Feishu"})).unwrap();
    let (ia, ib) = (a["id"].as_i64().unwrap(), b["id"].as_i64().unwrap());
    // 默认唯一：设 b 默认后 a 不再是默认。
    set_state(
        &store,
        &json!({"id":ib,"isDefault":true,"expectedVersion":1}),
    )
    .unwrap();
    let listed = list(&store).unwrap();
    let defaults = listed["channels"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["isDefault"] == json!(true))
        .count();
    assert_eq!(defaults, 1);
    // 版本推进与冲突。
    assert!(set_state(
        &store,
        &json!({"id":ib,"enabled":false,"expectedVersion":1})
    )
    .is_err());
    set_state(
        &store,
        &json!({"id":ib,"enabled":false,"expectedVersion":2}),
    )
    .unwrap();
    // 归档后列表仍在（恢复可达），归档不改 Secret。
    set_state(
        &store,
        &json!({"id":ib,"archived":true,"expectedVersion":3}),
    )
    .unwrap();
    let listed = list(&store).unwrap();
    assert_eq!(listed["channels"].as_array().unwrap().len(), 2);
    // 删除需要版本匹配，并连带清理投递记录。
    assert!(delete(&store, &json!({"id":ia,"expectedVersion":99})).is_err());
    delete(&store, &json!({"id":ia,"expectedVersion":1})).unwrap();
    assert_eq!(
        list(&store).unwrap()["channels"].as_array().unwrap().len(),
        1
    );
    // Secret 能力不可用时拒绝（不静默成功留下假 configured）。
    assert!(set_secret(
        &store,
        None,
        &json!({"id":ib,"field":"webhook_token","value":"x"})
    )
    .is_err());
}

#[test]
fn build_request_matches_official_protocol_per_channel() {
    let secret_values = |fields: &[&str]| -> Value {
        let mut obj = serde_json::Map::new();
        for f in fields {
            obj.insert((*f).to_string(), json!(format!("SECRET_{f}")));
        }
        Value::Object(obj)
    };
    // telegram：bot{token}/sendMessage + {chat_id,text}。
    let r = build_request(
        "telegram",
        &json!({"chat_id":"42"}),
        &secret_values(&["bot_token"]),
        "t",
        "b",
    )
    .unwrap();
    assert_eq!(
        r.url,
        "https://api.telegram.org/botSECRET_bot_token/sendMessage"
    );
    assert!(r.body.contains("\"chat_id\":\"42\"") && r.body.contains("\"text\":\"t\\nb\""));
    // bark：默认域名 + device key。
    let r = build_request(
        "bark",
        &json!({}),
        &secret_values(&["device_key"]),
        "t",
        "b",
    )
    .unwrap();
    assert_eq!(r.url, "https://api.day.app/SECRET_device_key");
    // 钉钉：access_token + timestamp&sign（HMAC-SHA256 Base64，含 +/= 字符集）。
    let r = build_request(
        "dingtalk",
        &json!({}),
        &secret_values(&["token", "secret"]),
        "t",
        "b",
    )
    .unwrap();
    assert!(r
        .url
        .starts_with("https://oapi.dingtalk.com/robot/send?access_token=SECRET_token&timestamp="));
    let parsed = url::Url::parse(&r.url).unwrap();
    let sign = parsed.query_pairs().find(|(k, _)| k == "sign").unwrap().1;
    assert!(sign
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '='));
    assert!(
        r.body.contains("SECRET_secret") == false,
        "secret must sign, not leak into body"
    );
    let unsigned = build_request(
        "dingtalk",
        &json!({"phones":"13800138000, 13900139000","keyword":"review"}),
        &json!({"token":"fixture-token"}),
        "t",
        "b",
    )
    .unwrap();
    assert_eq!(
        unsigned.url,
        "https://oapi.dingtalk.com/robot/send?access_token=fixture-token"
    );
    let body: Value = serde_json::from_str(&unsigned.body).unwrap();
    assert_eq!(
        body["at"]["atMobiles"],
        json!(["13800138000", "13900139000"])
    );
    assert_eq!(body["at"]["isAtAll"], false);
    assert_eq!(body["text"]["content"], "t\nb review");
    // 企业微信／飞书／Server酱／PushPlus／Discord／Pushover。
    let r = build_request(
        "wecom",
        &json!({}),
        &secret_values(&["webhook_key"]),
        "t",
        "b",
    )
    .unwrap();
    assert!(r
        .url
        .starts_with("https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=SECRET_webhook_key"));
    assert_eq!(
        serde_json::from_str::<Value>(&r.body).unwrap(),
        json!({"msgtype":"text","text":{"content":"t\nb"}})
    );
    let r = build_request(
        "feishu",
        &json!({}),
        &secret_values(&["webhook_token"]),
        "t",
        "b",
    )
    .unwrap();
    assert!(r
        .url
        .starts_with("https://open.feishu.cn/open-apis/bot/v2/hook/SECRET_webhook_token"));
    let r = build_request(
        "serverchan",
        &json!({}),
        &secret_values(&["sendkey"]),
        "t",
        "b",
    )
    .unwrap();
    assert_eq!(r.url, "https://sctapi.ftqq.com/SECRET_sendkey.send");
    let r = build_request(
        "pushplus",
        &json!({"topic":"dev"}),
        &secret_values(&["token"]),
        "t",
        "b",
    )
    .unwrap();
    assert_eq!(r.url, "https://www.pushplus.plus/send");
    assert!(r.body.contains("\"topic\":\"dev\""));
    let r = build_request(
        "discord",
        &json!({"webhook_id":"999"}),
        &secret_values(&["webhook_token"]),
        "t",
        "b",
    )
    .unwrap();
    assert_eq!(
        r.url,
        "https://discord.com/api/v10/webhooks/999/SECRET_webhook_token"
    );
    let r = build_request(
        "pushover",
        &json!({}),
        &secret_values(&["user_key", "app_token"]),
        "t",
        "b",
    )
    .unwrap();
    assert_eq!(r.url, "https://api.pushover.net/1/messages.json");
    assert!(r.body.contains("token=SECRET_app_token") && r.body.contains("user=SECRET_user_key"));
    // 缺 Secret 拒绝。
    assert!(build_request("telegram", &json!({"chat_id":"42"}), &json!({}), "t", "b").is_err());
}

#[test]
fn business_codes_per_channel_upstream_semantics() {
    // HTTP 200 不等于业务成功（方案 10）。
    assert!(business_ok("telegram", 200, "{\"ok\":true}").is_ok());
    assert!(business_ok("telegram", 200, "{\"ok\":false,\"error_code\":400}").is_err());
    assert!(business_ok("dingtalk", 200, "{\"errcode\":310000}").is_err());
    assert!(business_ok("wecom", 200, "{\"errcode\":0}").is_ok());
    assert!(business_ok("feishu", 200, "{\"code\":0}").is_ok());
    assert!(business_ok("serverchan", 200, "{\"code\":400}").is_err());
    assert!(business_ok("pushplus", 200, "{\"code\":200}").is_ok());
    assert!(business_ok("discord", 204, "").is_ok());
    assert!(business_ok("pushover", 200, "{\"status\":1}").is_ok());
    assert!(business_ok("pushover", 200, "{\"status\":0}").is_err());
}

#[test]
fn send_records_deliveries_with_silence_dedup_and_retry() {
    let store = Store::open_in_memory().unwrap();
    let created = save(&store, &json!({"kind":"wecom","name":"W"})).unwrap();
    let id = created["id"].as_i64().unwrap();
    let request =
        build_request("wecom", &json!({}), &json!({"webhook_key":"k"}), "t", "b").unwrap();

    // 默认策略：无静默、无去重、无重试。
    let policy = load_policy(&store);
    let (executor, seen) = recording_executor(vec![Ok((200, "{\"errcode\":0}".into()))]);
    let result = send(
        &store,
        "wecom",
        &request,
        id,
        "evt:1",
        &policy,
        false,
        true,
        || false,
        executor,
    )
    .unwrap();
    assert_eq!(result["status"], json!("sent"));
    assert_eq!(seen.borrow().len(), 1);
    // 投递事实落库。
    let deliveries = deliveries(&store, Some(id), 1).unwrap();
    assert_eq!(deliveries["deliveries"].as_array().unwrap().len(), 1);
    assert_eq!(deliveries["deliveries"][0]["status"], json!("sent"));

    // 去重：1 分钟窗口内同一主体第二发跳过（不触执行器）；策略带 1 次重试供后续用例。
    save_policy(
        &store,
        &json!({"silenceStart":"","silenceEnd":"","retryMax":1,"backoffBase":1,"dedupMinutes":60}),
    )
    .unwrap();
    let policy = load_policy(&store);
    let (executor, seen2) = recording_executor(vec![]);
    let result = send(
        &store,
        "wecom",
        &request,
        id,
        "evt:1",
        &policy,
        false,
        true,
        || false,
        executor,
    )
    .unwrap();
    assert_eq!(result["status"], json!("skipped_dedup"));
    assert_eq!(seen2.borrow().len(), 0);

    // 业务错误 + 1 次重试：两次尝试都失败 → failed，error_code 保留上游语义。
    let (executor, seen3) = recording_executor(vec![
        Ok((200, "{\"errcode\":300001}".into())),
        Ok((200, "{\"errcode\":300001}".into())),
    ]);
    let result = send(
        &store,
        "wecom",
        &request,
        id,
        "evt:2",
        &policy,
        false,
        true,
        || false,
        executor,
    )
    .unwrap();
    assert_eq!(result["status"], json!("failed"));
    assert_eq!(result["errorCode"], json!("upstream_business_error"));
    assert_eq!(
        seen3.borrow().len(),
        2,
        "retry_max=0 default must be raised via policy; policy.retry_max=1 here"
    );
    // 传输失败同样 failed 且注明不可保证未送达。
    let (executor, _) = recording_executor(vec![Err("network: timeout".into())]);
    let result = send(
        &store,
        "wecom",
        &request,
        id,
        "evt:3",
        &policy,
        false,
        true,
        || false,
        executor,
    )
    .unwrap();
    assert_eq!(result["status"], json!("failed"));

    // 静默：非测试发送在静默时段跳过；显式绕过（测试）仍可发。
    let silent_policy = SendPolicy {
        silence_start: Some("00:00".into()),
        silence_end: Some("23:59".into()),
        retry_max: 0,
        backoff_base: 1,
        dedup_minutes: 0,
    };
    let (executor, seen4) = recording_executor(vec![]);
    let result = send(
        &store,
        "wecom",
        &request,
        id,
        "evt:4",
        &silent_policy,
        false,
        false,
        || false,
        executor,
    )
    .unwrap();
    assert_eq!(result["status"], json!("skipped_silent"));
    assert_eq!(seen4.borrow().len(), 0);
    let (executor, seen5) = recording_executor(vec![Ok((200, "{\"errcode\":0}".into()))]);
    let result = send(
        &store,
        "wecom",
        &request,
        id,
        "evt:5",
        &silent_policy,
        true,
        true,
        || false,
        executor,
    )
    .unwrap();
    assert_eq!(result["status"], json!("sent"));
    assert_eq!(seen5.borrow().len(), 1);

    // 取消：发送前取消 → cancelled。
    let (executor, seen6) = recording_executor(vec![]);
    let result = send(
        &store,
        "wecom",
        &request,
        id,
        "evt:6",
        &policy,
        false,
        true,
        || true,
        executor,
    )
    .unwrap();
    assert_eq!(result["status"], json!("cancelled"));
    assert_eq!(seen6.borrow().len(), 0);
}

#[test]
fn silence_window_supports_cross_midnight() {
    let policy = |start: &str, end: &str| SendPolicy {
        silence_start: Some(start.into()),
        silence_end: Some(end.into()),
        retry_max: 0,
        backoff_base: 1,
        dedup_minutes: 0,
    };
    // 同日区间。
    assert!(in_silence(&policy("09:00", "18:00"), "10:00"));
    assert!(!in_silence(&policy("09:00", "18:00"), "08:59"));
    // 跨午夜。
    assert!(in_silence(&policy("23:00", "07:00"), "23:30"));
    assert!(in_silence(&policy("23:00", "07:00"), "03:00"));
    assert!(!in_silence(&policy("23:00", "07:00"), "12:00"));
    // 未配置 = 不静默。
    let open = SendPolicy {
        silence_start: None,
        silence_end: None,
        retry_max: 0,
        backoff_base: 1,
        dedup_minutes: 0,
    };
    assert!(!in_silence(&open, "12:00"));
    // 策略有界：重试上限 3、退避 1~30、去重 ≤1440。
    let store = Store::open_in_memory().unwrap();
    assert!(save_policy(
        &store,
        &json!({"silenceStart":"","silenceEnd":"","retryMax":4,"backoffBase":1,"dedupMinutes":0})
    )
    .is_err());
    assert!(save_policy(&store, &json!({"silenceStart":"09:00","silenceEnd":"","retryMax":0,"backoffBase":1,"dedupMinutes":0})).is_err());
    save_policy(&store, &json!({"silenceStart":"","silenceEnd":"","retryMax":3,"backoffBase":30,"dedupMinutes":1440})).unwrap();
    let policy = load_policy(&store);
    assert_eq!(policy.retry_max, 3);
    assert_eq!(policy.backoff_base, 30);
    assert_eq!(policy.dedup_minutes, 1440);
}

#[test]
fn send_errors_never_persist_upstream_echoes_and_backoff_is_cancellable() {
    let store = Store::open_in_memory().unwrap();
    let id = save(&store, &json!({"kind":"wecom","name":"isolated"})).unwrap()["id"]
        .as_i64()
        .unwrap();
    let request = build_request(
        "wecom",
        &json!({}),
        &json!({"webhook_key":"SECRET"}),
        "t",
        "b",
    )
    .unwrap();
    let policy = SendPolicy {
        silence_start: None,
        silence_end: None,
        retry_max: 0,
        backoff_base: 30,
        dedup_minutes: 60,
    };
    send(
        &store,
        "wecom",
        &request,
        id,
        "reject",
        &policy,
        false,
        true,
        || false,
        |_| {
            Ok((
                200,
                r#"{"errcode":1,"errmsg":"SECRET https://private-url"}"#.into(),
            ))
        },
    )
    .unwrap();
    send(
        &store,
        "wecom",
        &request,
        id,
        "transport",
        &policy,
        false,
        true,
        || false,
        |_| Err("SECRET https://private-url".into()),
    )
    .unwrap();
    let history = deliveries(&store, Some(id), 1).unwrap().to_string();
    assert!(!history.contains("SECRET") && !history.contains("private-url"));
    assert!(deliveries(&store, None, i64::MAX).is_err());
    let cancelled = std::cell::Cell::new(false);
    let start = std::time::Instant::now();
    let result = send(
        &store,
        "wecom",
        &request,
        id,
        "cancel",
        &SendPolicy {
            retry_max: 3,
            ..policy
        },
        false,
        true,
        || cancelled.get(),
        |_| {
            cancelled.set(true);
            Err("network".into())
        },
    )
    .unwrap();
    assert_eq!(result["status"], "cancelled");
    assert!(start.elapsed() < std::time::Duration::from_millis(200));
    let form = build_request(
        "pushover",
        &json!({}),
        &json!({"user_key":"u&x","app_token":"t+z"}),
        "a&b=中文",
        "hello+world",
    )
    .unwrap();
    let decoded: std::collections::HashMap<_, _> =
        url::form_urlencoded::parse(form.body.as_bytes())
            .into_owned()
            .collect();
    assert_eq!(decoded["title"], "a&b=中文");
    assert_eq!(decoded["token"], "t+z");
    assert_eq!(decoded["message"], "hello+world");
}
