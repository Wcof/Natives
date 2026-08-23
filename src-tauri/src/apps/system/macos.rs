//! macOS 原生 SystemApplicationDriver 实现（Phase E，APP-030 ~ APP-038）。
//!
//! 基于 Cocoa / AppKit 原生 API（`NSWorkspace` / `NSRunningApplication`）：
//! - 通过 Bundle ID 与 `.app` 路径精准匹配身份，杜绝模糊匹配；
//! - 严格禁止 `killall` / `pkill` / 模糊进程名强杀；
//! - 区分 `managed`（由本宿主发起）与 `preexisting`（外部已存在）所有权；
//! - 遵循 R-P2，系统应用目录扫描运行在 `spawn_blocking` 中。

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use objc::runtime::{Class, Object};
use objc::{msg_send, sel, sel_impl};
use rusqlite::Connection;
use tauri::AppHandle;

use super::{SystemAppCandidate, SystemDriver, SystemRunningIdentity};
use crate::{Error, Result};

pub struct MacosSystemDriver;

impl MacosSystemDriver {
    pub fn new() -> Self {
        Self
    }
}

// ── 辅助函数：NSString / NSURL 桥接 ─────────────────────────────────────

struct NSStringWrapper(*mut Object);

impl NSStringWrapper {
    fn new(s: &str) -> Self {
        unsafe {
            let cls = Class::get("NSString").expect("NSString class not found");
            let bytes = s.as_ptr() as *const std::ffi::c_void;
            let obj: *mut Object = msg_send![cls, alloc];
            // NSUTF8StringEncoding = 4
            let obj: *mut Object =
                msg_send![obj, initWithBytes:bytes length:s.len() encoding:4usize];
            Self(obj)
        }
    }

    fn as_ptr(&self) -> *mut Object {
        self.0
    }
}

impl Drop for NSStringWrapper {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let () = msg_send![self.0, release];
            }
        }
    }
}

unsafe fn nsstring_to_string(ns_str: *mut Object) -> Option<String> {
    if ns_str.is_null() {
        return None;
    }
    let utf8_ptr: *const std::ffi::c_char = msg_send![ns_str, UTF8String];
    if utf8_ptr.is_null() {
        return None;
    }
    Some(
        std::ffi::CStr::from_ptr(utf8_ptr)
            .to_string_lossy()
            .into_owned(),
    )
}

#[async_trait]
impl SystemDriver for MacosSystemDriver {
    fn force_terminate_supported(&self) -> bool {
        true
    }

