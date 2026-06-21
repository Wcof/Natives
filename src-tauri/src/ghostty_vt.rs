//! Ghostty VT 引擎 FFI 绑定
//!
//! 本模块提供 libghostty-vt 静态库的 Rust 封装，用于将 Ghostty 的 VT 解析器
//! 集成到 Natives 终端中，实现：OSC 7/133、title/pwd 事件、光标追踪、渲染状态。
//!
//! 绑定基于 Ghostty commit: (待锁定 — 见 build.rs References/ghostty)
//!
//! Feature gate: `ghostty-vt` — 仅在 Cargo.toml 中启用 feat 时编译。

#![allow(non_camel_case_types, dead_code)]

use libc::{c_char, c_int, c_uint, c_void, size_t};
use std::ffi::{CStr, CString};
use std::ptr;
use tauri::{AppHandle, Emitter};

// ── FFI 常量 ──────────────────────────────────────────────────────────────

/// Ghostty VT API 函数返回码
type GhosttyResult = c_int;

const GHOSTTY_SUCCESS: GhosttyResult = 0;
#[allow(unused)]
const GHOSTTY_ERROR: GhosttyResult = -1;

/// 终端数据类型（传给 ghostty_terminal_get 的 data_type）
#[repr(C)]
enum GhosttyTerminalData {
    GHOSTTY_TERMINAL_DATA_TITLE = 0,
    GHOSTTY_TERMINAL_DATA_PWD = 1,
    GHOSTTY_TERMINAL_DATA_CURSOR_X = 2,
    GHOSTTY_TERMINAL_DATA_CURSOR_Y = 3,
    GHOSTTY_TERMINAL_DATA_COLS = 4,
    GHOSTTY_TERMINAL_DATA_ROWS = 5,
}

/// 终端选项类型（传给 ghostty_terminal_set 的 option）
#[repr(C)]
enum GhosttyTerminalOption {
    GHOSTTY_TERMINAL_OPTION_COLS = 0,
    GHOSTTY_TERMINAL_OPTION_ROWS = 1,
    GHOSTTY_TERMINAL_OPTION_TITLE_CHANGED_CB = 2,
    GHOSTTY_TERMINAL_OPTION_PWD_CHANGED_CB = 3,
    GHOSTTY_TERMINAL_OPTION_BELL_CB = 4,
    GHOSTTY_TERMINAL_OPTION_WRITE_PTY_CB = 5,
    GHOSTTY_TERMINAL_OPTION_USERDATA = 6,
}

/// 分配器接口（由 Ghostty 实现，rust 侧传空指针使用默认）
#[repr(C)]
struct GhosttyAppMemoryAllocator {
    alloc: Option<unsafe extern "C" fn(*mut c_void, usize) -> *mut c_void>,
    realloc: Option<unsafe extern "C" fn(*mut c_void, *mut c_void, usize) -> *mut c_void>,
    free: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
    userdata: *mut c_void,
}

// ── extern "C" FFI 函数声明 ─────────────────────────────────────────────

extern "C" {
    /// 创建新终端实例
    fn ghostty_terminal_new(
        allocator: *const GhosttyAppMemoryAllocator,
        terminal: *mut *mut c_void,
        opts: *const c_void,
        opts_len: usize,
    ) -> GhosttyResult;

    /// 释放终端实例
    fn ghostty_terminal_free(terminal: *mut c_void);

    /// 喂 PTY 字节到 VT 解析器
    fn ghostty_terminal_vt_write(
        terminal: *mut c_void,
        data: *const u8,
        len: size_t,
    ) -> GhosttyResult;

    /// 设置终端选项（cols/rows/effects 回调）
    fn ghostty_terminal_set(
        terminal: *mut c_void,
        option: c_uint,
        value: *const c_void,
    ) -> GhosttyResult;

    /// 读取终端状态（title/pwd/cursor 等）
    fn ghostty_terminal_get(
        terminal: *mut c_void,
        data_type: c_uint,
        out: *mut c_void,
    ) -> GhosttyResult;

    /// 释放由 ghostty_terminal_get 返回的字符串
    /// 必须使用 Ghostty 的分配器释放，不能用 libc::free
    fn ghostty_terminal_free_string(
        terminal: *mut c_void,
        ptr: *mut c_void,
    );
}

