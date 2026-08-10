//! Read-only inspection views: overview, topology, hook catalog, workspace
//! aggregation, template list, prompt preview, run snapshots, external
//! runtime inspection, and project identity bookkeeping.

use super::control_profile::{layers_json, project_path, resolved_context};
use super::super::external_inspector;
use super::super::projection;
use super::super::repository;
use super::super::{opt_str, req_str, HarnessError};
use serde_json::Value;
use std::path::{Path, PathBuf};

// ── inspection ──────────────────────────────────────────────────────────────

pub(super) fn overview(params: &Value) -> Result<Value, HarnessError> {
    repository::with_conn(|conn| {
        let context = resolved_context(conn, params)?;
        let profiles = repository::list_profiles(conn, false)?;
        Ok(serde_json::json!({
            "topology_version": harness_core::topology::TOPOLOGY_VERSION,
            "hook_semantics_version": context.resolution.semantics.as_str(),
            "blueprint_schema_version": harness_core::blueprint::BLUEPRINT_SCHEMA_VERSION,
            "layers": layers_json(&context.layers),
            "hook_counts": projection::counts(&context.resolution),
            "issues": context.resolution.issues,
            "profiles": profiles.iter().map(|p| p.to_json()).collect::<Vec<_>>(),
            // Phase 3 owns per-invocation telemetry. Saying so beats an empty
            // array a caller would read as "nothing ever ran".
            "telemetry": { "available": false, "reason": "phase_3" },
        }))
    })
}

pub(super) fn topology(params: &Value) -> Result<Value, HarnessError> {
    repository::with_conn(|conn| {
        let context = resolved_context(conn, params)?;
        let mut value = projection::topology(&context.resolution);
        if let Some(object) = value.as_object_mut() {
            object.insert("layers".into(), layers_json(&context.layers));
        }
        Ok(value)
    })
}

pub(super) fn hook_catalog(params: &Value) -> Result<Value, HarnessError> {
    repository::with_conn(|conn| {
        let context = resolved_context(conn, params)?;
        let mut value = projection::hook_catalog(&context.resolution);
        if let Some(object) = value.as_object_mut() {
            object.insert("layers".into(), layers_json(&context.layers));
        }
        Ok(value)
    })
}

// ── run snapshots ───────────────────────────────────────────────────────────

pub(super) fn run_get_snapshot(params: &Value) -> Result<Value, HarnessError> {
    let run_id = req_str(params, &["run_id", "runId"])?;
    repository::with_conn(|conn| match repository::get_run_snapshot(conn, &run_id)? {
        Some(snapshot) => Ok(serde_json::json!({
            "run_id": run_id,
            "resolved": true,
            "canonical_hash": snapshot.canonical_hash(),
            "snapshot": snapshot,
        })),
        // Honest absence, not an error: a Run started before the resolve seam
        // was wired, or one that never reached start, genuinely has no
        // snapshot. The caller can tell that apart from a failure.
        None => Ok(serde_json::json!({
            "run_id": run_id,
            "resolved": false,
            "snapshot": Value::Null,
        })),
    })
}

// ── prompt preview and workspace ────────────────────────────────────────────

