use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let root = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let registry_path = root.join("modules/registry.json");
    println!("cargo:rerun-if-changed={}", registry_path.display());
    let registry: serde_json::Value =
        serde_json::from_slice(&fs::read(&registry_path).unwrap()).unwrap();
    let registrations = registry["apps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|app| {
            let id = app["appId"].as_str().unwrap();
            let factory = app["factory"].as_str().unwrap();
            format!("registry.register(\"{id}\", Box::new(|| Box::new({factory}())));")
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("module_registry.rs"),
        format!("{{\n{registrations}\n}}"),
    )
    .unwrap();
}
