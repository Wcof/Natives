//! collector 模块：30+ AI 编程工具本地采集目录定义、模型计价与增量扫描引擎。

pub mod host_sync;
pub mod pricing;
pub mod scanner;

#[derive(Debug, Clone)]
pub struct UsageSourceDef {
    pub id: &'static str,
    pub display_name: &'static str,
    pub category: &'static str,
    pub default_enabled: bool,
    pub relative_paths: &'static [&'static str],
}

pub const KNOWN_SOURCES: &[UsageSourceDef] = &[
    UsageSourceDef {
        id: "claude",
        display_name: "Claude Code",
        category: "cli",
        default_enabled: true,
        relative_paths: &[".claude/projects", ".claude/transcripts"],
    },
    UsageSourceDef {
        id: "codex",
        display_name: "Codex",
        category: "cli",
        default_enabled: true,
        relative_paths: &[".codex/sessions", ".codex/archived_sessions"],
    },
    UsageSourceDef {
        id: "opencode",
        display_name: "OpenCode",
        category: "cli",
        default_enabled: true,
        relative_paths: &[
            ".local/share/opencode",
            "Library/Application Support/opencode",
        ],
    },
    UsageSourceDef {
        id: "hermes",
        display_name: "Hermes Agent",
        category: "cli",
        default_enabled: true,
        relative_paths: &[".hermes"],
    },
    UsageSourceDef {
        id: "openclaw",
        display_name: "OpenClaw",
        category: "cli",
        default_enabled: true,
        relative_paths: &[".openclaw/agents"],
    },
    UsageSourceDef {
        id: "cursor",
        display_name: "Cursor",
        category: "ide",
        default_enabled: true,
        relative_paths: &[
            ".config/tokscale/cursor-cache",
            "Library/Application Support/Cursor",
        ],
    },
    UsageSourceDef {
        id: "antigravity",
        display_name: "Antigravity",
        category: "ide",
        default_enabled: true,
        relative_paths: &[".gemini/antigravity", ".gemini/antigravity-cli"],
    },
    UsageSourceDef {
        id: "cline",
        display_name: "Cline",
        category: "ide",
        default_enabled: true,
        relative_paths: &[".cline/data/sessions"],
    },
    UsageSourceDef {
        id: "amp",
        display_name: "Amp",
        category: "cli",
        default_enabled: true,
        relative_paths: &[".local/share/amp/threads"],
    },
    UsageSourceDef {
        id: "droid",
        display_name: "Factory Droid",
        category: "cli",
        default_enabled: true,
        relative_paths: &[".factory/sessions"],
    },
    UsageSourceDef {
        id: "kimi",
        display_name: "Kimi",
        category: "cli",
        default_enabled: true,
        relative_paths: &[".kimi/sessions", ".kimi-code/sessions"],
    },
    UsageSourceDef {
        id: "qwen",
        display_name: "Qwen",
        category: "cli",
        default_enabled: true,
        relative_paths: &[".qwen/projects"],
    },
    UsageSourceDef {
        id: "grok",
        display_name: "Grok Build",
        category: "cli",
        default_enabled: true,
        relative_paths: &[".grok/sessions", ".grok/logs"],
    },
    UsageSourceDef {
        id: "copilot",
        display_name: "GitHub Copilot",
        category: "ide",
        default_enabled: true,
        relative_paths: &[".copilot"],
    },
    UsageSourceDef {
        id: "pi",
        display_name: "Pi",
        category: "cli",
        default_enabled: true,
        relative_paths: &[".pi/agent/sessions", ".omp/agent/sessions"],
    },
    UsageSourceDef {
        id: "zed",
        display_name: "Zed",
        category: "ide",
        default_enabled: true,
        relative_paths: &[".local/share/zed/threads"],
    },
    UsageSourceDef {
        id: "kilo",
        display_name: "Kilo",
        category: "cli",
        default_enabled: true,
        relative_paths: &[".local/share/kilo"],
    },
    UsageSourceDef {
        id: "commandcode",
        display_name: "Command Code",
        category: "cli",
        default_enabled: true,
        relative_paths: &[".commandcode/projects"],
    },
    UsageSourceDef {
        id: "micode",
        display_name: "MiMo Code",
        category: "cli",
        default_enabled: false,
        relative_paths: &[".local/share/mimocode"],
    },
    UsageSourceDef {
        id: "zcode",
        display_name: "ZCode",
        category: "cli",
        default_enabled: true,
        relative_paths: &[".zcode/projects", ".zcode/cli"],
    },
    UsageSourceDef {
        id: "kiro",
        display_name: "Kiro",
        category: "cli",
        default_enabled: true,
        relative_paths: &[".kiro/sessions"],
    },
    UsageSourceDef {
        id: "codebuddy",
        display_name: "CodeBuddy",
        category: "ide",
        default_enabled: true,
        relative_paths: &[".codebuddy/projects"],
    },
    UsageSourceDef {
        id: "workbuddy",
        display_name: "WorkBuddy",
        category: "cli",
        default_enabled: true,
        relative_paths: &[".workbuddy/projects"],
    },
    UsageSourceDef {
        id: "proma",
        display_name: "Proma",
        category: "cli",
        default_enabled: true,
        relative_paths: &[".proma/agent-sessions"],
    },
    UsageSourceDef {
        id: "qodercn",
        display_name: "Qoder CN",
        category: "ide",
        default_enabled: false,
        relative_paths: &["Library/Application Support/QoderCN"],
    },
    UsageSourceDef {
        id: "reasonix",
        display_name: "Reasonix",
        category: "cli",
        default_enabled: true,
        relative_paths: &[".reasonix/stats", ".reasonix/sessions"],
    },
    UsageSourceDef {
        id: "dsh",
        display_name: "DeepSeek Harness",
        category: "cli",
        default_enabled: true,
        relative_paths: &[".dsh/sessions"],
    },
    UsageSourceDef {
        id: "cherrystudio",
        display_name: "Cherry Studio",
        category: "ide",
        default_enabled: true,
        relative_paths: &["Library/Application Support/CherryStudio"],
    },
    UsageSourceDef {
        id: "lmstudio",
        display_name: "LM Studio",
        category: "api",
        default_enabled: true,
        relative_paths: &[".lmstudio/server-logs"],
    },
    UsageSourceDef {
        id: "unsloth",
        display_name: "Unsloth",
        category: "cli",
        default_enabled: true,
        relative_paths: &[".unsloth/studio"],
    },
];

#[derive(Debug, Default, Clone)]
pub struct UnifiedCollectSummary {
    pub host_events: usize,
    pub host_tokens: i64,
    pub host_cost_micros: i64,
    pub local_sources: usize,
    pub local_records: usize,
    pub local_tokens: i64,
    pub local_cost_micros: i64,
}

pub fn collect_all(
    store: &crate::storage::Store,
    usage_db_override: Option<&std::path::Path>,
    home_override: Option<&std::path::Path>,
) -> Result<UnifiedCollectSummary, String> {
    // 1. 同步 Model Host usage.db 权威数据
    let host_res = host_sync::sync_from_host(store, usage_db_override).unwrap_or_default();

    // 2. 扫描本地 30+ 工具日志目录（补漏/离线场景）
    let local_res = scanner::scan_all_sources(store, home_override).unwrap_or_default();

    Ok(UnifiedCollectSummary {
        host_events: host_res.events_synced,
        host_tokens: host_res.total_tokens,
        host_cost_micros: host_res.total_cost_micros,
        local_sources: local_res.scanned_sources,
        local_records: local_res.new_records,
        local_tokens: local_res.total_tokens,
        local_cost_micros: local_res.total_cost_micros,
    })
}
