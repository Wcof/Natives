use crate::{Error, Result};
use std::process::Stdio;
use std::io::{BufRead, BufReader};
use tauri::Emitter;

/// Detect if a plugin is installed and return its version.
/// Returns Ok(None) if not installed.
#[tauri::command]
pub fn plugin_detect(name: String) -> Result<Option<String>> {
    let home = dirs::home_dir().ok_or_else(|| Error::Internal("Could not find home directory".to_string()))?;
    
    match name.as_str() {
        "rtk" => {
            let local_bin = home.join(".local").join("bin").join("rtk");
            let bin_to_use = if local_bin.exists() {
                local_bin.to_string_lossy().to_string()
            } else {
                "rtk".to_string()
            };
            Ok(run_version_cmd(&bin_to_use))
        }
        "codegraph" => {
            let local_bin = home.join(".local").join("bin").join("codegraph");
            let bin_to_use = if local_bin.exists() {
                local_bin.to_string_lossy().to_string()
            } else {
                "codegraph".to_string()
            };
            Ok(run_version_cmd(&bin_to_use))
        }
        "ccusage" => {
            // 1. Try directly invoking ccusage
            if let Some(v) = run_version_cmd("ccusage") {
                return Ok(Some(v));
            }
            // 2. Try running via npx --no-install
            let output = std::process::Command::new("npx")
                .args(["--no-install", "ccusage", "--version"])
                .output()
                .ok();
            if let Some(out) = output {
                if out.status.success() {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let version = parse_version_str(&stdout);
                    if !version.is_empty() {
                        return Ok(Some(version));
                    }
                }
            }
            Ok(None)
        }
        _ => Err(Error::InvalidInput(format!("Unknown plugin: {name}"))),
    }
}

/// Helper to run version command and return parsed version
fn run_version_cmd(bin: &str) -> Option<String> {
    let output = std::process::Command::new(bin)
        .arg("--version")
        .output()
        .ok()?;
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let version = parse_version_str(&stdout);
        if !version.is_empty() {
            return Some(version);
        }
    }
    None
}

/// Parse version from output string, extracting digits/dots
fn parse_version_str(text: &str) -> String {
    let trimmed = text.trim();
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    // Try to find the token containing digits/dots
    for part in parts {
        let clean = part.trim_start_matches('v');
        if !clean.is_empty() && clean.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            return clean.to_string();
        }
    }
    trimmed.to_string()
}

/// Install or update a plugin asynchronously.
/// Streams stdout/stderr via "plugin:install-log" events and emits "plugin:install-complete" on finish.
#[tauri::command]
pub fn plugin_install(name: String, app_handle: tauri::AppHandle) -> Result<()> {
    let cmd_str = match name.as_str() {
        "rtk" => "curl -fsSL https://raw.githubusercontent.com/rtk-ai/rtk/refs/heads/master/install.sh | sh",
        "codegraph" => "curl -fsSL https://raw.githubusercontent.com/colbymchenry/codegraph/main/install.sh | sh",
        "ccusage" => "npm install -g ccusage",
        _ => return Err(Error::InvalidInput(format!("Unknown plugin: {name}"))),
    };

    let name_clone = name.clone();
    let app_clone = app_handle.clone();
    
    std::thread::spawn(move || {
        let _ = app_clone.emit(
            "plugin:install-log",
            serde_json::json!({ "name": name_clone, "log": format!("Starting installation of {}...\nExecuting command: {}\n\n", name_clone, cmd_str) })
        );

        // Run via zsh login shell to inherit global path setup
        let mut child = match std::process::Command::new("/bin/zsh")
            .args(["-lc", cmd_str])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = app_clone.emit(
                    "plugin:install-complete",
                    serde_json::json!({ "name": name_clone, "success": false, "error": e.to_string() })
                );
                return;
            }
        };

        // Stream stdout
        let stdout = child.stdout.take().unwrap();
        let app_stdout = app_clone.clone();
        let name_stdout = name_clone.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                if let Ok(l) = line {
                    let _ = app_stdout.emit(
                        "plugin:install-log",
                        serde_json::json!({ "name": name_stdout, "log": format!("{}\n", l) })
                    );
                }
            }
        });

        // Stream stderr
        let stderr = child.stderr.take().unwrap();
        let app_stderr = app_clone.clone();
        let name_stderr = name_clone.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if let Ok(l) = line {
                    let _ = app_stderr.emit(
                        "plugin:install-log",
                        serde_json::json!({ "name": name_stderr, "log": format!("{}\n", l) })
                    );
                }
            }
        });

        // Wait for child to finish
        match child.wait() {
            Ok(status) => {
                if status.success() {
                    let _ = app_clone.emit(
                        "plugin:install-log",
                        serde_json::json!({ "name": name_clone, "log": format!("\n{} installed successfully!\n", name_clone) })
                    );
                    let _ = app_clone.emit(
                        "plugin:install-complete",
                        serde_json::json!({ "name": name_clone, "success": true })
                    );
                } else {
                    let err_msg = format!("Process exited with status code: {}", status.code().unwrap_or(-1));
                    let _ = app_clone.emit(
                        "plugin:install-log",
                        serde_json::json!({ "name": name_clone, "log": format!("\nError: {}\n", err_msg) })
                    );
                    let _ = app_clone.emit(
                        "plugin:install-complete",
                        serde_json::json!({ "name": name_clone, "success": false, "error": err_msg })
                    );
                }
            }
            Err(e) => {
                let _ = app_clone.emit(
                    "plugin:install-complete",
                    serde_json::json!({ "name": name_clone, "success": false, "error": e.to_string() })
                );
            }
        }
    });

    Ok(())
}

