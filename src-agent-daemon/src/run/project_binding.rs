//! Project identity and path binding.

use super::manager::RunManager;

impl RunManager {
    pub(crate) fn store_project_path(&self, run_id: &str, project_path: Option<&str>) {
        let Some(p) = project_path.map(str::trim).filter(|s| !s.is_empty()) else {
            return;
        };
        if let Ok(mut map) = self.project_paths.lock() {
            map.insert(run_id.to_string(), std::path::PathBuf::from(p));
        }
    }
    pub(crate) fn resolve_project_path(
        &self,
        run_id: &str,
        request_path: Option<&str>,
    ) -> Option<std::path::PathBuf> {
        // Explicit only — never daemon process cwd (full remediation 第五节).
        if let Some(p) = request_path.map(str::trim).filter(|s| !s.is_empty()) {
            return Some(std::path::PathBuf::from(p));
        }
        self.project_paths
            .lock()
            .ok()
            .and_then(|m| m.get(run_id).cloned())
    }
}
