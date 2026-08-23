//! macOS 原生 SystemApplicationDriver 实现（Phase E，APP-030 ~ APP-038 /
//! APPV2-T06 观察/激活/隐藏/正常终止补全）。
//!
//! 基于 Cocoa / AppKit 原生 API（`NSWorkspace` / `NSRunningApplication`）：
//! - **bundle ID 优先**（注册侧 Info.plist `CFBundleIdentifier`）+ **bundle 路径
//!   回退**（`NSRunningApplication.bundleURL`）识别运行实例，App 移动/升级后
//!   仍可按 bundle ID 重新定位，杜绝模糊匹配；
//! - 严格禁止 `killall` / `pkill` / 模糊进程名强杀；
//! - 区分 `managed`（由本宿主发起）与 `preexisting`（外部已存在）所有权；
//! - launch 后做**有上限的真实 observe 重试**确认运行（不再固定 300ms 假就绪）；
//! - terminate 后**验证进程消失**（~5s 上限），超时返回 typed 错误；
//! - `isHidden` / `isActive` 真实状态；读不到时 `unobservable=true`（不算失败）；
//! - 遵循 R-P2，系统应用目录扫描运行在 `spawn_blocking` 中。
//!
//! 真机行为（observe 命中 / hide / terminate 时序）依赖 NSWorkspace，不在
//! 单测覆盖 —— 由 T11 macOS packaged smoke 手工矩阵验证。

