// Launcher 引导流（生产交付链）：`native-file-host --launcher-setup`。
// 职责边界（用户方案）：只做 Extension 检测/准备/引导与握手验证，
// 不承载空间/文件/AI/应用中心等业务 UI，不常驻——有界轮询后退出。
//
// 流程：
//   1. 受管目录无 Extension → 从安装包布局准备（ZIP 校验→解压→current）
//   2. 打开 Chrome 扩展管理页 + Finder 定位受管 current 目录
//   3. 有界轮询握手标记（由 Extension↔Host 握手写入），成功即打印状态退出
// 系统调用只用固定程序 + 固定参数（/usr/bin/open），不拼接外部文本。
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crate::extension_provision::{
    current_extension_dir, provision_extension, read_managed_manifest_version,
};

/// 安装包布局中 Extension ZIP 的位置：`<exe>/../share/natives/ChromeExtension/`。
pub fn bundled_chrome_extension_dir(exe_dir: &Path) -> PathBuf {
    exe_dir.join("../share/natives/ChromeExtension")
}

/// 握手标记文件路径：由 Native Messaging "version" 握手写入。
pub fn handshake_marker_path(natives_root: &Path) -> PathBuf {
    natives_root.join("extensions/chrome/handshake.json")
}

fn natives_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".natives")
}

/// dispatch.rs 握手落盘使用；与 run() 共享同一根目录定义。
pub(crate) fn natives_root_pub() -> PathBuf {
    natives_root()
}

#[derive(serde::Serialize)]
struct SetupStatus {
    step: &'static str,
    extensionReady: bool,
    extensionDir: Option<String>,
    extensionVersion: Option<String>,
    handshakeVerified: bool,
}

