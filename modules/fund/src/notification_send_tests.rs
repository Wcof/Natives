//! mode/T03 回归（审计更正版）：完整类型化命令、确认/凭据/Keychain
//! locked/取消/事实重验拒绝路径与执行器调用计数（A08/A09/A10）。
//! 执行器注入（假渠道）＋SecretReveal 假实现，不触网不触 Keychain。

use super::*;
use crate::channels::{self, ExecuteResult, OutboundRequest};
use crate::storage::Store;
use serde_json::json;

type SeenRef = std::rc::Rc<std::cell::RefCell<Vec<OutboundRequest>>>;

fn recording_executor(
    responses: Vec<Result<(u16, String), String>>,
) -> (impl Fn(&OutboundRequest) -> ExecuteResult, SeenRef) {
    let seen: SeenRef = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let queue = std::cell::RefCell::new(responses.into_iter());
    let for_closure = seen.clone();
    (
        move |request: &OutboundRequest| {
            for_closure.borrow_mut().push(OutboundRequest {
                url: request.url.clone(),
                headers: request.headers.clone(),
                body: request.body.clone(),
                expect: request.expect,
            });
            queue.borrow_mut().next().unwrap_or(Ok((200, "{}".into())))
        },
        seen,
    )
}

/// 假 Secret 能力：可控返回（locked / 缺凭据 / 注入值）。
struct FakeSecrets {
    values: std::collections::HashMap<String, String>,
    locked: bool,
}

impl FakeSecrets {
    fn with_value(account: &str, value: &str) -> Self {
        let mut values = std::collections::HashMap::new();
        values.insert(account.to_owned(), value.to_owned());
        Self {
            values,
            locked: false,
        }
    }
    fn locked() -> Self {
        Self {
            values: std::collections::HashMap::new(),
            locked: true,
        }
    }
}

impl SecretReveal for FakeSecrets {
    fn reveal_secret(&self, account: &str) -> Result<Option<String>, (String, bool)> {
        if self.locked {
            return Err(("KEYCHAIN_LOCKED".into(), true));
        }
        Ok(self.values.get(account).cloned())
    }
}

fn wecom_channel(store: &Store) -> i64 {
    let created =
        channels::save(store, &json!({"kind":"wecom","name":"渠道","config":{}})).unwrap();
    created["id"].as_i64().unwrap()
}

fn report_subject(store: &Store) {
    store
        .with_write(|c| {
            c.execute(
                "INSERT INTO analysis_reports(task,scope_json,evidence_json,snapshot_id,data_revision,result_text,generated_at,saved_at) VALUES('diagnosis','{}','{}','snapshot','1','PRIVATE 99999 account','1','1')",
                [],
            )?;
            Ok::<_, crate::finance::FinanceError>(())
        })
        .unwrap();
}

#[test]
fn command_from_raw_types_full_business_facts_and_rejects_illegal_types() {
    // A08：业务命令完整携带 subject/token/requestId；非法类型 400。
    let ok = NotifyCommand::from_raw(
        &json!({"id":1,"subject":"report","subjectId":7,"language":"en","previewToken":"t","requestId":"r"}),
        true,
    )
    .unwrap();
    match &ok {
        NotifyCommand::Business(send) => {
            assert_eq!(send.channel_id, 1);
            assert_eq!(send.subject, NotifySubject::Report(7));
            assert_eq!(send.language, NotifyLanguage::En);
            assert_eq!(send.preview_token, "t");
            assert_eq!(send.request_id, "r");
        }
        _ => panic!("应为 Business 命令"),
    }
    // candidate：字符串身份入类型。
    let candidate = NotifyCommand::from_raw(
        &json!({"id":1,"subject":"candidate","subjectId":"c1","snapshotId":"s1","previewToken":"t","requestId":"r"}),
        true,
    )
    .unwrap();
    assert!(matches!(
        candidate,
        NotifyCommand::Business(BusinessSend {
            subject: NotifySubject::Candidate { .. },
            ..
        })
    ));
    // 非法类型：id 字符串、subjectId 对象、subject 未知、空 token/requestId。
    for raw in [
        json!({"id":"1","subject":"report","subjectId":1,"previewToken":"t","requestId":"r"}),
        json!({"id":1,"subject":"report","subjectId":{},"previewToken":"t","requestId":"r"}),
        json!({"id":1,"subject":"newsletter","previewToken":"t","requestId":"r"}),
        json!({"id":1,"subject":"report","subjectId":1,"previewToken":"","requestId":"r"}),
        json!({"id":1,"subject":"report","subjectId":1,"previewToken":"t","requestId":""}),
    ] {
        let err = NotifyCommand::from_raw(&raw, true).unwrap_err();
        assert!(
            matches!(err, NotifyError::Invalid { .. }),
            "应 Invalid: {raw}"
        );
    }
    // 测试入口白名单与缺省。
    let ok = NotifyCommand::from_raw(&json!({"id":2,"title":"t","requestId":"r2"}), false).unwrap();
    assert_eq!(ok.channel_id(), 2);
    let err = NotifyCommand::from_raw(&json!({"id":1,"title":"t","extra":1}), false).unwrap_err();
    assert!(matches!(err, NotifyError::Invalid { .. }));
}

