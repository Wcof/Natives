//! 独立 Web 会话鉴权与 Surface 租约状态机（ADR-0042）。

use app_runtime_core::session::{base64url, constant_time_eq, random_bytes};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

const SESSION_TTL: Duration = Duration::from_secs(12 * 3600); // 最长 12 小时
const SESSION_IDLE_TTL: Duration = Duration::from_secs(30 * 60); // 30 分钟无操作过期
const PAIRING_CODE_TTL: Duration = Duration::from_secs(5 * 60); // 配对码 5 分钟有效
const BOOTSTRAP_TOKEN_TTL: Duration = Duration::from_secs(60); // Bootstrap Token 60 秒有效
const MAX_PAIRING_ATTEMPTS: u32 = 5;
const SURFACE_LEASE_TTL: Duration = Duration::from_secs(75); // 75 秒租约到期

pub struct WebSession {
    pub csrf_token: String,
    pub created_at: Instant,
    pub last_active: Instant,
    pub surfaces: HashSet<String>,
}

pub struct BootstrapToken {
    pub token: String,
    pub expires_at: Instant,
}

pub struct PairingCode {
    pub code: String,
    pub expires_at: Instant,
    pub failed_attempts: u32,
}

pub struct SurfaceRecord {
    pub session_id: String,
    pub app_id: Option<String>,
    pub last_renewed: Instant,
}

pub struct AuthManager {
    bootstrap_token: Option<BootstrapToken>,
    pairing_code: Option<PairingCode>,
    sessions: HashMap<String, WebSession>,
    surfaces: HashMap<String, SurfaceRecord>,
    minute_attempts: u32,
    minute_reset: Instant,
}

impl AuthManager {
    pub fn new(bootstrap: Option<String>) -> Self {
        let now = Instant::now();
        let bootstrap_token = bootstrap
            .map(|tok| BootstrapToken {
                token: tok,
                expires_at: now + BOOTSTRAP_TOKEN_TTL,
            })
            .or_else(|| {
                let mut buf = [0u8; 32];
                if random_bytes(&mut buf) {
                    Some(BootstrapToken {
                        token: base64url(&buf),
                        expires_at: now + BOOTSTRAP_TOKEN_TTL,
                    })
                } else {
                    None
                }
            });
        Self {
            bootstrap_token,
            pairing_code: None,
            sessions: HashMap::new(),
            surfaces: HashMap::new(),
            minute_attempts: 0,
            minute_reset: now + Duration::from_secs(60),
        }
    }

    pub fn current_bootstrap_token(&self) -> Option<&str> {
        let now = Instant::now();
        self.bootstrap_token
            .as_ref()
            .filter(|b| now <= b.expires_at)
            .map(|b| b.token.as_str())
    }

