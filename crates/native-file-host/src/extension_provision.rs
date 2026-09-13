// Extension 受管目录准备（生产交付链，方案 §Extension ZIP）：
// natives-extension-{version}.zip → SHA-256 校验（对 SHA256SUMS）→
// 解压到 extensions/chrome/{version}/ → 原子切换 current 指针。
// 失败回退：current 始终指向上一可用版本，损坏的新版本目录整体删除。
// 仅接受存储型 ZIP（method=0，本仓库构建器产物）；压缩条目显式拒绝，
// 不引入 tar/zip 解码依赖面。
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const EOCD_MAGIC: u32 = 0x0605_4b50;
const LOCAL_MAGIC: u32 = 0x0403_4b50;
const MAX_ZIP_ENTRIES: usize = 4096;

/// 受管 Extension 根：`<natives_root>/extensions/chrome`。
/// 版本目录 `{semver}/` 与 `current` 文本指针（内容为版本号）同级。
pub fn managed_root(natives_root: &Path) -> PathBuf {
    natives_root.join("extensions/chrome")
}

pub fn current_extension_dir(natives_root: &Path) -> Option<PathBuf> {
    let root = managed_root(natives_root);
    let version = fs::read_to_string(root.join("current"))
        .ok()?
        .trim()
        .to_string();
    if version.is_empty() || version.contains('/') || version.contains("..") {
        return None;
    }
    let dir = root.join(&version);
    dir.is_dir().then_some(dir)
}

struct ZipEntry {
    name: String,
    method: u16,
    size: u32,
    offset: u32,
}

fn parse_zip_entries(bytes: &[u8]) -> Result<Vec<ZipEntry>, String> {
    // 定位 EOCD（无 comment，从尾部固定 22 字节）。
    if bytes.len() < 22 || bytes[bytes.len() - 22..].len() < 22 {
        return Err("zip too small".into());
    }
    let tail = &bytes[bytes.len() - 22..];
    if tail[..4] != EOCD_MAGIC.to_le_bytes() {
        return Err("zip EOCD magic missing".into());
    }
    let count = u16::from_le_bytes([tail[10], tail[11]]) as usize;
    let cd_size = u32::from_le_bytes([tail[12], tail[13], tail[14], tail[15]]) as usize;
    let cd_off = u32::from_le_bytes([tail[16], tail[17], tail[18], tail[19]]) as usize;
    if count > MAX_ZIP_ENTRIES || cd_off + cd_size > bytes.len() {
        return Err("zip central directory out of bounds".into());
    }
    let mut entries = Vec::with_capacity(count);
    let mut cur = cd_off;
    for _ in 0..count {
        let cd = bytes
            .get(cur..cur + 46)
            .ok_or_else(|| "central directory truncated".to_string())?;
        if cd[..4] != 0x0201_4b50_u32.to_le_bytes() {
            return Err("central directory magic missing".into());
        }
        let method = u16::from_le_bytes([cd[10], cd[11]]);
        let size = u32::from_le_bytes([cd[24], cd[25], cd[26], cd[27]]);
        let name_len = u16::from_le_bytes([cd[28], cd[29]]) as usize;
        let extra_len = u16::from_le_bytes([cd[30], cd[31]]) as usize;
        let comment_len = u16::from_le_bytes([cd[32], cd[33]]) as usize;
        let lho = u32::from_le_bytes([cd[42], cd[43], cd[44], cd[45]]);
        let name_start = cur + 46;
        let name_bytes = bytes
            .get(name_start..name_start + name_len)
            .ok_or_else(|| "central directory name truncated".to_string())?;
        let name = String::from_utf8(name_bytes.to_vec()).map_err(|_| "zip name not utf-8")?;
        if name.contains("..") || name.starts_with('/') || name.contains('\\') {
            return Err(format!("unsafe zip entry name: {name}"));
        }
        entries.push(ZipEntry {
            name,
            method,
            size,
            offset: lho,
        });
        cur = name_start + name_len + extra_len + comment_len;
    }
    Ok(entries)
}

