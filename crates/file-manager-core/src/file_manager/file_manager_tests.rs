use super::*;
use std::io::Write;

fn tmp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "natives-fm-{}-{}-{}",
        label,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn natural_cmp_numeric_order() {
    assert_eq!(natural_cmp("file2", "file10"), std::cmp::Ordering::Less);
    assert_eq!(natural_cmp("file10", "file2"), std::cmp::Ordering::Greater);
    assert_eq!(natural_cmp("a", "b"), std::cmp::Ordering::Less);
}

#[test]
fn valid_name_rejects_path_sep_and_dots() {
    assert!(valid_name("readme.md"));
    assert!(!valid_name(""));
    assert!(!valid_name("."));
    assert!(!valid_name(".."));
    assert!(!valid_name("a/b"));
    assert!(!valid_name("a\\b"));
    assert!(!valid_name("a\0b"));
}

#[cfg(unix)]
#[test]
fn authorization_rejects_missing_target_below_external_symlink() {
    use std::os::unix::fs::symlink;

    let base = tmp_dir("symlink-boundary");
    let link = base.join("outside");
    symlink("/etc", &link).expect("create symlink");
    let candidate = link.join("natives-new-file");

    let result = FileAccessPolicy::authorize_path_buf(&candidate, OperationPolicy::Write);
    assert!(
        result.is_err(),
        "a missing file below a symlink must stay denied"
    );
    let _ = std::fs::remove_file(&link);
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn authorization_rejects_dangling_symlink() {
    use std::os::unix::fs::symlink;

    let base = tmp_dir("dangling-symlink");
    let link = base.join("dangling");
    symlink("/path/that/does/not/exist", &link).expect("create dangling symlink");

    let result = FileAccessPolicy::authorize_path_buf(&link, OperationPolicy::Write);
    assert!(result.is_err(), "dangling symlinks must not be authorized");
    let _ = std::fs::remove_file(&link);
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn authorization_accepts_home_and_home_ancestors_read() {
    let home = dirs::home_dir().expect("home dir");
    let auth_home = FileAccessPolicy::authorize_path_buf(&home, OperationPolicy::Read);
    assert!(
        auth_home.is_ok(),
        "home directory must be authorized for read"
    );

    if let Some(parent) = home.parent() {
        if parent.exists() {
            let auth_parent = FileAccessPolicy::authorize_path_buf(parent, OperationPolicy::Read);
            assert!(
                auth_parent.is_ok(),
                "home ancestor directory must be authorized for read"
            );
            let auth_parent_write =
                FileAccessPolicy::authorize_path_buf(parent, OperationPolicy::Write);
            assert!(
                auth_parent_write.is_err(),
                "home ancestor directory must not be authorized for write"
            );
        }
    }
}

#[test]
fn detect_project_badge_priority() {
    let mut names = std::collections::HashSet::new();
    names.insert("package.json".into());
    names.insert("Cargo.toml".into());
    assert_eq!(detect_project_badge(&names).as_deref(), Some("node"));

    names.clear();
    names.insert("Cargo.toml".into());
    assert_eq!(detect_project_badge(&names).as_deref(), Some("rust"));

    names.clear();
    names.insert(".git".into());
    assert_eq!(detect_project_badge(&names).as_deref(), Some("git"));
}

#[test]
fn detect_file_kind_extensionless_and_images() {
    assert_eq!(detect_file_kind("Dockerfile"), "text");
    assert_eq!(detect_file_kind("Makefile"), "text");
    assert_eq!(detect_file_kind("photo.PNG"), "image");
    assert_eq!(detect_file_kind("archive.zip"), "archive");
    assert_eq!(detect_file_kind("library.jar"), "archive");
    assert_eq!(detect_file_kind("bundle.gz"), "archive");
    assert_eq!(detect_file_kind("notes.md"), "text");
}

#[test]
fn create_rename_copy_duplicate_roundtrip() {
    let dir = tmp_dir("roundtrip");
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    // Ensure path is under allowlist (home or /tmp)
    let base =
        if dir.starts_with(&home) || dir.starts_with("/tmp") || dir.starts_with("/private/tmp") {
            dir.clone()
        } else {
            let fallback =
                std::env::temp_dir().join(format!("natives-fm-home-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&fallback);
            fallback
        };

    let file = base.join("hello.txt");
    create_entry(file.to_str().unwrap(), "file").expect("create file");
    assert!(file.exists());

    let renamed = rename_entry(
        file.to_str().unwrap(),
        base.join("hello-renamed.txt").to_str().unwrap(),
    )
    .expect("rename");
    assert!(Path::new(&renamed).exists());
    assert!(!file.exists());

    let copied = copy_entry(&renamed, base.join("hello-copy.txt").to_str().unwrap()).expect("copy");
    assert!(Path::new(&copied).exists());
    assert!(Path::new(&renamed).exists());
    let copied_again = copy_entry(&renamed, base.join("hello-copy.txt").to_str().unwrap())
        .expect("deduplicate copy");
    assert_ne!(copied_again, copied);
    assert!(Path::new(&copied_again).exists());

    let dup = duplicate_entry(&renamed).expect("duplicate");
    assert!(Path::new(&dup).exists());
    assert_ne!(dup, renamed);

    let sub = base.join("sub");
    create_entry(sub.to_str().unwrap(), "folder").expect("create folder");
    assert!(sub.is_dir());

    // folder type alias
    let sub2 = base.join("sub2");
    create_entry(sub2.to_str().unwrap(), "dir").expect("create dir");
    assert!(sub2.is_dir());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn streaming_import_commits_chunks_without_overwrite() {
    let dir = tmp_dir("stream-import");
    let target = dir.join("payload.bin");
    let mut writer =
        begin_import(dir.to_str().unwrap(), "payload.bin", 5, "rename").expect("begin import");
    writer.write_chunk(0, b"he").expect("first chunk");
    writer.write_chunk(2, b"llo").expect("second chunk");
    let result = writer.finish().expect("finish import");
    assert_eq!(result.size, 5);
    assert_eq!(std::fs::read(&target).unwrap(), b"hello");

    let mut renamed = begin_import(dir.to_str().unwrap(), "payload.bin", 1, "rename")
        .expect("deduplicate import");
    assert_ne!(renamed.target_path(), target.as_path());
    renamed.write_chunk(0, b"!").unwrap();
    renamed.finish().unwrap();
    assert_eq!(std::fs::read(dir.join("payload (1).bin")).unwrap(), b"!");

    assert!(begin_import(dir.to_str().unwrap(), "payload.bin", 0, "skip").is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn streaming_import_rejects_bad_offsets_and_incomplete_data() {
    let dir = tmp_dir("stream-import-validation");
    let mut writer =
        begin_import(dir.to_str().unwrap(), "payload", 3, "rename").expect("begin import");
    assert!(writer.write_chunk(1, b"x").is_err());
    assert!(writer.finish().is_err());
    assert!(!dir.join("payload").exists());
    assert!(begin_import(dir.to_str().unwrap(), "../escape", 0, "rename").is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cancelled_streaming_import_drops_temporary_file() {
    let dir = tmp_dir("stream-import-cancel");
    {
        let mut writer =
            begin_import(dir.to_str().unwrap(), "payload.bin", 8, "rename").expect("begin import");
        writer.write_chunk(0, b"partial").expect("write partial");
    }
    let leftovers = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".natives-import-")
        })
        .count();
    assert_eq!(leftovers, 0);
    assert!(!dir.join("payload.bin").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn list_dir_detailed_includes_project_and_sorts_dirs_first() {
    let base = tmp_dir("listdetail");
    // Write under /tmp which is allowlisted
    let f = base.join("b.txt");
    let mut file = std::fs::File::create(&f).unwrap();
    file.write_all(b"x").unwrap();
    let d = base.join("a-dir");
    std::fs::create_dir(&d).unwrap();
    // project marker
    std::fs::write(base.join("package.json"), b"{}").unwrap();

    let result = list_dir_detailed(
        base.to_str().unwrap(),
        &ListDirOptions {
            sort_by: "name".into(),
            sort_dir: "asc".into(),
            show_hidden: false,
            probe_projects: false,
        },
    )
    .expect("list");
    assert_eq!(result.project.as_deref(), Some("node"));
    // directories first
    assert!(result.entries[0].is_dir, "first entry should be a dir");
    let descending = list_dir_detailed(
        base.to_str().unwrap(),
        &ListDirOptions {
            sort_by: "name".into(),
            sort_dir: "desc".into(),
            show_hidden: false,
            probe_projects: false,
        },
    )
    .expect("descending list");
    assert!(descending.entries[0].is_dir, "folders remain grouped first");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn list_dir_page_returns_bounded_window_and_has_more() {
    let base = tmp_dir("page");
    for name in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(base.join(name), name.as_bytes()).unwrap();
    }
    let (page, has_more) = list_dir_page(base.to_str().unwrap(), 1, 1).expect("page");
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].name, "b.txt");
    assert!(has_more);
    let (_, last_more) = list_dir_page(base.to_str().unwrap(), 2, 1).expect("last page");
    assert!(!last_more);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn list_dir_page_with_options_applies_hidden_and_size_sorting() {
    let base = tmp_dir("page-options");
    std::fs::write(base.join("small.txt"), b"a").unwrap();
    std::fs::write(base.join("large.txt"), b"aaaaaaaa").unwrap();
    std::fs::write(base.join(".hidden.txt"), b"hidden").unwrap();
    let options = ListDirOptions {
        sort_by: "size".into(),
        sort_dir: "desc".into(),
        show_hidden: true,
        probe_projects: false,
    };

    let (page, has_more) =
        list_dir_page_with_options(base.to_str().unwrap(), 0, 2, &options).unwrap();

    assert!(has_more);
    assert_eq!(page.entries[0].name, "large.txt");
    assert_eq!(page.entries[1].name, ".hidden.txt");
    let _ = std::fs::remove_dir_all(&base);
}

/// Frontend FileBrowser sends camelCase options via Tauri IPC.
/// Regression: without rename_all these fields were ignored → default sort only.
#[test]
fn list_dir_options_accepts_frontend_camel_case_json() {
    let v = serde_json::json!({
        "sortBy": "size",
        "sortDir": "desc",
        "showHidden": true,
        "probeProjects": false
    });
    let opts: ListDirOptions = serde_json::from_value(v).expect("deserialize camelCase");
    assert_eq!(opts.sort_by, "size");
    assert_eq!(opts.sort_dir, "desc");
    assert!(opts.show_hidden);
    assert!(!opts.probe_projects);
}

#[test]
fn list_dir_sorts_by_size_and_mtime() {
    let base = tmp_dir("sortkeys");
    let small = base.join("small.txt");
    let large = base.join("large.txt");
    std::fs::write(&small, b"a").unwrap();
    std::fs::write(&large, b"aaaaaaaaaa").unwrap();

    // Rewrite small after a brief delay so mtime-desc puts it first.
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&small, b"ab").unwrap();

    let by_size = list_dir_detailed(
        base.to_str().unwrap(),
        &ListDirOptions {
            sort_by: "size".into(),
            sort_dir: "desc".into(),
            show_hidden: false,
            probe_projects: false,
        },
    )
    .expect("list size");
    let size_names: Vec<&str> = by_size.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(size_names, vec!["large.txt", "small.txt"], "size desc");

    let by_mtime = list_dir_detailed(
        base.to_str().unwrap(),
        &ListDirOptions {
            sort_by: "mtime".into(),
            sort_dir: "desc".into(),
            show_hidden: false,
            probe_projects: false,
        },
    )
    .expect("list mtime");
    assert_eq!(
        by_mtime.entries[0].name, "small.txt",
        "mtime desc should put rewritten small first"
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn read_file_returns_mtime_and_kind() {
    let base = tmp_dir("readmeta");
    let f = base.join("note.md");
    std::fs::write(&f, b"# hi").unwrap();
    let r = read_file(f.to_str().unwrap()).expect("read");
    assert_eq!(r.content, "# hi");
    assert_eq!(r.kind, "text");
    assert!(!r.truncated);
    assert!(r.mtime > 0.0);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn read_file_rejects_binary_preview_transport() {
    let base = tmp_dir("binary-preview");
    let image = base.join("photo.png");
    std::fs::write(&image, b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").unwrap();
    let error = read_file(image.to_str().unwrap()).unwrap_err().to_string();
    assert!(error.contains("binary preview unsupported"), "{error}");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn image_preview_accepts_safe_formats_and_rejects_unknown() {
    let base = tmp_dir("image-preview");
    let image = base.join("photo.png");
    std::fs::write(&image, b"\x89PNG\r\n\x1a\n").unwrap();
    let preview = read_image_preview(image.to_str().unwrap()).expect("image preview");
    assert_eq!(preview.mime_type, "image/png");
    assert_eq!(preview.bytes, b"\x89PNG\r\n\x1a\n");
    let bitmap = base.join("photo.bmp");
    std::fs::write(&bitmap, b"BM\0\0").unwrap();
    let bitmap_preview = read_image_preview(bitmap.to_str().unwrap()).expect("bmp preview");
    assert_eq!(bitmap_preview.mime_type, "image/bmp");
    let avif = base.join("photo.avif");
    let mut avif_bytes = vec![0; 12];
    avif_bytes[4..12].copy_from_slice(b"ftypavif");
    std::fs::write(&avif, &avif_bytes).unwrap();
    let avif_preview = read_image_preview(avif.to_str().unwrap()).expect("avif preview");
    assert_eq!(avif_preview.mime_type, "image/avif");
    let unknown = base.join("photo.svg");
    std::fs::write(&unknown, b"<svg/>").unwrap();
    let error = read_image_preview(unknown.to_str().unwrap())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("image preview format unsupported"),
        "{error}"
    );
    let disguised = base.join("fake.png");
    std::fs::write(&disguised, b"not an image").unwrap();
    let error = read_image_preview(disguised.to_str().unwrap())
        .unwrap_err()
        .to_string();
    assert!(error.contains("image header is invalid"), "{error}");
    let oversized = base.join("large.png");
    std::fs::File::create(&oversized)
        .unwrap()
        .set_len(MAX_IMAGE_PREVIEW_BYTES + 1)
        .unwrap();
    let error = read_image_preview(oversized.to_str().unwrap())
        .unwrap_err()
        .to_string();
    assert!(error.contains("image preview is too large"), "{error}");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn clipboard_image_rejects_non_image_before_touching_clipboard() {
    let base = tmp_dir("clipboard-image");
    let text = base.join("note.txt");
    std::fs::write(&text, b"not an image").unwrap();
    let error = clipboard_copy_image(text.to_str().unwrap())
        .unwrap_err()
        .to_string();
    assert!(error.contains("not an image file"), "{error}");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn pdf_preview_accepts_bounded_pdf_and_rejects_invalid_header() {
    let base = tmp_dir("pdf-preview");
    let pdf = base.join("sample.pdf");
    std::fs::write(&pdf, b"%PDF-1.7\n").unwrap();
    let preview = read_pdf_preview(pdf.to_str().unwrap()).expect("pdf preview");
    assert_eq!(preview.mime_type, "application/pdf");
    let invalid = base.join("invalid.pdf");
    std::fs::write(&invalid, b"not a pdf").unwrap();
    let error = read_pdf_preview(invalid.to_str().unwrap())
        .unwrap_err()
        .to_string();
    assert!(error.contains("pdf header is invalid"), "{error}");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn media_preview_accepts_safe_audio_and_rejects_unknown() {
    let base = tmp_dir("media-preview");
    let audio = base.join("sample.mp3");
    std::fs::write(&audio, b"ID3").unwrap();
    let preview = read_media_preview(audio.to_str().unwrap()).expect("media preview");
    assert_eq!(preview.mime_type, "audio/mpeg");
    let disguised = base.join("fake.mp3");
    std::fs::write(&disguised, b"not audio").unwrap();
    let error = read_media_preview(disguised.to_str().unwrap())
        .unwrap_err()
        .to_string();
    assert!(error.contains("media header is invalid"), "{error}");
    let unknown = base.join("sample.exe");
    std::fs::write(&unknown, b"MZ").unwrap();
    let error = read_media_preview(unknown.to_str().unwrap())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("media preview format unsupported"),
        "{error}"
    );
    let oversized = base.join("large.mp4");
    std::fs::File::create(&oversized)
        .unwrap()
        .set_len(MAX_MEDIA_PREVIEW_BYTES + 1)
        .unwrap();
    let error = read_media_preview(oversized.to_str().unwrap())
        .unwrap_err()
        .to_string();
    assert!(error.contains("media preview is too large"), "{error}");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn atomic_write_rejects_stale_mtime_and_preserves_content() {
    let base = tmp_dir("atomic-write");
    let file = base.join("note.txt");
    std::fs::write(&file, b"before").unwrap();
    let original = stat_path(file.to_str().unwrap()).unwrap().mtime.unwrap();
    std::thread::sleep(std::time::Duration::from_millis(10));
    std::fs::write(&file, b"external").unwrap();

    let result = write_file_atomic(file.to_str().unwrap(), "replacement", Some(original)).unwrap();

    assert!(result.conflict);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "external");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn atomic_byte_write_truncation_keeps_utf8_boundary() {
    let base = tmp_dir("utf8-truncate");
    let file = base.join("large.txt");
    let content = "界".repeat(900_000);
    std::fs::write(&file, content.as_bytes()).unwrap();

    let result = read_file(file.to_str().unwrap()).unwrap();

    assert!(result.truncated);
    assert!(result.content.chars().all(|ch| ch == '界'));
    assert!(result.content.len() <= 256 * 1024);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn stat_path_missing_and_found() {
    let missing = stat_path("/tmp/natives-definitely-missing-xyz-12345").unwrap();
    assert!(!missing.found);
    assert!(missing.name.is_none());

    let base = tmp_dir("stat");
    let f = base.join("x.txt");
    std::fs::write(&f, b"1").unwrap();
    let found = stat_path(f.to_str().unwrap()).unwrap();
    assert!(found.found);
    assert_eq!(found.is_dir, Some(false));
    assert_eq!(found.name.as_deref(), Some("x.txt"));
    assert_eq!(found.kind.as_deref(), Some("text"));
    // dirHint = 所在目录
    assert_eq!(found.dir_hint.as_deref(), base.to_str());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn disk_usage_counts_nested_files_without_following_symlinks() {
    let root = tmp_dir("usage");
    std::fs::create_dir_all(root.join("nested")).unwrap();
    std::fs::write(root.join("a.txt"), b"123").unwrap();
    std::fs::write(root.join("nested/b.txt"), b"4567").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink("/etc", root.join("escape")).unwrap();
    }
    let usage = disk_usage(root.to_str().unwrap()).unwrap();
    assert!(usage.bytes >= 7);
    assert!(usage.files >= 2);
    assert!(usage.directories >= 2);
    assert_eq!(
        usage.items.first().map(|item| item.name.as_str()),
        Some("nested")
    );
    assert_eq!(usage.items.first().map(|item| item.size), Some(4));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn batch_copy_move_trash_and_roots() {
    let base = tmp_dir("batch");
    let a = base.join("a.txt");
    let b = base.join("b.txt");
    std::fs::write(&a, b"a").unwrap();
    std::fs::write(&b, b"b").unwrap();
    let dest = base.join("out");
    std::fs::create_dir(&dest).unwrap();

    let copied = copy_entries(
        &[
            a.to_string_lossy().to_string(),
            b.to_string_lossy().to_string(),
        ],
        dest.to_str().unwrap(),
    )
    .unwrap();
    assert_eq!(copied["count"], 2);
    assert!(dest.join("a.txt").exists());

    let dest2 = base.join("out2");
    std::fs::create_dir(&dest2).unwrap();
    let moved = move_entries(
        &[dest.join("a.txt").to_string_lossy().to_string()],
        dest2.to_str().unwrap(),
    )
    .unwrap();
    assert_eq!(moved["count"], 1);
    assert!(dest2.join("a.txt").exists());
    assert!(!dest.join("a.txt").exists());

    let roots = default_roots().unwrap();
    assert!(!roots.is_empty());
    // open_with default on a real file should not panic (may fail headless)
    let _ = open_with(b.to_str().unwrap(), "default");

    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_privacy_protected_home_directories_are_deferred() {
    let home = dirs::home_dir().unwrap();
    for name in [
        "Desktop",
        "Documents",
        "Downloads",
        "Library",
        "Movies",
        "Music",
        "Pictures",
    ] {
        assert!(is_macos_privacy_protected_home_child(&home.join(name)));
    }
    assert!(!is_macos_privacy_protected_home_child(
        &home.join("Projects")
    ));
}

#[test]
fn deduplicate_path_adds_counter() {
    let base = tmp_dir("dedupe");
    let f = base.join("doc.txt");
    std::fs::write(&f, b"a").unwrap();
    let next = deduplicate_path(&f).unwrap();
    assert_eq!(next.file_name().unwrap().to_str().unwrap(), "doc (1).txt");
    let _ = std::fs::remove_dir_all(&base);
}
