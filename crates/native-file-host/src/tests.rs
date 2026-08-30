use super::*;
use base64::Engine;
use notify::{event::EventKind, Event, RecursiveMode};
use search::{fuzzy_name_score, run_locate};
use watch::{is_noisy_watch_path, watch_event_kind, WATCH_MODE};

#[test]
fn rejects_unknown_methods() {
    let request = Request {
        id: "test".into(),
        method: "spawn_shell".into(),
        params: Value::Null,
    };
    assert_eq!(handle(&request).unwrap_err(), "unsupported method");
}

#[test]
fn rejects_path_traversal_folder_names() {
    let request = Request {
        id: "test".into(),
        method: "create_folder".into(),
        params: json!({"parent": "/tmp", "name": "../escape"}),
    };
    assert_eq!(handle(&request).unwrap_err(), "invalid folder name");
}

#[test]
fn rejects_invalid_request_envelope() {
    let request = Request {
        id: "test".into(),
        method: "roots".into(),
        params: Value::Null,
    };
    assert_eq!(
        validate_request(&request).unwrap_err(),
        "params must be an object"
    );
}

#[test]
fn rejects_unknown_request_method_before_dispatch() {
    let request = Request {
        id: "test".into(),
        method: "spawn_shell".into(),
        params: json!({}),
    };
    assert_eq!(
        validate_request(&request).unwrap_err(),
        "unsupported method"
    );
}

