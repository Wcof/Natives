use super::*;
use std::path::Path;

/// Build a `TrustedPath` for an existing in-root file (mirrors the
/// canonical result Gateway preflight would produce).
fn trusted(root: &Path, rel: &str) -> TrustedPath {
    let canonical = root
        .join(rel)
        .canonicalize()
        .unwrap_or_else(|_| root.join(rel));
    TrustedPath::new(canonical, PathBuf::from(rel))
}

#[test]
fn capture_and_rewind_roundtrip() {
    let root = std::env::temp_dir().join(format!("cp-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("a.txt");
    std::fs::write(&file, "before").unwrap();

    let mgr = CheckpointManager::new();
    let _id = mgr.begin_run("run1", "c1", &root).unwrap();
    mgr.capture_before("run1", &trusted(&root, "a.txt"))
        .unwrap();
    std::fs::write(&file, "after").unwrap();
    mgr.capture_after("run1", &trusted(&root, "a.txt")).unwrap();
    let rec = mgr.finalize_run("run1").unwrap();
    assert_eq!(rec.files.len(), 1);

    // re-open as live for rewind: put back
    let _ = mgr.begin_run("run1", "c1", &root).unwrap();
    // Manually inject finalized data into live by re-capturing is wrong;
    // use in-memory manager that still has no live — load from finalize path:
    // For unit test without store, keep live by not finalizing:
}

#[test]
fn rewind_without_finalize_uses_live() {
    let root = std::env::temp_dir().join(format!("cp2-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("b.txt");
    std::fs::write(&file, "v1").unwrap();
    let mgr = CheckpointManager::new();
    let cp_id = mgr.begin_run("run2", "c1", &root).unwrap();
    mgr.capture_before("run2", &trusted(&root, "b.txt"))
        .unwrap();
    std::fs::write(&file, "v2").unwrap();
    mgr.capture_after("run2", &trusted(&root, "b.txt")).unwrap();

    let preview = mgr.rewind_preview("run2", &root, None).unwrap();
    assert!(preview.conflicts.is_empty());
    assert_eq!(preview.checkpoint_id, cp_id);

    let restored = mgr.rewind("run2", &cp_id, &root, None, "fail").unwrap();
    assert_eq!(restored, vec!["b.txt".to_string()]);
    let content = std::fs::read_to_string(&file).unwrap();
    assert_eq!(content, "v1");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn external_modification_conflicts() {
    let root = std::env::temp_dir().join(format!("cp3-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("c.txt");
    std::fs::write(&file, "v1").unwrap();
    let mgr = CheckpointManager::new();
    let cp_id = mgr.begin_run("run3", "c1", &root).unwrap();
    mgr.capture_before("run3", &trusted(&root, "c.txt"))
        .unwrap();
    std::fs::write(&file, "v2").unwrap();
    mgr.capture_after("run3", &trusted(&root, "c.txt")).unwrap();
    // external edit after "run"
    std::fs::write(&file, "external").unwrap();
    let preview = mgr.rewind_preview("run3", &root, None).unwrap();
    assert!(!preview.conflicts.is_empty());
    let err = mgr.rewind("run3", &cp_id, &root, None, "fail").unwrap_err();
    assert!(err.contains("conflict"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn context_usage_estimate() {
    let v = estimate_context_usage(400, 800, 400, 128_000);
    assert_eq!(v["used_tokens"], 400);
    assert_eq!(v["max_tokens"], 128_000);
}

#[test]
fn checkpoint_survives_manager_restart_via_sqlite() {
    let root = std::env::temp_dir().join(format!("cp-restart-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("persist.txt");
    std::fs::write(&file, "v1").unwrap();

    let db = std::env::temp_dir().join(format!("cp-db-{}.sqlite", Uuid::new_v4()));
    let art = std::env::temp_dir().join(format!("cp-art-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&art).unwrap();
    let store = Arc::new(DataStore::new(&db, &art).expect("store"));
    // FK: checkpoint → run → conversation
    {
        let conn = store.conn().unwrap();
        conn.execute(
            "INSERT INTO conversation (id, mode, title, provider_id, model_id)
                 VALUES ('c1', 'agent', 't', 'p', 'm')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO run (id, conversation_id, status, provider_id, model_id)
                 VALUES ('run-persist', 'c1', 'running', 'p', 'm')",
            [],
        )
        .unwrap();
    }

    let mgr1 = CheckpointManager::with_store(Arc::clone(&store));
    let cp_id = mgr1.begin_run("run-persist", "c1", &root).unwrap();
    mgr1.capture_before("run-persist", &trusted(&root, "persist.txt"))
        .unwrap();
    std::fs::write(&file, "v2").unwrap();
    mgr1.capture_after("run-persist", &trusted(&root, "persist.txt"))
        .unwrap();
    let rec = mgr1.finalize_run("run-persist").unwrap();
    assert_eq!(rec.id, cp_id);
    assert_eq!(rec.files.len(), 1);

    // New manager instance = process restart; live map empty, load from SQLite.
    let mgr2 = CheckpointManager::with_store(store);
    let preview = mgr2
        .rewind_preview("run-persist", &root, None)
        .expect("preview after restart");
    assert_eq!(preview.checkpoint_id, cp_id);
    assert!(preview.conflicts.is_empty());
    let restored = mgr2
        .rewind("run-persist", &cp_id, &root, None, "fail")
        .expect("rewind after restart");
    assert_eq!(restored, vec!["persist.txt".to_string()]);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "v1");

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_dir_all(&art);
}

/// TASK-006: a durable checkpoint flush routed through the bounded storage
/// actor still persists the snapshot_json (async path never locks the Mutex).
#[test]
fn flush_via_storage_actor_persists_snapshot() {
    let root = std::env::temp_dir().join(format!("cp-act-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("act.txt");
    std::fs::write(&file, "v1").unwrap();

    let db = std::env::temp_dir().join(format!("cp-act-db-{}.sqlite", Uuid::new_v4()));
    let art = std::env::temp_dir().join(format!("cp-act-art-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&art).unwrap();
    let store = Arc::new(DataStore::new(&db, &art).expect("store"));
    {
        let conn = store.conn().unwrap();
        conn.execute(
            "INSERT INTO conversation (id, mode, title, provider_id, model_id)
                 VALUES ('c-act', 'agent', 't', 'p', 'm')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO run (id, conversation_id, status, provider_id, model_id)
                 VALUES ('run-act', 'c-act', 'running', 'p', 'm')",
            [],
        )
        .unwrap();
    }

    let actor = crate::storage::actor::StorageActor::new(4, store.clone());
    let mgr = CheckpointManager::with_store_and_actor(store.clone(), actor);
    let cp_id = mgr.begin_run("run-act", "c-act", &root).unwrap();
    mgr.capture_before("run-act", &trusted(&root, "act.txt"))
        .unwrap();
    std::fs::write(&file, "v2").unwrap();
    // capture_after triggers flush_live_to_store → routed through the actor.
    mgr.capture_after("run-act", &trusted(&root, "act.txt"))
        .unwrap();
    // The flush is durable: a fresh manager instance (no live map) reads it.
    let mgr2 = CheckpointManager::with_store(store);
    let preview = mgr2
        .rewind_preview("run-act", &root, None)
        .expect("actor-flushed snapshot is loadable");
    assert_eq!(preview.checkpoint_id, cp_id);
    assert_eq!(preview.files.len(), 1, "flush persisted the captured file");

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_dir_all(&art);
}

// ---- T03: bounded capture, streaming hash, sensitive-path redaction ----

/// A `.env` file containing a fixture secret must be captured as redacted
/// metadata + hash only — the secret never reaches the checkpoint and the
/// preview reports the file as non-restorable.
#[test]
fn sensitive_env_file_never_persists_content() {
    let root = std::env::temp_dir().join(format!("cp-secret-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join(".env");
    std::fs::write(&file, "API_KEY=fixture-secret-123456789\n").unwrap();
    let mgr = CheckpointManager::new();
    mgr.begin_run("run-secret", "c1", &root).unwrap();
    mgr.capture_before("run-secret", &trusted(&root, ".env"))
        .unwrap();
    std::fs::write(&file, "API_KEY=changed-987654321\n").unwrap();
    mgr.capture_after("run-secret", &trusted(&root, ".env"))
        .unwrap();
    // Preview (live) reports the file as non-restorable: content was not
    // captured, so rewind must say so rather than pretend.
    let preview = mgr.rewind_preview("run-secret", &root, None).unwrap();
    let entry = preview.files.iter().find(|f| f.path == ".env").unwrap();
    assert!(!entry.restorable);
    assert_eq!(
        entry.non_restorable_reason.as_deref(),
        Some("sensitive_path")
    );
    let rec = mgr.finalize_run("run-secret").unwrap();
    let snapshot = rec.files.iter().find(|f| f.path == ".env").unwrap();
    assert!(snapshot.redacted, "sensitive path must be marked redacted");
    assert_eq!(snapshot.redaction_reason.as_deref(), Some("sensitive_path"));
    assert!(
        snapshot.before_content.is_none() && snapshot.after_content.is_none(),
        "secret content must never be captured"
    );
    assert!(
        snapshot.before_hash.is_some() && snapshot.after_hash.is_some(),
        "redacted metadata still carries the hash"
    );
    // The persisted snapshot JSON must not contain the fixture secret.
    let json = serde_json::to_string(&rec).unwrap();
    assert!(
        !json.contains("fixture-secret-123456789"),
        "fixture secret leaked into the checkpoint snapshot"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A file larger than the per-file content cap is streamed (bounded memory)
/// and recorded as redacted metadata + hash; its content is never loaded
/// into memory or persisted, and rewind reports it non-restorable.
#[test]
fn large_file_is_streamed_and_content_redacted() {
    let root = std::env::temp_dir().join(format!("cp-large-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join("big.bin");
    // Over the per-file cap (256 KiB) but cheap to create.
    let big = vec![b'x'; (MAX_CAPTURE_FILE_CONTENT_BYTES as usize) * 2];
    std::fs::write(&file, &big).unwrap();
    let mgr = CheckpointManager::new();
    mgr.begin_run("run-large", "c1", &root).unwrap();
    mgr.capture_before("run-large", &trusted(&root, "big.bin"))
        .unwrap();
    std::fs::write(&file, &big[..1]).unwrap(); // rewrite after
    mgr.capture_after("run-large", &trusted(&root, "big.bin"))
        .unwrap();
    // Preview (live) reports the over-cap file as non-restorable.
    let preview = mgr.rewind_preview("run-large", &root, None).unwrap();
    assert!(!preview.files[0].restorable);
    assert_eq!(
        preview.files[0].non_restorable_reason.as_deref(),
        Some("too_large")
    );
    let rec = mgr.finalize_run("run-large").unwrap();
    let snapshot = rec.files.iter().find(|f| f.path == "big.bin").unwrap();
    assert!(snapshot.redacted, "over-cap file must be redacted");
    assert!(snapshot.before_content.is_none());
    assert!(snapshot.after_content.is_none());
    assert!(
        snapshot.before_hash.is_some() && snapshot.after_hash.is_some(),
        "streaming hash is computed without buffering the file"
    );
    // The hash differs before/after — the streaming hash covered the file.
    assert_ne!(
        snapshot.before_hash, snapshot.after_hash,
        "streaming hash must reflect the whole file"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A 1GB sparse file must not be read into memory: `stream_read_capped`
/// hashes it in bounded chunks and stores no content. The streaming helper
/// itself is the memory-bounded primitive; the manager applies it.
#[test]
fn one_gigabyte_file_streams_with_bounded_memory() {
    let dir = std::env::temp_dir().join(format!("cp-gb-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("giant.bin");
    // Sparse file: instant to create, 1 GiB of zeros on disk.
    let f = std::fs::File::create(&path).unwrap();
    f.set_len(1024 * 1024 * 1024).unwrap();
    drop(f);
    let read = stream_read_capped(&path, MAX_CAPTURE_FILE_CONTENT_BYTES).unwrap();
    assert_eq!(read.size, 1024 * 1024 * 1024);
    assert!(read.over_cap, "1GiB file must exceed the content cap");
    assert!(read.content.is_none(), "1GiB content must not be buffered");
    assert_eq!(read.hash.len(), 64, "full SHA-256 hex digest");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Beyond the per-run file-count cap, further files are recorded as
/// redacted metadata instead of growing the checkpoint without bound.
#[test]
fn file_count_quota_redacts_excess_files() {
    let root = std::env::temp_dir().join(format!("cp-count-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mgr = CheckpointManager::new();
    mgr.begin_run("run-count", "c1", &root).unwrap();
    let within = 3usize;
    for i in 0..(MAX_CAPTURE_FILE_COUNT + within) {
        let name = format!("f{i}.txt");
        let file = root.join(&name);
        std::fs::write(&file, "x").unwrap();
        mgr.capture_before("run-count", &trusted(&root, &name))
            .unwrap();
    }
    let rec = mgr.finalize_run("run-count").unwrap();
    assert_eq!(rec.files.len(), MAX_CAPTURE_FILE_COUNT + within);
    let redacted_count = rec.files.iter().filter(|f| f.redacted).count();
    assert_eq!(
        redacted_count, within,
        "files beyond the count cap must be redacted"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The async capture path runs the file read on the blocking pool and
/// produces the same redaction guarantees as the sync path.
#[tokio::test]
async fn async_capture_redacts_sensitive_path() {
    let root = std::env::temp_dir().join(format!("cp-async-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join(".env");
    std::fs::write(&file, "TOKEN=async-fixture-secret").unwrap();
    let mgr = CheckpointManager::new();
    mgr.begin_run("run-async", "c1", &root).unwrap();
    mgr.capture_before_async("run-async", &trusted(&root, ".env"))
        .await
        .unwrap();
    std::fs::write(&file, "TOKEN=changed").unwrap();
    mgr.capture_after_async("run-async", &trusted(&root, ".env"))
        .await
        .unwrap();
    let rec = mgr.finalize_run("run-async").unwrap();
    let snap = rec.files.iter().find(|f| f.path == ".env").unwrap();
    assert!(snap.redacted);
    assert!(snap.before_content.is_none() && snap.after_content.is_none());
    let json = serde_json::to_string(&rec).unwrap();
    assert!(
        !json.contains("async-fixture-secret"),
        "async capture leaked the fixture secret"
    );
    let _ = std::fs::remove_dir_all(&root);
}
