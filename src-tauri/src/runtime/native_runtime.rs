//! runtime/native_runtime.rs — Native Runtime 协调层（增强版）
//!
//! 将各原子能力层组装为 `AgentRuntime` 实现。
//! 职责仅为编排协调，不包含具体逻辑。
//!
//! 增强特性：
//!   - 集成 PluginManager（从 plugins/ 目录加载插件）
//!   - 集成 HookPipeline（4 点 Hook：PreToolUse/PostToolUse/Stop/UserPromptSubmit）
//!   - 集成 RuleEngine（.local.md 规则引擎）
//!   - 集成 CapabilityRegistry（原子能力注册表）
//!   - 集成 AgentLoop（带循环检测/自愈熔断的状态机）
//!
//! 架构：
//!   NativeRuntime (协调器)
//!     ├── PluginManager ─── 插件发现/加载
//!     ├── CapabilityRegistry (原子能力注册表)
//!     │     └── 插件注册的额外能力
//!     ├── HookPipeline (Pre/PostToolUse + Stop + UserPromptSubmit)
//!     │     └── 插件注册的外部脚本 hooks
//!     ├── RuleEngine (.local.md 规则匹配 + 动态规则)
//!     ├── AgentLoop (带循环检测的状态机)
//!     └── SseClient (流式通信)

