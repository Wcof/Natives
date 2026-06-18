use crate::{Error, Result};

#[tauri::command]
pub fn show_item_in_folder(path: String) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R", &path])
            .spawn()
            .map_err(|e| Error::Internal(e.to_string()))?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(Error::NotImplemented("showItemInFolder on non-macOS".into()))
    }
}

#[tauri::command]
pub fn open_path(path: String) -> Result<()> {
    open::that(&path).map_err(|e| Error::Internal(e.to_string()))
}