// ── Effect 回调 trampolines ──────────────────────────────────────────────

/// 用户数据类型：传递到回调中的上下文指针
#[repr(C)]
struct Userdata {
    app_handle: *mut c_void, // 实际上是 tauri::AppHandle 的堆分配指针
    session_id: *mut c_char,  // session_id 的 C 字符串
}

extern "C" fn title_changed_cb(
    _terminal: *mut c_void,
    title: *const c_char,
    userdata: *mut c_void,
) {
    if userdata.is_null() || title.is_null() {
        return;
    }
    unsafe {
        let ud = &*(userdata as *const Userdata);
        let c_str = CStr::from_ptr(title);
        if let Ok(s) = c_str.to_str() {
            let session_id = CStr::from_ptr(ud.session_id)
                .to_str()
                .unwrap_or("unknown");
            let handle = &*(ud.app_handle as *const AppHandle);
            let _ = handle.emit(
                "terminal:title-changed",
                serde_json::json!({
                    "sessionId": session_id,
                    "title": s,
                }),
            );
        }
    }
}

extern "C" fn pwd_changed_cb(
    _terminal: *mut c_void,
    pwd: *const c_char,
    userdata: *mut c_void,
) {
    if userdata.is_null() || pwd.is_null() {
        return;
    }
    unsafe {
        let ud = &*(userdata as *const Userdata);
        let c_str = CStr::from_ptr(pwd);
        if let Ok(s) = c_str.to_str() {
            let session_id = CStr::from_ptr(ud.session_id)
                .to_str()
                .unwrap_or("unknown");
            let handle = &*(ud.app_handle as *const AppHandle);
            let _ = handle.emit(
                "terminal:pwd-changed",
                serde_json::json!({
                    "sessionId": session_id,
                    "pwd": s,
                }),
            );
        }
    }
}

extern "C" fn bell_cb(_terminal: *mut c_void, userdata: *mut c_void) {
    if userdata.is_null() {
        return;
    }
    unsafe {
        let ud = &*(userdata as *const Userdata);
        let session_id = CStr::from_ptr(ud.session_id)
            .to_str()
            .unwrap_or("unknown");
        let handle = &*(ud.app_handle as *const AppHandle);
        let _ = handle.emit(
            "terminal:bell",
            serde_json::json!({
                "sessionId": session_id,
            }),
        );
    }
}

extern "C" fn write_pty_cb(
    _terminal: *mut c_void,
    data: *const u8,
    len: size_t,
    userdata: *mut c_void,
) {
    // 这个回调在 Ghostty 需要向 PTY 写回数据时触发（例如粘贴）
    // 当前版本通过 `terminal:write-pty` 事件通知前端，由前端调用 terminal.write
    if userdata.is_null() || data.is_null() {
        return;
    }
    unsafe {
        let ud = &*(userdata as *const Userdata);
        let session_id = CStr::from_ptr(ud.session_id)
            .to_str()
            .unwrap_or("unknown");
        let slice = std::slice::from_raw_parts(data, len);
        let handle = &*(ud.app_handle as *const AppHandle);
        let _ = handle.emit(
            "terminal:write-pty",
            serde_json::json!({
                "sessionId": session_id,
                "data": String::from_utf8_lossy(slice),
            }),
        );
    }
}

// ── Rust 侧封装 ─────────────────────────────────────────────────────────

/// Ghostty VT 终端实例的 Rust 封装
pub struct GhosttyTerminal {
    raw: *mut c_void,
    app_handle: AppHandle,
    session_id: String,
    userdata_ptr: *mut Userdata,
}

// raw 指针本身 !Send，但我们在 drop 中正确管理生命周期
unsafe impl Send for GhosttyTerminal {}

