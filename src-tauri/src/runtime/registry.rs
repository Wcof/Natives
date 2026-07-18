//! runtime/registry.rs — Runtime 注册表 + 分流逻辑
//!
//! `resolve_runtime` 按优先级 Claude CLI > Codex CLI > Native 自动分流；
//! 显式 override 不可用时抛错而非静默降级；降级到 Native 时返回 hint 字符串。

#![allow(dead_code, unused_imports, unused_variables)]
use super::AgentRuntime;
use crate::Result;
use std::sync::Arc;
use tokio::sync::RwLock;

lazy_static::lazy_static! {
    static ref REGISTRY: RwLock<Vec<Arc<dyn AgentRuntime>>> = RwLock::new(vec![]);
}

/// 注册一个 runtime（在 lib.rs setup 钩子里调用）
pub async fn register(runtime: Arc<dyn AgentRuntime>) {
    let mut all = REGISTRY.write().await;
    if all.iter().any(|r| r.id() == runtime.id()) {
        return;
    }
    all.push(runtime);
}

/// 按优先级 Claude CLI > Codex CLI > Native 分流。
pub async fn resolve_runtime(
    override_id: Option<&str>,
) -> Result<(Arc<dyn AgentRuntime>, Option<String>)> {
    let all = REGISTRY.read().await;
    let order = ["claude_cli", "codex_cli", "native"];

    if let Some(id) = override_id {
        if let Some(rt) = all.iter().find(|r| r.id() == id && r.is_available()) {
            return Ok((rt.clone(), None));
        }
        return Err(crate::Error::InvalidInput(format!(
            "Runtime '{}' is explicitly requested but not available",
            id
        )));
    }

    for id in order {
        if let Some(rt) = all.iter().find(|r| r.id() == id && r.is_available()) {
            let downgrade_hint = if id == "native" {
                Some("未检测到 Claude/Codex CLI，已降级为内置引擎，建议安装以获得更好体验".into())
            } else {
                None
            };
            return Ok((rt.clone(), downgrade_hint));
        }
    }
    Err(crate::Error::Internal(
        "no runtime available (native should always be registered)".into(),
    ))
}

/// 列出所有已注册 runtime 的元信息
pub async fn list_runtime_metadata() -> Vec<RuntimeMetadata> {
    let all = REGISTRY.read().await;
    all.iter()
        .map(|r| RuntimeMetadata {
            id: r.id().to_string(),
            display_name: r.display_name().to_string(),
            available: r.is_available(),
        })
        .collect()
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMetadata {
    pub id: String,
    pub display_name: String,
    pub available: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use crate::runtime::{AgentRuntime, EventStream, RuntimeStreamOptions};

    /// 序列化所有 registry 测试——避免全局 REGISTRY 并发污染
    static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    struct StubRuntime {
        id_str: &'static str,
        available: bool,
    }

    #[async_trait]
    impl AgentRuntime for StubRuntime {
        fn id(&self) -> &'static str { self.id_str }
        fn display_name(&self) -> &'static str { self.id_str }
        fn is_available(&self) -> bool { self.available }
        async fn stream(&self, _options: RuntimeStreamOptions) -> crate::Result<EventStream> {
            unimplemented!("stub")
        }
        fn interrupt(&self, _session_id: &str) {}
        fn dispose(&self) {}
    }

    async fn setup_registry(rts: Vec<StubRuntime>) {
        let mut all = REGISTRY.write().await;
        all.clear();
        for rt in rts {
            all.push(Arc::new(rt));
        }
    }

    #[tokio::test]
    async fn resolve_native_returns_downgrade_hint_when_no_cli() {
        let _g = TEST_LOCK.lock().await;
        setup_registry(vec![StubRuntime { id_str: "native", available: true }]).await;
        let (rt, hint) = resolve_runtime(None).await.unwrap();
        assert_eq!(rt.id(), "native");
        assert!(hint.is_some(), "降级到 Native 必须返回提示");
    }

    #[tokio::test]
    async fn resolve_prefers_claude_cli_when_available() {
        let _g = TEST_LOCK.lock().await;
        setup_registry(vec![
            StubRuntime { id_str: "native", available: true },
            StubRuntime { id_str: "codex_cli", available: true },
            StubRuntime { id_str: "claude_cli", available: true },
        ]).await;
        let (rt, hint) = resolve_runtime(None).await.unwrap();
        assert_eq!(rt.id(), "claude_cli");
        assert!(hint.is_none(), "非降级时 hint 为 None");
    }

    #[tokio::test]
    async fn resolve_prefers_codex_cli_when_claude_unavailable() {
        let _g = TEST_LOCK.lock().await;
        setup_registry(vec![
            StubRuntime { id_str: "native", available: true },
            StubRuntime { id_str: "codex_cli", available: true },
            StubRuntime { id_str: "claude_cli", available: false },
        ]).await;
        let (rt, hint) = resolve_runtime(None).await.unwrap();
        assert_eq!(rt.id(), "codex_cli");
        assert!(hint.is_none());
    }

    #[tokio::test]
    async fn resolve_override_unavailable_throws_error() {
        let _g = TEST_LOCK.lock().await;
        setup_registry(vec![
            StubRuntime { id_str: "native", available: true },
            StubRuntime { id_str: "claude_cli", available: false },
        ]).await;
        let result = resolve_runtime(Some("claude_cli")).await;
        assert!(result.is_err(), "显式指定不可用 runtime 必须报错而非静默降级");
    }

    #[tokio::test]
    async fn resolve_override_available_returns_it() {
        let _g = TEST_LOCK.lock().await;
        setup_registry(vec![
            StubRuntime { id_str: "native", available: true },
            StubRuntime { id_str: "codex_cli", available: true },
            StubRuntime { id_str: "claude_cli", available: true },
        ]).await;
        let (rt, hint) = resolve_runtime(Some("codex_cli")).await.unwrap();
        assert_eq!(rt.id(), "codex_cli");
        assert!(hint.is_none(), "显式指定非降级");
    }

    #[tokio::test]
    async fn register_dedup_by_id() {
        let _g = TEST_LOCK.lock().await;
        setup_registry(vec![StubRuntime { id_str: "native", available: true }]).await;
        register(Arc::new(StubRuntime { id_str: "native", available: true })).await;
        let all = REGISTRY.read().await;
        assert_eq!(all.len(), 1, "重复 id 不应被注册两次");
    }
}
