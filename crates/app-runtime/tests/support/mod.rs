//! Immutable executable per integration-test process. Cargo/dev may replace the
//! shared target binary; activation must hash exactly the executable we launch.
use std::path::PathBuf;
use std::sync::OnceLock;

pub fn runtime_binary() -> PathBuf {
    static SNAPSHOT: OnceLock<(tempfile::TempDir, PathBuf)> = OnceLock::new();
    SNAPSHOT
        .get_or_init(|| {
            let mut source = std::env::current_exe().expect("test executable");
            source.pop();
            if source.ends_with("deps") {
                source.pop();
            }
            source.push(format!(
                "natives-app-runtime{}",
                std::env::consts::EXE_SUFFIX
            ));
            let directory = tempfile::tempdir().expect("isolated executable directory");
            let binary = directory.path().join(source.file_name().unwrap());
            std::fs::copy(&source, &binary).expect("snapshot built Runtime executable");
            (directory, binary)
        })
        .1
        .clone()
}
