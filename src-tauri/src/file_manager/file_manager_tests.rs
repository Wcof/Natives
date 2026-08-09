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

/// recent_files 契约：条目为完整 FileEntry（name/kind/hidden/dirHint 后端补齐），
/// 按 mtime 降序，隐藏文件在遍历时被跳过。
#[test]
fn recent_files_returns_full_entries_with_dir_hint() {
    // recent_files 内部会 canonicalize（macOS 下 /var → /private/var），
    // 先归一化 base 以便 dirHint 断言按同一路径比较
    let base = std::fs::canonicalize(tmp_dir("recent")).unwrap();
    std::fs::write(base.join("old.md"), b"# old").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let sub = base.join("sub");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("new.png"), b"png").unwrap();
    // 隐藏文件应被跳过
    std::fs::write(base.join(".hidden.txt"), b"h").unwrap();

    let entries = recent_files(base.to_str().unwrap()).expect("recent");
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["new.png", "old.md"], "mtime 降序且不含隐藏文件");

    let newest = &entries[0];
    assert_eq!(newest.kind, "image", "kind 由后端 detect_file_kind 计算");
    assert!(!newest.is_dir);
    assert!(!newest.hidden);
    assert_eq!(
        newest.dir_hint.as_deref(),
        sub.to_str(),
        "dirHint 指向来源目录"
    );
    assert!(newest.mtime > 0.0);

    let older = &entries[1];
    assert_eq!(older.kind, "text");
    assert_eq!(older.dir_hint.as_deref(), base.to_str());

    let _ = std::fs::remove_dir_all(&base);
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

#[test]
fn deduplicate_path_adds_counter() {
    let base = tmp_dir("dedupe");
    let f = base.join("doc.txt");
    std::fs::write(&f, b"a").unwrap();
    let next = deduplicate_path(&f).unwrap();
    assert_eq!(next.file_name().unwrap().to_str().unwrap(), "doc (1).txt");
    let _ = std::fs::remove_dir_all(&base);
}