fn extract_entry<'a>(bytes: &'a [u8], entry: &ZipEntry) -> Result<&'a [u8], String> {
    if entry.method != 0 {
        return Err(format!("compressed zip entry not accepted: {}", entry.name));
    }
    let header = bytes
        .get(entry.offset as usize..entry.offset as usize + 30)
        .ok_or_else(|| "local header out of bounds".to_string())?;
    if header[..4] != LOCAL_MAGIC.to_le_bytes() {
        return Err(format!("local header magic missing: {}", entry.name));
    }
    let name_len = u16::from_le_bytes([header[26], header[27]]) as usize;
    let extra_len = u16::from_le_bytes([header[28], header[29]]) as usize;
    let data_start = entry.offset as usize + 30 + name_len + extra_len;
    bytes
        .get(data_start..data_start + entry.size as usize)
        .ok_or_else(|| format!("entry data out of bounds: {}", entry.name))
}

/// 校验（SHA256SUMS 含 zip 条目）→ 解压到 {version}/ → 切换 current。
/// 任何失败都删除半成品目录并保留现有 current（回退语义）。
pub fn provision_extension(
    natives_root: &Path,
    zip_path: &Path,
    sums_path: &Path,
    version: &str,
) -> Result<PathBuf, String> {
    if version.is_empty() || version.contains('/') || version.contains("..") {
        return Err(format!("invalid extension version: {version}"));
    }
    let zip_bytes = fs::read(zip_path).map_err(|e| format!("read zip: {e}"))?;
    let actual = hex_sha256(&zip_bytes);
    let sums = fs::read_to_string(sums_path).map_err(|e| format!("read SHA256SUMS: {e}"))?;
    let expected = sums
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .find(|_| true)
        .ok_or("SHA256SUMS empty")?;
    if !constant_eq(actual.as_bytes(), expected.as_bytes()) {
        return Err(format!(
            "extension zip sha256 mismatch: expected {expected}, got {actual}"
        ));
    }

    let entries = parse_zip_entries(&zip_bytes)?;
    if !entries.iter().any(|e| e.name == "manifest.json") {
        return Err("extension zip missing manifest.json".into());
    }

    let root = managed_root(natives_root);
    let version_dir = root.join(version);
    let _ = fs::remove_dir_all(&version_dir);
    fs::create_dir_all(&version_dir).map_err(|e| format!("create version dir: {e}"))?;
    for entry in &entries {
        let target = version_dir.join(&entry.name);
        if entry.name.ends_with('/') {
            fs::create_dir_all(&target).map_err(|e| format!("mkdir {}: {e}", entry.name))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir parent: {e}"))?;
        }
        let data = extract_entry(&zip_bytes, entry)?;
        fs::write(&target, data).map_err(|e| format!("write {}: {e}", entry.name))?;
    }
    // manifest 校验（解压后再查一次版本一致性）。
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(version_dir.join("manifest.json"))
            .map_err(|e| format!("read manifest: {e}"))?,
    )
    .map_err(|e| format!("parse manifest: {e}"))?;
    if manifest.get("version").and_then(|v| v.as_str()) != Some(version) {
        let _ = fs::remove_dir_all(&version_dir);
        return Err("manifest version != provisioned version".into());
    }
    // 最后写 current 指针：写入临时文件后 rename，保证读侧只见完整内容。
    fs::write(root.join(".current.tmp"), format!("{version}\n"))
        .map_err(|e| format!("write current tmp: {e}"))?;
    fs::rename(root.join(".current.tmp"), root.join("current"))
        .map_err(|e| format!("switch current: {e}"))?;
    Ok(version_dir)
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// 从解压目录读取受管 Extension 元信息（握手用 version）。
pub fn read_managed_manifest_version(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join("manifest.json")).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
    manifest.get("version")?.as_str().map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        // 与 scripts/extension-package.mjs createStoredZip 相同的最小存储型 ZIP。
        let mut out: Vec<u8> = Vec::new();
        let mut central: Vec<u8> = Vec::new();
        let mut offset = 0u32;
        for (name, data) in entries {
            let name = name.as_bytes();
            let crc = crc32(data);
            out.extend_from_slice(&0x0403_4b50_u32.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0x0800u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name);
            out.extend_from_slice(data);

            central.extend_from_slice(&0x0201_4b50_u32.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&0x0800u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name);
            offset += 30 + name.len() as u32 + data.len() as u32;
        }
        let cd_off = offset;
        let cd_size = central.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&EOCD_MAGIC.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut table = [0u32; 256];
        for n in 0..256u32 {
            let mut c = n;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            table[n as usize] = c;
        }
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c = table[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
        }
        c ^ 0xFFFF_FFFF
    }

    fn write_zip(
        dir: &Path,
        entries: &[(&str, &[u8])],
        corrupt: bool,
    ) -> (PathBuf, PathBuf, String) {
        let mut zip = stored_zip(entries);
        if corrupt && !zip.is_empty() {
            let mid = zip.len() / 2;
            zip[mid] ^= 0xFF;
        }
        let zip_path = dir.join("ext.zip");
        fs::write(&zip_path, &zip).unwrap();
        let sha = hex_sha256(&zip);
        let sums = dir.join("SHA256SUMS");
        fs::write(&sums, format!("{sha}  ext.zip\n")).unwrap();
        (zip_path, sums, sha)
    }

    #[test]
    fn provisions_and_switches_current() {
        let root = std::env::temp_dir().join(format!("natives-ext-prov-ok-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let manifest = br#"{"manifest_version":3,"version":"0.1.0","name":"Natives"}"#;
        let (zip, sums, _) = write_zip(
            &root,
            &[("manifest.json", manifest), ("app.js", b"//x")],
            false,
        );
        let dir = provision_extension(&root, &zip, &sums, "0.1.0").unwrap();
        assert_eq!(dir, root.join("extensions/chrome/0.1.0"));
        assert_eq!(
            fs::read_to_string(root.join("extensions/chrome/current"))
                .unwrap()
                .trim(),
            "0.1.0"
        );
        assert_eq!(fs::read_to_string(dir.join("app.js")).unwrap(), "//x");
        assert_eq!(
            read_managed_manifest_version(&current_extension_dir(&root).unwrap()),
            Some("0.1.0".into())
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sha_mismatch_rejected_and_current_preserved() {
        let root =
            std::env::temp_dir().join(format!("natives-ext-prov-bad-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let manifest = br#"{"manifest_version":3,"version":"0.2.0","name":"Natives"}"#;
        let (zip, _, _) = write_zip(&root, &[("manifest.json", manifest)], true);
        let sums = root.join("SHA256SUMS");
        fs::write(&sums, format!("{}  ext.zip\n", "0".repeat(64))).unwrap();
        let err = provision_extension(&root, &zip, &sums, "0.2.0").unwrap_err();
        assert!(err.contains("sha256 mismatch"), "got: {err}");
        // 失败后无 current、无半成品版本目录。
        assert!(current_extension_dir(&root).is_none());
        assert!(!root.join("extensions/chrome/0.2.0").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn version_switch_and_previous_fallback() {
        let root = std::env::temp_dir().join(format!("natives-ext-prov-sw-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let m1 = br#"{"manifest_version":3,"version":"1.0.0","name":"Natives"}"#;
        let m2 = br#"{"manifest_version":3,"version":"1.1.0","name":"Natives"}"#;
        let (zip1, sums1, _) = write_zip(&root, &[("manifest.json", m1)], false);
        provision_extension(&root, &zip1, &sums1, "1.0.0").unwrap();
        let (zip2, sums2, _) = write_zip(&root, &[("manifest.json", m2)], false);
        provision_extension(&root, &zip2, &sums2, "1.1.0").unwrap();
        assert_eq!(
            fs::read_to_string(root.join("extensions/chrome/current"))
                .unwrap()
                .trim(),
            "1.1.0"
        );
        // 旧版本目录保留（回退能力）。
        assert!(root.join("extensions/chrome/1.0.0/manifest.json").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn unsafe_entry_names_rejected() {
        let root =
            std::env::temp_dir().join(format!("natives-ext-prov-unsafe-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let zip = stored_zip(&[("../escape.js", b"x")]);
        let zip_path = root.join("ext.zip");
        fs::write(&zip_path, &zip).unwrap();
        let sums = root.join("SHA256SUMS");
        fs::write(&sums, format!("{}  ext.zip\n", hex_sha256(&zip))).unwrap();
        let err = provision_extension(&root, &zip_path, &sums, "0.1.0").unwrap_err();
        assert!(err.contains("unsafe zip entry name"), "got: {err}");
        let _ = fs::remove_dir_all(&root);
    }
}
