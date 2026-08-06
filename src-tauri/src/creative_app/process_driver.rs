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

/// Shell/script interpreters a Python profile must never reference (T06).
///
/// A proposal whose interpreter is `/bin/sh`, `/bin/bash`, `/usr/bin/env`, a
/// JS runtime, etc. is a red flag: it would let the agent execute arbitrary
/// commands through the "python" launch path. The basename must be Python-like.
pub const FORBIDDEN_INTERPRETER_NAMES: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "dash",
    "ksh",
    "fish",
    "tcsh",
    "env",
    "node",
    "deno",
    "ruby",
    "perl",
    "php",
    "pwsh",
    "powershell",
];

/// Whether an interpreter basename is plausibly a Python executable.
pub fn is_plausible_python_interpreter(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("python")
}

/// Validate a Python profile. Returns Err on invalid configuration.
pub fn validate_python_profile(profile: &PythonLaunchProfile) -> Result<()> {
    if profile.interpreter.is_empty() {
        return Err(Error::InvalidInput(
            "python interpreter cannot be empty".into(),
        ));
    }
    // T06: the interpreter must look like a Python executable. `/bin/sh` and
    // friends are rejected here (before the user ever sees the proposal) so a
    // "python app" can never silently run a shell. File existence + identity
    // are verified again at approval (`resolve_python_interpreter`).
    let interpreter_name = Path::new(&profile.interpreter)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if FORBIDDEN_INTERPRETER_NAMES.contains(&interpreter_name) {
        return Err(Error::InvalidInput(format!(
            "interpreter {interpreter_name} is not a Python executable"
        )));
    }
    if !is_plausible_python_interpreter(interpreter_name) {
        return Err(Error::InvalidInput(format!(
            "interpreter must be a Python executable, got '{interpreter_name}'"
        )));
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
            return Err(Error::InvalidInput(
                "python entry must not escape cwd".into(),
            ));
        }
    }
    // cwd_relative must not escape the project root.
    let cwd = Path::new(&profile.cwd_relative);
    if cwd.is_absolute()
        || cwd
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(Error::InvalidInput(
            "python cwd must be inside project root".into(),
        ));
    }
    Ok(())
}

/// Resolve a Python interpreter to its canonical absolute path at approval
/// time. The interpreter must exist, be a regular executable file, and be a
/// Python executable — never a shell. Returns the canonical path.
pub fn resolve_python_interpreter(interpreter: &str) -> Result<String> {
    let name = Path::new(interpreter)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if !is_plausible_python_interpreter(name) || FORBIDDEN_INTERPRETER_NAMES.contains(&name) {
        return Err(Error::InvalidInput(format!(
            "interpreter '{interpreter}' is not a Python executable"
        )));
    }
    let path = Path::new(interpreter);
    if !path.is_absolute() {
        return Err(Error::InvalidInput(
            "python interpreter must be an absolute path".into(),
        ));
    }
    let canonical = std::fs::canonicalize(path).map_err(|e| {
        Error::InvalidInput(format!(
            "python interpreter not found or not accessible: {interpreter}: {e}"
        ))
    })?;
    let meta = std::fs::metadata(&canonical)
        .map_err(|e| Error::InvalidInput(format!("cannot stat {canonical:?}: {e}")))?;
    if !meta.is_file() {
        return Err(Error::InvalidInput(format!(
            "python interpreter is not a regular file: {}",
            canonical.display()
        )));
    }
    ensure_executable(&canonical, &meta)?;
    Ok(canonical.to_string_lossy().to_string())
}

