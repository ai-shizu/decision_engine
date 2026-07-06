use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn engine_path(manifest_dir: &Path, target: &str) -> PathBuf {
    let file_name = if target.contains("windows") {
        format!("pkb-engine-{target}.exe")
    } else {
        format!("pkb-engine-{target}")
    };
    manifest_dir.join("binaries").join(file_name)
}

fn ensure_engine_placeholder(manifest_dir: &Path, target: &str) {
    let path = engine_path(manifest_dir, target);
    if path.is_file() {
        return;
    }

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let stub = b"PKB dev placeholder. Run build.cmd before release.\r\n";
    let _ = fs::write(&path, stub);
    println!(
        "cargo:warning=Created dev engine placeholder: {} (run build.cmd for release)",
        path.display()
    );
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let target = env::var("TARGET").unwrap_or_else(|_| "aarch64-pc-windows-msvc".to_string());
    ensure_engine_placeholder(&manifest_dir, &target);
    tauri_build::build();
}