/// Uninstall a plugin asynchronously.
/// Streams stdout/stderr via "plugin:install-log" events and emits "plugin:uninstall-complete" on finish.
#[tauri::command]
pub fn plugin_uninstall(name: String, app_handle: tauri::AppHandle) -> Result<()> {
    let name_clone = name.clone();
    let app_clone = app_handle.clone();

    match name.as_str() {
        "ccusage" => {
            std::thread::spawn(move || {
                let _ = app_clone.emit(
                    "plugin:install-log",
                    serde_json::json!({ "name": name_clone, "log": "Uninstalling ccusage via npm...\nExecuting: npm uninstall -g ccusage\n\n" })
                );

                let mut child = match std::process::Command::new("/bin/zsh")
                    .args(["-lc", "npm uninstall -g ccusage"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = app_clone.emit(
                            "plugin:uninstall-complete",
                            serde_json::json!({ "name": name_clone, "success": false, "error": e.to_string() })
                        );
                        return;
                    }
                };

                // Stream stdout
                let stdout = child.stdout.take().unwrap();
                let app_stdout = app_clone.clone();
                let name_stdout = name_clone.clone();
                std::thread::spawn(move || {
                    let reader = BufReader::new(stdout);
                    for line in reader.lines() {
                        if let Ok(l) = line {
                            let _ = app_stdout.emit(
                                "plugin:install-log",
                                serde_json::json!({ "name": name_stdout, "log": format!("{}\n", l) })
                            );
                        }
                    }
                });

                // Stream stderr
                let stderr = child.stderr.take().unwrap();
                let app_stderr = app_clone.clone();
                let name_stderr = name_clone.clone();
                std::thread::spawn(move || {
                    let reader = BufReader::new(stderr);
                    for line in reader.lines() {
                        if let Ok(l) = line {
                            let _ = app_stderr.emit(
                                "plugin:install-log",
                                serde_json::json!({ "name": name_stderr, "log": format!("{}\n", l) })
                            );
                        }
                    }
                });

                // Wait for process to end
                match child.wait() {
                    Ok(status) => {
                        if status.success() {
                            let _ = app_clone.emit(
                                "plugin:install-log",
                                serde_json::json!({ "name": name_clone, "log": "\nccusage uninstalled successfully!\n" })
                            );
                            let _ = app_clone.emit(
                                "plugin:uninstall-complete",
                                serde_json::json!({ "name": name_clone, "success": true })
                            );
                        } else {
                            let err_msg = format!("Process exited with status code: {}", status.code().unwrap_or(-1));
                            let _ = app_clone.emit(
                                "plugin:install-log",
                                serde_json::json!({ "name": name_clone, "log": format!("\nError: {}\n", err_msg) })
                            );
                            let _ = app_clone.emit(
                                "plugin:uninstall-complete",
                                serde_json::json!({ "name": name_clone, "success": false, "error": err_msg })
                            );
                        }
                    }
                    Err(e) => {
                        let _ = app_clone.emit(
                            "plugin:uninstall-complete",
                            serde_json::json!({ "name": name_clone, "success": false, "error": e.to_string() })
                        );
                    }
                }
            });
        }
        "rtk" | "codegraph" => {
            std::thread::spawn(move || {
                let home = match dirs::home_dir() {
                    Some(h) => h,
                    None => {
                        let _ = app_clone.emit(
                            "plugin:uninstall-complete",
                            serde_json::json!({ "name": name_clone, "success": false, "error": "Could not find home directory" })
                        );
                        return;
                    }
                };
                let local_bin = home.join(".local").join("bin").join(&name_clone);
                let _ = app_clone.emit(
                    "plugin:install-log",
                    serde_json::json!({ "name": name_clone, "log": format!("Deleting binary file at: {}\n\n", local_bin.display()) })
                );

                if local_bin.exists() {
                    match std::fs::remove_file(&local_bin) {
                        Ok(_) => {
                            let _ = app_clone.emit(
                                "plugin:install-log",
                                serde_json::json!({ "name": name_clone, "log": format!("Deleted binary file for {}.\n", name_clone) })
                            );
                            let _ = app_clone.emit(
                                "plugin:uninstall-complete",
                                serde_json::json!({ "name": name_clone, "success": true })
                            );
                        }
                        Err(e) => {
                            let _ = app_clone.emit(
                                "plugin:uninstall-complete",
                                serde_json::json!({ "name": name_clone, "success": false, "error": e.to_string() })
                            );
                        }
                    }
                } else {
                    let _ = app_clone.emit(
                        "plugin:install-log",
                        serde_json::json!({ "name": name_clone, "log": format!("Binary file for {} does not exist in ~/.local/bin/\n", name_clone) })
                    );
                    let _ = app_clone.emit(
                        "plugin:uninstall-complete",
                        serde_json::json!({ "name": name_clone, "success": true })
                    );
                }
            });
        }
        _ => return Err(Error::InvalidInput(format!("Unknown plugin: {name}"))),
    }

    Ok(())
}
