use crate::{Error, Result};

#[tauri::command]
pub fn clipboard_write(text: String) -> Result<()> {
    // Use pbcopy on macOS
    use std::io::Write;
    let mut child = std::process::Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| Error::Internal(format!("pbcopy failed: {e}")))?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| Error::Internal("pbcopy stdin not available".into()))?
        .write_all(text.as_bytes())
        .map_err(|e| Error::Internal(format!("pbcopy write failed: {e}")))?;
    Ok(())
}

#[tauri::command]
pub fn clipboard_read() -> Result<String> {
    let output = std::process::Command::new("pbpaste")
        .output()
        .map_err(|e| Error::Internal(format!("pbpaste failed: {e}")))?;
    String::from_utf8(output.stdout).map_err(|e| Error::Internal(e.to_string()))
}
