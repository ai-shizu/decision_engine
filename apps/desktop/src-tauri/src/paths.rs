use std::env;
use std::path::{Path, PathBuf};

/// ユーザーデータ用ルート (%LOCALAPPDATA%\PKB 等)
pub fn user_data_root() -> PathBuf {
    if let Ok(local) = env::var("LOCALAPPDATA") {
        return PathBuf::from(local).join("PKB");
    }
    if let Ok(home) = env::var("USERPROFILE") {
        return PathBuf::from(home).join("PKB");
    }
    PathBuf::from(".")
}

fn dev_repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// リポジトリルート / データルートを解決する。
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

    if cfg!(debug_assertions) {
        return repo;
    }

    user_data_root()
}

pub fn run_engine_script() -> PathBuf {
    project_root().join("src").join("python").join("run_engine.py")
}

/// 初回起動用に data/ 等を作成
pub fn ensure_data_layout(root: &Path) {
    for sub in ["data/raw", "data/processed", "data/knowledge", "build", "models", "logs"] {
        let _ = std::fs::create_dir_all(root.join(sub));
    }
}

/// Windows (x64/ARM64) / macOS 向け Python 実行ファイルを探す。
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

#[cfg(all(windows, target_arch = "aarch64"))]
pub fn bundled_engine_name() -> &'static str {
    "pkb-engine-aarch64-pc-windows-msvc.exe"
}

#[cfg(all(windows, target_arch = "x86_64"))]
pub fn bundled_engine_name() -> &'static str {
    "pkb-engine-x86_64-pc-windows-msvc.exe"
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub fn bundled_engine_name() -> &'static str {
    "pkb-engine-aarch64-apple-darwin"
}

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
pub fn bundled_engine_name() -> &'static str {
    "pkb-engine-x86_64-apple-darwin"
}

#[cfg(target_os = "linux")]
pub fn bundled_engine_name() -> &'static str {
    "pkb-engine-x86_64-unknown-linux-gnu"
}

pub fn bundled_engine_path() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let dir = exe.parent()?;
    let path = dir.join(bundled_engine_name());
    if path.is_file() && path.metadata().map(|m| m.len() > 1024 * 1024).unwrap_or(false) {
        Some(path)
    } else {
        None
    }
}

fn is_python_executable(path: &Path) -> bool {
    path.is_file()
}

fn python_names_on_path() -> &'static [&'static str] {
    if cfg!(windows) {
        &["python.exe", "python3.exe", "python", "python3"]
    } else {
        &["python3", "python"]
    }
}

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

fn find_on_path(name: &str) -> Option<PathBuf> {
    let command = if cfg!(windows) { "where" } else { "which" };
    let output = std::process::Command::new(command).arg(name).output().ok()?;
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