use super::{AgentRuntime, EventStream, RuntimeEvent, RuntimeStreamOptions};
use crate::commands::executor_settings::load_executor_settings;
use crate::module_manager::modules_root;
use crate::runtime::native::agent_loop::{AgentLoop, LoopConfig};
use crate::runtime::native::capability::{CapabilityMeta, CapabilityRegistry};
use crate::runtime::native::command_agent_skill::{Agent, Command, Skill};
use crate::runtime::native::hook_pipeline::HookPipeline;
use crate::runtime::native::plugin_system::{McpManifest, PluginManager, PluginMeta};
use crate::runtime::native::rule_engine::RuleEngine;
use crate::Result;
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeRuntimeCatalog {
    pub capabilities: Vec<CapabilityMeta>,
    pub plugins: Vec<PluginMeta>,
    pub hooks: Vec<HookCatalogItem>,
    pub rules: Vec<RuleCatalogItem>,
    pub commands: Vec<Command>,
    pub skills: Vec<Skill>,
    pub agents: Vec<Agent>,
    pub mcp_servers: Vec<McpManifest>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookCatalogItem {
    pub plugin_name: String,
    pub hook_point: String,
    pub count: usize,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleCatalogItem {
    pub name: String,
    pub event: String,
    pub action: String,
    pub priority: i32,
}

pub struct NativeRuntime {
    app_handle: tauri::AppHandle,
    /// 全局能力注册表（线程安全）
    registry: Arc<Mutex<CapabilityRegistry>>,
    /// 全局 hook 管道
    hook_pipeline: Arc<Mutex<HookPipeline>>,
    /// 全局规则引擎
    rule_engine: Arc<Mutex<RuleEngine>>,
    /// 全局插件管理器
    plugin_manager: Arc<Mutex<PluginManager>>,
    /// 项目根目录
    project_root: Option<PathBuf>,
}

impl NativeRuntime {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        let modules_dir = modules_root();

        // 初始化能力注册表
        let mut registry = CapabilityRegistry::new();
        for cap in crate::runtime::native::capability::create_default_capabilities(&modules_dir) {
            registry.register(cap);
        }

        // 初始化 hook 管道
        let mut pipeline = HookPipeline::new();
        pipeline.register_defaults();

        // 初始化规则引擎
        let mut rule_engine = RuleEngine::new();

        // 初始化插件管理器
        let mut plugin_manager = PluginManager::new();

        // 添加插件搜索路径：项目根目录的 plugins/
        let project_root = std::env::current_dir().ok();
        if let Some(ref root) = project_root {
            let plugins_dir = root.join("plugins");
            if plugins_dir.exists() {
                plugin_manager.add_search_path(plugins_dir);
            }
            // 也检查 ~/.natives/plugins/
            if let Some(home) = dirs::home_dir() {
                let global_plugins = home.join(".natives").join("plugins");
                plugin_manager.add_search_path(global_plugins);
            }
        }

        // 发现并加载插件
        plugin_manager.discover_all();

        // 从插件注册 hooks
        plugin_manager.register_hooks(&mut pipeline);

        // 从插件加载规则
        plugin_manager.load_plugin_rules(&mut rule_engine);

        // 从 .claude 目录加载规则
        if let Some(ref root) = project_root {
            let claude_dir = root.join(".claude");
            if claude_dir.exists() {
                rule_engine.load_rules(&claude_dir);
                println!("[NativeRuntime] Loaded rules from .claude/");
            }
        }

        let rt = Self {
            app_handle,
            registry: Arc::new(Mutex::new(registry)),
            hook_pipeline: Arc::new(Mutex::new(pipeline)),
            rule_engine: Arc::new(Mutex::new(rule_engine)),
            plugin_manager: Arc::new(Mutex::new(plugin_manager)),
            project_root,
        };

        println!(
            "[NativeRuntime] Initialized with {} capabilities, {} rules, {} plugins",
            rt.registry.blocking_lock().count(),
            rt.rule_engine.blocking_lock().rules().len(),
            rt.plugin_manager.blocking_lock().plugin_count(),
        );
        rt
    }

    /// 获取能力注册表（供其他模块查询/管理）
    pub fn registry(&self) -> Arc<Mutex<CapabilityRegistry>> {
        self.registry.clone()
    }

    /// 获取插件管理器（供其他模块查询）
    pub fn plugin_manager(&self) -> Arc<Mutex<PluginManager>> {
        self.plugin_manager.clone()
    }

    /// 获取 hook 管道（供其他模块注册额外 hook）
    pub fn hook_pipeline(&self) -> Arc<Mutex<HookPipeline>> {
        self.hook_pipeline.clone()
    }

    /// 获取规则引擎（供其他模块添加规则）
    pub fn rule_engine(&self) -> Arc<Mutex<RuleEngine>> {
        self.rule_engine.clone()
    }

    pub async fn catalog(&self) -> NativeRuntimeCatalog {
        let registry = self.registry.lock().await;
        let plugin_manager = self.plugin_manager.lock().await;
        let rule_engine = self.rule_engine.lock().await;
        build_catalog_from_parts(
            &registry,
            &plugin_manager,
            &rule_engine,
            self.project_root.as_ref(),
        )
    }
}

pub fn build_native_runtime_catalog() -> NativeRuntimeCatalog {
    let modules_dir = modules_root();
    let mut registry = CapabilityRegistry::new();
    registry.register_all(
        crate::runtime::native::capability::create_default_capabilities(&modules_dir),
    );

    let project_root = std::env::current_dir().ok();
    let mut plugin_manager = PluginManager::new();
    if let Some(ref root) = project_root {
        let plugins_dir = root.join("plugins");
        if plugins_dir.exists() {
            plugin_manager.add_search_path(plugins_dir);
        }
    }
    if let Some(home) = dirs::home_dir() {
        let global_plugins = home.join(".natives").join("plugins");
        plugin_manager.add_search_path(global_plugins);
    }
    plugin_manager.discover_all();

    let mut rule_engine = RuleEngine::new();
    plugin_manager.load_plugin_rules(&mut rule_engine);
    if let Some(ref root) = project_root {
        let claude_dir = root.join(".claude");
        if claude_dir.exists() {
            rule_engine.load_rules(&claude_dir);
        }
    }

    build_catalog_from_parts(
        &registry,
        &plugin_manager,
        &rule_engine,
        project_root.as_ref(),
    )
}

fn build_catalog_from_parts(
    registry: &CapabilityRegistry,
    plugin_manager: &PluginManager,
    rule_engine: &RuleEngine,
    project_root: Option<&PathBuf>,
) -> NativeRuntimeCatalog {
    let plugin_roots = plugin_manager.plugin_roots();
    let project_root = project_root.cloned().unwrap_or_else(|| PathBuf::from("."));
    let mut cas =
        crate::runtime::native::command_agent_skill::CasManager::new(project_root, plugin_roots);
    cas.load_all();

    let hooks = plugin_manager
        .hooks_summary()
        .into_iter()
        .map(|(plugin_name, hook_point, count)| HookCatalogItem {
            plugin_name,
            hook_point,
            count,
        })
        .collect();

    let rules = rule_engine
        .rules()
        .iter()
        .map(|rule| RuleCatalogItem {
            name: rule.name.clone(),
            event: format!("{:?}", rule.event),
            action: format!("{:?}", rule.action),
            priority: rule.priority,
        })
        .collect();

    NativeRuntimeCatalog {
        capabilities: registry.list_metadata(),
        plugins: plugin_manager.list_plugins(),
        hooks,
        rules,
        commands: cas.commands().to_vec(),
        skills: cas.skills().to_vec(),
        agents: cas.agents().to_vec(),
        mcp_servers: plugin_manager.list_mcp_manifests(),
    }
}

#[async_trait]
impl AgentRuntime for NativeRuntime {
    fn id(&self) -> &'static str {
        "native"
    }
    fn display_name(&self) -> &'static str {
        "Native Runtime (Atomic Capability Engine v2)"
    }
    fn is_available(&self) -> bool {
        true
    }

    async fn stream(&self, options: RuntimeStreamOptions) -> Result<EventStream> {
        // ── 1. 解密凭据（P1 安全）──
        let provider_id = options.provider_id.clone();
        let (api_key, base_url) = resolve_provider_credentials(&provider_id).await?;

        let app = self.app_handle.clone();
        let session_id = options.session_id.clone();
        let mut cancel_rx = options.abort_receiver;
        let modules_dir = modules_root();

        // ── 2. 使用可选的工作目录 ──
        let working_dir = options
            .working_directory
            .clone()
            .or_else(|| self.project_root.clone());

        // 组装上下文
        let system_prompt = if let Some(ref wd) = working_dir {
            crate::runtime::native::context_assembler::assemble(Some(wd), &options.prompt, 8000)
                .await
                .ok()
                .map(|c| c.system_prompt)
                .unwrap_or_default()
        } else {
            String::new()
        };
        let mut initial_messages: Vec<serde_json::Value> = vec![];
        if !system_prompt.is_empty() {
            initial_messages.push(json!({ "role": "system", "content": system_prompt }));
        }

        // UserPromptSubmit hook
        {
            let pipeline = self.hook_pipeline.lock().await;
            let context = json!({
                "session_id": session_id,
                "working_directory": working_dir,
            });
            match pipeline.on_prompt_submit(&options.prompt, &context) {
                crate::runtime::native::hook_pipeline::PromptAction::Allow(modified) => {
                    initial_messages.push(json!({ "role": "user", "content": modified }));
                }
                crate::runtime::native::hook_pipeline::PromptAction::Block(msg) => {
                    return Err(crate::Error::InvalidInput(format!(
                        "Prompt blocked by hook: {msg}"
                    )));
                }
            }
        }

        // ── 3. 加载执行设置 ──
        let exec_settings = load_executor_settings();
        let enabled_tools = exec_settings.enabled_tools;
        let max_self_heal: u32 = exec_settings.max_self_heal;
        let max_steps: u32 = exec_settings.max_steps.unwrap_or(50);
        let permission_mode = match options
            .runtime_options
            .get("permission_profile")
            .and_then(serde_json::Value::as_str)
        {
            Some("readonly") => crate::runtime::native::capability::PermissionMode::Deny,
            Some("full_access") => crate::runtime::native::capability::PermissionMode::Allow,
            _ => crate::runtime::native::capability::PermissionMode::Ask,
        };

        // ── 4. 创建事件通道 ──
        let (tx, rx) = tokio::sync::mpsc::channel::<RuntimeEvent>(64);

        // ── 5. 启动 Agent Loop（传递所有增强组件） ──
        let app2 = app.clone();
        let sid = session_id.clone();

        // 提前 clone 需要跨 async 闭包的 Arc<Mutex<...>>
        let hook_pipeline_clone = self.hook_pipeline.clone();
        let rule_engine_clone = self.rule_engine.clone();

        tokio::spawn(async move {
            let config = LoopConfig {
                max_steps,
                max_self_heal,
                doom_threshold: 3,
                model: options.model.clone(),
                base_url,
                api_key,
            };

            // 从 Arc<Mutex> 获取共享引用
            let pipeline = hook_pipeline_clone.lock().await;
            let engine = rule_engine_clone.lock().await;

            let mut loop_runner = AgentLoop::new(config, initial_messages);
            loop_runner
                .run(
                    &tx,
                    &app2,
                    &sid,
                    &modules_dir,
                    working_dir.clone(),
                    &enabled_tools,
                    &pipeline,
                    &engine,
                    permission_mode,
                    &mut cancel_rx,
                )
                .await;

            // 记录最终状态
            println!(
                "[NativeRuntime] Session {} completed: {:?} ({} steps, {} self-heal)",
                sid,
                loop_runner.state(),
                loop_runner.step(),
                loop_runner.self_heal_count(),
            );

            // pipeline 和 engine 在闭包结束自动释放
        });

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    fn interrupt(&self, session_id: &str) {
        crate::assistant_stream_proxy::cancel_stream_sync(session_id);
    }

    fn dispose(&self) {
        println!("[NativeRuntime] Disposed");
    }
}

