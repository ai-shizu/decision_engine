use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::paths::{
    bundled_engine_path, ensure_data_layout, find_python_executable, project_root, run_engine_script,
};

static REQ_COUNTER: AtomicU64 = AtomicU64::new(1);

struct EngineProcess {
    child: Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

pub struct EngineManager {
    process: Mutex<Option<EngineProcess>>,
    ready: Mutex<bool>,
    restart_lock: Mutex<()>,
    app: Mutex<Option<AppHandle>>,
}

impl EngineManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            process: Mutex::new(None),
            ready: Mutex::new(false),
            restart_lock: Mutex::new(()),
            app: Mutex::new(None),
        })
    }

    fn log(line: &str) {
        eprintln!("[PKB] {line}");
    }

    fn is_pipe_error(err: &str) -> bool {
        err.contains("書き込み失敗")
            || err.contains("読み取り失敗")
            || err.contains("終了しました")
            || err.contains("応答を返さず終了")
            || err.contains("valid UTF-8")
    }

    pub fn shutdown(self: &Arc<Self>) {
        if self.is_ready() {
            let _ = self.invoke_sync("shutdown", json!({}), None, false);
        }
        let mut guard = self.process.lock().unwrap();
        if let Some(mut proc) = guard.take() {
            let _ = proc.child.kill();
            let _ = proc.child.wait();
        }
        *self.ready.lock().unwrap() = false;
    }

    pub async fn start(self: &Arc<Self>, app: AppHandle) -> Result<(), String> {
        *self.app.lock().unwrap() = Some(app);
        let _lock = self.restart_lock.lock().unwrap();
        self.shutdown();
        self.boot_engine()
    }

    /// Python からの中間イベント行 ({"id", "event", ...}) をフロントへ転送する。
    fn forward_event(self: &Arc<Self>, payload: &Value) {
        if let Some(app) = self.app.lock().unwrap().as_ref() {
            if let Err(e) = app.emit("pkb-engine-event", payload) {
                Self::log(&format!("イベント転送失敗: {e}"));
            }
        }
    }

    pub async fn restart(self: &Arc<Self>) -> Result<(), String> {
        let _lock = self.restart_lock.lock().unwrap();
        self.shutdown();
        self.boot_engine()
    }

    fn boot_engine(self: &Arc<Self>) -> Result<(), String> {
        let root = project_root();
        ensure_data_layout(&root);
        let script = run_engine_script();

        Self::log(&format!("data root: {}", root.display()));

        let mut proc = if cfg!(debug_assertions) {
            if !script.is_file() {
                return Err(format!(
                    "run_engine.py が見つかりません: {}",
                    script.display()
                ));
            }
            Self::log("dev: Python エンジンを起動");
            spawn_python_engine(&root, &script)?
        } else if let Some(path) = bundled_engine_path() {
            Self::log(&format!("release: {}", path.display()));
            spawn_bundled_engine(&path, &root)?
        } else if script.is_file() {
            Self::log("release: 同梱エンジンなし — Python フォールバック");
            spawn_python_engine(&root, &script)?
        } else {
            return Err(
                "PKB エンジンが見つかりません。build.cmd で再ビルドしてください。".to_string(),
            );
        };

        let ready_line = read_line(&mut proc.stdout)?;
        let ready: Value = serde_json::from_str(&ready_line)
            .map_err(|e| format!("エンジン ready 解析失敗: {e} — {ready_line}"))?;
        if ready.get("event").and_then(|v| v.as_str()) != Some("ready") {
            return Err(format!("エンジン ready 失敗: {ready_line}"));
        }

        *self.process.lock().unwrap() = Some(proc);
        *self.ready.lock().unwrap() = true;
        Self::log("エンジン ready (stdio IPC, 完全オフライン)");
        Ok(())
    }

    pub fn is_ready(self: &Arc<Self>) -> bool {
        *self.ready.lock().unwrap()
    }

    pub async fn invoke(
        self: &Arc<Self>,
        cmd: &str,
        params: Value,
        cid: Option<u64>,
    ) -> Result<Value, String> {
        match self.invoke_sync(cmd, params.clone(), cid, true) {
            Ok(v) => Ok(v),
            Err(err) if Self::is_pipe_error(&err) => {
                Self::log(&format!("IPC 失敗、エンジン再起動: {err}"));
                self.restart().await?;
                self.invoke_sync(cmd, params, cid, false)
            }
            Err(err) => Err(err),
        }
    }

    fn invoke_sync(
        self: &Arc<Self>,
        cmd: &str,
        params: Value,
        cid: Option<u64>,
        _allow_restart: bool,
    ) -> Result<Value, String> {
        if !self.is_ready() {
            return Err("PKB エンジンが ready ではありません".to_string());
        }

        let id = REQ_COUNTER.fetch_add(1, Ordering::Relaxed);
        let request = json!({ "id": id, "cid": cid, "cmd": cmd, "params": params });

        let mut guard = self.process.lock().unwrap();
        let proc = guard
            .as_mut()
            .ok_or_else(|| "PKB エンジンが起動していません".to_string())?;

        if !child_alive(proc) {
            *self.ready.lock().unwrap() = false;
            return Err(
                "PKB エンジンが終了しました。アプリを再起動するか、操作をやり直してください。".to_string(),
            );
        }

        let payload = format!("{request}\n");
        if let Err(e) = proc.stdin.write_all(payload.as_bytes()) {
            *self.ready.lock().unwrap() = false;
            return Err(format!("エンジン書き込み失敗: {e}"));
        }
        if let Err(e) = proc.stdin.flush() {
            *self.ready.lock().unwrap() = false;
            return Err(format!("エンジン flush 失敗: {e}"));
        }

        // 最終応答 ({"ok": ...}) まで読み続け、途中のイベント行はフロントへ転送する
        loop {
            let response_line = read_line(&mut proc.stdout)?;
            let response: Value = serde_json::from_str(&response_line)
                .map_err(|e| format!("応答 JSON 解析失敗: {e} — {response_line}"))?;

            if response.get("ok").is_none() && response.get("event").is_some() {
                self.forward_event(&response);
                continue;
            }

            if !response.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                let err = response
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error");
                return Err(err.to_string());
            }

            return Ok(response.get("result").cloned().unwrap_or(Value::Null));
        }
    }
}