    fn discover(&self, _conn: &Connection) -> Result<Vec<SystemAppCandidate>> {
        let scan_dirs = vec![
            PathBuf::from("/Applications"),
            PathBuf::from("/System/Applications"),
            dirs::home_dir()
                .map(|h| h.join("Applications"))
                .unwrap_or_default(),
        ];

        let mut candidates = Vec::new();
        let mut seen_paths = std::collections::HashSet::new();

        for dir in scan_dirs {
            if !dir.exists() || !dir.is_dir() {
                continue;
            }

            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("app") {
                        let path_str = path.to_string_lossy().to_string();
                        if seen_paths.contains(&path_str) {
                            continue;
                        }
                        seen_paths.insert(path_str.clone());

                        let info_plist = path.join("Contents/Info.plist");
                        let mut display_name = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("Unknown")
                            .to_string();
                        let mut bundle_id = None;

                        if info_plist.exists() {
                            unsafe {
                                let path_ns = NSStringWrapper::new(&info_plist.to_string_lossy());
                                let cls =
                                    Class::get("NSDictionary").expect("NSDictionary not found");
                                let dict: *mut Object =
                                    msg_send![cls, dictionaryWithContentsOfFile: path_ns.as_ptr()];
                                if !dict.is_null() {
                                    let key_bid = NSStringWrapper::new("CFBundleIdentifier");
                                    let bid_obj: *mut Object =
                                        msg_send![dict, objectForKey: key_bid.as_ptr()];
                                    bundle_id = nsstring_to_string(bid_obj);

                                    let key_name = NSStringWrapper::new("CFBundleDisplayName");
                                    let name_obj: *mut Object =
                                        msg_send![dict, objectForKey: key_name.as_ptr()];
                                    if let Some(n) = nsstring_to_string(name_obj) {
                                        if !n.trim().is_empty() {
                                            display_name = n;
                                        }
                                    } else {
                                        let key_cname = NSStringWrapper::new("CFBundleName");
                                        let cname_obj: *mut Object =
                                            msg_send![dict, objectForKey: key_cname.as_ptr()];
                                        if let Some(n) = nsstring_to_string(cname_obj) {
                                            if !n.trim().is_empty() {
                                                display_name = n;
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        candidates.push(SystemAppCandidate {
                            bundle_id,
                            path: path_str,
                            display_name,
                            icon_path: None,
                        });
                    }
                }
            }
        }

        candidates.sort_by(|a, b| a.display_name.cmp(&b.display_name));
        Ok(candidates)
    }

    async fn launch_or_activate(
        &self,
        _app: &AppHandle,
        _application_id: &str,
        path: &str,
    ) -> Result<SystemRunningIdentity> {
        let app_path = Path::new(path);
        if !app_path.exists() {
            return Err(Error::NotFound(format!(
                "application path does not exist: {path}"
            )));
        }

        // 1. 先检查是否已有运行中的实例（通过 NSWorkspace runningApplications）
        let existing = self.observe(path).await?;
        if let Some(mut ident) = existing {
            // 已存在外部/前序实例：将其前台激活
            unsafe {
                let cls_run =
                    Class::get("NSRunningApplication").expect("NSRunningApplication not found");
                if let Some(pid) = ident.pid {
                    let running_app: *mut Object =
                        msg_send![cls_run, runningApplicationWithProcessIdentifier: pid as i32];
                    if !running_app.is_null() {
                        // NSApplicationActivateIgnoringOtherApps = 1 << 1
                        let () = msg_send![running_app, activateWithOptions: 2usize];
                    }
                }
            }

            ident.ownership = "preexisting".into();
            return Ok(ident);
        }

        // 2. 未运行：通过 NSWorkspace launchApplication
        unsafe {
            let cls_ws = Class::get("NSWorkspace").expect("NSWorkspace not found");
            let ws: *mut Object = msg_send![cls_ws, sharedWorkspace];
            let path_ns = NSStringWrapper::new(path);
            let cls_url = Class::get("NSURL").expect("NSURL not found");
            let url: *mut Object = msg_send![cls_url, fileURLWithPath: path_ns.as_ptr()];

            let cls_conf = Class::get("NSWorkspaceOpenConfiguration");
            if let Some(conf_cls) = cls_conf {
                let conf: *mut Object = msg_send![conf_cls, configuration];
                let () = msg_send![conf, setActivates: true];
                let () = msg_send![ws, openURL:url configuration:conf completionHandler:std::ptr::null::<()>()];
            } else {
                let () = msg_send![ws, openURL: url];
            }
        }

        // 等待应用就绪并获取 PID
        tokio::time::sleep(Duration::from_millis(300)).await;

        let observed = self.observe(path).await?;
        let (pid, bundle_id) = match observed {
            Some(i) => (i.pid, i.bundle_id),
            None => (None, None),
        };

        Ok(SystemRunningIdentity {
            bundle_id,
            path: path.to_string(),
            pid,
            ownership: "managed".into(),
        })
    }

    async fn terminate(&self, _app: &AppHandle, _application_id: &str, path: &str) -> Result<()> {
        let live = self.observe(path).await?;
        if let Some(ident) = live {
            if let Some(pid) = ident.pid {
                unsafe {
                    let cls_run =
                        Class::get("NSRunningApplication").expect("NSRunningApplication not found");
                    let running_app: *mut Object =
                        msg_send![cls_run, runningApplicationWithProcessIdentifier: pid as i32];
                    if !running_app.is_null() {
                        let _ok: bool = msg_send![running_app, terminate];
                    }
                }

                // 验证进程是否在 3 秒内退出
                for _ in 0..15 {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    if self.observe(path).await?.is_none() {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    async fn force_terminate(
        &self,
        _app: &AppHandle,
        _application_id: &str,
        identity: &SystemRunningIdentity,
    ) -> Result<()> {
        if let Some(pid) = identity.pid {
            unsafe {
                let cls_run =
                    Class::get("NSRunningApplication").expect("NSRunningApplication not found");
                let running_app: *mut Object =
                    msg_send![cls_run, runningApplicationWithProcessIdentifier: pid as i32];
                if !running_app.is_null() {
                    let _ok: bool = msg_send![running_app, forceTerminate];
                }
            }

            for _ in 0..10 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                if self.observe(&identity.path).await?.is_none() {
                    break;
                }
            }
        }

        Ok(())
    }

    async fn observe(&self, path: &str) -> Result<Option<SystemRunningIdentity>> {
        let path_owned = path.to_string();
        let res = unsafe {
            let cls_ws = Class::get("NSWorkspace").expect("NSWorkspace not found");
            let ws: *mut Object = msg_send![cls_ws, sharedWorkspace];
            let running_apps: *mut Object = msg_send![ws, runningApplications];
            if running_apps.is_null() {
                return Ok(None);
            }

            let mut matched = None;
            let count: usize = msg_send![running_apps, count];
            for i in 0..count {
                let r_app: *mut Object = msg_send![running_apps, objectAtIndex: i];
                let is_term: bool = msg_send![r_app, isTerminated];
                if is_term {
                    continue;
                }

                let b_url: *mut Object = msg_send![r_app, bundleURL];
                if !b_url.is_null() {
                    let b_path: *mut Object = msg_send![b_url, path];
                    if let Some(p_str) = nsstring_to_string(b_path) {
                        if p_str == path_owned {
                            let pid: i32 = msg_send![r_app, processIdentifier];
                            let b_id: *mut Object = msg_send![r_app, bundleIdentifier];
                            let bundle_id = nsstring_to_string(b_id);

                            matched = Some(SystemRunningIdentity {
                                bundle_id,
                                path: path_owned.clone(),
                                pid: Some(pid as u32),
                                ownership: "preexisting".into(),
                            });
                            break;
                        }
                    }
                }
            }
            matched
        };

        Ok(res)
    }
}