/// 解密 provider 凭据（P1 安全：全程 Rust 内存，不经过前端）
async fn resolve_provider_credentials(provider_id: &str) -> Result<(String, String)> {
    let db = crate::db::get_assistant_db_conn()
        .map_err(|e| crate::Error::Internal(format!("DB connection: {e}")))?;

    let row = db
        .query_row(
            "SELECT k.api_key_encrypted, k.dek_encrypted, p.base_url
         FROM provider_api_keys k
         JOIN user_providers p ON k.provider_id = p.id
         WHERE k.provider_id = ?1
         ORDER BY k.created_at ASC LIMIT 1",
            rusqlite::params![provider_id],
            |row| {
                let encrypted_key: String = row.get(0)?;
                let dek_encrypted: String = row.get(1)?;
                let base_url: String = row.get(2)?;
                Ok((encrypted_key, dek_encrypted, base_url))
            },
        )
        .map_err(|e| {
            crate::Error::InvalidInput(format!("Provider '{provider_id}' not found: {e}"))
        })?;

    let (encrypted_key, dek_encrypted, base_url) = row;

    // 解密 API key — 优先使用信封解密，遗留 Key 回退到旧版解密
    let api_key = if !dek_encrypted.is_empty() {
        crate::provider_key_manager::envelope_decrypt(&encrypted_key, &dek_encrypted, &db)
            .map_err(|e| crate::Error::Internal(format!("Decrypt API key: {e}")))?
    } else {
        let encryption_key = crate::env_manager::get_encryption_key(&db)
            .map_err(|e| crate::Error::Internal(format!("Get encryption key: {e}")))?;
        crate::env_manager::decrypt(&encrypted_key, &encryption_key)
            .map_err(|e| crate::Error::Internal(format!("Decrypt API key (legacy): {e}")))?
    };

    Ok((api_key, base_url))
}
