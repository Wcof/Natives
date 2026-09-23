//! 会话鉴权（托管应用契约 v1 §5.2/§7；计划 §19 安全收口）。
//!
//! - instanceId/generation/challenge：≥128 位随机（base64url）
//! - CSPRNG：统一 `getrandom` crate（禁止平台手搓，计划 §19.1/§36.1）
//! - challenge：≥128 位 base64url、单 generation 一次性、不可重放（§19.2）
//! - token：32 字节 CSPRNG、base64url，绑定 generation + challenge_hash +
//!   issued_at + expiry（§19.3），最长 15 分钟
//! - 过期/撤销后业务请求返回 APP_SESSION_INVALID；stop/revoke/重载立即撤销
//! - rotate：撤销旧 bearer 与旧 challenge 集合，产生新 generation
//! - 以单调时钟计时，不依赖挂钟回拨

use crate::error::{AppErrorBody, AppErrorCode};
use crate::protocol::{INSTANCE_ID_BYTES, SESSION_TOKEN_BYTES, SESSION_TTL_SECS};
use std::collections::HashSet;
use std::time::Instant;

/// OS CSPRNG 字节（`getrandom` 跨平台统一实现）；
/// 失败即无法签发（调用方映射 APP_START_FAILED，不得用弱随机降级）。
pub fn random_bytes(buf: &mut [u8]) -> bool {
    getrandom::fill(buf).is_ok()
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

/// 一次 issue 出的会话能力：token 绑定 generation + challenge_hash + expiry（§19.3）。
pub struct Session {
    token: String,
    challenge_hash: String,
    #[allow(dead_code)]
    issued_at: Instant,
    expires_at: Instant,
}

impl Session {
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Token 绑定的 challenge 摘要（§19.3：审计/排障可见，不含原始 challenge）。
    pub fn challenge_hash(&self) -> &str {
        &self.challenge_hash
    }

    pub fn is_valid(&self, now: Instant) -> bool {
        now < self.expires_at
    }
}

/// 每 instance/generation 一个会话管理器。
pub struct SessionManager {
    generation: String,
    session: Option<Session>,
    /// 本 generation 内已消费的 challenge 哈希：单次有效、不可重放（§19.2）。
    used_challenges: HashSet<String>,
}

impl SessionManager {
    pub fn new() -> Option<Self> {
        let generation = random_id()?;
        Some(Self {
            generation,
            session: None,
            used_challenges: HashSet::new(),
        })
    }

    pub fn generation(&self) -> &str {
        &self.generation
    }

    /// issue：challenge 必填、≥128 位、单 generation 一次性（不可重放）；
    /// 返回 (token, expiresAtSecs)。
    pub fn issue(
        &mut self,
        challenge: &str,
    ) -> Result<crate::protocol::SessionIssueResult, AppErrorBody> {
        // challenge 形状：≥128 位熵 → base64url 编码至少 22 个字符（§19.2）。
        if challenge.is_empty() {
            return Err(AppErrorBody::new(
                AppErrorCode::SessionInvalid,
                "challenge required",
                false,
            ));
        }
        if challenge.len() < 22
            || !challenge
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(AppErrorBody::new(
                AppErrorCode::SessionInvalid,
                "challenge must be a 128-bit base64url value",
                false,
            ));
        }
        let challenge_hash = crate::sha256_hex(challenge.as_bytes());
        if !self.used_challenges.insert(challenge_hash.clone()) {
            // 重放检测：同一 challenge 在本 generation 内已消费。
            return Err(AppErrorBody::new(
                AppErrorCode::SessionInvalid,
                "challenge replayed",
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
        let now = Instant::now();
        self.session = Some(Session {
            token: token.clone(),
            challenge_hash,
            issued_at: now,
            expires_at: now + std::time::Duration::from_secs(SESSION_TTL_SECS),
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

    /// rotate：撤销当前会话并换发新 generation（契约 app:session op=rotate）；
    /// 旧 bearer 与旧 challenge 集合一并失效（§19.3）。
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
        self.used_challenges.clear();
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
        let issued = manager.issue("abcdefghijklmnopqrstuvwxyz12").unwrap();
        assert_eq!(issued.token.len(), 43); // 32 bytes → 43 base64url chars
        assert_eq!(issued.generation, manager.generation());
        manager
            .authorize(&issued.generation, &format!("Bearer {}", issued.token))
            .unwrap();
    }

    #[test]
    fn wrong_token_generation_or_missing_session_rejected() {
        let mut manager = SessionManager::new().unwrap();
        let issued = manager.issue("abcdefghijklmnopqrstuvwxyz12").unwrap();
        let bad = manager.authorize(&issued.generation, "Bearer AAAA");
        assert_eq!(bad.unwrap_err().code, "APP_SESSION_INVALID");
        let stale = manager.authorize("old-generation", &format!("Bearer {}", issued.token));
        assert_eq!(stale.unwrap_err().code, "APP_SESSION_INVALID");
        manager.revoke();
        let revoked = manager.authorize(&issued.generation, &format!("Bearer {}", issued.token));
        assert_eq!(revoked.unwrap_err().code, "APP_SESSION_INVALID");
    }

    #[test]
    fn challenge_is_required_and_must_be_128bit() {
        let mut manager = SessionManager::new().unwrap();
        assert_eq!(manager.issue("").unwrap_err().code, "APP_SESSION_INVALID");
        // 短于 128 位熵的 challenge 拒绝（§19.2）。
        assert_eq!(
            manager.issue("short").unwrap_err().code,
            "APP_SESSION_INVALID"
        );
        // 非法字符拒绝。
        assert_eq!(
            manager
                .issue("abcdefghijklmnopqrstuvwxyz1+")
                .unwrap_err()
                .code,
            "APP_SESSION_INVALID"
        );
    }

    #[test]
    fn challenge_cannot_be_replayed_within_generation() {
        let mut manager = SessionManager::new().unwrap();
        let challenge = "abcdefghijklmnopqrstuvwxyz12";
        manager.issue(challenge).unwrap();
        let replay = manager.issue(challenge).unwrap_err();
        assert_eq!(replay.code, "APP_SESSION_INVALID");
        assert!(replay.message.contains("replayed"));
    }

    #[test]
    fn rotate_changes_generation_and_revokes_session() {
        let mut manager = SessionManager::new().unwrap();
        let issued = manager.issue("abcdefghijklmnopqrstuvwxyz12").unwrap();
        let new_generation = manager.rotate().unwrap();
        assert_ne!(new_generation, issued.generation);
        assert_eq!(manager.generation(), new_generation);
        let revoked = manager.authorize(&issued.generation, &format!("Bearer {}", issued.token));
        assert_eq!(revoked.unwrap_err().code, "APP_SESSION_INVALID");
        // rotate 后旧 challenge 集合清空：同 challenge 可在新 generation 复用。
        manager.issue("abcdefghijklmnopqrstuvwxyz12").unwrap();
    }
}