fn child_alive(proc: &mut EngineProcess) -> bool {
    match proc.child.try_wait() {
        Ok(Some(_)) => false,
        Ok(None) => true,
        Err(_) => false,
    }
}

fn read_line(reader: &mut BufReader<std::process::ChildStdout>) -> Result<String, String> {
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| format!("エンジン読み取り失敗: {e}"))?;
    if line.trim().is_empty() {
        return Err("エンジンが応答を返さず終了した可能性があります".to_string());
    }
    Ok(line)
}

fn engine_log_file(root: &std::path::Path) -> Option<std::fs::File> {
    let log_dir = root.join("logs");
    let _ = std::fs::create_dir_all(&log_dir);
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("engine.log"))
        .ok()
}

fn spawn_python_engine(root: &std::path::Path, script: &std::path::Path) -> Result<EngineProcess, String> {
    let python = find_python_executable()
        .ok_or_else(|| "Python が見つかりません。PKB_PYTHON を設定してください。".to_string())?;

    let mut cmd = Command::new(&python);
    cmd.arg("-u")
        .arg("-X")
        .arg("utf8")
        .arg(script)
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .env("PKB_ENGINE", "1")
        .env("PKB_PROJECT_ROOT", root.to_string_lossy().to_string())
        .current_dir(root.join("src").join("python"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());

    if let Some(log) = engine_log_file(root) {
        cmd.stderr(Stdio::from(log));
    } else {
        cmd.stderr(Stdio::null());
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    spawn_process(cmd)
}

fn spawn_bundled_engine(path: &std::path::Path, root: &std::path::Path) -> Result<EngineProcess, String> {
    let mut cmd = Command::new(path);
    cmd.env("PYTHONUTF8", "1")
        .env("PKB_ENGINE", "1")
        .env("PKB_PROJECT_ROOT", root.to_string_lossy().to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());

    if let Some(log) = engine_log_file(root) {
        cmd.stderr(Stdio::from(log));
    } else {
        cmd.stderr(Stdio::null());
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    spawn_process(cmd)
}

fn spawn_process(mut cmd: Command) -> Result<EngineProcess, String> {
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("エンジン起動失敗: {e}"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "stdin pipe を取得できません".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "stdout pipe を取得できません".to_string())?;
    Ok(EngineProcess {
        child,
        stdin,
        stdout: BufReader::new(stdout),
    })
}
