//! Bounded idempotency receipts for non-secret configuration transactions.
use crate::{finance::FinanceError, storage::Store};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
pub(crate) fn replay(
    store: &Store,
    namespace: &str,
    input: &Value,
) -> Result<Option<Value>, FinanceError> {
    let Some(id) = input["requestId"].as_str().filter(|s| {
        !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    }) else {
        return Ok(None);
    };
    store.with_read(|conn| {
        let text: Option<String> = conn
            .query_row(
                "SELECT value FROM app_meta WHERE key=?1",
                [format!("config_op:{namespace}:{id}")],
                |r| r.get(0),
            )
            .optional()?;
        match text {
            None => Ok(None),
            Some(text) => {
                let receipt: Value = serde_json::from_str(&text)
                    .map_err(|_| FinanceError::Invalid("配置回执损坏".into()))?;
                if receipt["input"] != *input {
                    return Err(FinanceError::Conflict("请求身份已使用".into()));
                }
                Ok(Some(receipt["result"].clone()))
            }
        }
    })
}
pub(crate) fn write(
    store: &Store,
    namespace: &str,
    input: &Value,
    work: impl FnOnce(&Connection) -> Result<Value, FinanceError>,
) -> Result<Value, FinanceError> {
    if let Some(result) = replay(store, namespace, input)? {
        return Ok(result);
    }
    let key = if let Some(id) = input.get("requestId") {
        let id = id
            .as_str()
            .filter(|s| {
                !s.is_empty()
                    && s.len() <= 128
                    && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
            .ok_or_else(|| FinanceError::Invalid("请求身份无效".into()))?;
        Some(format!("config_op:{namespace}:{id}"))
    } else {
        None
    };
    store.with_write(|conn|{
        if let Some(key)=key.as_ref(){
            let text:Option<String>=conn.query_row("SELECT value FROM app_meta WHERE key=?1",[key],|r|r.get(0)).optional()?;
            if let Some(text)=text{
                let receipt:Value=serde_json::from_str(&text).map_err(|_|FinanceError::Invalid("配置回执损坏".into()))?;
                if receipt["input"]!=*input{return Err(FinanceError::Conflict("请求身份已使用".into()));}
                return Ok(receipt["result"].clone());
            }
        }
        let result=work(conn)?;
        if let Some(key)=key{
            conn.execute("INSERT INTO app_meta(key,value) VALUES(?1,?2)",params![key,json!({"input":input,"result":result}).to_string()])?;
            conn.execute("DELETE FROM app_meta WHERE key IN(SELECT key FROM app_meta WHERE key LIKE 'config_op:%' ORDER BY rowid DESC LIMIT -1 OFFSET 200)",[])?;
        }
        Ok(result)
    })
}