#[test]
fn business_send_requires_confirmation_and_rejects_forgery_without_side_effects() {
    // A09：伪造 token → Conflict；执行器 0 次。
    let store = Store::open_in_memory().unwrap();
    let id = wecom_channel(&store);
    report_subject(&store);
    let (execute, seen) = recording_executor(vec![]);
    let unconfirmed = NotifyCommand::from_raw(
        &json!({"id":id,"subject":"report","subjectId":1,"language":"zh_CN","previewToken":"forged","requestId":"r1"}),
        true,
    )
    .unwrap();
    let err = send_with_executor(&store, None, &unconfirmed, || false, &execute).unwrap_err();
    assert!(
        matches!(err, NotifyError::Conflict { .. }),
        "应 Conflict: {err:?}"
    );
    assert!(seen.borrow().is_empty());
}

#[test]
fn missing_disabled_channel_and_cancel_reject_before_side_effects() {
    let store = Store::open_in_memory().unwrap();
    let id = wecom_channel(&store);
    let (execute, seen) = recording_executor(vec![]);
    let raw = json!({"id":9999,"title":"t","body":"b","requestId":"r"});
    let cmd = NotifyCommand::from_raw(&raw, false).unwrap();
    let err = send_with_executor(&store, None, &cmd, || false, &execute).unwrap_err();
    assert!(matches!(err, NotifyError::NotFound), "应 NotFound: {err:?}");
    // 取消：既有 cancelled 回执，无外发（渠道仍启用；取消检查先于 secrets）。
    let existing = NotifyCommand::from_raw(
        &json!({"id":id,"title":"t","body":"b","requestId":"r2"}),
        false,
    )
    .unwrap();
    let ok = send_with_executor(&store, None, &existing, || true, &execute).unwrap();
    assert_eq!(ok["status"], "cancelled");
    channels::set_state(
        &store,
        &json!({"id":id,"enabled":false,"expectedVersion":1,"requestId":"st"}),
    )
    .unwrap();
    let err = send_with_executor(&store, None, &existing, || false, &execute).unwrap_err();
    assert!(
        matches!(err, NotifyError::Conflict { .. }),
        "停用后应 Conflict: {err:?}"
    );
    assert!(seen.borrow().is_empty());
}

#[test]
fn fact_change_between_prepare_and_execute_is_rejected() {
    // A09：准备到执行之间渠道版本变化 → Conflict，执行器 0 次。
    let store = Store::open_in_memory().unwrap();
    let id = wecom_channel(&store);
    let (execute, seen) = recording_executor(vec![]);
    let raw = json!({"id":id,"title":"t","body":"b","requestId":"r"});
    let cmd = NotifyCommand::from_raw(&raw, false).unwrap();
    let mutated = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let shared = std::sync::Arc::new(store);
    let hook = {
        let store = shared.clone();
        let mutated = mutated.clone();
        move || {
            if !mutated.swap(true, std::sync::atomic::Ordering::SeqCst) {
                store
                    .with_write(|c: &rusqlite::Connection| -> rusqlite::Result<()> {
                        c.execute(
                            "UPDATE notify_channels SET version=version+1 WHERE id=?1",
                            [id],
                        )?;
                        Ok(())
                    })
                    .unwrap();
            }
            false
        }
    };
    let err = send_with_executor(&shared, None, &cmd, hook, &execute).unwrap_err();
    assert!(
        matches!(err, NotifyError::Conflict { .. }),
        "应 Conflict，实际 {err:?}"
    );
    assert!(err.message().contains("渠道已变化"));
    assert!(seen.borrow().is_empty(), "事实变化后不得外发");
}

