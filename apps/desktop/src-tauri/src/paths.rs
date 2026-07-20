use std::env;
use std::path::{Path, PathBuf};

/// ユーザーデータ用ルート
///   Windows: %LOCALAPPDATA%\PKB
///   macOS:   ~/Library/Application Support/PKB
///            (App Sandbox 時は Containers/.../Data/Library/Application Support/PKB)
///   iOS:     $HOME/Library/Application Support/com.ai-shizu.pkb
///            ($HOME = app container; Tauri `app_data_dir()` と同型)
///   Linux:   $XDG_DATA_HOME/PKB または ~/.local/share/PKB
#[cfg(windows)]
#[cfg_attr(debug_assertions, allow(dead_code))]
pub fn user_data_root() -> PathBuf {
    if let Ok(local) = env::var("LOCALAPPDATA") {
        return PathBuf::from(local).join("PKB");
    }
    if let Ok(home) = env::var("USERPROFILE") {
        return PathBuf::from(home).join("PKB");
    }
    PathBuf::from(".")
}

#[cfg(target_os = "macos")]
#[cfg_attr(debug_assertions, allow(dead_code))]
pub fn user_data_root() -> PathBuf {
    const APP_CONTAINER_ID: &str = "com.ai-shizu.pkb";
    if let Ok(home) = env::var("HOME") {
        let home = PathBuf::from(home);
        if let Ok(container_id) = env::var("APP_SANDBOX_CONTAINER_ID") {
            if container_id == APP_CONTAINER_ID {
                return home
                    .join("Library")
                    .join("Containers")
                    .join(container_id)
                    .join("Data")
                    .join("Library")
                    .join("Application Support")
                    .join("PKB");
            }
        }
        return home.join("Library").join("Application Support").join("PKB");
    }
    PathBuf::from(".")
}

/// iOS App Sandbox: `HOME` is the app container root (not a Unix user home).
/// Prefer `Library/Application Support/<bundle id>` so this matches Tauri's
/// `PathResolver::app_data_dir()` used by Vault spawn in `lib.rs`.
/// Do not fall through to XDG / `~/.local/share` (M19-A finding).
#[cfg(target_os = "ios")]
#[cfg_attr(debug_assertions, allow(dead_code))]
pub fn user_data_root() -> PathBuf {
    const BUNDLE_ID: &str = "com.ai-shizu.pkb";
    if let Ok(home) = env::var("HOME") {
        if !home.is_empty() {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join(BUNDLE_ID);
        }
    }
    PathBuf::from(".")
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "ios")))]
#[cfg_attr(debug_assertions, allow(dead_code))]
pub fn user_data_root() -> PathBuf {
    if let Ok(xdg) = env::var("XDG_DATA_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("PKB");
        }
    }
    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home).join(".local").join("share").join("PKB");
    }
    PathBuf::from(".")
}

#[cfg(debug_assertions)]
fn dev_repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// リポジトリルート / データルートを解決する。
#[cfg(debug_assertions)]
pub fn project_root() -> PathBuf {
    if let Ok(root) = env::var("PKB_PROJECT_ROOT") {
        let path = PathBuf::from(root);
        if path.is_dir() {
            return path;
        }
    }

    let repo = dev_repo_root();
    let repo_script = repo.join("src").join("python").join("run_engine.py");
    if repo_script.is_file() {
        return repo.canonicalize().unwrap_or(repo);
    }

    repo
}

#[cfg(not(debug_assertions))]
pub fn project_root() -> PathBuf {
    user_data_root()
}

#[cfg(debug_assertions)]
pub fn run_engine_script() -> PathBuf {
    project_root()
        .join("src")
        .join("python")
        .join("run_engine.py")
}

/// 初回起動用に data/ 等を作成
pub fn ensure_data_layout(root: &Path) {
    for sub in [
        "data/raw",
        "data/processed",
        "data/knowledge",
        "build",
        "models",
        "logs",
    ] {
        let _ = std::fs::create_dir_all(root.join(sub));
    }
}

/// Windows (x64/ARM64) / macOS 向け Python 実行ファイルを探す。
#[cfg(debug_assertions)]
pub fn find_python_executable() -> Option<PathBuf> {
    if let Ok(custom) = env::var("PKB_PYTHON") {
        let path = PathBuf::from(&custom);
        if is_python_executable(&path) {
            return Some(path);
        }
    }

    for path in python_candidates() {
        if is_python_executable(&path) {
            return Some(path);
        }
    }

    for name in python_names_on_path() {
        if let Some(path) = find_on_path(name) {
            if is_python_executable(&path) {
                return Some(path);
            }
        }
    }

    None
}

#[cfg(all(not(debug_assertions), windows, target_arch = "aarch64"))]
pub fn bundled_engine_name() -> &'static str {
    "pkb-engine-aarch64-pc-windows-msvc.exe"
}

#[cfg(all(not(debug_assertions), windows, target_arch = "x86_64"))]
pub fn bundled_engine_name() -> &'static str {
    "pkb-engine-x86_64-pc-windows-msvc.exe"
}

#[cfg(all(not(debug_assertions), target_os = "macos", target_arch = "aarch64"))]
pub fn bundled_engine_name() -> &'static str {
    "pkb-engine-aarch64-apple-darwin"
}

#[cfg(all(not(debug_assertions), target_os = "macos", target_arch = "x86_64"))]
pub fn bundled_engine_name() -> &'static str {
    "pkb-engine-x86_64-apple-darwin"
}

#[cfg(all(not(debug_assertions), target_os = "linux"))]
pub fn bundled_engine_name() -> &'static str {
    "pkb-engine-x86_64-unknown-linux-gnu"
}

#[cfg(not(debug_assertions))]
pub fn bundled_engine_path() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let dir = exe.parent()?;
    let path = dir.join(bundled_engine_name());
    if path.is_file()
        && path
            .metadata()
            .map(|m| m.len() > 1024 * 1024)
            .unwrap_or(false)
    {
        Some(path)
    } else {
        None
    }
}

#[cfg(debug_assertions)]
fn is_python_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(debug_assertions)]
fn python_names_on_path() -> &'static [&'static str] {
    if cfg!(windows) {
        &["python.exe", "python3.exe", "python", "python3"]
    } else {
        &["python3", "python"]
    }
}

#[cfg(debug_assertions)]
fn python_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();

    if cfg!(windows) {
        if let Ok(local) = env::var("LOCALAPPDATA") {
            let base = PathBuf::from(local).join("Programs").join("Python");
            for ver in [
                "Python312-arm64",
                "Python312",
                "Python313-arm64",
                "Python313",
            ] {
                out.push(base.join(ver).join("python.exe"));
            }
        }
    } else if cfg!(target_os = "macos") {
        out.push(PathBuf::from("/opt/homebrew/bin/python3"));
        out.push(PathBuf::from("/usr/local/bin/python3"));
    } else {
        out.push(PathBuf::from("/usr/bin/python3"));
    }

    out
}

#[cfg(debug_assertions)]
fn find_on_path(name: &str) -> Option<PathBuf> {
    let command = if cfg!(windows) { "where" } else { "which" };
    let output = std::process::Command::new(command)
        .arg(name)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()?
        .trim()
        .to_string();
    if line.is_empty() {
        None
    } else {
        Some(PathBuf::from(line))
    }
}