/// 读取握手标记；`not_before` 之后写入的标记才算本次会话的新握手。
pub fn read_fresh_handshake(
    natives_root: &Path,
    not_before: SystemTime,
) -> Option<serde_json::Value> {
    let path = handshake_marker_path(natives_root);
    let meta = fs::metadata(&path).ok()?;
    let modified = meta.modified().ok()?;
    if modified < not_before {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    value.get("extensionVersion").and_then(|v| v.as_str())?;
    Some(value)
}

/// 打开引导所需的外部页面/目录。拆出以便测试注入。
fn reveal(extension_dir: &Path) -> Result<(), String> {
    // Chrome 扩展管理页：固定程序 + 固定 URL 字面量。
    let _ = std::process::Command::new("/usr/bin/open")
        .args(["-a", "Google Chrome", "chrome://extensions"])
        .status();
    // Finder 定位受管目录（用户在 Chrome 中选择的就是这个文件夹）。
    std::process::Command::new("/usr/bin/open")
        .arg(extension_dir)
        .status()
        .map_err(|e| format!("open Finder: {e}"))?;
    Ok(())
}

fn run(natives_root: &Path, exe_dir: &Path, poll: Duration) -> Result<SetupStatus, String> {
    let started = Instant::now();
    let mut extension_dir = current_extension_dir(natives_root);
    if extension_dir.is_none() {
        // 首启：从安装包布局准备受管目录。
        let bundle = bundled_chrome_extension_dir(exe_dir);
        let sums = fs::read_dir(&bundle)
            .ok()
            .and_then(|entries| {
                entries.flatten().find_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    name.starts_with("SHA256SUMS").then(|| e.path())
                })
            })
            .ok_or_else(|| {
                format!(
                    "installer bundle missing SHA256SUMS in {}",
                    bundle.display()
                )
            })?;
        let zip = fs::read_dir(&bundle)
            .ok()
            .and_then(|entries| {
                entries.flatten().find_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    (name.starts_with("natives-extension-") && name.ends_with(".zip"))
                        .then(|| e.path())
                })
            })
            .ok_or_else(|| {
                format!(
                    "installer bundle missing natives-extension-*.zip in {}",
                    bundle.display()
                )
            })?;
        let version = zip
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .and_then(|n| {
                n.trim_start_matches("natives-extension-")
                    .trim_end_matches(".zip")
                    .to_string()
                    .into()
            });
        let version = version.ok_or("cannot derive extension version from zip name")?;
        let dir = provision_extension(natives_root, &zip, &sums, &version)?;
        extension_dir = Some(dir);
    }
    let extension_dir = extension_dir.ok_or("extension dir unavailable")?;
    let extension_version = read_managed_manifest_version(&extension_dir);
    let not_before = SystemTime::now();
    let _ = reveal(&extension_dir);

    // 有界轮询握手标记（Extension 加载后首次连接即写）；超时如实报告，不常驻。
    while started.elapsed() < poll {
        if let Some(handshake) = read_fresh_handshake(natives_root, not_before) {
            let status = SetupStatus {
                step: "handshake_verified",
                extensionReady: true,
                extensionDir: Some(extension_dir.to_string_lossy().into_owned()),
                extensionVersion: handshake
                    .get("extensionVersion")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .or(extension_version),
                handshakeVerified: true,
            };
            return Ok(status);
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Ok(SetupStatus {
        step: "waiting_for_user_load",
        extensionReady: true,
        extensionDir: Some(extension_dir.to_string_lossy().into_owned()),
        extensionVersion: extension_version,
        handshakeVerified: false,
    })
}

/// CLI 入口：`--launcher-setup [--setup-timeout-secs N]`。输出状态 JSON 后退出。
pub fn run_cli(args: &[String]) -> i32 {
    let timeout = args
        .iter()
        .position(|a| a == "--setup-timeout-secs")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(300)
        .min(3600);
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    let root = natives_root();
    match run(&root, &exe_dir, Duration::from_secs(timeout)) {
        Ok(status) => {
            println!("{}", serde_json::to_string(&status).unwrap_or_default());
            if status.handshakeVerified {
                0
            } else {
                2
            }
        }
        Err(err) => {
            println!("{}", serde_json::json!({ "step": "error", "error": err }));
            1
        }
    }
}

/// 稳定 Extension ID（与 scripts/extension-id-key.b64 派生值一致，
/// 测试 enforce 一致性）。本地加载已解压模式下 Chrome 由 manifest key
/// 派生此 ID；Native Messaging allowlist 必须引用同一 ID。
pub const STABLE_EXTENSION_ID: &str = "gehmgcnlpdepnpmcbbdaijabcjdnbfmh";

/// 二次启动默认模式：Extension 已就绪且握手已完成 → 直接打开 Chrome 中的
/// Natives（chrome-extension://<stable-id>/space.html）；否则进入首次引导。
/// 同时完成核心 Host NM manifest 注册（用户级，幂等）。
pub fn run_launcher_default() -> i32 {
    let root = natives_root();
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    // 核心 Host 注册（com.natives.file_manager / com.natives.model_host）：
    // manifest 指向安装包内二进制，allowed_origins 锁定稳定 Extension ID。
    if let Err(err) = register_core_hosts(&root, &exe_dir) {
        println!("{}", serde_json::json!({ "step": "error", "error": err }));
        return 1;
    }
    let ready = current_extension_dir(&root).is_some() && handshake_marker_path(&root).exists();
    if ready {
        let url = format!("chrome-extension://{STABLE_EXTENSION_ID}/space.html");
        let _ = std::process::Command::new("/usr/bin/open")
            .args(["-a", "Google Chrome", &url])
            .status();
        println!(
            "{}",
            serde_json::json!({ "step": "opened_natives", "url": url })
        );
        0
    } else {
        println!(
            "{}",
            serde_json::json!({ "step": "setup_required", "reason": "extension not loaded yet" })
        );
        2
    }
}

/// 写入核心 Host 的 Chrome NM manifest（用户级目录，幂等覆盖自身文件）。
fn register_core_hosts(root: &Path, exe_dir: &Path) -> Result<(), String> {
    let dir =
        crate::app_host_manifest::chrome_manifest_dir().ok_or("cannot resolve Chrome NM dir")?;
    fs::create_dir_all(&dir).map_err(|e| format!("create NM dir: {e}"))?;
    let core_dir = exe_dir.join("../share/natives/Runtime");
    let origin = format!("chrome-extension://{STABLE_EXTENSION_ID}/");
    for (host, binary) in [
        ("com.natives.file_manager", "native-file-host"),
        ("com.natives.model_host", "model-host"),
    ] {
        let manifest = serde_json::json!({
            "name": host,
            "path": core_dir.join(binary).to_string_lossy(),
            "type": "stdio",
            "allowed_origins": [origin],
        });
        fs::write(
            dir.join(format!("{host}.json")),
            serde_json::to_string(&manifest).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("write {host} manifest: {e}"))?;
    }
    let _ = root; // 根目录仅用于握手标记判定，注册本身不落盘额外状态。
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_handshake_requires_newer_mtime_and_extension_version() {
        let root = std::env::temp_dir().join(format!("natives-launcher-hs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("extensions/chrome")).unwrap();
        let marker = handshake_marker_path(&root);
        fs::write(
            &marker,
            r#"{"extensionVersion":"0.1.0","hostVersion":"0.1.0","protocolVersion":1}"#,
        )
        .unwrap();
        // 标记早于 not_before → 不算本次会话新握手。
        let later = SystemTime::now() + Duration::from_secs(3600);
        assert!(read_fresh_handshake(&root, later).is_none());
        // 同一时间窗内有 extensionVersion → 有效。
        assert!(read_fresh_handshake(&root, SystemTime::now() - Duration::from_secs(60)).is_some());
        // 缺 extensionVersion 的标记无效。
        fs::write(&marker, r#"{"hostVersion":"0.1.0"}"#).unwrap();
        assert!(read_fresh_handshake(&root, SystemTime::now() - Duration::from_secs(60)).is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bundled_layout_path_is_relative_to_exe() {
        let p = bundled_chrome_extension_dir(Path::new("/Applications/Natives.app/Contents/MacOS"));
        assert_eq!(
            p,
            PathBuf::from(
                "/Applications/Natives.app/Contents/MacOS/../share/natives/ChromeExtension"
            )
        );
    }

    #[test]
    fn stable_extension_id_matches_build_key_material() {
        // 守护测试：Rust 侧 allowlist 引用的 ID 必须与构建脚本密钥材料
        // 派生的 ID 一致；漂移即 Native Messaging 握手被 Chrome 拒绝。
        let key = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/extension-id-key.b64");
        let key = fs::read_to_string(key).unwrap();
        let key = key.trim();
        use base64::Engine;
        use sha2::{Digest, Sha256};
        let der = base64::engine::general_purpose::STANDARD
            .decode(key)
            .map_err(|e| format!("decode key: {e}"))
            .unwrap();
        let hash = Sha256::digest(&der);
        let derived: String = hash[..16]
            .iter()
            .flat_map(|b| [(b >> 4) & 0xF, b & 0xF])
            .map(|n| char::from(b'a' + n as u8))
            .collect();
        assert_eq!(STABLE_EXTENSION_ID, derived);
    }
}
