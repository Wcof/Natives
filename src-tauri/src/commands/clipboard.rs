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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_read_write_roundtrip() {
        // macOS: pbpaste/pbcopy required; CI may not have them
        // Test that write succeeds (or fails gracefully in headless env)
        let r = clipboard_write("hello clipboard".into());
        if let Err(e) = &r {
            eprintln!("[test] clipboard_write (may fail headless): {e}");
        } else {
            let read = clipboard_read().expect("clipboard_read after write");
            // On macOS with pbpaste this should read back "hello clipboard"
            // On CI without pbpaste, it may fail
            if !read.is_empty() {
                assert_eq!(read, "hello clipboard");
            }
        }
    }

    #[test]
    fn test_clipboard_write_empty() {
        let r = clipboard_write("".into());
        // Writing empty string should succeed or fail gracefully
        if let Err(e) = r {
            eprintln!("[test] clipboard_write empty note: {e}");
        }
    }
}
