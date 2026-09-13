//! 会话鉴权（托管应用契约 v1 §5.2/§7）。
//!
//! - instanceId/generation/challenge：≥128 位随机（base64url）
//! - token：32 字节 CSPRNG、base64url，绑定 instance/generation，最长 15 分钟
//! - 过期/撤销后业务请求返回 APP_SESSION_INVALID；stop/revoke/重载立即撤销
//! - 以单调时钟计时，不依赖挂钟回拨

use crate::error::{AppErrorBody, AppErrorCode};
use crate::protocol::{INSTANCE_ID_BYTES, SESSION_TOKEN_BYTES, SESSION_TTL_SECS};
use std::time::Instant;

/// OS CSPRNG 字节；失败即无法签发（调用方映射 APP_START_FAILED，不得用弱随机降级）。
pub fn random_bytes(buf: &mut [u8]) -> bool {
    getrandom_fill(buf)
}

#[cfg(target_os = "macos")]
fn getrandom_fill(buf: &mut [u8]) -> bool {
    extern "C" {
        fn getentropy(buffer: *mut u8, size: usize) -> i32;
    }
    unsafe { getentropy(buf.as_mut_ptr(), buf.len()) == 0 }
}

#[cfg(target_os = "linux")]
fn getrandom_fill(buf: &mut [u8]) -> bool {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(buf))
        .is_ok()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn getrandom_fill(_buf: &mut [u8]) -> bool {
    false
}

const B64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

pub fn base64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64URL[(n >> 18) as usize & 63] as char);
        out.push(B64URL[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(B64URL[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(B64URL[n as usize & 63] as char);
        }
    }
    out
}

/// 常量时间比较（防时序侧信道）。
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// 一次 issue 出的会话能力。
pub struct Session {
    token: String,
    expires_at: Instant,
}

impl Session {
    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn is_valid(&self, now: Instant) -> bool {
        now < self.expires_at
    }
}

/// 每 instance/generation 一个会话管理器。
pub struct SessionManager {
    generation: String,
    session: Option<Session>,
}

impl SessionManager {
    pub fn new() -> Option<Self> {
        let generation = random_id()?;
        Some(Self {
            generation,
            session: None,
        })
    }

    pub fn generation(&self) -> &str {
        &self.generation
    }

    /// issue：challenge 必填；返回 (token, expiresAtSecs)。
    pub fn issue(
        &mut self,
        challenge: &str,
    ) -> Result<crate::protocol::SessionIssueResult, AppErrorBody> {
        if challenge.is_empty() {
            return Err(AppErrorBody::new(
                AppErrorCode::SessionInvalid,
                "challenge required",
                false,
            ));
        }
        let mut bytes = [0u8; SESSION_TOKEN_BYTES];
        if !random_bytes(&mut bytes) {
            return Err(AppErrorBody::new(
                AppErrorCode::StartFailed,
                "no OS randomness",
                false,
            ));
        }
        let token = base64url(&bytes);
        self.session = Some(Session {
            token: token.clone(),
            expires_at: Instant::now() + std::time::Duration::from_secs(SESSION_TTL_SECS),
        });
        Ok(crate::protocol::SessionIssueResult {
            generation: self.generation.clone(),
            token,
            expires_at: SESSION_TTL_SECS,
        })
    }

    /// 校验 bearer；错 token、旧 generation、过期一律 APP_SESSION_INVALID。
    pub fn authorize(&self, generation: &str, bearer: &str) -> Result<(), AppErrorBody> {
        if generation != self.generation {
            return Err(AppErrorBody::new(
                AppErrorCode::SessionInvalid,
                "stale generation",
                false,
            ));
        }
        let Some(session) = &self.session else {
            return Err(AppErrorBody::new(
                AppErrorCode::SessionInvalid,
                "no active session",
                false,
            ));
        };
        if !session.is_valid(Instant::now()) {
            return Err(AppErrorBody::new(
                AppErrorCode::SessionInvalid,
                "session expired",
                false,
            ));
        }
        let expected = format!("Bearer {}", session.token());
        if !constant_time_eq(bearer, &expected) {
            return Err(AppErrorBody::new(
                AppErrorCode::SessionInvalid,
                "invalid bearer",
                false,
            ));
        }
        Ok(())
    }

    /// rotate：撤销当前会话并换发新 generation（契约 app:session op=rotate）。
    pub fn rotate(&mut self) -> Result<String, AppErrorBody> {
        let mut bytes = [0u8; INSTANCE_ID_BYTES];
        if !random_bytes(&mut bytes) {
            return Err(AppErrorBody::new(
                AppErrorCode::StartFailed,
                "no OS randomness",
                false,
            ));
        }
        self.generation = base64url(&bytes);
        self.session = None;
        Ok(self.generation.clone())
    }

    /// revoke：立即失效（stop/revoke/load/navigation 共用）。
    pub fn revoke(&mut self) {
        self.session = None;
    }
}

/// 128-bit base64url identifier，供 instanceId/challenge 等非 Secret wire 字段使用。
pub fn random_id() -> Option<String> {
    let mut bytes = [0u8; INSTANCE_ID_BYTES];
    if !random_bytes(&mut bytes) {
        return None;
    }
    Some(base64url(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_and_authorize_round_trip() {
        let mut manager = SessionManager::new().unwrap();
        let issued = manager.issue("challenge-1").unwrap();
        assert_eq!(issued.token.len(), 43); // 32 bytes → 43 base64url chars
        assert_eq!(issued.generation, manager.generation());
        manager
            .authorize(&issued.generation, &format!("Bearer {}", issued.token))
            .unwrap();
    }

    #[test]
    fn wrong_token_generation_or_missing_session_rejected() {
        let mut manager = SessionManager::new().unwrap();
        let issued = manager.issue("c").unwrap();
        let bad = manager.authorize(&issued.generation, "Bearer AAAA");
        assert_eq!(bad.unwrap_err().code, "APP_SESSION_INVALID");
        let stale = manager.authorize("old-generation", &format!("Bearer {}", issued.token));
        assert_eq!(stale.unwrap_err().code, "APP_SESSION_INVALID");
        manager.revoke();
        let revoked = manager.authorize(&issued.generation, &format!("Bearer {}", issued.token));
        assert_eq!(revoked.unwrap_err().code, "APP_SESSION_INVALID");
    }

    #[test]
    fn challenge_is_required() {
        let mut manager = SessionManager::new().unwrap();
        assert_eq!(manager.issue("").unwrap_err().code, "APP_SESSION_INVALID");
    }

    #[test]
    fn rotate_changes_generation_and_revokes_session() {
        let mut manager = SessionManager::new().unwrap();
        let issued = manager.issue("c").unwrap();
        let new_generation = manager.rotate().unwrap();
        assert_ne!(new_generation, issued.generation);
        assert_eq!(manager.generation(), new_generation);
        let revoked = manager.authorize(&issued.generation, &format!("Bearer {}", issued.token));
        assert_eq!(revoked.unwrap_err().code, "APP_SESSION_INVALID");
    }
}
