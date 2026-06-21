fn main() {
    // ── libghostty-vt 静态库编译（feature gate ghostty-vt） ──
    // 策略：优先尝试用 zig 构建真实 Ghostty VT 库；
    // 若失败（环境不兼容、源代码缺失等），自动编译 C 退化桩以保证链接通过。

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let install_dir = std::path::Path::new(&out_dir).join("ghostty-vt");
    let lib_dir = install_dir.join("lib");
    let lib_file = lib_dir.join("libghostty-vt.a");

    // 检查是否已缓存过真实库（避免每次 rebuild）
    let real_lib_exists = lib_file.exists();

    if !real_lib_exists {
        // 尝试用 zig 构建真实 Ghostty VT 库
        let ghostty_src = std::path::Path::new("References/ghostty");
        let has_ghostty_src = ghostty_src.join("build.zig").exists();

        let zig_built = if has_ghostty_src {
            // 确定 zig 命令：Ghostty 需要 Zig v0.15.x
            let zig_candidates = ["zig-0.15", "zig"];
            let zig_bin = zig_candidates.iter().find(|&&name| {
                std::process::Command::new(name)
                    .arg("version")
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
            });

            if let Some(zig) = zig_bin {
                println!("cargo:rerun-if-changed=References/ghostty/src/");
                println!("cargo:rerun-if-changed=References/ghostty/build.zig");

                let status = std::process::Command::new(zig)
                    .args(["build", "-Demit-lib-vt", "--prefix", &install_dir.to_string_lossy()])
                    .current_dir(ghostty_src)
                    .status()
                    .expect("failed to run zig build for ghostty-vt");

                if status.success() {
                    // 拷贝头文件供 IDE 索引
                    let include_dir = install_dir.join("include");
                    if include_dir.exists() {
                        let dest = std::path::Path::new(&out_dir).join("ghostty-vt-headers");
                        let _ = std::fs::remove_dir_all(&dest);
                        let _ = copy_dir_recursive(&include_dir, &dest);
                    }
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        if !zig_built {
            // 降级：编译 C 退化桩
            std::fs::create_dir_all(&lib_dir).unwrap();
            let stub_c = std::path::Path::new("ghostty-vt-stub.c");
            if stub_c.exists() {
                let status = std::process::Command::new("cc")
                    .args([
                        "-c", "-o",
                        &lib_dir.join("ghostty-vt-stub.o").to_string_lossy(),
                        &stub_c.to_string_lossy(),
                    ])
                    .status()
                    .expect("failed to compile ghostty-vt stub");

                if status.success() {
                    let status = std::process::Command::new("ar")
                        .args([
                            "rcs",
                            &lib_file.to_string_lossy(),
                            &lib_dir.join("ghostty-vt-stub.o").to_string_lossy(),
                        ])
                        .status()
                        .expect("failed to archive ghostty-vt stub");
                    if !status.success() {
                        println!("cargo:warning=ar failed for ghostty-vt stub");
                    }
                } else {
                    println!("cargo:warning=cc failed for ghostty-vt stub");
                }
            }
        }
    }

    // ── 链接 libghostty-vt.a ──
    if lib_file.exists() {
        println!("cargo:rustc-link-search={}", lib_dir.display());
        println!("cargo:rustc-link-lib=static=ghostty-vt");
    }

    // ── Tauri 标准构建 ──
    tauri_build::build()
}

/// Recursively copy a directory (simple implementation for build.rs)
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
