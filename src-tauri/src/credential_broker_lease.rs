//! Credential lease registry — in-memory short-TTL + revocation authority
//! (W3 split from credential_broker.rs).

use assistant_protocol::v2::credential as wire;

#[derive(Debug, Clone)]
pub struct LeaseEntry {
    pub provider_id: String,
    pub key_id: String,
    pub run_id: String,
    pub session_id: Option<String>,
    pub issued_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub revoked: bool,
}

/// In-memory registry of issued credential leases. This is the fast authority
/// for TTL expiry and revocation; durable `provider_key_leases` rows (Host
/// natives.db) back it so revocation survives host restarts. Never holds key
/// material.
#[derive(Debug, Default)]
pub struct CredentialLeaseRegistry {
    inner: std::sync::Mutex<std::collections::HashMap<String, LeaseEntry>>,
}

static LEASE_REGISTRY: std::sync::OnceLock<CredentialLeaseRegistry> = std::sync::OnceLock::new();

pub fn lease_registry() -> &'static CredentialLeaseRegistry {
    LEASE_REGISTRY.get_or_init(CredentialLeaseRegistry::default)
}

impl CredentialLeaseRegistry {
    /// Issue a new Run-bound lease and record it. Returns the public lease
    /// metadata (no secret).
    pub fn issue(
        &self,
        provider_id: &str,
        key_id: &str,
        run_id: &str,
        session_id: Option<String>,
        ttl: chrono::Duration,
    ) -> wire::CredentialLeaseMeta {
        let meta =
            wire::CredentialLeaseMeta::new(provider_id, key_id, run_id, session_id.clone(), ttl);
        if let Ok(mut guard) = self.inner.lock() {
            guard.insert(
                meta.lease_id.clone(),
                LeaseEntry {
                    provider_id: meta.provider_id.clone(),
                    key_id: meta.key_id.clone(),
                    run_id: meta.run_id.clone(),
                    session_id,
                    issued_at: chrono::Utc::now(),
                    expires_at: meta.expires_at,
                    revoked: false,
                },
            );
        }
        meta
    }

    /// Revoke a lease. A revoked lease reports inactive and the durable
    /// `provider_key_leases` row is released (Host-authoritative natives.db).
    pub fn revoke(
        &self,
        lease_id: &str,
        run_id: &str,
    ) -> std::result::Result<wire::CredentialLeaseStatus, String> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| "lease registry lock poisoned".to_string())?;
        let entry = guard
            .get_mut(lease_id)
            .ok_or_else(|| "lease not found".to_string())?;
        if entry.run_id != run_id {
            return Err("lease run mismatch; revoke rejected".into());
        }
        entry.revoked = true;
        let status = lease_status_from_entry(lease_id, entry, chrono::Utc::now());
        // Durable release (best effort) — the Host is the natives.db writer.
        let _ = crate::key_lease::release_key_lease_for_run(&entry.run_id);
        Ok(status)
    }

    /// Report public lease status (no secret). Unknown leases fail closed.
    pub fn status(
        &self,
        lease_id: &str,
    ) -> std::result::Result<wire::CredentialLeaseStatus, String> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| "lease registry lock poisoned".to_string())?;
        let entry = guard
            .get(lease_id)
            .ok_or_else(|| "lease not found or expired".to_string())?;
        Ok(lease_status_from_entry(lease_id, entry, chrono::Utc::now()))
    }
}

fn lease_status_from_entry(
    lease_id: &str,
    entry: &LeaseEntry,
    now: chrono::DateTime<chrono::Utc>,
) -> wire::CredentialLeaseStatus {
    let expired = now >= entry.expires_at;
    wire::CredentialLeaseStatus {
        lease_id: lease_id.to_string(),
        provider_id: entry.provider_id.clone(),
        key_id: entry.key_id.clone(),
        run_id: entry.run_id.clone(),
        active: !entry.revoked && !expired,
        revoked: entry.revoked,
        issued_at: entry.issued_at,
        expires_at: entry.expires_at,
    }
}