#![cfg(target_os = "macos")]
#![allow(unexpected_cfgs)]

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use core_foundation::array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef};
use core_foundation::base::{CFRelease, CFRetain, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::string::{CFString, CFStringRef};
use objc::runtime::{Class, Object};
use objc::{msg_send, sel, sel_impl};
use rusqlite::Connection;
use tauri::AppHandle;

use super::{
    state_from_observation, validate_dock_rect, wait_until_true, wait_until_true_before_timeout,
    DockCapability, DockResult, DockStatus, LiveProcessObs, SystemAppCandidate, SystemDockRect,
    SystemDriver, SystemLaunchResult, SystemRunningIdentity, SystemRunningState,
    LAUNCH_CONFIRM_ATTEMPTS, LAUNCH_CONFIRM_POLL, TERMINATE_VERIFY_POLL, TERMINATE_VERIFY_POLLS,
};
use crate::{Error, Result};

pub struct MacosSystemDriver;

type AxUiElementRef = *const std::ffi::c_void;
type AxValueRef = *const std::ffi::c_void;
type AxError = i32;

const AX_OK: AxError = 0;
const AX_VALUE_CG_POINT: u32 = 1;
const AX_VALUE_CG_SIZE: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct CgPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct CgSize {
    width: f64,
    height: f64,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateApplication(pid: libc::pid_t) -> AxUiElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AxUiElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AxError;
    fn AXUIElementIsAttributeSettable(
        element: AxUiElementRef,
        attribute: CFStringRef,
        settable: *mut u8,
    ) -> AxError;
    fn AXUIElementSetAttributeValue(
        element: AxUiElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AxError;
    fn AXValueCreate(value_type: u32, value: *const std::ffi::c_void) -> AxValueRef;
    fn AXValueGetValue(value: AxValueRef, value_type: u32, output: *mut std::ffi::c_void) -> bool;
}

impl MacosSystemDriver {
    pub fn new() -> Self {
        Self
    }
}

// ── 辅助函数：NSString / NSURL 桥接 ─────────────────────────────────────

/// 创建 owned NSString（调用方负责 drop；供本模块与 `mod.rs::read_bundle_identifier`
/// 共用）。
pub(crate) fn new_nsstring(s: &str) -> NSStringWrapper {
    NSStringWrapper::new(s)
}

/// NSString → Rust String（null/不可读 = None）。
pub(crate) fn nsstring_to_rust(ns_str: *mut Object) -> Option<String> {
    unsafe { nsstring_to_string(ns_str) }
}

pub(crate) struct NSStringWrapper(*mut Object);

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

    /// 底层指针（`mod.rs::read_bundle_identifier` 跨模块复用）。
    pub(crate) fn as_ptr(&self) -> *mut Object {
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

pub(crate) unsafe fn nsstring_to_string(ns_str: *mut Object) -> Option<String> {
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

// ── APPV2-T06 匹配与控制辅助 ─────────────────────────────────────────────

/// terminate 验证窗口总时长（秒，用于 typed 超时错误文案）。
pub(crate) fn terminate_verify_seconds() -> u32 {
    u32::try_from(u128::from(TERMINATE_VERIFY_POLLS) * TERMINATE_VERIFY_POLL.as_millis() / 1000)
        .unwrap_or(5)
}

/// 解析目标 bundle id：注册侧已知优先，否则从 `.app/Contents/Info.plist`
/// 读 `CFBundleIdentifier`（两者皆无 = None → 纯路径回退匹配）。
fn resolve_bundle_id(known: Option<&str>, app_path: &str) -> Option<String> {
    known
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| super::read_bundle_identifier(app_path))
}

/// 在 `NSWorkspace.runningApplications` 中定位目标实例。
///
/// 匹配优先级（APPV2-T06：bundle ID 优先 + 路径回退）：
/// 1. `bundleIdentifier == bundle_id`（App 移动/升级后仍能定位）；
/// 2. `bundleURL.path == app_path`（bundle ID 未知或无匹配时的回退）。
///
/// 只跳过 `isTerminated` 的僵尸条目，绝不模糊匹配进程名。
fn find_live_process(app_path: &str, bundle_id: Option<&str>) -> Option<LiveProcessObs> {
    unsafe {
        let cls_ws = Class::get("NSWorkspace")?;
        let ws: *mut Object = msg_send![cls_ws, sharedWorkspace];
        let running_apps: *mut Object = msg_send![ws, runningApplications];
        if running_apps.is_null() {
            return None;
        }

        let count: usize = msg_send![running_apps, count];
        let mut path_match: Option<LiveProcessObs> = None;
        for i in 0..count {
            let r_app: *mut Object = msg_send![running_apps, objectAtIndex: i];
            let is_term: bool = msg_send![r_app, isTerminated];
            if is_term {
                continue;
            }

            let pid: i32 = msg_send![r_app, processIdentifier];
            let b_id: *mut Object = msg_send![r_app, bundleIdentifier];
            let rid = nsstring_to_string(b_id);
            let b_url: *mut Object = msg_send![r_app, bundleURL];
            let r_path = if b_url.is_null() {
                None
            } else {
                let p: *mut Object = msg_send![b_url, path];
                nsstring_to_string(p)
            };

            let obs = LiveProcessObs {
                pid: pid as u32,
                bundle_id: rid.clone(),
                bundle_path: r_path.clone(),
                hidden: false,
                active: false,
                unobservable: false,
            };

            // 1. bundle ID 精确命中（优先，且立即胜出）
            if let (Some(bid), Some(rid)) = (bundle_id, rid.as_deref()) {
                if !bid.is_empty() && rid == bid {
                    return Some(fill_window_state(r_app, obs));
                }
            }
            // 2. bundle 路径精确命中（记录首个，继续找 bundle ID 命中）
            if path_match.is_none() {
                match r_path.as_deref() {
                    Some(rp) if rp == app_path => {
                        path_match = Some(fill_window_state(r_app, obs));
                    }
                    _ => {}
                }
            }
        }
        path_match
    }
}

/// 读取 `NSRunningApplication` 的 `isHidden` / `isActive` 真实状态。
///
/// 任一属性方法不存在（`respondsToSelector:` = false）→ `unobservable=true`
/// + 默认值，不视为 observe 失败（T06 验收：unobservable 可被区分；AX 属性
/// 读不到不算失败）。
fn fill_window_state(r_app: *mut Object, mut obs: LiveProcessObs) -> LiveProcessObs {
    unsafe {
        let can_hidden: bool = msg_send![r_app, respondsToSelector: sel!(isHidden)];
        let can_active: bool = msg_send![r_app, respondsToSelector: sel!(isActive)];
        if can_hidden && can_active {
            let hidden: bool = msg_send![r_app, isHidden];
            let active: bool = msg_send![r_app, isActive];
            obs.hidden = hidden;
            obs.active = active;
        } else {
            obs.unobservable = true;
        }
        obs
    }
}

/// 对运行实例执行 `activateWithOptions:`（`NSApplicationActivateIgnoringOtherApps`）。
/// 返回是否真实执行到目标对象。
fn activate_running(r_app: Option<*mut Object>) -> bool {
    let Some(r_app) = r_app else {
        return false;
    };
    unsafe {
        // NSApplicationActivateIgnoringOtherApps = 1 << 1
        let () = msg_send![r_app, activateWithOptions: 2usize];
    }
    true
}

/// 取运行实例对象句柄（pid → `NSRunningApplication.runningApplicationWithProcessIdentifier:`）。
/// 类缺失（不应发生）= None，映射为 typed 状态而非 panic（T06 step 7）。
fn running_app_for_pid(pid: u32) -> Option<*mut Object> {
    unsafe {
        let cls_run = Class::get("NSRunningApplication")?;
        let running_app: *mut Object =
            msg_send![cls_run, runningApplicationWithProcessIdentifier: pid as i32];
        if running_app.is_null() {
            None
        } else {
            Some(running_app)
        }
    }
}

/// hide/unhide 通用执行体：目标未运行 = `Ok(false)`（无副作用，不算错误）；
/// 运行中则调用 AppKit 实例方法，返回是否真实执行。
///
/// - hide → `NSRunningApplication.hide:`（AppKit 标准隐藏，等价 ⌘H）；
/// - unhide → `NSRunningApplication.activateWithOptions:`（AppKit 无独立
///   unhide 方法：被隐藏应用被激活时自动解除隐藏并回到前台）。
///
/// force 能力不在此暴露 —— 单独保留在 `force_terminate`（新 UI 不接线）。
fn hide_or_unhide(app_path: &str, bundle_id: Option<&str>, hide: bool) -> Result<bool> {
    let bundle_id = resolve_bundle_id(bundle_id, app_path);
    let bid_ref = bundle_id.as_deref();
    let Some(obs) = find_live_process(app_path, bid_ref) else {
        return Ok(false); // 未运行：hide/unhide 无目标，真实执行=false
    };
    let Some(r_app) = running_app_for_pid(obs.pid) else {
        return Ok(false);
    };
    unsafe {
        if hide {
            let () = msg_send![r_app, hide];
        } else {
            let () = msg_send![r_app, activateWithOptions: 2usize];
        }
    }
    Ok(true)
}

fn dock_result(
    status: DockStatus,
    capability: DockCapability,
    message: Option<String>,
) -> DockResult {
    DockResult {
        status,
        capability,
        message,
    }
}

unsafe fn ax_copy(element: AxUiElementRef, attribute: &str) -> Option<CFTypeRef> {
    let key = CFString::new(attribute);
    let mut value: CFTypeRef = std::ptr::null();
    if AXUIElementCopyAttributeValue(element, key.as_concrete_TypeRef(), &mut value) == AX_OK
        && !value.is_null()
    {
        Some(value)
    } else {
        None
    }
}

unsafe fn ax_string(element: AxUiElementRef, attribute: &str) -> Option<String> {
    let value = ax_copy(element, attribute)?;
    let string = CFString::wrap_under_create_rule(value as CFStringRef).to_string();
    Some(string)
}

unsafe fn ax_bool(element: AxUiElementRef, attribute: &str) -> Option<bool> {
    let value = ax_copy(element, attribute)?;
    Some(bool::from(CFBoolean::wrap_under_create_rule(value as _)))
}

unsafe fn ax_settable(element: AxUiElementRef, attribute: &str) -> bool {
    let key = CFString::new(attribute);
    let mut settable = 0u8;
    AXUIElementIsAttributeSettable(element, key.as_concrete_TypeRef(), &mut settable) == AX_OK
        && settable != 0
}

unsafe fn standard_window(app: AxUiElementRef) -> Option<AxUiElementRef> {
    for attribute in ["AXFocusedWindow", "AXMainWindow"] {
        if let Some(window) = ax_copy(app, attribute) {
            if ax_string(window, "AXSubrole").as_deref() == Some("AXStandardWindow") {
                return Some(window);
            }
            CFRelease(window);
        }
    }

    let windows = ax_copy(app, "AXWindows")?;
    let array = windows as CFArrayRef;
    let mut chosen = None;
    for index in 0..CFArrayGetCount(array) {
        let window = CFArrayGetValueAtIndex(array, index) as AxUiElementRef;
        if ax_string(window, "AXSubrole").as_deref() == Some("AXStandardWindow") {
            CFRetain(window);
            chosen = Some(window);
            break;
        }
    }
    CFRelease(windows);
    chosen
}

unsafe fn ax_value_point(value: AxValueRef) -> Option<CgPoint> {
    let mut point = CgPoint { x: 0.0, y: 0.0 };
    if AXValueGetValue(value, AX_VALUE_CG_POINT, &mut point as *mut _ as *mut _) {
        Some(point)
    } else {
        None
    }
}

unsafe fn ax_value_size(value: AxValueRef) -> Option<CgSize> {
    let mut size = CgSize {
        width: 0.0,
        height: 0.0,
    };
    if AXValueGetValue(value, AX_VALUE_CG_SIZE, &mut size as *mut _ as *mut _) {
        Some(size)
    } else {
        None
    }
}

unsafe fn dock_window(window: AxUiElementRef, rect: SystemDockRect) -> DockResult {
    if ax_bool(window, "AXFullScreen") == Some(true) {
        return dock_result(
            DockStatus::Unsupported,
            DockCapability::FullscreenUnsupported,
            None,
        );
    }
    if !ax_settable(window, "AXPosition") {
        return dock_result(DockStatus::Unsupported, DockCapability::NotMovable, None);
    }
    if !ax_settable(window, "AXSize") {
        return dock_result(DockStatus::Unsupported, DockCapability::NotResizable, None);
    }

    let point = CgPoint {
        x: rect.x,
        y: rect.y,
    };
    let size = CgSize {
        width: rect.width,
        height: rect.height,
    };
    let point_value = AXValueCreate(AX_VALUE_CG_POINT, &point as *const _ as *const _);
    let size_value = AXValueCreate(AX_VALUE_CG_SIZE, &size as *const _ as *const _);
    if point_value.is_null() || size_value.is_null() {
        if !point_value.is_null() {
            CFRelease(point_value);
        }
        if !size_value.is_null() {
            CFRelease(size_value);
        }
        return dock_result(
            DockStatus::Failed,
            DockCapability::Available,
            Some("Unable to create AX window values".into()),
        );
    }
    let position_error = AXUIElementSetAttributeValue(
        window,
        CFString::new("AXPosition").as_concrete_TypeRef(),
        point_value,
    );
    let size_error = AXUIElementSetAttributeValue(
        window,
        CFString::new("AXSize").as_concrete_TypeRef(),
        size_value,
    );
    CFRelease(point_value);
    CFRelease(size_value);
    if position_error != AX_OK || size_error != AX_OK {
        return dock_result(
            DockStatus::Failed,
            DockCapability::Available,
            Some(format!(
                "AX window update failed ({position_error}/{size_error})"
            )),
        );
    }

    let actual_point = ax_copy(window, "AXPosition").and_then(|value| {
        let result = ax_value_point(value as AxValueRef);
        CFRelease(value);
        result
    });
    let actual_size = ax_copy(window, "AXSize").and_then(|value| {
        let result = ax_value_size(value as AxValueRef);
        CFRelease(value);
        result
    });
    let matches = actual_point.zip(actual_size).is_some_and(|(p, s)| {
        (p.x - rect.x).abs() <= 2.0
            && (p.y - rect.y).abs() <= 2.0
            && (s.width - rect.width).abs() <= 2.0
            && (s.height - rect.height).abs() <= 2.0
    });
    if matches {
        dock_result(DockStatus::Docked, DockCapability::Available, None)
    } else {
        dock_result(
            DockStatus::Failed,
            DockCapability::Available,
            Some("Window did not accept the requested bounds".into()),
        )
    }
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
        bundle_id: Option<&str>,
    ) -> Result<SystemLaunchResult> {
        let app_path = Path::new(path);
        if !app_path.exists() {
            // 路径失效 = not_installed（typed，UI 可恢复重新选择）。
            return Err(Error::NotFound(format!(
                "application path does not exist: {path}"
            )));
        }

        let bid = resolve_bundle_id(bundle_id, path);
        let bid_ref = bid.as_deref();

        // 1. 已有运行实例 → 前台激活（launched=false, confirmed=true）
        if let Some(obs) = find_live_process(path, bid_ref) {
            let activated = activate_running(running_app_for_pid(obs.pid));
            let state = state_from_observation(path, Some(&obs), true);
            return Ok(SystemLaunchResult {
                identity: SystemRunningIdentity {
                    bundle_id: state.bundle_id,
                    path: state
                        .bundle_path
                        .clone()
                        .unwrap_or_else(|| path.to_string()),
                    pid: state.pid,
                    ownership: "preexisting".into(),
                },
                launched: false,
                // 激活是同步 AppKit 调用：实例本就运行，激活成功即确认。
                confirmed: activated,
            });
        }

        // 2. 未运行：NSWorkspace 启动（launchApplication）。
        //    NSWorkspace/NSURL 类缺失（不应发生）→ typed NotFound，不 panic
        //    （T06 step 7：Objective-C 错误映射为 typed Rust error）。
        let (ws, url) = unsafe {
            let Some(cls_ws) = Class::get("NSWorkspace") else {
                return Err(Error::NotFound(format!(
                    "NSWorkspace unavailable; cannot launch {path}"
                )));
            };
            let ws: *mut Object = msg_send![cls_ws, sharedWorkspace];
            let path_ns = NSStringWrapper::new(path);
            let Some(cls_url) = Class::get("NSURL") else {
                return Err(Error::NotFound(format!(
                    "NSURL unavailable; cannot launch {path}"
                )));
            };
            let url: *mut Object = msg_send![cls_url, fileURLWithPath: path_ns.as_ptr()];
            (ws, url)
        };
        unsafe {
            let cls_conf = Class::get("NSWorkspaceOpenConfiguration");
            if let Some(conf_cls) = cls_conf {
                let conf: *mut Object = msg_send![conf_cls, configuration];
                let () = msg_send![conf, setActivates: true];
                let () = msg_send![ws, openURL:url configuration:conf completionHandler:std::ptr::null::<()>()];
            } else {
                let () = msg_send![ws, openURL: url];
            }
        }

        // 3. APPV2-T06：不再固定 sleep(300ms) 宣称就绪 —— 有上限的真实
        // observe 重试（20 × 150ms ≈ 3s）直到 running=true。超时不失败：
        // launched=true / confirmed=false（UI 显示 starting，下次 observe 收敛）。
        let confirmed = wait_until_true(
            || async { find_live_process(path, bid_ref).is_some() },
            LAUNCH_CONFIRM_ATTEMPTS,
            Some(LAUNCH_CONFIRM_POLL),
        )
        .await;

        let state = self.observe(path, bundle_id).await?;
        Ok(SystemLaunchResult {
            identity: SystemRunningIdentity {
                bundle_id: state.bundle_id.clone(),
                path: state
                    .bundle_path
                    .clone()
                    .unwrap_or_else(|| path.to_string()),
                pid: state.pid,
                ownership: "managed".into(),
            },
            launched: true,
            confirmed,
        })
    }

    async fn terminate(
        &self,
        _app: &AppHandle,
        _application_id: &str,
        path: &str,
        bundle_id: Option<&str>,
    ) -> Result<()> {
        let bid = resolve_bundle_id(bundle_id, path);
        let bid_ref = bid.as_deref();

        let Some(obs) = find_live_process(path, bid_ref) else {
            // 未运行 = 已处于停止态（幂等，与 stop 既有口径一致）。
            return Ok(());
        };

        // graceful：NSWorkspace 语义的 `terminate`（等价 ⌘Q 请求）。
        let Some(r_app) = running_app_for_pid(obs.pid) else {
            return Ok(());
        };
        unsafe {
            let () = msg_send![r_app, terminate];
        }

        // APPV2-T06：终止后必须验证进程消失（~5s 上限）；超时返回 typed Err，
        // 不再无条件 Ok。
        let gone = wait_until_true_before_timeout(
            || async { find_live_process(path, bid_ref).is_none() },
            TERMINATE_VERIFY_POLLS,
            Some(TERMINATE_VERIFY_POLL),
        )
        .await;
        if !gone {
            return Err(Error::Internal(format!(
                "terminate timed out: process {pid} of {path} is still running after {sec}s (use force stop)",
                pid = obs.pid,
                sec = terminate_verify_seconds()
            )));
        }
        Ok(())
    }

    async fn force_terminate(
        &self,
        _app: &AppHandle,
        _application_id: &str,
        identity: &SystemRunningIdentity,
    ) -> Result<()> {
        // 底层强杀能力保留（不扩展 kill -9 之外的行为；新 UI 不暴露）。
        // 与 graceful terminate 同口径：先验证消失，超时返回 typed Err。
        let bid = identity.bundle_id.clone();
        let bid_ref = bid.as_deref();
        let live = find_live_process(&identity.path, bid_ref).or_else(|| {
            identity.pid.map(|pid| LiveProcessObs {
                pid,
                bundle_id: None,
                bundle_path: None,
                hidden: false,
                active: false,
                unobservable: true,
            })
        });
        let Some(obs) = live else {
            return Ok(()); // 未运行 = 已停止（幂等）
        };

        let Some(r_app) = running_app_for_pid(obs.pid) else {
            return Ok(());
        };
        unsafe {
            let () = msg_send![r_app, forceTerminate];
        }

        let gone = wait_until_true_before_timeout(
            || async {
                find_live_process(&identity.path, bid_ref).is_none()
                    && running_app_for_pid(obs.pid).is_none()
            },
            TERMINATE_VERIFY_POLLS,
            Some(TERMINATE_VERIFY_POLL),
        )
        .await;
        if !gone {
            return Err(Error::Internal(format!(
                "force terminate timed out: process {} of {} still running",
                obs.pid, identity.path
            )));
        }
        Ok(())
    }

    async fn observe(&self, path: &str, bundle_id: Option<&str>) -> Result<SystemRunningState> {
        // 安装判定：注册路径仍存在？（路径失效 = not_installed，typed）
        let installed = Path::new(path).exists();
        let bid = resolve_bundle_id(bundle_id, path);
        let bid_ref = bid.as_deref();
        let live = find_live_process(path, bid_ref);
        // 进程存在时 installed 必为 true（bundle 路径可能已移动 —— 以实例为准）。
        Ok(state_from_observation(
            path,
            live.as_ref(),
            installed || live.is_some(),
        ))
    }

    async fn hide(&self, path: &str, bundle_id: Option<&str>) -> Result<bool> {
        hide_or_unhide(path, bundle_id, true)
    }

    async fn unhide(&self, path: &str, bundle_id: Option<&str>) -> Result<bool> {
        hide_or_unhide(path, bundle_id, false)
    }

    async fn dock(
        &self,
        path: &str,
        bundle_id: Option<&str>,
        rect: SystemDockRect,
    ) -> Result<DockResult> {
        let rect = validate_dock_rect(rect)?;
        if unsafe { !AXIsProcessTrusted() } {
            return Ok(dock_result(
                DockStatus::PermissionRequired,
                DockCapability::PermissionRequired,
                None,
            ));
        }
        let bid = resolve_bundle_id(bundle_id, path);
        let Some(live) = find_live_process(path, bid.as_deref()) else {
            return Err(Error::NotFound(format!(
                "system application is not running: {path}"
            )));
        };
        unsafe {
            let app = AXUIElementCreateApplication(live.pid as libc::pid_t);
            if app.is_null() {
                return Ok(dock_result(
                    DockStatus::Failed,
                    DockCapability::Available,
                    Some("Unable to inspect the application window".into()),
                ));
            }
            let result = match standard_window(app) {
                Some(window) => {
                    let result = dock_window(window, rect);
                    CFRelease(window);
                    result
                }
                None => dock_result(
                    DockStatus::Unsupported,
                    DockCapability::NoStandardWindow,
                    None,
                ),
            };
            CFRelease(app);
            Ok(result)
        }
    }
}

#[cfg(test)]
mod tests {
    //! APPV2-T06：macOS 驱动的真机行为（observe 命中 / hide / terminate 时序）
    //! 依赖 NSWorkspace / AppKit，单测环境不强行覆盖 —— 由 T11 macOS packaged
    //! smoke 手工矩阵验证（见 apps-center-v2-tasks.md T11）。
    //! 平台无关的匹配/状态映射/重试逻辑在 `super::super::tests` 覆盖。

    use super::*;

    /// bundle ID 优先于路径回退（纯参数优先级逻辑，不涉及真机调用）。
    #[test]
    fn resolve_bundle_id_prefers_registered_and_trims() {
        assert_eq!(
            resolve_bundle_id(Some("  com.foo "), "/does/not/matter"),
            Some("com.foo".into())
        );
        assert_eq!(resolve_bundle_id(Some(""), "/does/not/matter"), None);
        assert_eq!(resolve_bundle_id(None, "/does/not/matter"), None);
    }

    /// terminate 超时的 typed 错误必须携带可诊断信息（进程 pid + 路径）。
    #[test]
    fn terminate_timeout_error_is_typed() {
        let err = Error::Internal(format!(
            "terminate timed out: process {pid} of {path} is still running after {sec}s (use force stop)",
            pid = 1234u32,
            path = "/Applications/Fake.app",
            sec = terminate_verify_seconds()
        ));
        let msg = err.to_string();
        assert!(msg.contains("1234") && msg.contains("/Applications/Fake.app"));
        assert!(msg.contains("5s"));
    }
}