#[test]
fn missing_credential_and_keychain_locked_fail_honestly_with_zero_sends() {
    // A09 更正：缺凭据与 Keychain locked → SecretUnavailable，执行器 0 次。
    let store = Store::open_in_memory().unwrap();
    let created = channels::save(
        &store,
        &json!({"kind":"telegram","name":"TG","config":{"chat_id":"123"}}),
    )
    .unwrap();
    let id = created["id"].as_i64().unwrap();
    let raw = json!({"id":id,"title":"Natives","body":"连通性测试","requestId":"r"});
    let cmd = NotifyCommand::from_raw(&raw, false).unwrap();
    // 缺凭据（无 Secret 能力）。
    let (execute, seen) = recording_executor(vec![]);
    let err = send_with_executor(&store, None, &cmd, || false, &execute).unwrap_err();
    assert!(
        matches!(err, NotifyError::SecretUnavailable { .. }),
        "{err:?}"
    );
    assert!(err.message().contains("bot_token"));
    assert!(seen.borrow().is_empty());
    // Keychain locked（注入假实现）。
    let (execute, seen) = recording_executor(vec![]);
    let err = send_with_executor(
        &store,
        Some(&FakeSecrets::locked()),
        &cmd,
        || false,
        &execute,
    )
    .unwrap_err();
    assert!(
        matches!(err, NotifyError::SecretUnavailable { .. }),
        "{err:?}"
    );
    assert!(
        err.message().contains("KEYCHAIN_LOCKED"),
        "{}",
        err.message()
    );
    assert!(seen.borrow().is_empty());
}

#[test]
fn test_send_with_injected_secrets_and_executor_records_delivery_once() {
    // A09：模拟渠道（注入 Secret + 执行器）成功 → sent、执行器 1 次、投递事实。
    let store = Store::open_in_memory().unwrap();
    let created = channels::save(
        &store,
        &json!({"kind":"telegram","name":"TG","config":{"chat_id":"123"}}),
    )
    .unwrap();
    let id = created["id"].as_i64().unwrap();
    let raw = json!({"id":id,"title":"Natives","body":"连通性测试","requestId":"r"});
    let cmd = NotifyCommand::from_raw(&raw, false).unwrap();
    let secrets = FakeSecrets::with_value(&format!("channel:{id}:bot_token"), "fixture-token");
    let (execute, seen) = recording_executor(vec![Ok((200, "{\"ok\":true}".into()))]);
    let ok = send_with_executor(&store, Some(&secrets), &cmd, || false, &execute).unwrap();
    assert_eq!(ok["status"], "sent");
    assert_eq!(seen.borrow().len(), 1, "确认一次只调用一次渠道");
    let deliveries = channels::deliveries(&store, Some(id), 1).unwrap();
    assert_eq!(deliveries["deliveries"][0]["status"], "sent");
}

#[test]
fn business_confirmed_command_carries_full_typed_facts() {
    // A08：确认后的业务命令直接携带全部事实（不再透传 raw JSON）；
    // 确定性重建恰为 7 字段契约。
    let store = Store::open_in_memory().unwrap();
    let id = wecom_channel(&store);
    report_subject(&store);
    let preview = crate::notification_subjects::preview(
        &store,
        &json!({"id":id,"subject":"report","subjectId":1}),
    )
    .unwrap();
    let cmd = NotifyCommand::from_raw(
        &json!({
            "id": id,
            "subject": "report",
            "subjectId": 1,
            "language": "zh_CN",
            "previewToken": preview["previewToken"],
            "requestId": "r1",
        }),
        true,
    )
    .unwrap();
    match &cmd {
        NotifyCommand::Business(send) => {
            assert_eq!(send.request_id, "r1");
            assert_eq!(send.subject, NotifySubject::Report(1));
            // report 无 snapshotId → 6 键；candidate 恰为 7 键（白名单全集）。
            assert_eq!(send.fact_input().as_object().unwrap().keys().count(), 6);
        }
        _ => panic!("应为 Business"),
    }
}
