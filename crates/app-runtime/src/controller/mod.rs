mod handshake;
mod session;
mod start;
mod status;
mod stopping;

pub(crate) use super::Controller;

use crate::http::write_error;
use app_runtime_core::error::AppErrorCode;
use app_runtime_core::module::ModuleRegistry;
use app_runtime_core::protocol::AppRequest;
use std::path::Path;

impl Controller {
    pub(super) fn handle_request(
        &self,
        req: &AppRequest,
        origin: &str,
        apps_root: &Path,
        registry: &ModuleRegistry,
    ) {
        match req.method.as_str() {
            "app:handshake" => self.handle_handshake(req, origin, apps_root, registry),
            "app:start" => self.handle_start(req),
            "app:status" => self.handle_status(req),
            "app:data_status" => self.handle_data_status(req),
            "app:session" => self.handle_session(req),
            "app:stop" => self.handle_stop(req),
            _ => write_error(
                &req.id,
                AppErrorCode::ProtocolMismatch,
                format!("unknown method '{}'", req.method),
                false,
            ),
        }
    }
}
