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

    // iOS has no Python sidecar; skip placeholder binaries for apple-ios targets.
    if !target.contains("ios") {
        ensure_engine_placeholder(&manifest_dir, &target);
    }

    if target.contains("windows-msvc") {
        println!("cargo::rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo::rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' publicKeyToken='6595b64144ccf1df' language='*' processorArchitecture='*'"
        );

        let attributes = tauri_build::Attributes::new().windows_attributes(
            tauri_build::WindowsAttributes::new_without_app_manifest(),
        );
        tauri_build::try_build(attributes).expect("failed to run Tauri build script");
    } else {
        tauri_build::build();
    }
}
