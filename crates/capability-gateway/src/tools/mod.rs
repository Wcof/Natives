//! Built-in tool implementations for the capability gateway.

mod apply_patch_parser;
pub mod creative_draft;
mod extra;
mod fs;
pub mod plan;
pub mod proposal;
mod ssrf;
mod terminal;
mod todo;
mod web;
pub mod web_search;

pub use apply_patch_parser::{parse_patch_input, PatchOp};
pub use creative_draft::{creative_draft_tools, creative_handoff_tool, CREATIVE_DRAFT_TOOL_NAMES};
pub use plan::plan_mode_tools;
pub use proposal::creative_proposal_tool;
pub use ssrf::validate_fetch_url;
pub use web_search::{web_search_tool, SearchBackend, SearchProvider};

use crate::{PathScope, PermissionClass, SideEffect, Tool};
use std::sync::Arc;

use fs::{EditFileTool, GrepTool, ListDirTool, ReadFileTool, SearchFilesTool, WriteFileTool};
use terminal::RunTerminalTool;
use todo::TodoWriteTool;
use web::WebFetchTool;

pub fn builtin_tools() -> Vec<Tool> {
    vec![
        // Ordinary-assistant handoff (batch 3): creating a draft is benign and
        // draft-scoped, so it ships in the default surface; the creative-session
        // writing tools stay out of it.
        creative_handoff_tool(),
        // Agent creative proposal (batch 10 CR-1001): structurally validates and
        // forwards an app-creation/start proposal for user approval. The Host
        // gate is the real security boundary; this tool never registers anything.
        creative_proposal_tool(),
        Tool {
            name: "read_file",
            description: "Read a text file (supports offset/limit; binary returns metadata only)",
            schema: serde_json::json!({
                "type":"object",
                "properties":{
                    "path":{"type":"string"},
                    "offset":{"type":"integer","minimum":0},
                    "limit":{"type":"integer","minimum":1},
                    "max_chars":{"type":"integer","minimum":1}
                },
                "required":["path"]
            }),
            side_effect: SideEffect::ReadOnly,
            permission_class: PermissionClass::ProjectRead,
            path_scope: PathScope::Any,
            timeout_ms: 10000,
            output_limit: 1_048_576,
            cancellable: true,
            parallel_safe: true,
            conflict_key: None,
            idempotency: None,
        per_call_resource: None,
        handler: Arc::new(ReadFileTool),
        },
        Tool {
            name: "search_files",
            description: "Search for files matching a pattern",
            schema: serde_json::json!({"type":"object","properties":{"pattern":{"type":"string"},"root":{"type":"string"}},"required":["pattern"]}),
            side_effect: SideEffect::ReadOnly,
            permission_class: PermissionClass::ProjectRead,
            path_scope: PathScope::Any,
            timeout_ms: 30000,
            output_limit: 1_048_576,
            cancellable: true,
            parallel_safe: true,
            conflict_key: None,
            idempotency: None,
        per_call_resource: None,
        handler: Arc::new(SearchFilesTool),
        },
        Tool {
            name: "write_file",
            description: "Write content to a file (legacy; prefer apply_patch for new runs)",
            schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]}),
            side_effect: SideEffect::Write,
            permission_class: PermissionClass::ProjectWrite,
            path_scope: PathScope::Any,
            timeout_ms: 10000,
            output_limit: 1_048_576,
            cancellable: true,
            parallel_safe: false,
            // ponytail: global project-write lease; derive a project-scoped key when the contract supports it.
            conflict_key: Some("project-write".into()),
            idempotency: None,
        per_call_resource: None,
        handler: Arc::new(WriteFileTool),
        },
        Tool {
            name: "list_dir",
            description: "List directory entries (stable sort, cursor pagination)",
            schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"limit":{"type":"integer"},"cursor":{"type":"string"}},"required":[]}),
            side_effect: SideEffect::ReadOnly,
            permission_class: PermissionClass::ProjectRead,
            path_scope: PathScope::Any,
            timeout_ms: 10000,
            output_limit: 1_048_576,
            cancellable: true,
            parallel_safe: true,
            conflict_key: None,
            idempotency: None,
        per_call_resource: None,
        handler: Arc::new(ListDirTool),
        },
        Tool {
            name: "grep",
            description: "Search file contents (ripgrep JSON when available)",
            schema: serde_json::json!({"type":"object","properties":{"pattern":{"type":"string"},"root":{"type":"string"},"glob":{"type":"string"}},"required":["pattern"]}),
            side_effect: SideEffect::ReadOnly,
            permission_class: PermissionClass::ProjectRead,
            path_scope: PathScope::Any,
            timeout_ms: 30000,
            output_limit: 1_048_576,
            cancellable: true,
            parallel_safe: true,
            conflict_key: None,
            idempotency: None,
        per_call_resource: None,
        handler: Arc::new(GrepTool),
        },
        Tool {
            name: "edit_file",
            description: "Replace the first occurrence of old_string with new_string (legacy; prefer apply_patch)",
            schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"old_string":{"type":"string"},"new_string":{"type":"string"}},"required":["path","old_string","new_string"]}),
            side_effect: SideEffect::Write,
            permission_class: PermissionClass::ProjectWrite,
            path_scope: PathScope::Any,
            timeout_ms: 10000,
            output_limit: 1_048_576,
            cancellable: true,
            parallel_safe: false,
            conflict_key: Some("project-write".into()),
            idempotency: None,
        per_call_resource: None,
        handler: Arc::new(EditFileTool),
        },
        Tool {
            name: "run_terminal",
            description: "Run a shell command in the project (15s foreground budget then auto-background)",
            schema: serde_json::json!({
                "type":"object",
                "properties":{
                    "command":{"type":"string"},
                    "args":{"type":"array","items":{"type":"string"}},
                    "cwd":{"type":"string"},
                    "timeout_ms":{"type":"integer"},
                    "background":{"type":"boolean"},
                    "description":{"type":"string"}
                },
                "required":["command"]
            }),
            side_effect: SideEffect::Process,
            permission_class: PermissionClass::DestructiveCommand,
            path_scope: PathScope::Any,
            timeout_ms: 300_000,
            output_limit: 64_000,
            cancellable: true,
            parallel_safe: false,
            conflict_key: None,
            idempotency: None,
        per_call_resource: None,
        handler: Arc::new(RunTerminalTool),
        },
        Tool {
            name: "web_fetch",
            description: "Fetch a public HTTP(S) URL (SSRF-safe DNS checks)",
            schema: serde_json::json!({"type":"object","properties":{"url":{"type":"string"},"max_bytes":{"type":"integer"}},"required":["url"]}),
            side_effect: SideEffect::Network,
            permission_class: PermissionClass::ExternalWrite,
            path_scope: PathScope::None,
            timeout_ms: 20000,
            output_limit: 64_000,
            cancellable: true,
            parallel_safe: false,
            conflict_key: None,
            idempotency: None,
        per_call_resource: None,
        handler: Arc::new(WebFetchTool),
        },
        Tool {
            name: "todo_write",
            description: "Update the agent todo list",
            schema: serde_json::json!({"type":"object","properties":{"todos":{"type":"array"}},"required":["todos"]}),
            side_effect: SideEffect::Write,
            permission_class: PermissionClass::AlwaysAllowed,
            path_scope: PathScope::None,
            timeout_ms: 5000,
            output_limit: 16_000,
            cancellable: true,
            parallel_safe: false,
            conflict_key: None,
            idempotency: None,
        per_call_resource: None,
        handler: Arc::new(TodoWriteTool),
        },
    ]
    .into_iter()
    .chain(extra::extra_builtin_tools())
    .chain(plan::plan_mode_tools())
    // `web_search` is registered only when a real backend is configured. An
    // unconfigured install must not advertise a search tool it cannot honour —
    // the model would spend a turn discovering that, and the only alternative
    // (returning something) would be fabricated data.
    .chain(web_search::is_configured().then(web_search::web_search_tool))
    .collect()
}
