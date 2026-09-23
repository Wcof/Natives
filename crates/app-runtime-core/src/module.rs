//! 官方内置应用契约与编译期注册表（ADR-0031 / 技术 07 规范）。

use crate::http::HttpRequest;
use crate::protocol::DataStatusResult;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 模块健康状态（轻量本地检查结果；禁止发起外部网络请求）。
pub struct AppHealth {
    pub ok: bool,
    pub detail: String,
}

/// 模块描述符
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModuleDescriptor {
    pub app_id: String,
    pub display_name: String,
    pub module_api_version: u32,
    pub data_schema_version: u32,
    pub capability_version: u32,
}

/// Runtime 注入的受控上下文
#[derive(Debug, Clone)]
pub struct ModuleContext {
    pub app_id: String,
    pub product_version: String,
    pub module_data_root: PathBuf,
    pub imports_root: PathBuf,
    pub cache_root: PathBuf,
    pub logs_root: PathBuf,
    pub keychain_namespace: String,
    pub activation_generation: u64,
    /// 长任务（网络/导入/同步）必须轮询的取消信号；进程关闭时由 Runtime 触发。
    pub cancellation: std::sync::Arc<crate::cancellation::CancellationToken>,
    /// 资源硬门（HTTP/并发/超时/关闭期限）。
    pub limits: crate::limits::RuntimeLimits,
}

impl ModuleContext {
    pub fn for_app(apps_root: &Path, app_id: &str, product_version: &str, generation: u64) -> Self {
        let app_root = apps_root.join(app_id);
        Self {
            app_id: app_id.to_string(),
            product_version: product_version.to_string(),
            module_data_root: app_root.join("data"),
            imports_root: app_root.join("imports"),
            cache_root: app_root.join("cache"),
            logs_root: app_root.join("logs"),
            keychain_namespace: format!("com.natives.app.{app_id}"),
            activation_generation: generation,
            cancellation: std::sync::Arc::new(crate::cancellation::CancellationToken::new()),
            limits: crate::limits::RuntimeLimits::default(),
        }
    }
}

/// HTTP 响应结构
#[derive(Debug, Clone)]
pub struct ModuleHttpResponse {
    pub status_code: u16,
    pub reason: String,
    pub content_type: String,
    pub body: Vec<u8>,
}

impl ModuleHttpResponse {
    pub fn ok_json(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status_code: 200,
            reason: "OK".into(),
            content_type: "application/json".into(),
            body: body.into(),
        }
    }

    pub fn ok_html(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status_code: 200,
            reason: "OK".into(),
            content_type: "text/html; charset=utf-8".into(),
            body: body.into(),
        }
    }

    pub fn error(code: u16, reason: &str, message: &str) -> Self {
        Self {
            status_code: code,
            reason: reason.into(),
            content_type: "application/json".into(),
            body: format!("{{\"error\":{:?}}}", message).into_bytes(),
        }
    }
}

/// 官方内置应用契约 trait
pub trait BuiltInAppModule: Send + Sync {
    /// 模块描述符
    fn descriptor(&self) -> ModuleDescriptor;

    /// 使用 Runtime 注入的上下文初始化
    fn initialize(&mut self, context: &ModuleContext) -> Result<(), String>;

    /// 启动模块业务状态（启动 store、准备监听）
    fn start(&mut self) -> Result<(), String>;

    /// 处理 UI 静态资源请求（无需 Bearer token，初始化页面使用）
    fn handle_ui(&self, path: &str) -> Result<Option<ModuleHttpResponse>, String>;

    /// 处理业务 API 请求（必须持有合法 Bearer token）
    fn handle_api(
        &self,
        req: &HttpRequest,
        route: &str,
    ) -> Result<ModuleHttpResponse, (u16, String)>;

    /// 数据状态查询
    fn data_status(&self) -> Result<DataStatusResult, String>;

    /// 健康检查
    fn health(&self) -> Result<AppHealth, String>;

    /// 彻底关闭与释放所有资源
    fn shutdown(&mut self) -> Result<(), String>;
}

/// 编译期注册工厂
pub type ModuleFactory = Box<dyn Fn() -> Box<dyn BuiltInAppModule> + Send + Sync>;

/// 编译期 ModuleRegistry
#[derive(Default)]
pub struct ModuleRegistry {
    factories: HashMap<String, ModuleFactory>,
}

impl ModuleRegistry {
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    pub fn register(&mut self, app_id: &str, factory: ModuleFactory) {
        self.factories.insert(app_id.to_string(), factory);
    }

    pub fn contains(&self, app_id: &str) -> bool {
        self.factories.contains_key(app_id)
    }

    pub fn create(&self, app_id: &str) -> Option<Box<dyn BuiltInAppModule>> {
        self.factories.get(app_id).map(|f| f())
    }

    pub fn registered_app_ids(&self) -> Vec<String> {
        let mut ids: Vec<_> = self.factories.keys().cloned().collect();
        ids.sort();
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyModule;
    impl BuiltInAppModule for DummyModule {
        fn descriptor(&self) -> ModuleDescriptor {
            ModuleDescriptor {
                app_id: "test".into(),
                display_name: "Test Module".into(),
                module_api_version: 1,
                data_schema_version: 2,
                capability_version: 3,
            }
        }
        fn initialize(&mut self, _ctx: &ModuleContext) -> Result<(), String> {
            Ok(())
        }
        fn start(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn handle_ui(&self, _path: &str) -> Result<Option<ModuleHttpResponse>, String> {
            Ok(Some(ModuleHttpResponse::ok_html("<h1>test</h1>")))
        }
        fn handle_api(
            &self,
            _req: &HttpRequest,
            _route: &str,
        ) -> Result<ModuleHttpResponse, (u16, String)> {
            Ok(ModuleHttpResponse::ok_json("{\"ok\":true}"))
        }
        fn data_status(&self) -> Result<DataStatusResult, String> {
            Ok(DataStatusResult {
                current_schema: 2,
                migration_state: "ready".into(),
                last_data_writer_version: "1.0.0".into(),
                has_committed_new_writes: false,
                previous_version_compatible: true,
            })
        }
        fn health(&self) -> Result<AppHealth, String> {
            Ok(AppHealth {
                ok: true,
                detail: "healthy".into(),
            })
        }
        fn shutdown(&mut self) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn registry_register_and_create() {
        let mut registry = ModuleRegistry::new();
        assert_eq!(registry.registered_app_ids().len(), 0);
        assert!(!registry.contains("test"));

        registry.register("test", Box::new(|| Box::new(DummyModule)));
        assert!(registry.contains("test"));
        assert_eq!(registry.registered_app_ids(), vec!["test".to_string()]);

        let module = registry.create("test").unwrap();
        let desc = module.descriptor();
        assert_eq!(desc.app_id, "test");
        assert_eq!(desc.module_api_version, 1);
        assert_eq!(desc.data_schema_version, 2);
    }

    #[test]
    fn module_context_paths() {
        let root = Path::new("/tmp/natives_test/apps");
        let ctx = ModuleContext::for_app(root, "test", "0.1.0", 3);
        assert_eq!(ctx.app_id, "test");
        assert_eq!(
            ctx.module_data_root,
            PathBuf::from("/tmp/natives_test/apps/test/data")
        );
        assert_eq!(
            ctx.imports_root,
            PathBuf::from("/tmp/natives_test/apps/test/imports")
        );
        assert_eq!(ctx.keychain_namespace, "com.natives.app.test");
        assert_eq!(ctx.activation_generation, 3);
    }
}