    fn check_rate_limit(&mut self) -> Result<(), &'static str> {
        let now = Instant::now();
        if now >= self.minute_reset {
            self.minute_attempts = 0;
            self.minute_reset = now + Duration::from_secs(60);
        }
        if self.minute_attempts >= 10 {
            return Err("ERR_RATE_LIMIT_EXCEEDED");
        }
        self.minute_attempts += 1;
        Ok(())
    }

    pub fn generate_pairing_code(&mut self) -> Result<String, &'static str> {
        let mut buf = [0u8; 10];
        if !random_bytes(&mut buf) {
            return Err("ERR_CSPRNG_FAILED");
        }
        // Base32 编码生成 10 位可读验证码
        const B32_ALPHABET: &[u8; 32] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
        let mut code = String::with_capacity(10);
        for &b in buf.iter().take(10) {
            code.push(B32_ALPHABET[(b as usize) % 32] as char);
        }
        self.pairing_code = Some(PairingCode {
            code: code.clone(),
            expires_at: Instant::now() + PAIRING_CODE_TTL,
            failed_attempts: 0,
        });
        Ok(code)
    }

    pub fn generate_bootstrap_token(&mut self) -> Result<String, &'static str> {
        let mut bytes = [0u8; 32];
        if !random_bytes(&mut bytes) {
            return Err("ERR_CSPRNG_FAILED");
        }
        let token = base64url(&bytes);
        self.bootstrap_token = Some(BootstrapToken {
            token: token.clone(),
            expires_at: Instant::now() + BOOTSTRAP_TOKEN_TTL,
        });
        Ok(token)
    }

    pub fn exchange_bootstrap(&mut self, token: &str) -> Result<(String, String), &'static str> {
        self.check_rate_limit()?;
        let Some(bootstrap) = &self.bootstrap_token else {
            return Err("ERR_BOOTSTRAP_INVALID");
        };
        if Instant::now() > bootstrap.expires_at {
            self.bootstrap_token = None;
            return Err("ERR_BOOTSTRAP_EXPIRED");
        }
        if !constant_time_eq(token, &bootstrap.token) {
            return Err("ERR_BOOTSTRAP_INVALID");
        }
        // 一次性消费
        self.bootstrap_token = None;
        self.create_session()
    }

    pub fn exchange_pairing_code(&mut self, code: &str) -> Result<(String, String), &'static str> {
        self.check_rate_limit()?;
        let Some(pairing) = &mut self.pairing_code else {
            return Err("ERR_PAIRING_CODE_NOT_FOUND");
        };
        if Instant::now() > pairing.expires_at {
            self.pairing_code = None;
            return Err("ERR_PAIRING_CODE_EXPIRED");
        }
        if !constant_time_eq(&code.to_ascii_uppercase(), &pairing.code) {
            pairing.failed_attempts += 1;
            if pairing.failed_attempts >= MAX_PAIRING_ATTEMPTS {
                self.pairing_code = None;
                return Err("ERR_PAIRING_CODE_LOCKED");
            }
            return Err("ERR_PAIRING_CODE_MISMATCH");
        }
        self.pairing_code = None;
        self.create_session()
    }

    fn create_session(&mut self) -> Result<(String, String), &'static str> {
        self.reap_expired();
        if self.sessions.len() >= 64 {
            return Err("ERR_SESSION_LIMIT");
        }
        let mut session_buf = [0u8; 32];
        let mut csrf_buf = [0u8; 32];
        if !random_bytes(&mut session_buf) || !random_bytes(&mut csrf_buf) {
            return Err("ERR_CSPRNG_FAILED");
        }
        let session_id = base64url(&session_buf);
        let csrf_token = base64url(&csrf_buf);
        let now = Instant::now();
        self.sessions.insert(
            session_id.clone(),
            WebSession {
                csrf_token: csrf_token.clone(),
                created_at: now,
                last_active: now,
                surfaces: HashSet::new(),
            },
        );
        Ok((session_id, csrf_token))
    }

    pub fn validate_session(&mut self, session_id: &str) -> bool {
        self.reap_expired();
        if let Some(session) = self.sessions.get_mut(session_id) {
            let now = Instant::now();
            if now.duration_since(session.created_at) > SESSION_TTL
                || now.duration_since(session.last_active) > SESSION_IDLE_TTL
            {
                return false;
            }
            session.last_active = now;
            true
        } else {
            false
        }
    }

    pub fn validate_csrf(&self, session_id: &str, csrf: &str) -> bool {
        if let Some(session) = self.sessions.get(session_id) {
            constant_time_eq(&session.csrf_token, csrf)
        } else {
            false
        }
    }

    pub fn get_csrf_token(&self, session_id: &str) -> Option<&str> {
        self.sessions.get(session_id).map(|s| s.csrf_token.as_str())
    }

    pub fn revoke_session(&mut self, session_id: &str) {
        if let Some(session) = self.sessions.remove(session_id) {
            for surface_id in session.surfaces {
                self.surfaces.remove(&surface_id);
            }
        }
    }

    pub fn register_surface(
        &mut self,
        session_id: &str,
        app_id: Option<String>,
    ) -> Result<String, &'static str> {
        if !self.validate_session(session_id) {
            return Err("ERR_SESSION_INVALID");
        }
        if self.surfaces.len() >= 128
            || self
                .sessions
                .get(session_id)
                .is_some_and(|s| s.surfaces.len() >= 32)
        {
            return Err("ERR_SURFACE_LIMIT");
        }
        let mut buf = [0u8; 16];
        if !random_bytes(&mut buf) {
            return Err("ERR_CSPRNG_FAILED");
        }
        let surface_id = format!("surf_{}", base64url(&buf));
        let now = Instant::now();
        self.surfaces.insert(
            surface_id.clone(),
            SurfaceRecord {
                session_id: session_id.to_string(),
                app_id,
                last_renewed: now,
            },
        );
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.surfaces.insert(surface_id.clone());
        }
        Ok(surface_id)
    }

    pub fn renew_surface(
        &mut self,
        session_id: &str,
        surface_id: &str,
    ) -> Result<(), &'static str> {
        if !self.validate_session(session_id) {
            return Err("ERR_SESSION_INVALID");
        }
        if let Some(surface) = self.surfaces.get_mut(surface_id) {
            if surface.session_id != session_id {
                return Err("ERR_SURFACE_NOT_OWNED");
            }
            surface.last_renewed = Instant::now();
            Ok(())
        } else {
            Err("ERR_SURFACE_NOT_FOUND")
        }
    }

    pub fn owns_surface(&mut self, session_id: &str, surface_id: &str) -> bool {
        self.validate_session(session_id) && self.owns_active_surface(session_id, surface_id)
    }

    // SSE liveness must not count as user activity and renew the session forever.
    pub fn owns_active_surface(&self, session_id: &str, surface_id: &str) -> bool {
        self.surfaces.get(surface_id).is_some_and(|surface| {
            surface.session_id == session_id && surface.last_renewed.elapsed() <= SURFACE_LEASE_TTL
        })
    }

    pub fn authorize_app_surface(
        &mut self,
        session_id: &str,
        surface_id: &str,
        app_id: Option<&str>,
    ) -> bool {
        if !self.validate_session(session_id) {
            return false;
        }
        let Some(surface) = self.surfaces.get_mut(surface_id) else {
            return false;
        };
        if surface.session_id != session_id
            || Instant::now().duration_since(surface.last_renewed) > SURFACE_LEASE_TTL
        {
            return false;
        }
        match (&surface.app_id, app_id) {
            (Some(bound), Some(requested)) => bound == requested,
            (Some(_), None) => true,
            (None, Some(requested)) if matches!(requested, "fund" | "tokenusage") => {
                surface.app_id = Some(requested.to_string());
                true
            }
            _ => false,
        }
    }

    pub fn revoke_surface(
        &mut self,
        session_id: &str,
        surface_id: &str,
    ) -> Result<bool, &'static str> {
        if let Some(surface) = self.surfaces.get(surface_id) {
            if surface.session_id != session_id {
                return Err("ERR_SURFACE_NOT_OWNED");
            }
        } else {
            return Ok(false); // 幂等成功，不释放不属于本会话的 Runtime。
        }
        self.surfaces.remove(surface_id);
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.surfaces.remove(surface_id);
        }
        Ok(true)
    }

    pub fn active_surface_count(&self) -> usize {
        let now = Instant::now();
        self.surfaces
            .values()
            .filter(|s| now.duration_since(s.last_renewed) <= SURFACE_LEASE_TTL)
            .count()
    }

    pub fn active_surface_ids(&mut self) -> HashSet<String> {
        self.reap_expired();
        self.surfaces.keys().cloned().collect()
    }

    pub fn reap_expired(&mut self) {
        let now = Instant::now();
        // 清理超期会话
        self.sessions.retain(|_, s| {
            now.duration_since(s.created_at) <= SESSION_TTL
                && now.duration_since(s.last_active) <= SESSION_IDLE_TTL
        });
        // 清理超期 Surface
        let valid_sessions = &self.sessions;
        self.surfaces.retain(|_, s| {
            valid_sessions.contains_key(&s.session_id)
                && now.duration_since(s.last_renewed) <= SURFACE_LEASE_TTL
        });
        for session in self.sessions.values_mut() {
            session.surfaces.retain(|id| self.surfaces.contains_key(id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_are_bounded_without_evicting_active_owners() {
        let mut auth = AuthManager::new(None);
        let mut ids = Vec::new();
        for _ in 0..64 {
            ids.push(auth.create_session().unwrap().0);
        }
        assert_eq!(auth.create_session(), Err("ERR_SESSION_LIMIT"));
        assert!(auth.validate_session(&ids[0]));
        auth.revoke_session(&ids[0]);
        assert!(auth.create_session().is_ok());
    }
}