#[test]
fn version_reports_stable_protocol_and_host_version() {
    let request = Request {
        id: "version".into(),
        method: "version".into(),
        params: json!({}),
    };
    assert_eq!(validate_request(&request), Ok(()));
    let result = handle(&request).expect("version response");
    assert_eq!(result["protocolVersion"], 1);
    assert_eq!(result["hostVersion"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn version_rejects_parameters() {
    let request = Request {
        id: "version".into(),
        method: "version".into(),
        params: json!({"extra": true}),
    };
    assert_eq!(validate_request(&request).unwrap_err(), "unknown parameter");
}

#[test]
fn workspace_widget_add_uses_envelope_workspace_ownership() {
    let path =
        std::env::temp_dir().join(format!("natives-widget-envelope-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = WorkspaceStore::open(&path).expect("open workspace store");
    let session = store.session().expect("seeded session");
    let workspace = &session.workspaces[0];
    let request = Request {
        id: "widget-add".into(),
        method: "workspace_widget_upsert".into(),
        params: json!({
            "workspaceId": workspace.id,
            "widget": {
                "id": "",
                "key": "widget/weather",
                "order": 0,
                "enabled": true,
                "configJson": {},
                "displayJson": { "position": "middleCentre" }
            },
            "expectedRevision": workspace.revision
        }),
    };
    let result =
        workspace_dispatch(&store, &request).expect("add widget without nested workspaceId");
    assert_eq!(result["widgets"][0]["key"], "widget/weather");
    let _ = std::fs::remove_file(path);
}

#[test]
fn search_honors_cancellation_token() {
    let request = Request {
        id: "search".into(),
        method: "search".into(),
        params: json!({"path": "/tmp", "query": "natives"}),
    };
    let token = AtomicBool::new(true);
    assert_eq!(
        run_search(&request, Some(&token)).unwrap_err(),
        "search cancelled"
    );
}

#[test]
fn fuzzy_name_score_accepts_ordered_fragments_only() {
    assert!(fuzzy_name_score("my-project.md", "mprj").is_some());
    assert!(fuzzy_name_score("my-project.md", "pjrm").is_none());
}

#[test]
fn content_search_returns_first_matching_line() {
    let dir = std::env::temp_dir().join(format!("natives-content-search-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("notes.txt");
    std::fs::write(&file, "before\nneedle appears here\nafter\n").unwrap();
    let request = Request {
        id: "search".into(),
        method: "search".into(),
        params: json!({"path": dir, "query": "needle", "content": true, "recursive": false}),
    };
    assert_eq!(validate_request(&request), Ok(()));
    let result = run_search(&request, None).unwrap();
    assert_eq!(result["entries"][0]["match"], "needle appears here");
    assert_eq!(
        result["entries"][0]["matchLines"][0],
        "2: needle appears here"
    );
    assert_eq!(result["entries"][0]["matchCount"], 1);
    assert_eq!(
        result["entries"][0]["matchLines"].as_array().map(Vec::len),
        Some(1)
    );
    assert!(
        result["entries"][0]["searchScore"]
            .as_i64()
            .unwrap_or_default()
            >= 5_010
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn search_respects_show_hidden_flag() {
    let dir = std::env::temp_dir().join(format!("natives-hidden-search-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let hidden = dir.join(".fanbox-hidden.txt");
    std::fs::write(&hidden, "hidden").unwrap();
    let base = |show_hidden| Request {
        id: "search".into(),
        method: "search".into(),
        params: json!({"path": dir, "query": "fanbox", "recursive": false, "showHidden": show_hidden}),
    };
    assert_eq!(validate_request(&base(false)), Ok(()));
    assert!(run_search(&base(false), None).unwrap()["entries"]
        .as_array()
        .unwrap()
        .is_empty());
    let result = run_search(&base(true), None).unwrap();
    assert_eq!(result["entries"].as_array().unwrap().len(), 1);
    assert_eq!(result["entries"][0]["name"], ".fanbox-hidden.txt");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn content_search_skips_binary_and_oversized_files() {
    let dir = std::env::temp_dir().join(format!("natives-content-boundary-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("binary.bin"), [0, 159, 146, 150]).unwrap();
    std::fs::write(dir.join("needle-name.txt"), "unrelated").unwrap();
    std::fs::write(
        dir.join("large.txt"),
        format!("{}needle", "x".repeat(2 * 1024 * 1024)),
    )
    .unwrap();
    let request = Request {
        id: "search".into(),
        method: "search".into(),
        params: json!({"path": dir, "query": "needle", "content": true, "recursive": false}),
    };
    assert_eq!(validate_request(&request), Ok(()));
    let result = run_search(&request, None).unwrap();
    assert_eq!(result["entries"].as_array().map(Vec::len), Some(0));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn content_search_reports_unavailable_pdf_extractor() {
    let dir = std::env::temp_dir().join(format!("natives-pdf-search-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = dir.join("notes.pdf");
    let mut bytes = b"%PDF-1.7\n".to_vec();
    bytes.resize(3 * 1024 * 1024, b' ');
    std::fs::write(&pdf, bytes).unwrap();
    let request = Request {
        id: "pdf-search".into(),
        method: "search".into(),
        params: json!({"path": dir, "query": "needle", "content": true, "recursive": false}),
    };
    let result = run_search(&request, None).unwrap();
    assert_eq!(result["contentUnavailable"], true);
    assert!(result["entries"].as_array().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn sanitizes_internal_errors() {
    assert_eq!(
        safe_error("IO error: /Users/secret/token".into()),
        "文件操作失败"
    );
    assert_eq!(
        safe_error("invalid folder name".into()),
        "invalid folder name"
    );
}

#[test]
fn locate_rejects_unsafe_queries() {
    let request = Request {
        id: "locate".into(),
        method: "locate".into(),
        params: json!({"query": "../secret"}),
    };
    assert_eq!(validate_request(&request), Ok(()));
    assert_eq!(run_locate(&request).unwrap(), json!({"entries": []}));
    let option_like = Request {
        params: json!({"query": "-name"}),
        ..request
    };
    assert_eq!(run_locate(&option_like).unwrap(), json!({"entries": []}));
}

#[test]
fn preview_honors_cancellation_token() {
    let request = Request {
        id: "preview".into(),
        method: "read_file".into(),
        params: json!({"path": "/tmp/not-used"}),
    };
    let token = AtomicBool::new(true);
    assert_eq!(
        run_preview(&request, Some(&token)).unwrap_err(),
        "preview cancelled"
    );
}

#[test]
fn batch_honors_cancellation_token_and_schema() {
    let destination = std::env::temp_dir().join(format!(
        "natives-host-cancel-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let request = Request {
        id: "batch".into(),
        method: "copy_batch".into(),
        params: json!({"paths": ["/tmp/not-used"], "dest": destination.to_string_lossy()}),
    };
    assert_eq!(validate_request(&request), Ok(()));
    let token = AtomicBool::new(true);
    let result = run_batch(&request, &token, None).expect("cancelled batch response");
    assert_eq!(result["cancelled"], true);
    assert_eq!(result["count"], 0);
    assert_eq!(result["copied"].as_array().map(Vec::len), Some(0));
    assert!(!destination.exists());
    let cancel = Request {
        id: "cancel".into(),
        method: "batch_cancel".into(),
        params: json!({"requestId": "batch"}),
    };
    assert_eq!(validate_request(&cancel), Ok(()));
}

#[test]
fn batch_copy_preserves_destination_directory_semantics() {
    let base = std::env::temp_dir().join(format!(
        "natives-host-batch-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let source = base.join("source.txt");
    let destination = base.join("new-destination");
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(&source, b"batch").unwrap();
    let request = Request {
        id: "batch-copy".into(),
        method: "copy_batch".into(),
        params: json!({"paths": [source.to_string_lossy()], "dest": destination.to_string_lossy()}),
    };
    let token = AtomicBool::new(false);
    let result = run_batch(&request, &token, None).expect("batch copy");
    assert_eq!(result["count"], 1);
    assert!(destination.join("source.txt").is_file());
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn duplicate_batch_reports_progress_shape_and_result() {
    let base = std::env::temp_dir().join(format!(
        "natives-host-duplicate-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&base).unwrap();
    let source = base.join("source.txt");
    std::fs::write(&source, b"duplicate").unwrap();
    let request = Request {
        id: "duplicate-batch".into(),
        method: "duplicate_batch".into(),
        params: json!({"paths": [source.to_string_lossy()]}),
    };
    assert_eq!(validate_request(&request), Ok(()));
    let token = AtomicBool::new(false);
    let result = run_batch(&request, &token, None).expect("duplicate batch");
    assert_eq!(result["count"], 1);
    assert_eq!(result["duplicated"].as_array().map(Vec::len), Some(1));
    let duplicate = result["duplicated"][0].as_str().expect("duplicate path");
    assert!(std::path::Path::new(duplicate).is_file());
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn shutdown_cancels_and_joins_background_jobs() {
    let token = Arc::new(AtomicBool::new(false));
    let worker_token = Arc::clone(&token);
    let handle = thread::spawn(move || {
        while !worker_token.load(Ordering::Relaxed) {
            thread::yield_now();
        }
    });
    let mut jobs = vec![handle];
    let active = Arc::new(Mutex::new(HashMap::from([(String::from("job"), token)])));

    shutdown_jobs(&active, &mut jobs);

    assert!(jobs.is_empty());
    assert!(active.lock().unwrap().is_empty());
}

#[test]
fn reaps_finished_background_jobs() {
    let handle = thread::spawn(|| {});
    while !handle.is_finished() {
        thread::yield_now();
    }
    let mut jobs = vec![handle];

    reap_finished_jobs(&mut jobs);

    assert!(jobs.is_empty());
}

#[test]
fn watches_only_the_current_directory() {
    assert_eq!(WATCH_MODE, RecursiveMode::NonRecursive);
}

#[test]
fn rejects_unknown_envelope_fields() {
    let parsed =
        serde_json::from_str::<Request>(r#"{"id":"x","method":"roots","params":{},"shell":"no"}"#);
    assert!(parsed.is_err());
}

#[test]
fn rejects_unknown_method_parameters_and_bad_limits() {
    let unknown = Request {
        id: "x".into(),
        method: "roots".into(),
        params: json!({"path": "/tmp"}),
    };
    assert_eq!(validate_request(&unknown).unwrap_err(), "unknown parameter");
    let bad_limit = Request {
        id: "x".into(),
        method: "list_dir".into(),
        params: json!({"path": "/tmp", "limit": 0}),
    };
    assert_eq!(validate_request(&bad_limit).unwrap_err(), "invalid limit");
    let bad_offset = Request {
        id: "x".into(),
        method: "list_dir".into(),
        params: json!({"path": "/tmp", "offset": 100_001, "limit": 1}),
    };
    assert_eq!(validate_request(&bad_offset).unwrap_err(), "invalid offset");
}

#[test]
fn accepts_file_write_conflict_and_listing_options() {
    let list = Request {
        id: "list".into(),
        method: "list_dir".into(),
        params: json!({
            "path": "/tmp",
            "offset": 0,
            "limit": 25,
            "sortBy": "mtime",
            "sortDir": "desc",
            "showHidden": true,
            "probeProjects": false
        }),
    };
    assert_eq!(validate_request(&list), Ok(()));

    let write = Request {
        id: "write".into(),
        method: "write_file".into(),
        params: json!({
            "parent": "/tmp",
            "name": "note.txt",
            "data": "",
            "expectedMtime": 12.5
        }),
    };
    assert_eq!(validate_request(&write), Ok(()));

    let image = Request {
        id: "image".into(),
        method: "image_preview".into(),
        params: json!({"path": "/tmp/photo.png"}),
    };
    assert_eq!(validate_request(&image), Ok(()));

    let copy_image = Request {
        id: "copy-image".into(),
        method: "copy_image".into(),
        params: json!({"path": "/tmp/photo.png"}),
    };
    assert_eq!(validate_request(&copy_image), Ok(()));

    let editor = Request {
        id: "editor".into(),
        method: "editor".into(),
        params: json!({"path": "/tmp/note.txt"}),
    };
    assert_eq!(validate_request(&editor), Ok(()));

    let pdf = Request {
        id: "pdf".into(),
        method: "pdf_preview".into(),
        params: json!({"path": "/tmp/sample.pdf"}),
    };
    assert_eq!(validate_request(&pdf), Ok(()));

    let media = Request {
        id: "media".into(),
        method: "media_preview".into(),
        params: json!({"path": "/tmp/sample.mp3"}),
    };
    assert_eq!(validate_request(&media), Ok(()));
}

#[test]
fn editor_dispatch_rejects_missing_path_before_spawning() {
    let request = Request {
        id: "editor-missing".into(),
        method: "editor".into(),
        params: json!({
            "path": format!("/tmp/natives-editor-missing-{}", std::process::id())
        }),
    };
    assert!(handle(&request).is_err());
}

#[test]
fn image_preview_dispatch_returns_safe_data_url_parts() {
    let path = std::env::temp_dir().join(format!(
        "natives-host-image-{}-{}.png",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, b"\x89PNG\r\n\x1a\n").unwrap();
    let request = Request {
        id: "image".into(),
        method: "image_preview".into(),
        params: json!({"path": path.to_string_lossy()}),
    };
    let result = handle(&request).expect("image preview dispatch");
    assert_eq!(result["mimeType"], "image/png");
    assert!(result["data"].as_str().is_some_and(|data| !data.is_empty()));
    let _ = std::fs::remove_file(path);
}

#[test]
fn pdf_preview_dispatch_returns_bounded_data_url_parts() {
    let path = std::env::temp_dir().join(format!(
        "natives-host-pdf-{}-{}.pdf",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, b"%PDF-1.7\n").unwrap();
    let request = Request {
        id: "pdf".into(),
        method: "pdf_preview".into(),
        params: json!({"path": path.to_string_lossy()}),
    };
    let result = handle(&request).expect("pdf preview dispatch");
    assert_eq!(result["mimeType"], "application/pdf");
    assert!(result["data"].as_str().is_some_and(|data| !data.is_empty()));
    let _ = std::fs::remove_file(path);
}

#[test]
fn media_preview_dispatch_returns_audio_data_url_parts() {
    let path = std::env::temp_dir().join(format!(
        "natives-host-media-{}-{}.mp3",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, b"ID3").unwrap();
    let request = Request {
        id: "media".into(),
        method: "media_preview".into(),
        params: json!({"path": path.to_string_lossy()}),
    };
    let result = handle(&request).expect("media preview dispatch");
    assert_eq!(result["mimeType"], "audio/mpeg");
    assert!(result["data"].as_str().is_some_and(|data| !data.is_empty()));
    let _ = std::fs::remove_file(path);
}

#[test]
fn write_file_dispatch_reports_mtime_conflict_without_overwrite() {
    let dir = std::env::temp_dir().join(format!(
        "natives-host-write-conflict-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("note.txt");
    std::fs::write(&path, b"before").unwrap();
    let expected_mtime = crate::file_manager::stat_path(path.to_str().unwrap())
        .unwrap()
        .mtime
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(10));
    std::fs::write(&path, b"external").unwrap();
    let request = Request {
        id: "write".into(),
        method: "write_file".into(),
        params: json!({
            "parent": dir.to_string_lossy(),
            "name": "note.txt",
            "data": base64::engine::general_purpose::STANDARD.encode(b"replacement"),
            "expectedMtime": expected_mtime
        }),
    };
    let result = handle(&request).expect("write dispatch");
    assert_eq!(result["conflict"], true);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "external");
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn write_file_dispatch_rejects_symlink_escape() {
    use std::os::unix::fs::symlink;
    let dir = std::env::temp_dir().join(format!(
        "natives-host-write-symlink-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let target = std::path::PathBuf::from("/etc/hosts");
    symlink(&target, dir.join("note.txt")).unwrap();
    let request = Request {
        id: "write".into(),
        method: "write_file".into(),
        params: json!({
            "parent": dir.to_string_lossy(),
            "name": "note.txt",
            "data": base64::engine::general_purpose::STANDARD.encode(b"overwrite")
        }),
    };
    assert!(handle(&request).is_err());
    assert!(std::fs::read_to_string(&target).is_ok());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn create_folder_dispatch_uses_core_folder_type() {
    let parent = std::env::temp_dir().join(format!(
        "natives-host-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&parent).unwrap();
    let request = Request {
        id: "folder".into(),
        method: "create_folder".into(),
        params: json!({"parent": parent.to_string_lossy(), "name": "created"}),
    };

    let result = handle(&request).expect("create folder");

    assert_eq!(result["ok"], true);
    assert!(parent.join("created").is_dir());
    let _ = std::fs::remove_dir_all(parent);
}

#[test]
fn rejects_invalid_batch_shape_and_sort_values() {
    let batch = Request {
        id: "batch".into(),
        method: "copy_batch".into(),
        params: json!({"paths": ["/tmp/a", 42], "dest": "/tmp"}),
    };
    assert_eq!(validate_request(&batch).unwrap_err(), "invalid path entry");
    let sort = Request {
        id: "sort".into(),
        method: "list_dir".into(),
        params: json!({"path": "/tmp", "sortBy": "random"}),
    };
    assert_eq!(validate_request(&sort).unwrap_err(), "invalid sortBy");
}

#[test]
fn rejects_invalid_create_zip_paths() {
    let request = Request {
        id: "zip".into(),
        method: "create_zip".into(),
        params: json!({"paths": ["/tmp/a", 42], "dest": "/tmp", "name": "bundle.zip"}),
    };
    assert_eq!(
        validate_request(&request).unwrap_err(),
        "invalid archive input path"
    );
}

#[test]
fn normalizes_notify_events_and_filters_editor_noise() {
    let created = Event {
        kind: EventKind::Create(notify::event::CreateKind::File),
        paths: vec![std::path::PathBuf::from("/tmp/note.txt")],
        attrs: Default::default(),
    };
    assert_eq!(watch_event_kind(&created), Some("created"));
    assert!(!is_noisy_watch_path(std::path::Path::new("/tmp/note.txt")));
    assert!(is_noisy_watch_path(std::path::Path::new("/tmp/.note.swp")));
}
