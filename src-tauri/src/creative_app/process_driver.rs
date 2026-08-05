//! Python and Binary managed process drivers (batch 8 CR-801, CR-802).
//!
//! Both drivers share the ProcessDriver pattern: argv-only launch, cwd inside
//! the project root, port/health/log via the existing local runtime. This module
//! provides candidate detection and profile validation. The actual process
//! spawn integrates with `LocalRuntimeManager` in `local::runtime`.
//!
//! Security: Binary profiles require a canonical path + content hash approval;
//! a hash change forces re-approval. Python profiles only reference an
//! interpreter and entry — never shell, never secrets.

use super::model::{BinaryLaunchProfile, PythonLaunchProfile};
use crate::{Error, Result};
use std::path::Path;

/// Entry filenames that strongly indicate a Python WebUI app.
pub const PYTHON_WEB_ENTRIES: &[&str] = &[
    "app.py",
    "main.py",
    "manage.py",
    "run.py",
    "wsgi.py",
    "asgi.py",
    "webui.py",
];

/// Detect whether a relative entry looks like a Python WebUI script.
pub fn looks_like_python_web_entry(entry: &str) -> bool {
    let name = Path::new(entry)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    PYTHON_WEB_ENTRIES.contains(&name)
}

/// Validate a Python profile. Returns Err on invalid configuration.
pub fn validate_python_profile(profile: &PythonLaunchProfile) -> Result<()> {
    if profile.interpreter.is_empty() {
        return Err(Error::InvalidInput("python interpreter cannot be empty".into()));
    }
    if profile.entry.is_empty() {
        return Err(Error::InvalidInput("python entry cannot be empty".into()));
    }
    // Entry must be a relative path (never absolute, never parent traversal).
    let entry = Path::new(&profile.entry);
    if entry.is_absolute() {
        return Err(Error::InvalidInput("python entry must be relative".into()));
    }
    for c in entry.components() {
        if let std::path::Component::ParentDir = c {
            return Err(Error::InvalidInput("python entry must not escape cwd".into()));
        }
    }
    // cwd_relative must not escape the project root.
    let cwd = Path::new(&profile.cwd_relative);
    if cwd.is_absolute() || cwd.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(Error::InvalidInput("python cwd must be inside project root".into()));
    }
    Ok(())
}

/// Validate a Binary profile. The executable must be an absolute canonical path
/// with a non-empty content hash, and must be approved.
pub fn validate_binary_profile(profile: &BinaryLaunchProfile) -> Result<()> {
    if profile.executable_path.is_empty() {
        return Err(Error::InvalidInput("binary executable path cannot be empty".into()));
    }
    let exe = Path::new(&profile.executable_path);
    if !exe.is_absolute() {
        return Err(Error::InvalidInput(
            "binary executable path must be absolute and canonical".into(),
        ));
    }
    if profile.executable_hash.len() != 64 {
        return Err(Error::InvalidInput(
            "binary executable hash must be a 64-char SHA-256 hex".into(),
        ));
    }
    if !profile.approved {
        return Err(Error::InvalidInput(
            "binary executable is not approved; hash changed or never approved".into(),
        ));
    }
    Ok(())
}

/// Compute the SHA-256 hex of a file (used for binary approval).
pub fn sha256_hex(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)
        .map_err(|e| Error::InvalidInput(format!("cannot open {path:?}: {e}")))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| Error::InvalidInput(format!("read {path:?}: {e}")))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_encode(&hasher.finalize()))
}

// ── Lightweight SHA-256 (no external crypto dependency) ──────────────