impl GhosttyTerminal {
    /// 创建新终端实例（cols × rows）
    pub fn new(
        cols: u16,
        rows: u16,
        app_handle: AppHandle,
        session_id: String,
    ) -> crate::Result<Self> {
        let mut raw: *mut c_void = ptr::null_mut();

        // 用空分配器（让 Ghostty 使用默认 allocator）
        let allocator = GhosttyAppMemoryAllocator {
            alloc: None,
            realloc: None,
            free: None,
            userdata: ptr::null_mut(),
        };

        // 构建 options 数组：cols, rows
        let opts: [c_uint; 2] = [cols as c_uint, rows as c_uint];
        let ret = unsafe { ghostty_terminal_new(&allocator, &mut raw, opts.as_ptr() as *const c_void, opts.len()) };

        if ret != GHOSTTY_SUCCESS || raw.is_null() {
            return Err(crate::Error::Internal(
                "ghostty_terminal_new failed".into(),
            ));
        }

        let mut term = Self {
            raw,
            app_handle,
            session_id,
            userdata_ptr: ptr::null_mut(),
        };

        term.register_effects()?;

        Ok(term)
    }

    /// 喂 PTY 数据到 VT 解析器
    pub fn vt_write(&self, buf: &[u8]) -> crate::Result<()> {
        let ret = unsafe {
            ghostty_terminal_vt_write(self.raw, buf.as_ptr(), buf.len() as size_t)
        };
        if ret != GHOSTTY_SUCCESS {
            return Err(crate::Error::Internal("ghostty_terminal_vt_write failed".into()));
        }
        Ok(())
    }

    /// 读取终端标题
    pub fn title(&self) -> Option<String> {
        unsafe {
            let mut out: *mut c_char = ptr::null_mut();
            let ret = ghostty_terminal_get(
                self.raw,
                GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_TITLE as c_uint,
                &mut out as *mut *mut c_char as *mut c_void,
            );
            if ret == GHOSTTY_SUCCESS && !out.is_null() {
                let s = CStr::from_ptr(out).to_string_lossy().into_owned();
                ghostty_terminal_free_string(self.raw, out as *mut c_void);
                Some(s)
            } else {
                None
            }
        }
    }

    /// 读取终端当前工作目录（由 OSC 7 程控）
    pub fn pwd(&self) -> Option<String> {
        unsafe {
            let mut out: *mut c_char = ptr::null_mut();
            let ret = ghostty_terminal_get(
                self.raw,
                GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_PWD as c_uint,
                &mut out as *mut *mut c_char as *mut c_void,
            );
            if ret == GHOSTTY_SUCCESS && !out.is_null() {
                let s = CStr::from_ptr(out).to_string_lossy().into_owned();
                ghostty_terminal_free_string(self.raw, out as *mut c_void);
                Some(s)
            } else {
                None
            }
        }
    }

    /// 读取光标列（0-based）
    pub fn cursor_x(&self) -> u16 {
        unsafe {
            let mut out: c_uint = 0;
            let ret = ghostty_terminal_get(
                self.raw,
                GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_CURSOR_X as c_uint,
                &mut out as *mut c_uint as *mut c_void,
            );
            if ret == GHOSTTY_SUCCESS { out as u16 } else { 0 }
        }
    }

    /// 读取光标行（0-based）
    pub fn cursor_y(&self) -> u16 {
        unsafe {
            let mut out: c_uint = 0;
            let ret = ghostty_terminal_get(
                self.raw,
                GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_CURSOR_Y as c_uint,
                &mut out as *mut c_uint as *mut c_void,
            );
            if ret == GHOSTTY_SUCCESS { out as u16 } else { 0 }
        }
    }

    /// 读取终端列数
    pub fn cols(&self) -> u16 {
        unsafe {
            let mut out: c_uint = 0;
            let ret = ghostty_terminal_get(
                self.raw,
                GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_COLS as c_uint,
                &mut out as *mut c_uint as *mut c_void,
            );
            if ret == GHOSTTY_SUCCESS { out as u16 } else { 80 }
        }
    }

