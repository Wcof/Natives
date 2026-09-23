// Launcher 引导流（生产交付链）：`native-file-host --launcher-setup`。
// 职责边界（用户方案）：只做 Extension 检测/准备/引导与握手验证，
// 不承载空间/文件/AI/应用中心等业务 UI，不常驻——有界轮询后退出。
//
// 流程（2026-09-13 可见主入口修订 / §1.1、§1.3、§3.3）：
//   1. 打开随包离线 HTML 指南（file:，Chrome）+ 原生降级提示
//   2. 打开 Chrome 扩展管理页 + 复制/Finder 定位固定系统源扩展目录
//   3. 有界轮询握手标记（由 Extension↔Host 握手写入），成功即打印状态退出
// 无 ZIP 准备链、无 current/版本目录；目录缺失 = 安装不完整（T02）。
// 系统调用只用固定程序 + 固定参数（/usr/bin/open），不拼接外部文本。
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crate::app_store::AppStore;
use crate::extension_provision::read_managed_manifest_version;

/// 随包固定解压扩展目录（契约 §3.2）：pkg 安装到受限系统源；
/// 不再使用 bundle 相对布局（D06：清单生成实际路径）。
pub fn system_chrome_extension_dir() -> PathBuf {
    crate::app_product::default_system_source().join("ChromeExtension")
}

/// 握手标记文件路径：由 Native Messaging "version" 握手写入。
pub fn handshake_marker_path(natives_root: &Path) -> PathBuf {
    natives_root.join("extensions/chrome/handshake.json")
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
enum Entry {
    Product,
    Config,
    Setup,
}

/// 入口三态（§1.4 日常使用）：系统扩展缺失 → 完整引导；产品已配置 →
/// 直接进产品；扩展已装未配置 → 进配置入口。旧标记仅作提示线索。
fn decide_entry(system_extension_ok: bool, product_configured: bool, natives_root: &Path) -> Entry {
    if !system_extension_ok {
        return Entry::Setup;
    }
    if product_configured {
        return Entry::Product;
    }
    if read_handshake_matching_version(natives_root, &system_chrome_extension_dir()) {
        return Entry::Config;
    }
    Entry::Setup
}

/// D03：握手标记的 extensionVersion 必须与当前系统扩展一致；旧版本
/// 标记意味着扩展需要重载，不能作为就绪证据。
fn read_handshake_matching_version(natives_root: &Path, extension_dir: &Path) -> bool {
    let marker = handshake_marker_path(natives_root);
    let Ok(value) = fs::read_to_string(&marker)
        .map(|s| serde_json::from_str::<serde_json::Value>(&s).unwrap_or(serde_json::Value::Null))
    else {
        return false;
    };
    let recorded = value.get("extensionVersion").and_then(|v| v.as_str());
    let Some(recorded) = recorded else {
        return false;
    };
    let manifest = extension_dir.join("manifest.json");
    let Some(text) = fs::read_to_string(&manifest).ok() else {
        return false;
    };
    let current = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("version").and_then(|v| v.as_str()).map(String::from));
    current.as_deref() == Some(recorded)
}

/// Chrome 发现（D05）：系统级与用户级应用目录都检查。
fn chrome_installed() -> bool {
    if Path::new("/Applications/Google Chrome.app").exists() {
        return true;
    }
    dirs::home_dir()
        .map(|home| home.join("Applications/Google Chrome.app").exists())
        .unwrap_or(false)
}

fn natives_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(crate::app_signing::natives_dir_name())
}

/// dispatch.rs 握手落盘使用；与 run() 共享同一根目录定义。
pub(crate) fn natives_root_pub() -> PathBuf {
    natives_root()
}