pub(super) fn prompt_preview(params: &Value) -> Result<Value, HarnessError> {
    repository::with_conn(|conn| {
        let context = resolved_context(conn, params)?;
        let mut prompt_blocks = Vec::new();
        let mut replacements = std::collections::BTreeMap::new();
        for layer in context.layers.iter() {
            if let Some(version) = repository::get_version(conn, &layer.version_id)? {
                let doc = version.document()?;
                for replacement in doc.builtin_prompt_replacements {
                    replacements.insert(replacement.surface_id.clone(), replacement);
                }
                for block in doc.prompt_blocks.iter().filter(|b| b.enabled) {
                    prompt_blocks.push(block.clone());
                }
            }
        }
        let mut blocks = prompt_blocks
            .iter()
            .map(|block| {
                serde_json::json!({
                    "id": block.id, "name": block.name, "order": block.order, "placement": block.placement,
                    "source_digest": harness_core::sha256_hex(&block.markdown),
                    "token_estimate": block.markdown.chars().count().div_ceil(4),
                })
            })
            .collect::<Vec<_>>();
        blocks.sort_by_key(|b| b.get("order").and_then(Value::as_i64).unwrap_or_default());
        let surface_id = crate::production::CREATIVE_DRAFT_PROMPT_SURFACE_ID;
        let default = crate::production::builtin_prompt_surface_default(surface_id)
            .expect("registered Native prompt surface");
        let replaced = replacements.contains_key(surface_id);
        let effective_digest = harness_core::sha256_hex(
            replacements
                .get(surface_id)
                .map(|item| item.markdown.as_str())
                .unwrap_or(default),
        );
        let replacements = replacements.into_values().collect::<Vec<_>>();
        let project_path = opt_str(params, &["project_path", "projectPath"]).map(PathBuf::from);
        let compiled = crate::production::compile_effective_prompt(
            Some("creative_draft"),
            None,
            None,
            project_path.as_deref(),
            None,
            &prompt_blocks,
            &replacements,
            None,
        );
        let prompt_summary = harness_core::PromptPlanSummary::from(&compiled);
        Ok(serde_json::json!({
            "blocks": blocks,
            "layers": prompt_summary.layers,
            "effective_prompt_hash": prompt_summary.effective_prompt_hash,
            "token_estimate": prompt_summary.token_estimate,
            "builtin_surfaces": [{
                "surface_id": surface_id,
                "default_markdown": default,
                "default_digest": harness_core::sha256_hex(default),
                "effective_digest": effective_digest,
                "replaced": replaced,
            }],
            "raw_persisted": false
        }))
    })
}

pub(super) fn workspace_get(
    request: &assistant_protocol::v2::HarnessWorkspaceGetRequest,
) -> Result<Value, HarnessError> {
    let params = serde_json::to_value(request)
        .map_err(|error| HarnessError::internal(format!("serialize workspace request: {error}")))?;
    let prompt_plan = prompt_preview(&params)?;
    repository::with_conn(|conn| {
        let overview_val = overview(&params)?;
        let topology_val = topology(&params)?;
        let catalog_val = hook_catalog(&params)?;

        let profile_id = request.profile_id.clone();
        let draft_val = if let Some(pid) = &profile_id {
            repository::get_draft(conn, pid).ok().flatten().map(|d| {
                serde_json::json!({
                    "profile_id": d.profile_id,
                    "base_version_id": d.base_version_id,
                    "revision": d.revision,
                    "updated_at": d.updated_at,
                    "document": serde_json::from_str::<Value>(&d.document_json)
                        .unwrap_or(Value::Null),
                    "source_candidate": d.source_candidate_json
                        .as_deref()
                        .and_then(|value| serde_json::from_str::<Value>(value).ok()),
                })
            })
        } else {
            None
        };

        Ok(serde_json::json!({
            "overview": overview_val,
            "topology": topology_val,
            "catalog": catalog_val,
            "prompt_plan": prompt_plan,
            "draft": draft_val,
        }))
    })
}