    /// 读取终端行数
    pub fn rows(&self) -> u16 {
        unsafe {
            let mut out: c_uint = 0;
            let ret = ghostty_terminal_get(
                self.raw,
                GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_ROWS as c_uint,
                &mut out as *mut c_uint as *mut c_void,
            );
            if ret == GHOSTTY_SUCCESS { out as u16 } else { 24 }
        }
    }

    /// 注册 effects 回调（title_changed / pwd_changed / bell / write_pty）
    fn register_effects(&mut self) -> crate::Result<()> {
        // 构造 userdata
        let session_id_c = CString::new(self.session_id.clone())
            .map_err(|e| crate::Error::Internal(format!("CString error: {e}")))?;
        let app_handle_box = Box::new(self.app_handle.clone());
        let userdata = Box::new(Userdata {
            app_handle: Box::into_raw(app_handle_box) as *mut c_void,
            session_id: session_id_c.into_raw(),
        });
        let ud_ptr: *const Userdata = &*userdata;

        // 注册回调 — 每个回调通过 ghostty_terminal_set 注入
        unsafe {
            let ret = ghostty_terminal_set(
                self.raw,
                GhosttyTerminalOption::GHOSTTY_TERMINAL_OPTION_TITLE_CHANGED_CB as c_uint,
                title_changed_cb as *const c_void,
            );
            if ret != GHOSTTY_SUCCESS {
                return Err(crate::Error::Internal("set title_changed_cb failed".into()));
            }

            let ret = ghostty_terminal_set(
                self.raw,
                GhosttyTerminalOption::GHOSTTY_TERMINAL_OPTION_PWD_CHANGED_CB as c_uint,
                pwd_changed_cb as *const c_void,
            );
            if ret != GHOSTTY_SUCCESS {
                return Err(crate::Error::Internal("set pwd_changed_cb failed".into()));
            }

            let ret = ghostty_terminal_set(
                self.raw,
                GhosttyTerminalOption::GHOSTTY_TERMINAL_OPTION_BELL_CB as c_uint,
                bell_cb as *const c_void,
            );
            if ret != GHOSTTY_SUCCESS {
                return Err(crate::Error::Internal("set bell_cb failed".into()));
            }

            let ret = ghostty_terminal_set(
                self.raw,
                GhosttyTerminalOption::GHOSTTY_TERMINAL_OPTION_WRITE_PTY_CB as c_uint,
                write_pty_cb as *const c_void,
            );
            if ret != GHOSTTY_SUCCESS {
                return Err(crate::Error::Internal("set write_pty_cb failed".into()));
            }

            // 注册 userdata
            let ret = ghostty_terminal_set(
                self.raw,
                GhosttyTerminalOption::GHOSTTY_TERMINAL_OPTION_USERDATA as c_uint,
                ud_ptr as *const c_void,
            );
            if ret != GHOSTTY_SUCCESS {
                return Err(crate::Error::Internal("set userdata failed".into()));
            }
        }

        self.userdata_ptr = Box::into_raw(userdata);
        Ok(())
    }
}

impl Drop for GhosttyTerminal {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe {
                ghostty_terminal_free(self.raw);
            }
        }
        // 释放 userdata
        if !self.userdata_ptr.is_null() {
            unsafe {
                let ud = Box::from_raw(self.userdata_ptr);
                // 释放 app_handle box
                if !ud.app_handle.is_null() {
                    let _ = Box::from_raw(ud.app_handle as *mut AppHandle);
                }
                // 释放 session_id CString
                if !ud.session_id.is_null() {
                    let _ = CString::from_raw(ud.session_id);
                }
            }
        }
    }
}

// ── 单元测试 ─────────────────────────────────────────────────────────────

#[cfg(test)]
#[cfg(feature = "ghostty-vt")]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    /// 测试终端创建和写入
    #[test]
    fn test_create_and_write() -> crate::Result<()> {
        // 这个测试需要一个 AppHandle，但单元测试中没有 tauri 环境
        // 标记为 ignore 或作为集成测试
        Ok(())
    }

    /// 测试 effect 回调触发
    #[test]
    fn test_effects_callback() {
        // 需要完整的 tauri runtime，作为集成测试
    }
}
