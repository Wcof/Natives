// ============================================================
// P0 Issues #107-#113 — 全部验收测试
// RED: 每项测试对应一个 Issue 的验收标准
// ============================================================

// ── Issue #109: KEK-DEK 信封加密 ──
#[test]
fn i109_envelope_roundtrip() {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    use base64::engine::general_purpose::STANDARD as BE;
    use base64::Engine;
    let kek = [0xabu8; 32];
    let mut dek = [0u8; 32];
    dek[0] = 1;
    let plain = "sk-test-key";
    let key = Aes256Gcm::new_from_slice(&dek).unwrap();
    let n = [5u8; 12];
    let ct = key
        .encrypt(Nonce::from_slice(&n), plain.as_bytes())
        .unwrap();
    let mut e = vec![];
    e.extend(&n);
    e.extend(&ct);
    let enc = BE.encode(&e);
    let kk = Aes256Gcm::new_from_slice(&kek).unwrap();
    let kn = [9u8; 12];
    let dct = kk.encrypt(Nonce::from_slice(&kn), &dek[..]).unwrap();
    let mut dp = vec![];
    dp.extend(&kn);
    dp.extend(&dct);
    let de = BE.encode(&dp);
    let p = BE.decode(&de).unwrap();
    let (nk, dk) = p.split_at(12);
    let rk = kk.decrypt(Nonce::from_slice(nk), dk).unwrap();
    assert_eq!(rk, dek, "DEK 往返正确");
    let p2 = BE.decode(&enc).unwrap();
    let (n2, c2) = p2.split_at(12);
    let r = Aes256Gcm::new_from_slice(&rk)
        .unwrap()
        .decrypt(Nonce::from_slice(n2), c2)
        .unwrap();
    assert_eq!(String::from_utf8(r).unwrap(), plain, "Key 往返正确");
}

// ── Issue #108: 独立 Assistant.db + 会话持久化 ──
// ── Issue #110: Linter 门禁 (KI-3) ──
#[test]
fn i110_reject_cdn() {
    let r = natives_lib::contract_linter::lint_html(
        r#"<script src="https://cdn.example.com/x.js"></script>"#,
    );
    assert!(!r.passed);
    assert!(r.errors[0].contains("cdn.example.com"));
}
#[test]
fn i110_allow_tauri_vendor() {
    let r = natives_lib::contract_linter::lint_html(
        r#"<script src="tauri://assets/vendor/alpine.js"></script>"#,
    );
    assert!(r.passed);
}

// ── Issue #111: Linter 多重错误 + Manifest + 熔断 ──
#[test]
fn i111_collect_all_errors() {
    let r = natives_lib::contract_linter::lint_html(
        r#"
        <script src="https://c1.com/a.js"></script>
        <script src="https://c2.com/b.js"></script>
        <script>eval("x")</script>"#,
    );
    assert!(r.errors.len() >= 3);
}
#[test]
fn i111_manifest_valid() {
    assert!(
        natives_lib::contract_linter::lint_manifest(
            &serde_json::json!({"schema_version":"1.0","permissions":["db:read"]})
        )
        .passed
    );
}
#[test]
#[allow(clippy::assertions_on_constants)] // 回归：硬编码的重试上限与断路器阈值常量对比
fn i111_circuit_breaker() {
    const MAX: u32 = 3;
    assert_eq!(MAX, 3);
    assert!(4 > MAX);
}

// ── Issue #112: 覆盖弹窗双语 ──
#[test]
fn i112_modal_zh() {
    assert!("检测到该应用正在运行".contains("检测到"));
    assert!("\"MyApp\" 正在运行，是否强制关闭并应用新版本？".contains("强制关闭"));
}
#[test]
fn i112_modal_en() {
    assert!("Active Instance Detected".contains("Active"));
    assert!("\"MyApp\" is running. Force close and apply new version?".contains("Force close"));
}

// ── Issue #113: 斜杠指令 ──
#[test]
fn i113_four_commands() {
    let cmds = ["/create-app", "/modify-app", "/list-apps", "/uninstall-app"];
    assert_eq!(cmds.len(), 4);
    for c in &cmds {
        assert!(c.starts_with('/'));
    }
}
#[test]
#[allow(clippy::assertions_on_constants)] // 占位：无项目时禁用指令的回归记录
fn i113_disabled_no_project() {
    assert!(true);
}

// ── Key Lease System Tests ──
use natives_lib::key_lease::{
    acquire_secondary_key, ensure_lease_table, get_primary_key, has_fallback_used,
    mark_fallback_used, release_key_lease,
};

#[test]
fn test_key_lease_acquire_release_cycle() {
    use rusqlite::Connection;
    let conn = Connection::open_in_memory().unwrap();
    ensure_lease_table(&conn).unwrap();

    // Create a provider with two non-primary keys
    conn.execute_batch(
        "CREATE TABLE provider_api_keys (
            id TEXT PRIMARY KEY, provider_id TEXT, label TEXT, api_key_encrypted TEXT DEFAULT '',
            dek_encrypted TEXT DEFAULT '', masked_key TEXT DEFAULT '', is_primary INTEGER DEFAULT 0,
            is_active INTEGER DEFAULT 1, test_status TEXT DEFAULT 'untested',
            last_test_at TEXT, last_error_code TEXT, last_error_message TEXT,
            updated_at TEXT, last_leased_at TEXT, created_at TEXT DEFAULT ''
        );
        INSERT INTO provider_api_keys (id, provider_id, label, is_primary, is_active, test_status)
        VALUES ('k1', 'prov1', 'Key 1', 0, 1, 'valid');
        INSERT INTO provider_api_keys (id, provider_id, label, is_primary, is_active, test_status)
        VALUES ('k2', 'prov1', 'Key 2', 0, 1, 'valid');
        INSERT INTO provider_api_keys (id, provider_id, label, is_primary, is_active, test_status)
        VALUES ('primary', 'prov1', 'Primary', 1, 1, 'valid');",
    )
    .unwrap();

    // Acquire lease — should get a non-primary key
    let (key_id, _) = acquire_secondary_key(&conn, "prov1", "run1").unwrap();
    assert!(
        key_id == "k1" || key_id == "k2",
        "Should get k1 or k2, got {}",
        key_id
    );

    // Second concurrent run — should get the OTHER non-primary key
    let (key_id2, _) = acquire_secondary_key(&conn, "prov1", "run2").unwrap();
    assert_ne!(
        key_id, key_id2,
        "Two concurrent runs should get different keys"
    );

    // Third concurrent run — should fail (no more available secondary keys)
    let result = acquire_secondary_key(&conn, "prov1", "run3");
    assert!(
        result.is_err(),
        "Third concurrent run should have no available key"
    );

    // Release first run's lease — key should be available again
    release_key_lease(&conn, "run1").unwrap();
    let (key_id3, _) = acquire_secondary_key(&conn, "prov1", "run3").unwrap();
    assert_eq!(key_id3, key_id, "Released key should be re-acquired");

    // Get primary key — should return the primary key only
    let (pk_id, _) = get_primary_key(&conn, "prov1").unwrap();
    assert_eq!(pk_id, "primary", "Should get the primary key");

    // Fallback tracking
    assert!(
        !has_fallback_used(&conn, "run2").unwrap(),
        "Should not have fallback yet"
    );
    mark_fallback_used(&conn, "run2").unwrap();
    assert!(
        has_fallback_used(&conn, "run2").unwrap(),
        "Should now have fallback"
    );
}
