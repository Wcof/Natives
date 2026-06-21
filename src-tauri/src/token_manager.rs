use crate::db;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const TOKEN_TTL_MS: u64 = 24 * 60 * 60 * 1000; // 24 hours
const ROTATION_INTERVAL_MS: u64 = (TOKEN_TTL_MS as f64 * 0.7) as u64; // 16.8 hours
const SECRET_KEY: &str = "_token_master_secret";

#[derive(Debug, Clone)]
struct TokenEntry {
    #[allow(dead_code)]
    token_hash: String,
    module_id: String,
    created_at: u64,
}

pub struct TokenManager {
    secret: String,
    tokens: Mutex<HashMap<String, TokenEntry>>,
}

impl TokenManager {
    /// Create a new TokenManager, loading or generating the master secret from DB.
    pub fn new(conn: &Connection) -> Self {
        let secret = Self::get_or_create_secret(conn);
        Self {
            secret,
            tokens: Mutex::new(HashMap::new()),
        }
    }

    fn get_or_create_secret(conn: &Connection) -> String {
        match db::get_setting(conn, SECRET_KEY) {
            Ok(Some(s)) => s,
            _ => {
                let new_secret = Self::generate_random_hex(32);
                if let Err(e) = db::set_setting(conn, SECRET_KEY, &new_secret) {
                    eprintln!("[TokenManager] failed to persist secret: {e}");
                }
                new_secret
            }
        }
    }

    fn generate_random_hex(bytes: usize) -> String {
        use std::io::Read;
        let mut rng = std::fs::File::open("/dev/urandom")
            .expect("failed to open /dev/urandom");
        let mut buf = vec![0u8; bytes];
        rng.read_exact(&mut buf)
            .expect("failed to read from /dev/urandom");
        hex::encode(buf)
    }

    fn hmac_sha256(key: &str, data: &str) -> String {
        // Simple HMAC-SHA256 using sha2 crate
        // key XOR ipad/opad
        let block_size = 64;
        let mut key_bytes = vec![0u8; block_size];
        let key_data = key.as_bytes();
        if key_data.len() > block_size {
            let mut hasher = Sha256::new();
            hasher.update(key_data);
            let hash = hasher.finalize();
            key_bytes[..32].copy_from_slice(&hash);
        } else {
            key_bytes[..key_data.len()].copy_from_slice(key_data);
        }

        let mut ipad = vec![0x36u8; block_size];
        let mut opad = vec![0x5cu8; block_size];
        for i in 0..block_size {
            ipad[i] ^= key_bytes[i];
            opad[i] ^= key_bytes[i];
        }

        let mut inner = Sha256::new();
        inner.update(&ipad);
        inner.update(data.as_bytes());
        let inner_hash = inner.finalize();

        let mut outer = Sha256::new();
        outer.update(&opad);
        outer.update(&inner_hash);
        hex::encode(outer.finalize())
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    /// Generate a session token for a module. Returns the token string.
    pub fn generate(&self, module_id: &str) -> String {
        let nonce = Self::generate_random_hex(8);
        let timestamp = Self::now_ms();
        let input = format!("{module_id}:{timestamp}:{nonce}");
        let hmac = Self::hmac_sha256(&self.secret, &input);
        let token = format!("{hmac}:{timestamp}");

        let entry = TokenEntry {
            token_hash: hmac.clone(),
            module_id: module_id.to_string(),
            created_at: timestamp,
        };

        let mut tokens = self.tokens.lock().unwrap();
        tokens.insert(hmac, entry);
        token
    }

    /// Validate a session token for a specific module.
    pub fn validate(&self, token: &str, module_id: &str) -> bool {
        let parts: Vec<&str> = token.splitn(2, ':').collect();
        if parts.len() != 2 {
            return false;
        }
        let hash = parts[0];

        let mut tokens = self.tokens.lock().unwrap();

        // Lazy eviction of expired tokens
        let now = Self::now_ms();
        tokens.retain(|_, entry| now.saturating_sub(entry.created_at) < TOKEN_TTL_MS);

        match tokens.get(hash) {
            Some(entry) => {
                entry.module_id == module_id && now.saturating_sub(entry.created_at) < TOKEN_TTL_MS
            }
            None => false,
        }
    }

    /// Rotate stale tokens. Returns count of tokens removed.
    pub fn rotate_stale(&self) -> usize {
        let mut tokens = self.tokens.lock().unwrap();
        let now = Self::now_ms();
        let before = tokens.len();
        tokens.retain(|_, entry| now.saturating_sub(entry.created_at) < ROTATION_INTERVAL_MS);
        before - tokens.len()
    }
}