pub(super) fn template_list(_params: &Value) -> Result<Value, HarnessError> {
    Ok(serde_json::json!({
        "items": [
            {
                "id": "tpl-project-prompt",
                "name": "项目提示词策略",
                "description": "规范 AI 项目级全局架构约定与安全防守基线",
                "category": "prompt",
                "template": {
                    "schema_version": 3,
                    "prompt_blocks": [{
                        "id": "11111111-1111-1111-1111-111111111111",
                        "name": "项目架构准则",
                        "markdown": "## 项目架构与规范\n- 必须使用 RTK 执行 Shell 命令\n- 严禁假数据与空 fallback",
                        "enabled": true,
                        "order": 1,
                        "placement": "after_project_instructions"
                    }]
                }
            },
            {
                "id": "tpl-cmd-quality-gate",
                "name": "Command 质量门禁",
                "description": "在提交代码与删除敏感目录前触发指令质量审计",
                "category": "command",
                "template": {
                    "schema_version": 3,
                    "native_hooks": [{
                        "id": "22222222-2222-2222-2222-222222222222",
                        "name": "Git Command Gate",
                        "enabled": true,
                        "event": "PreToolUse",
                        "order": 10,
                        "matcher": "run_command|Bash",
                        "conditions": [{
                            "field": "command",
                            "operator": "regex_match",
                            "pattern": "git\\s+(commit|push)"
                        }],
                        "timeout_ms": 10000,
                        "failure_policy": "fail",
                        "adapter": {
                            "type": "command",
                            "program": "cargo",
                            "args": ["test", "--workspace"],
                            "trusted": false
                        }
                    }]
                }
            },
            {
                "id": "tpl-http-audit-notice",
                "name": "HTTP 审计通知",
                "description": "工具调用后向审计 Webhook 发送结构化通知",
                "category": "http",
                "template": {
                    "schema_version": 3,
                    "native_hooks": [{
                        "id": "33333333-3333-3333-3333-333333333333",
                        "name": "HTTP Audit Webhook",
                        "enabled": true,
                        "event": "PostToolUse",
                        "order": 20,
                        "matcher": "*",
                        "conditions": [],
                        "timeout_ms": 5000,
                        "failure_policy": "skip",
                        "adapter": {
                            "type": "http",
                            "url": "http://127.0.0.1:8080/audit",
                            "allow_hosts": ["127.0.0.1", "localhost"]
                        }
                    }]
                }
            },
            {
                "id": "tpl-mcp-security-scan",
                "name": "MCP 安全扫描",
                "description": "调用高危 MCP 工具前执行二次安全检查",
                "category": "mcp",
                "template": {
                    "schema_version": 3,
                    "native_hooks": [{
                        "id": "44444444-4444-4444-4444-444444444444",
                        "name": "MCP Tool Gate",
                        "enabled": true,
                        "event": "PreToolUse",
                        "order": 5,
                        "matcher": "mcp:*",
                        "conditions": [],
                        "timeout_ms": 10000,
                        "failure_policy": "fail",
                        "adapter": {
                            "type": "mcp_tool",
                            "server_id": "security_scanner",
                            "tool_name": "scan_input",
                            "input_template": "{\"input\": \"${input}\"}"
                        }
                    }]
                }
            },
            {
                "id": "tpl-prompt-risk-assessment",
                "name": "Prompt 风险判断",
                "description": "使用同 Provider 模型执行 Tool 使用前的风险判断",
                "category": "prompt_eval",
                "template": {
                    "schema_version": 3,
                    "native_hooks": [{
                        "id": "55555555-5555-5555-5555-555555555555",
                        "name": "Safety Risk Evaluation",
                        "enabled": true,
                        "event": "PreToolUse",
                        "order": 2,
                        "matcher": "run_command",
                        "conditions": [],
                        "timeout_ms": 8000,
                        "failure_policy": "fail",
                        "adapter": {
                            "type": "prompt",
                            "template": "Evaluate if the tool input executes irreversible systemic damage. Reply allow or deny."
                        }
                    }]
                }
            },
            {
                "id": "tpl-agent-completion-check",
                "name": "Agent 完成度检查",
                "description": "Run 结束前启动 Agent 检查任务完成质量",
                "category": "agent",
                "template": {
                    "schema_version": 3,
                    "native_hooks": [{
                        "id": "66666666-6666-6666-6666-666666666666",
                        "name": "Agent Quality Assancer",
                        "enabled": true,
                        "event": "Stop",
                        "order": 1,
                        "matcher": "*",
                        "conditions": [],
                        "timeout_ms": 30000,
                        "failure_policy": "skip",
                        "adapter": {
                            "type": "agent",
                            "prompt": "Verify if all user requirement criteria are met in project repository.",
                            "max_steps": 3,
                            "readonly_tools": ["view_file", "list_dir", "grep_search"]
                        }
                    }]
                }
            }
        ]
    }))
}

// ── external runtime inspection and project identity ────────────────────────

pub(super) fn external_inspect(params: &Value) -> Result<Value, HarnessError> {
    let p_path = project_path(params);
    let report = external_inspector::inspect_external_runtimes(p_path.as_deref());
    serde_json::to_value(report).map_err(|e| HarnessError::internal(e.to_string()))
}

pub(super) fn project_identity_register(params: &Value) -> Result<Value, HarnessError> {
    repository::with_conn(|conn| {
        let path = req_str(params, &["canonical_path", "canonicalPath", "path"])?;
        let name = opt_str(params, &["name"]).unwrap_or_else(|| {
            Path::new(&path)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "project".into())
        });
        repository::register_project_identity(conn, &path, &name)
    })
}

pub(super) fn project_identity_list(_params: &Value) -> Result<Value, HarnessError> {
    repository::with_conn(|conn| {
        let items = repository::list_project_identities(conn)?;
        Ok(serde_json::json!({ "items": items }))
    })
}
