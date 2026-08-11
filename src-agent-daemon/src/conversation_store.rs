use crate::storage::DataStore;
use assistant_protocol::v2::methods::names;
use rusqlite::{params, OptionalExtension};
use serde_json::Value;
#[path = "conversation_context.rs"]
mod conversation_context;
#[path = "conversation_fork.rs"]
mod conversation_fork;
#[path = "conversation_messages.rs"]
mod conversation_messages;
#[path = "conversation_store_delete.rs"]
mod conversation_store_delete;
#[path = "conversation_store_mutate.rs"]
mod conversation_store_mutate;
#[path = "conversation_store_query.rs"]
mod conversation_store_query;
#[path = "conversation_store_queue.rs"]
mod conversation_store_queue;
#[path = "conversation_store_search.rs"]
mod conversation_store_search;
#[path = "conversation_store_stub.rs"]
mod conversation_store_stub;

pub use conversation_context::{
    backfill_context_snapshots, load_active_context_messages, load_active_context_snapshot,
    load_active_context_snapshot_for_checkpoint, ActiveContextSnapshot,
};
pub(crate) use conversation_context::{
    block_image, block_text, latest_context_summary, parse_content_block, parse_tool_result_blocks,
    reasoning_block_from_events,
};
pub(crate) use conversation_fork::fork;
pub use conversation_messages::{
    append_agent_message, append_trigger_message, delete_message, engine_history,
    load_agent_messages,
};
pub(crate) use conversation_messages::{append_message, get_messages, get_messages_page};
pub(crate) use conversation_store_delete::delete;
pub(crate) use conversation_store_mutate::{
    archive, create, rename, update_model, update_permission,
};
pub use conversation_store_query::permission_profile;
pub(crate) use conversation_store_query::{get, list, list_page};
pub use conversation_store_queue::persist_queued_input_and_ack;
pub use conversation_store_search::search_messages;
pub use conversation_store_stub::ensure_conversation_stub;

#[cfg(test)]
#[path = "conversation_store_tests.rs"]
mod tests;

/// Max bytes of attachment content inlined into the model context (T209).
/// Oversized attachments degrade to an explicit marker instead of being read.
const MAX_ATTACHMENT_BYTES: usize = 256 * 1024;

/// Standard base64 engine for attachment data URLs.
fn base64_engine() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

pub async fn request(method: &str, params: Value) -> Result<Value, String> {
    match method {
        names::CONVERSATION_CREATE => create(params),
        names::CONVERSATION_LIST => list(params),
        "conversation.listPage" => list_page(params),
        names::CONVERSATION_GET => get(params),
        names::CONVERSATION_FORK => fork(params),
        names::CONVERSATION_GET_MESSAGES => get_messages(params),
        "conversation.getMessagesPage" => get_messages_page(params),
        names::CONVERSATION_APPEND_MESSAGE => append_message(params),
        names::CONVERSATION_RENAME => rename(params),
        names::CONVERSATION_UPDATE_MODEL => update_model(params),
        names::CONVERSATION_UPDATE_PERMISSION => update_permission(params),
        names::CONVERSATION_ARCHIVE => archive(params),
        names::CONVERSATION_DELETE => delete(params).await,
        _ => Err(format!("unsupported conversation method: {method}")),
    }
}

/// Public wrapper for internal message append (used by subagent_store).
pub fn append_message_public(params: Value) -> Result<Value, String> {
    append_message(params)
}

pub(crate) fn store() -> Result<DataStore, String> {
    // W2: single Daemon DataStore open path (assistant.db authority + test hook).
    crate::storage::open_daemon_store()
}

fn id_param(params: &Value) -> Result<&str, String> {
    params
        .get("id")
        .or_else(|| params.get("conversation_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| "id is required".into())
}

fn required_str<'a>(params: &'a Value, key: &str) -> Result<&'a str, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{key} is required"))
}

#[cfg(test)]
pub(crate) mod test_support {
    pub(crate) fn env_lock() -> crate::storage::EnvTestGuard {
        crate::storage::DataStore::env_test_lock()
    }

    pub(crate) struct ClearTestDb;
    impl Drop for ClearTestDb {
        fn drop(&mut self) {
            crate::storage::set_test_db_override(None, None);
        }
    }

    pub(crate) struct EnvRestore {
        pub(crate) db: Option<String>,
        pub(crate) asst: Option<String>,
        pub(crate) rt: Option<String>,
    }
    impl Drop for EnvRestore {
        fn drop(&mut self) {
            match self.db.take() {
                Some(v) => std::env::set_var("NATIVES_DB_PATH", v),
                None => std::env::remove_var("NATIVES_DB_PATH"),
            }
            match self.asst.take() {
                Some(v) => std::env::set_var("NATIVES_ASSISTANT_DB_PATH", v),
                None => std::env::remove_var("NATIVES_ASSISTANT_DB_PATH"),
            }
            match self.rt.take() {
                Some(v) => std::env::set_var("NATIVES_RUNTIME_DIR", v),
                None => std::env::remove_var("NATIVES_RUNTIME_DIR"),
            }
        }
    }
}
