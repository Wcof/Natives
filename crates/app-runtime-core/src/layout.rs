//! Core-owned built-in module runtime layout discovery.

use std::path::PathBuf;

fn parts() -> Option<(PathBuf, String, String)> {
    let executable = std::env::current_exe().ok()?;
    let version = executable.parent()?;
    let runtime = version.parent()?;
    if runtime.file_name()?.to_str()? != "runtime" {
        return None;
    }
    let app = runtime.parent()?;
    Some((
        app.parent()?.to_path_buf(),
        app.file_name()?.to_str()?.to_string(),
        version.file_name()?.to_str()?.to_string(),
    ))
}

pub fn installed_apps_root() -> Option<PathBuf> {
    parts().map(|value| value.0)
}

pub fn installed_identity() -> Option<(String, String)> {
    parts().map(|value| (value.1, value.2))
}

pub fn installed_parts() -> Option<(PathBuf, String, String)> {
    parts()
}