#[derive(serde::Serialize)]
struct SetupStatus {
    step: &'static str,
    #[serde(rename = "extensionReady")]
    extension_ready: bool,
    #[serde(rename = "extensionDir")]
    extension_dir: Option<String>,
    #[serde(rename = "extensionVersion")]
    extension_version: Option<String>,
    #[serde(rename = "handshakeVerified")]
    handshake_verified: bool,
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
/// §1.3：先打开随包离线指南（file:），再开扩展管理页并在 Finder 定位；
/// §1.1：复制固定目录路径；Chrome 缺失时用原生对话框如实提示。
fn reveal(extension_dir: &Path, onboarding_html: Option<&Path>) -> Result<(), String> {
    if !chrome_installed() {
        let _ = std::process::Command::new("/usr/bin/osascript")
            .args(["-e", "display dialog \"Natives 需要 Google Chrome。请从 google.com/chrome 安装后重新双击 Natives。\\n\\nNatives requires Google Chrome.\" buttons {\"好 / OK\"} default button 1 with title \"Natives\""])
            .status();
        return Err("chrome_missing: /Applications/Google Chrome.app not found".into());
    }
    if let Some(html) = onboarding_html {
        let _ = std::process::Command::new("/usr/bin/open")
            .args(["-a", "Google Chrome", &html.to_string_lossy()])
            .status();
    }
    // Chrome 扩展管理页：固定程序 + 固定 URL 字面量。
    let _ = std::process::Command::new("/usr/bin/open")
        .args(["-a", "Google Chrome", "chrome://extensions"])
        .status();
    // Finder 定位固定系统扩展目录（直接高亮选中，供用户拖入 Chrome 扩展页）：
    std::process::Command::new("/usr/bin/open")
        .args(["-R", &extension_dir.to_string_lossy()])
        .status()
        .map_err(|e| format!("open Finder: {e}"))?;
    Ok(())
}

fn run(
    natives_root: &Path,
    poll: Duration,
    onboarding_html: Option<&Path>,
    no_open: bool,
) -> Result<SetupStatus, String> {
    let started = Instant::now();
    // §3.3：扩展目录就是 pkg 安装的固定系统源目录。无 ZIP 准备链、
    // 无 current/版本目录（D07）；目录缺失 = 安装不完整（T02）。
    let extension_dir = system_chrome_extension_dir();
    if !extension_dir.join("manifest.json").exists() {
        return Err(format!(
            "installer incomplete: system ChromeExtension missing at {}",
            extension_dir.display()
        ));
    }
    let extension_version = read_managed_manifest_version(&extension_dir);
    let not_before = SystemTime::now();
    if !no_open {
        let _ = reveal(&extension_dir, onboarding_html);
    }

    // 有界轮询握手标记（Extension 加载后首次连接即写）；超时如实报告，不常驻。
    while started.elapsed() < poll {
        if let Some(handshake) = read_fresh_handshake(natives_root, not_before) {
            let status = SetupStatus {
                step: "handshake_verified",
                extension_ready: true,
                extension_dir: Some(extension_dir.to_string_lossy().into_owned()),
                extension_version: handshake
                    .get("extensionVersion")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .or(extension_version),
                handshake_verified: true,
            };
            return Ok(status);
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Ok(SetupStatus {
        step: "waiting_for_user_load",
        extension_ready: true,
        extension_dir: Some(extension_dir.to_string_lossy().into_owned()),
        extension_version,
        handshake_verified: false,
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
    let onboarding = args
        .iter()
        .position(|a| a == "--onboarding-html")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from);
    // --no-open：打开动作由 Natives.app 原生窗口负责（§1.3 状态职责表），
    // Host 只做有界握手轮询（原生窗的"重新检测"使用）。
    // --reveal-extension-dir：别名/定位逻辑单一来源在 Host（§1.4 不在两处
    // 实现链接逻辑），原生窗的"显示扩展文件夹"经此复用。
    let no_open = args.iter().any(|a| a == "--no-open");
    if args.iter().any(|a| a == "--reveal-extension-dir") {
        let dir = system_chrome_extension_dir();
        let mut child = match std::process::Command::new("/usr/bin/open")
            .args(["-R", &dir.to_string_lossy()])
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                println!(
                    "{}",
                    serde_json::json!({ "step": "error", "error": format!("open Finder: {e}") })
                );
                return 1;
            }
        };
        let _ = child.wait();
        println!(
            "{}",
            serde_json::json!({ "step": "revealed", "dir": dir.to_string_lossy() })
        );
        return 0;
    }
    let root = natives_root();
    match run(
        &root,
        Duration::from_secs(timeout),
        onboarding.as_deref(),
        no_open,
    ) {
        Ok(status) => {
            println!("{}", serde_json::to_string(&status).unwrap_or_default());
            if status.handshake_verified {
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
pub fn run_launcher_default(args: &[String]) -> i32 {
    let root = natives_root();
    // default 模式不打开指南（未就绪时由外层包装转入 --launcher-setup，
    // 指南在 setup 流程打开）；args 中的 --onboarding-html 由 setup 消费。
    let _ = args;
    // 核心 Host 注册（com.natives.file_manager / com.natives.model_host）。
    if let Err(err) = register_core_hosts() {
        println!("{}", serde_json::json!({ "step": "error", "error": err }));
        return 1;
    }
    // 就绪判定（§1.1"已就绪直接进入"）：近期真实握手标记 + 系统扩展目录
    // 完整。挑战级会话关联（本次 challenge 绑定）是 P3 后续项，当前以
    // 有界时限替代纯文件存在判定。
    // D03：不再用 30 天旧标记判成功。就绪 = 存在握手标记且其记录的
    // extensionVersion 与系统扩展 manifest 版本一致（升级后需重载）。
    // D03 + 用户反馈：入口三态——系统扩展缺失走完整引导；产品已配置
    // （配置经扩展前台完成 = 扩展已装）直接进产品；扩展已装未配置进
    // 配置入口。实时连接由产品页自身握手核验，不依赖旧标记。
    let no_open = args.iter().any(|a| a == "--no-open");
    let system_extension_ok = system_chrome_extension_dir().join("manifest.json").exists();
    let configured = AppStore::open(&crate::workspace_store::default_db_path())
        .and_then(|store| store.product_status())
        .map(|status| status.configured)
        .unwrap_or(false);
    match decide_entry(system_extension_ok, configured, &root) {
        Entry::Product => {
            let url = format!("chrome-extension://{STABLE_EXTENSION_ID}/space.html");
            if !no_open {
                let _ = std::process::Command::new("/usr/bin/open")
                    .args(["-a", "Google Chrome", &url])
                    .status();
            }
            println!(
                "{}",
                serde_json::json!({ "step": if no_open { "ready" } else { "opened_natives" }, "url": url })
            );
            0
        }
        Entry::Config => {
            // §1.4 加载后自动进入配置入口；实时连接由页面握手核验。
            let url = format!("chrome-extension://{STABLE_EXTENSION_ID}/apps.html");
            if !no_open {
                let _ = std::process::Command::new("/usr/bin/open")
                    .args(["-a", "Google Chrome", &url])
                    .status();
            }
            println!(
                "{}",
                serde_json::json!({ "step": if no_open { "ready" } else { "opened_config" }, "url": url })
            );
            0
        }
        Entry::Setup => {
            println!(
                "{}",
                serde_json::json!({ "step": "setup_required", "reason": "extension not loaded yet" })
            );
            2
        }
    }
}

/// 写入核心 Host 的 Chrome NM manifest。pkg 安装场景下系统级注册已由
/// 安装引擎写入并指向真实系统源。此时若存在用户级注册（例如历史开发留存），
/// Chrome 会优先读取用户级目录从而遮蔽系统注册（方案 §3.3），导致
/// "Access to the specified native messaging host is forbidden"；因此系统级
/// 存在时清理遮蔽的用户级清单。无系统级时（纯便携/开发），写入当前稳定 ID。
fn register_core_hosts() -> Result<(), String> {
    let dir =
        crate::app_host_manifest::chrome_manifest_dir().ok_or("cannot resolve Chrome NM dir")?;
    if Path::new("/Library/Google/Chrome/NativeMessagingHosts/com.natives.file_manager.json")
        .exists()
    {
        clean_shadowing_user_manifests();
        return Ok(());
    }
    fs::create_dir_all(&dir).map_err(|e| format!("create NM dir: {e}"))?;
    let core_dir = crate::app_product::default_system_source();
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
    Ok(())
}

/// 清理遮蔽系统级注册的用户级主 Host 清单（Chrome 与 Chromium）。
/// 只删除确属于本产品的 com.natives.file_manager 与 com.natives.model_host，
/// 绝不宽泛删除其他软件的清单。
fn clean_shadowing_user_manifests() {
    let home = dirs::home_dir();
    let user_dirs = [
        crate::app_host_manifest::chrome_manifest_dir(),
        home.as_ref()
            .map(|h| h.join("Library/Application Support/Chromium/NativeMessagingHosts")),
    ];
    for user_dir in user_dirs.into_iter().flatten() {
        clean_shadowing_manifests_in(&user_dir);
    }
}

fn clean_shadowing_manifests_in(dir: &Path) {
    for host in ["com.natives.file_manager", "com.natives.model_host"] {
        let file = dir.join(format!("{host}.json"));
        if file.exists() {
            let _ = fs::remove_file(&file);
        }
    }
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
    fn extension_dir_resolves_from_fixed_system_source() {
        // §3.3：扩展目录 = pkg 安装的固定系统源目录（D06：不再 bundle 相对）。
        let expected = crate::app_product::default_system_source()
            .join("ChromeExtension")
            .join("manifest.json");
        assert_eq!(
            system_chrome_extension_dir().join("manifest.json"),
            expected
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

    #[test]
    fn shadowing_user_manifests_are_cleaned_up() {
        let root = std::env::temp_dir().join(format!(
            "natives-shadow-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let fm = root.join("com.natives.file_manager.json");
        let mh = root.join("com.natives.model_host.json");
        let other = root.join("com.other.app.json");
        fs::write(&fm, b"{}").unwrap();
        fs::write(&mh, b"{}").unwrap();
        fs::write(&other, b"{}").unwrap();
        clean_shadowing_manifests_in(&root);
        assert!(!fm.exists(), "com.natives.file_manager must be cleaned");
        assert!(!mh.exists(), "com.natives.model_host must be cleaned");
        assert!(other.exists(), "unrelated manifest must be preserved");
        let _ = fs::remove_dir_all(&root);
    }
}