#[cfg(unix)]
fn ensure_executable(path: &Path, meta: &std::fs::Metadata) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if meta.permissions().mode() & 0o111 == 0 {
        return Err(Error::InvalidInput(format!(
            "file is not executable: {}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_executable(_path: &Path, _meta: &std::fs::Metadata) -> Result<()> {
    Ok(())
}

/// Validate a Binary profile structurally. The executable must be an absolute
/// path. T06: the agent's `approved` flag and hash are untrusted and are NOT
/// checked here — the Host recomputes file identity and records approval at the
/// user's explicit approve action (`resolve_binary_identity`).
pub fn validate_binary_profile(profile: &BinaryLaunchProfile) -> Result<()> {
    if profile.executable_path.is_empty() {
        return Err(Error::InvalidInput(
            "binary executable path cannot be empty".into(),
        ));
    }
    let exe = Path::new(&profile.executable_path);
    if !exe.is_absolute() {
        return Err(Error::InvalidInput(
            "binary executable path must be absolute and canonical".into(),
        ));
    }
    Ok(())
}

/// Resolve a binary executable to its canonical path and Host-recomputed
/// SHA-256 at approval time. The executable must exist and be a regular file.
/// Returns `(canonical_path, sha256_hex)`.
pub fn resolve_binary_identity(executable_path: &str) -> Result<(String, String)> {
    let path = Path::new(executable_path);
    if !path.is_absolute() {
        return Err(Error::InvalidInput(
            "binary executable path must be absolute".into(),
        ));
    }
    let canonical = std::fs::canonicalize(path).map_err(|e| {
        Error::InvalidInput(format!(
            "binary executable not found or not accessible: {executable_path}: {e}"
        ))
    })?;
    let meta = std::fs::metadata(&canonical)
        .map_err(|e| Error::InvalidInput(format!("cannot stat {canonical:?}: {e}")))?;
    if !meta.is_file() {
        return Err(Error::InvalidInput(format!(
            "binary executable is not a regular file: {}",
            canonical.display()
        )));
    }
    let hash = sha256_hex(&canonical)?;
    Ok((canonical.to_string_lossy().to_string(), hash))
}

/// Verify that a binary's current content still matches an expected identity
/// (canonical path + SHA-256) right before it is launched. Any change — content
/// replacement, symlink target swap — re-canonicalizes and re-hashes, and a
/// mismatch invalidates the authorization. Returns the re-canonicalized path.
///
/// This is the T06 "变化即授权失效" guard. It is also the TOCTOU mitigation:
/// the caller must open and spawn FROM the returned canonical path, and T09's
/// launch path re-runs this check in the same window.
pub fn verify_binary_identity(executable_path: &str, expected_hash: &str) -> Result<String> {
    let (canonical, current_hash) = resolve_binary_identity(executable_path)?;
    if current_hash != expected_hash {
        return Err(Error::InvalidInput(format!(
            "binary executable changed since approval (hash mismatch); re-approval required"
        )));
    }
    Ok(canonical)
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
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
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
            self.buffer[self.buffer_len..self.buffer_len + take].copy_from_slice(&data[..take]);
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
        let pad_len = if self.buffer_len < 56 {
            56 - self.buffer_len
        } else {
            120 - self.buffer_len
        };
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
            port: super::super::model::LaunchPort {
                mode: super::super::model::LaunchPortMode::Auto,
                value: None,
            },
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
    fn python_profile_rejects_shell_as_fake_interpreter() {
        // T06: `/bin/sh` masquerading as a Python interpreter is blocked before
        // the proposal is ever shown to the user.
        let base = PythonLaunchProfile {
            schema_version: 1,
            interpreter: "/bin/sh".into(),
            entry: "app.py".into(),
            args: vec![],
            cwd_relative: ".".into(),
            environment_keys: vec![],
            port: super::super::model::LaunchPort {
                mode: super::super::model::LaunchPortMode::Auto,
                value: None,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 60_000,
            is_venv: false,
        };
        for shell in [
            "/bin/sh",
            "/bin/bash",
            "/usr/bin/env",
            "/usr/bin/node",
            "sh",
        ] {
            let mut p = base.clone();
            p.interpreter = shell.into();
            assert!(
                validate_python_profile(&p).is_err(),
                "shell interpreter {shell} must be rejected"
            );
        }
        // A real venv python passes.
        let mut ok = base.clone();
        ok.interpreter = "/proj/.venv/bin/python3".into();
        assert!(validate_python_profile(&ok).is_ok());
    }

    #[test]
    fn binary_profile_structural_validation() {
        // T06: the agent's `approved` flag and hash are untrusted and must NOT
        // be required by the structural validator — the Host recomputes file
        // identity at approve time.
        let profile = BinaryLaunchProfile {
            schema_version: 1,
            executable_path: "/usr/local/bin/myapp".into(),
            executable_hash: String::new(),
            approved: false,
            args: vec![],
            cwd_relative: ".".into(),
            environment_keys: vec![],
            port: super::super::model::LaunchPort {
                mode: super::super::model::LaunchPortMode::Auto,
                value: None,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 60_000,
        };
        // No approval claim, no agent hash — still structurally valid.
        assert!(validate_binary_profile(&profile).is_ok());

        // Relative path rejected.
        let mut bad = profile.clone();
        bad.executable_path = "bin/myapp".into();
        assert!(validate_binary_profile(&bad).is_err());

        // Empty path rejected.
        let mut bad2 = profile.clone();
        bad2.executable_path = "".into();
        assert!(validate_binary_profile(&bad2).is_err());
    }

    #[test]
    fn resolve_binary_identity_recomputes_hash_and_blocks_swap() {
        // T06: the Host recomputes the executable's SHA-256 at approval (never
        // trusting an agent-supplied value), and a content swap invalidates the
        // identity right before launch.
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("myapp");
        std::fs::write(&bin, b"#!/bin/sh\necho v1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let (canonical, hash) = resolve_binary_identity(bin.to_str().unwrap()).unwrap();
        assert!(canonical.ends_with("myapp"));
        assert_eq!(hash.len(), 64);

        // Same content passes identity verification.
        assert_eq!(
            verify_binary_identity(&canonical, &hash).unwrap(),
            canonical
        );

        // Replace the content → identity mismatch → authorization invalid.
        std::fs::write(&bin, b"#!/bin/sh\necho v2 - swapped\n").unwrap();
        assert!(verify_binary_identity(&canonical, &hash).is_err());
    }

    #[test]
    fn resolve_binary_identity_rejects_missing_and_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nope");
        assert!(resolve_binary_identity(missing.to_str().unwrap()).is_err());

        let dir = tmp.path().join("adir");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(resolve_binary_identity(dir.to_str().unwrap()).is_err());
    }

    #[test]
    fn resolve_python_interpreter_rejects_shell_and_accepts_real_python() {
        let tmp = tempfile::tempdir().unwrap();
        // A real (fake) python executable named python3.
        let py = tmp.path().join("python3");
        std::fs::write(&py, b"#!/usr/bin/env python3\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&py, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        for shell in ["/bin/sh", "/bin/bash", "/usr/bin/env", "/usr/bin/node"] {
            assert!(
                resolve_python_interpreter(shell).is_err(),
                "shell interpreter {shell} must be blocked at approval"
            );
        }
        assert!(
            resolve_python_interpreter(py.to_str().unwrap()).is_ok(),
            "a python3 executable must resolve"
        );
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
