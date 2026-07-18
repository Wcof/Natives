//! runtime/cli_permission.rs — CLI 写盘权限分流
//!
//! `~/.natives/modules/` 路径必须走 Rust 注入的 write_module（KI 门禁），
//! 其他路径放行 CLI 原生 Edit/Bash 工具。

#![allow(dead_code, unused_imports, unused_variables)]
use std::path::Path;

pub enum WriteKind { ModulePath, GeneralPath }

pub fn classify_write(path: &Path) -> WriteKind {
    let modules_root = dirs::home_dir()
        .unwrap_or_default()
        .join(".natives")
        .join("modules");
    if path.starts_with(&modules_root) {
        WriteKind::ModulePath
    } else {
        WriteKind::GeneralPath
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_classify_module_path() {
        let home = dirs::home_dir().unwrap_or_default();
        let path = home.join(".natives").join("modules").join("my-app");
        assert!(matches!(classify_write(&path), WriteKind::ModulePath));
    }

    #[test]
    fn test_classify_general_path() {
        let path = PathBuf::from("/tmp/some_file.txt");
        assert!(matches!(classify_write(&path), WriteKind::GeneralPath));
    }
}
