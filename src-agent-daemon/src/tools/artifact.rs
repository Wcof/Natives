//! Large-output artifact attachment for the gated tool runtime.

use serde_json::Value;

/// If a tool result is large, persist it as an artifact and replace the
/// in-memory result with an artifact pointer + preview so the engine never
/// keeps an unbounded blob in the transcript.
pub fn attach_tool_output_artifact(run_id: &str, call_id: &str, output: &mut Value) {
    const ARTIFACT_THRESHOLD: usize = 16 * 1024;
    let serialized = output.to_string();
    if serialized.len() < ARTIFACT_THRESHOLD {
        return;
    }
    let preview: String = serialized.chars().take(4_000).collect();
    if let Ok(meta) = crate::artifact_store::global_artifacts().put(
        run_id,
        &format!("tool-{call_id}.json"),
        serialized.as_bytes(),
        Some("application/json".into()),
    ) {
        *output = serde_json::json!({
            "artifact_id": meta.id,
            "preview": preview,
            "truncated": true,
            "bytes": serialized.len(),
        });
    }
}
