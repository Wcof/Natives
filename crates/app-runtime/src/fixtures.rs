#[cfg(feature = "test-fixture")]
use app_runtime_core::http::HttpRequest;
#[cfg(feature = "test-fixture")]
use app_runtime_core::module::{
    BuiltInAppModule, ModuleContext, ModuleDescriptor, ModuleHttpResponse,
};

/// 计划 §44 test-only 第二模块：证明架构不是"只对 Fund 有效"——
/// 两个模块可同时在 Registry、独立进程绑定、独立数据根、独立关闭。
#[cfg(feature = "test-fixture")]
pub(crate) struct SecondFixtureModule;

#[cfg(feature = "test-fixture")]
impl BuiltInAppModule for SecondFixtureModule {
    fn descriptor(&self) -> ModuleDescriptor {
        ModuleDescriptor {
            app_id: "fixture-second".into(),
            display_name: "Fixture Second".into(),
            module_api_version: 1,
            data_schema_version: 1,
            capability_version: 1,
        }
    }
    fn initialize(&mut self, _ctx: &ModuleContext) -> Result<(), String> {
        Ok(())
    }
    fn start(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn handle_ui(&self, path: &str) -> Result<Option<ModuleHttpResponse>, String> {
        if path == "/" || path == "/index.html" {
            Ok(Some(ModuleHttpResponse::ok_html("<h1>fixture</h1>")))
        } else {
            Ok(None)
        }
    }
    fn handle_api(
        &self,
        _req: &HttpRequest,
        route: &str,
    ) -> Result<ModuleHttpResponse, (u16, String)> {
        if route == "/api/ping" {
            Ok(ModuleHttpResponse::ok_json("{\"pong\":true}"))
        } else {
            Err((404, "not found".into()))
        }
    }
    fn data_status(&self) -> Result<app_runtime_core::protocol::DataStatusResult, String> {
        Ok(app_runtime_core::protocol::DataStatusResult {
            current_schema: 1,
            migration_state: "ready".into(),
            last_data_writer_version: "fixture".into(),
            has_committed_new_writes: false,
            previous_version_compatible: true,
        })
    }
    fn health(&self) -> Result<app_runtime_core::module::AppHealth, String> {
        Ok(app_runtime_core::module::AppHealth {
            ok: true,
            detail: "fixture-second ready".into(),
        })
    }
    fn shutdown(&mut self) -> Result<(), String> {
        Ok(())
    }
}