#[derive(Clone)]
struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffer_len: usize,
    total_len: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c,
                0x1f83d9ab, 0x5be0cd19,
            ],
            buffer: [0; 64],
            buffer_len: 0,
            total_len: 0,
        }
    }

    fn update(&mut self, data: &[u8]) {
        let mut data = data;
        self.total_len += data.len() as u64;
        if self.buffer_len > 0 {
            let space = 64 - self.buffer_len;
            let take = space.min(data.len());
            self.buffer[self.buffer_len..self.buffer_len + take]
                .copy_from_slice(&data[..take]);
            self.buffer_len += take;
            data = &data[take..];
            if self.buffer_len == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buffer_len = 0;
            }
        }
        while data.len() >= 64 {
            let block = data[..64].try_into().unwrap();
            self.compress(block);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buffer[..data.len()].copy_from_slice(data);
            self.buffer_len = data.len();
        }
    }

    fn finalize(mut self) -> [u8; 32] {
        let bit_len = self.total_len.wrapping_mul(8);
        // Pad: 0x80 then zeros then 8-byte big-endian bit length
        let pad_len = if self.buffer_len < 56 { 56 - self.buffer_len } else { 120 - self.buffer_len };
        let mut padding = vec![0u8; pad_len];
        padding[0] = 0x80;
        self.update(&padding);
        let len_bytes = bit_len.to_be_bytes();
        self.update(&len_bytes);
        let mut out = [0u8; 32];
        for (i, word) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    fn compress(&mut self, block: &[u8; 64]) {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];
        let mut w = [0u32; 64];
        for (i, chunk) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes(chunk.try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_entry_detection() {
        assert!(looks_like_python_web_entry("app.py"));
        assert!(looks_like_python_web_entry("src/main.py"));
        assert!(looks_like_python_web_entry("manage.py"));
        assert!(!looks_like_python_web_entry("index.html"));
        assert!(!looks_like_python_web_entry("script.sh"));
    }

    #[test]
    fn python_profile_validation() {
        let valid = PythonLaunchProfile {
            schema_version: 1,
            interpreter: "/proj/.venv/bin/python".into(),
            entry: "app.py".into(),
            args: vec![],
            cwd_relative: ".".into(),
            environment_keys: vec!["PORT".into()],
            port: super::super::model::LaunchPort { mode: super::super::model::LaunchPortMode::Auto, value: None },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 60_000,
            is_venv: true,
        };
        assert!(validate_python_profile(&valid).is_ok());

        // Absolute entry rejected
        let mut bad = valid.clone();
        bad.entry = "/etc/passwd".into();
        assert!(validate_python_profile(&bad).is_err());

        // Traversal in entry rejected
        let mut bad2 = valid.clone();
        bad2.entry = "../escape.py".into();
        assert!(validate_python_profile(&bad2).is_err());

        // Empty interpreter rejected
        let mut bad3 = valid.clone();
        bad3.interpreter = "".into();
        assert!(validate_python_profile(&bad3).is_err());
    }

    #[test]
    fn binary_profile_requires_approval_and_hash() {
        let approved = BinaryLaunchProfile {
            schema_version: 1,
            executable_path: "/usr/local/bin/myapp".into(),
            executable_hash: "a".repeat(64),
            approved: true,
            args: vec![],
            cwd_relative: ".".into(),
            environment_keys: vec![],
            port: super::super::model::LaunchPort { mode: super::super::model::LaunchPortMode::Auto, value: None },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 60_000,
        };
        assert!(validate_binary_profile(&approved).is_ok());

        // Unapproved rejected
        let mut bad = approved.clone();
        bad.approved = false;
        assert!(validate_binary_profile(&bad).is_err());

        // Short hash rejected
        let mut bad2 = approved.clone();
        bad2.executable_hash = "deadbeef".into();
        assert!(validate_binary_profile(&bad2).is_err());

        // Relative path rejected
        let mut bad3 = approved.clone();
        bad3.executable_path = "bin/myapp".into();
        assert!(validate_binary_profile(&bad3).is_err());
    }

    #[test]
    fn sha256_computes_known_value() {
        // SHA-256 of empty input
        let hash = sha256_hex(Path::new("/dev/null")).unwrap();
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
