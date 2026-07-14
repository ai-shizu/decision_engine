use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::os_sandbox::ChildControl;
#[cfg(not(debug_assertions))]
use crate::os_sandbox::spawn_kernel_sandboxed;
use crate::paths::{ensure_data_layout, project_root};
#[cfg(not(debug_assertions))]
use crate::paths::bundled_engine_path;
#[cfg(debug_assertions)]
use crate::paths::{find_python_executable, run_engine_script};

static REQ_COUNTER: AtomicU64 = AtomicU64::new(1);

// ---------------------------------------------------------------------------
// Typed errors / policy (§2.2) — policy gating wired in STEP 4
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransportFailure {
    Write,
    Flush,
    Read,
    Eof,
    EngineExited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProtocolFailure {
    InvalidUtf8,
    InvalidJson,
    MalformedResponse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InvokeError {
    NotReady,
    Transport(TransportFailure),
    Protocol(ProtocolFailure),
    Remote(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReplayPolicy {
    RetryOnceAfterRestart,
    NoReplay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryAction {
    FailFast,
    RestartOnly,
    RestartThenRetryOnce,
}

const MSG_NOT_READY: &str = "PKB エンジンが ready ではありません";
const MSG_OUTCOME_UNKNOWN: &str =
    "エンジンとの通信が途切れました。処理が完了している可能性があります。状態を確認してから再実行してください。";
const MSG_ENGINE_UNAVAILABLE: &str =
    "PKB エンジンを再起動できませんでした。アプリを再起動してください。";
const MSG_RETRY_FAILED: &str =
    "エンジンとの通信に失敗しました。もう一度お試しください。";

pub(crate) fn replay_policy(cmd: &str) -> ReplayPolicy {
    match cmd {
        "health"
        | "settings.get"
        | "record.load"
        | "calendar.event_dates"
        | "import.stats"
        | "es.view"
        | "import.classify"
        | "oracle.payload"
        | "twin.forecast"
        | "profile.source_code"
        | "probe.status"
        | "context.manifest.latest" => ReplayPolicy::RetryOnceAfterRestart,
        _ => ReplayPolicy::NoReplay,
    }
}

pub(crate) fn recovery_action(
    err: &InvokeError,
    policy: ReplayPolicy,
) -> RecoveryAction {
    match err {
        InvokeError::Remote(_) | InvokeError::NotReady => RecoveryAction::FailFast,
        InvokeError::Transport(_) | InvokeError::Protocol(_) => match policy {
            ReplayPolicy::RetryOnceAfterRestart => RecoveryAction::RestartThenRetryOnce,
            ReplayPolicy::NoReplay => RecoveryAction::RestartOnly,
        },
    }
}

// ---------------------------------------------------------------------------
// Connection seam (§2.6)
// ---------------------------------------------------------------------------

pub(crate) trait EngineConnection: Send {
    fn send_line(&mut self, line: &str) -> Result<(), TransportFailure>;
    fn recv_line(&mut self) -> Result<String, InvokeError>;
    fn is_alive(&mut self) -> bool;
    fn kill(&mut self);
}

pub(crate) trait EngineConnector: Send + Sync {
    fn connect(&self) -> Result<Box<dyn EngineConnection>, String>;
}

pub(crate) type EventSink = Box<dyn Fn(&Value) + Send + Sync>;

struct ProcessConnection {
    child: Box<dyn ChildControl>,
    stdin: Box<dyn Write + Send>,
    stdout: BufReader<Box<dyn Read + Send>>,
    stderr_worker: Option<JoinHandle<()>>,
}

impl ProcessConnection {
    fn join_stderr_worker(&mut self) {
        if let Some(handle) = self.stderr_worker.take() {
            // Worker panic must not poison the engine process.
            let _ = handle.join();
        }
    }
}

impl Drop for ProcessConnection {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.join_stderr_worker();
    }
}

impl EngineConnection for ProcessConnection {
    fn send_line(&mut self, line: &str) -> Result<(), TransportFailure> {
        if let Err(_e) = self.stdin.write_all(line.as_bytes()) {
            return Err(TransportFailure::Write);
        }
        if let Err(_e) = self.stdin.flush() {
            return Err(TransportFailure::Flush);
        }
        Ok(())
    }

    fn recv_line(&mut self) -> Result<String, InvokeError> {
        let mut line = String::new();
        match self.stdout.read_line(&mut line) {
            Ok(0) => Err(InvokeError::Transport(TransportFailure::Eof)),
            Ok(_) => {
                if line.trim().is_empty() {
                    Err(InvokeError::Transport(TransportFailure::Eof))
                } else {
                    Ok(line)
                }
            }
            Err(e) if e.kind() == ErrorKind::InvalidData => {
                Err(InvokeError::Protocol(ProtocolFailure::InvalidUtf8))
            }
            Err(_) => Err(InvokeError::Transport(TransportFailure::Read)),
        }
    }

    fn is_alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(Some(_)) => false,
            Ok(None) => true,
            Err(_) => false,
        }
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.join_stderr_worker();
    }
}

struct ProcessConnector;

impl EngineConnector for ProcessConnector {
    fn connect(&self) -> Result<Box<dyn EngineConnection>, String> {
        let root = project_root();
        ensure_data_layout(&root);
        EngineManager::log(&format!("data root: {}", root.display()));
        let mut conn = spawn_configured_engine(&root)?;

        let ready_line = conn.recv_line().map_err(|e| format!("エンジン ready 読取失敗: {e:?}"))?;
        let ready: Value = serde_json::from_str(&ready_line)
            .map_err(|e| format!("エンジン ready 解析失敗: {e} — {ready_line}"))?;
        if ready.get("event").and_then(|v| v.as_str()) != Some("ready") {
            return Err(format!("エンジン ready 失敗: {ready_line}"));
        }

        EngineManager::log("エンジン ready (stdio IPC, 完全オフライン)");
        Ok(Box::new(conn))
    }
}

// ---------------------------------------------------------------------------
// EngineManager
// ---------------------------------------------------------------------------

pub struct EngineManager {
    connector: Mutex<Box<dyn EngineConnector>>,
    connection: Mutex<Option<Box<dyn EngineConnection>>>,
    ready: Mutex<bool>,
    restart_lock: Mutex<()>,
    event_sink: Mutex<Option<EventSink>>,
    app: Mutex<Option<AppHandle>>,
}

impl EngineManager {
    pub fn new() -> Arc<Self> {
        Self::with_connector(Box::new(ProcessConnector))
    }

    pub(crate) fn with_connector(connector: Box<dyn EngineConnector>) -> Arc<Self> {
        Arc::new(Self {
            connector: Mutex::new(connector),
            connection: Mutex::new(None),
            ready: Mutex::new(false),
            restart_lock: Mutex::new(()),
            event_sink: Mutex::new(None),
            app: Mutex::new(None),
        })
    }

    fn log(line: &str) {
        eprintln!("[PKB] {line}");
    }

    pub(crate) fn install_event_sink(self: &Arc<Self>, sink: EventSink) {
        *self.event_sink.lock().unwrap() = Some(sink);
    }

    fn forward_event(self: &Arc<Self>, payload: &Value) {
        if let Some(sink) = self.event_sink.lock().unwrap().as_ref() {
            sink(payload);
        }
    }

    pub fn is_ready(self: &Arc<Self>) -> bool {
        *self.ready.lock().unwrap()
    }

    fn kill_current(self: &Arc<Self>) {
        if let Some(mut conn) = self.connection.lock().unwrap().take() {
            conn.kill();
        }
    }

    pub(crate) fn boot(self: &Arc<Self>) -> Result<(), String> {
        let conn = self.connector.lock().unwrap().connect()?;
        *self.connection.lock().unwrap() = Some(conn);
        *self.ready.lock().unwrap() = true;
        Ok(())
    }

    /// Transport recovery restart: kill → connect only (no graceful shutdown write).
    pub(crate) fn restart_blocking(self: &Arc<Self>) -> Result<(), String> {
        let _lock = self.restart_lock.lock().unwrap();
        self.kill_current();
        *self.ready.lock().unwrap() = false;
        self.boot()
    }

    pub fn shutdown(self: &Arc<Self>) {
        if self.is_ready() {
            let params = json!({});
            let _ = self.invoke_sync("shutdown", &params, None);
        }
        self.kill_current();
        *self.ready.lock().unwrap() = false;
    }

    pub async fn start(self: &Arc<Self>, app: AppHandle) -> Result<(), String> {
        *self.app.lock().unwrap() = Some(app.clone());
        let app_for_sink = app;
        self.install_event_sink(Box::new(move |payload: &Value| {
            if let Err(e) = app_for_sink.emit("pkb-engine-event", payload) {
                EngineManager::log(&format!("イベント転送失敗: {e}"));
            }
        }));
        let _lock = self.restart_lock.lock().unwrap();
        // Graceful stop of any prior process, then boot (same as legacy start).
        if self.is_ready() {
            let params = json!({});
            let _ = self.invoke_sync("shutdown", &params, None);
        }
        self.kill_current();
        *self.ready.lock().unwrap() = false;
        self.boot()
    }

    pub async fn restart(self: &Arc<Self>) -> Result<(), String> {
        self.restart_blocking()
    }

    pub async fn invoke(
        self: &Arc<Self>,
        cmd: &str,
        params: Value,
        cid: Option<u64>,
    ) -> Result<Value, String> {
        self.invoke_blocking(cmd, params, cid)
    }

    /// Transport/Protocol → `recovery_action` (RestartOnly | RestartThenRetryOnce).
    /// Remote / NotReady → FailFast (no restart). At most one restart and one resend.
    pub(crate) fn invoke_blocking(
        self: &Arc<Self>,
        cmd: &str,
        params: Value,
        cid: Option<u64>,
    ) -> Result<Value, String> {
        match self.invoke_sync(cmd, &params, cid) {
            Ok(v) => Ok(v),
            Err(InvokeError::Remote(msg)) => Err(msg),
            Err(InvokeError::NotReady) => Err(MSG_NOT_READY.to_string()),
            Err(err @ (InvokeError::Transport(_) | InvokeError::Protocol(_))) => {
                match &err {
                    InvokeError::Transport(t) => {
                        Self::log(&format!("IPC Transport({t:?}), エンジン再起動"));
                    }
                    InvokeError::Protocol(p) => {
                        Self::log(&format!("IPC Protocol({p:?}), エンジン再起動"));
                    }
                    _ => {}
                }
                match recovery_action(&err, replay_policy(cmd)) {
                    RecoveryAction::FailFast => Err(MSG_RETRY_FAILED.to_string()),
                    RecoveryAction::RestartOnly => match self.restart_blocking() {
                        Ok(()) => Err(MSG_OUTCOME_UNKNOWN.to_string()),
                        Err(_) => Err(MSG_ENGINE_UNAVAILABLE.to_string()),
                    },
                    RecoveryAction::RestartThenRetryOnce => match self.restart_blocking() {
                        Err(_) => Err(MSG_ENGINE_UNAVAILABLE.to_string()),
                        Ok(()) => match self.invoke_sync(cmd, &params, cid) {
                            Ok(v) => Ok(v),
                            Err(InvokeError::Remote(msg)) => Err(msg),
                            Err(InvokeError::NotReady) => Err(MSG_NOT_READY.to_string()),
                            Err(InvokeError::Transport(_)) | Err(InvokeError::Protocol(_)) => {
                                Err(MSG_RETRY_FAILED.to_string())
                            }
                        },
                    },
                }
            }
        }
    }

    pub(crate) fn invoke_sync(
        self: &Arc<Self>,
        cmd: &str,
        params: &Value,
        cid: Option<u64>,
    ) -> Result<Value, InvokeError> {
        if !self.is_ready() {
            return Err(InvokeError::NotReady);
        }

        let id = REQ_COUNTER.fetch_add(1, Ordering::Relaxed);
        let request = json!({ "id": id, "cid": cid, "cmd": cmd, "params": params });
        let payload = format!("{request}\n");

        let mut guard = self.connection.lock().unwrap();
        let conn = match guard.as_mut() {
            Some(c) => c,
            None => {
                *self.ready.lock().unwrap() = false;
                return Err(InvokeError::NotReady);
            }
        };

        if !conn.is_alive() {
            *self.ready.lock().unwrap() = false;
            return Err(InvokeError::Transport(TransportFailure::EngineExited));
        }

        if let Err(t) = conn.send_line(&payload) {
            *self.ready.lock().unwrap() = false;
            return Err(InvokeError::Transport(t));
        }

        // Final response ({"ok": ...}); forward intermediate event lines.
        loop {
            let response_line = match conn.recv_line() {
                Ok(line) => line,
                Err(e) => {
                    match &e {
                        InvokeError::Transport(_) | InvokeError::Protocol(_) => {
                            *self.ready.lock().unwrap() = false;
                        }
                        InvokeError::NotReady | InvokeError::Remote(_) => {}
                    }
                    return Err(e);
                }
            };

            let response: Value = match serde_json::from_str(&response_line) {
                Ok(v) => v,
                Err(_) => {
                    *self.ready.lock().unwrap() = false;
                    return Err(InvokeError::Protocol(ProtocolFailure::InvalidJson));
                }
            };

            if response.get("ok").is_none() && response.get("event").is_some() {
                self.forward_event(&response);
                continue;
            }

            if response.get("ok").is_none() && response.get("event").is_none() {
                *self.ready.lock().unwrap() = false;
                return Err(InvokeError::Protocol(ProtocolFailure::MalformedResponse));
            }

            if !response.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                let err = response
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error")
                    .to_string();
                return Err(InvokeError::Remote(err));
            }

            return Ok(response.get("result").cloned().unwrap_or(Value::Null));
        }
    }
}

#[cfg(debug_assertions)]
fn spawn_configured_engine(root: &Path) -> Result<ProcessConnection, String> {
    if std::env::var("PKB_UNSAFE_DEV_ENGINE").as_deref() != Ok("1") {
        return Err(
            "System Python engine is disabled. Set PKB_UNSAFE_DEV_ENGINE=1 only for explicit insecure development."
                .to_string(),
        );
    }
    let script = run_engine_script();
    if !script.is_file() {
        return Err(format!(
            "run_engine.py が見つかりません: {}",
            script.display()
        ));
    }
    EngineManager::log(
        "UNSAFE DEVELOPMENT ENGINE: system Python has no kernel sandbox guarantee",
    );
    spawn_python_engine(root, &script)
}

#[cfg(not(debug_assertions))]
fn spawn_configured_engine(root: &Path) -> Result<ProcessConnection, String> {
    let path = bundled_engine_path().ok_or_else(|| {
        "Bundled PKB engine is required in production; fallback is forbidden.".to_string()
    })?;
    EngineManager::log(&format!("release: {}", path.display()));
    spawn_bundled_engine(&path, root)
}

#[cfg(debug_assertions)]
fn spawn_python_engine(
    root: &std::path::Path,
    script: &std::path::Path,
) -> Result<ProcessConnection, String> {
    let python = find_python_executable()
        .ok_or_else(|| "Python が見つかりません。PKB_PYTHON を設定してください。".to_string())?;

    let mut cmd = Command::new(&python);
    configure_offline_child_environment(&mut cmd, root);
    cmd.arg("-u")
        .arg("-X")
        .arg("utf8")
        .arg(script)
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .current_dir(root.join("src").join("python"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    spawn_process(cmd, root)
}

#[cfg(not(debug_assertions))]
fn spawn_bundled_engine(
    path: &std::path::Path,
    root: &std::path::Path,
) -> Result<ProcessConnection, String> {
    let mut cmd = Command::new(path);
    configure_offline_child_environment(&mut cmd, root);
    cmd.env("PYTHONUTF8", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let spawned = spawn_kernel_sandboxed(cmd, root)
        .map_err(|e| format!("PKB engine kernel sandbox unavailable: {e}"))?;
    spawn_connection(
        spawned.child,
        spawned.stdin,
        spawned.stdout,
        spawned.stderr,
        root,
    )
}

fn configure_offline_child_environment(cmd: &mut Command, root: &Path) {
    const SAFE_HOST_ENV: &[&str] = &[
        "APPDATA",
        "HOME",
        "HOMEDRIVE",
        "HOMEPATH",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "LOCALAPPDATA",
        "PROGRAMDATA",
        "SystemRoot",
        "TEMP",
        "TMP",
        "TMPDIR",
        "TZ",
        "USERPROFILE",
        "WINDIR",
    ];

    cmd.env_clear();
    for key in SAFE_HOST_ENV {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    cmd.env("HF_HUB_OFFLINE", "1")
        .env("TRANSFORMERS_OFFLINE", "1")
        .env("HF_DATASETS_OFFLINE", "1")
        .env("HF_HUB_DISABLE_TELEMETRY", "1")
        .env("DO_NOT_TRACK", "1")
        .env("LLAMA_ARG_OFFLINE", "1")
        .env("NO_PROXY", "*")
        .env("no_proxy", "*")
        .env("PKB_ENGINE", "1")
        .env("PKB_PROJECT_ROOT", root.to_string_lossy().to_string());
}

#[cfg(debug_assertions)]
fn spawn_process(mut cmd: Command, root: &Path) -> Result<ProcessConnection, String> {
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
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "stderr pipe を取得できません".to_string())?;

    let log_root = root.to_path_buf();
    let stderr_worker = thread::spawn(move || {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            collect_engine_stderr(&log_root, stderr, EngineLogPolicy::production());
        }));
    });

    Ok(ProcessConnection {
        child: Box::new(child),
        stdin: Box::new(stdin),
        stdout: BufReader::new(Box::new(stdout)),
        stderr_worker: Some(stderr_worker),
    })
}

#[cfg(not(debug_assertions))]
fn spawn_connection(
    child: Box<dyn ChildControl>,
    stdin: Box<dyn Write + Send>,
    stdout: Box<dyn Read + Send>,
    stderr: Box<dyn Read + Send>,
    root: &Path,
) -> Result<ProcessConnection, String> {
    let log_root = root.to_path_buf();
    let stderr_worker = thread::spawn(move || {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            collect_engine_stderr(&log_root, stderr, EngineLogPolicy::production());
        }));
    });

    Ok(ProcessConnection {
        child,
        stdin,
        stdout: BufReader::new(stdout),
        stderr_worker: Some(stderr_worker),
    })
}

// ---------------------------------------------------------------------------
// Finding 12 — finite sterile engine diagnostic log
// ---------------------------------------------------------------------------

pub(crate) const ENGINE_LOG_MAX_BYTES: u64 = 1_048_576; // 1 MiB per file
pub(crate) const ENGINE_LOG_BACKUP_COUNT: usize = 2;
pub(crate) const ENGINE_LOG_HEADER: &str = "[PKB_ENGINE_LOG_V1]\n";
pub(crate) const ENGINE_DIAG_LINE: &str = "[PKB_DIAG_V1] REQUEST_FAILED";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EngineLogPolicy {
    pub max_bytes: u64,
    pub backup_count: usize,
}

impl EngineLogPolicy {
    pub(crate) fn production() -> Self {
        Self {
            max_bytes: ENGINE_LOG_MAX_BYTES,
            backup_count: ENGINE_LOG_BACKUP_COUNT,
        }
    }
}

fn owned_log_path(dir: &Path, index: usize) -> PathBuf {
    if index == 0 {
        dir.join("engine.log")
    } else {
        dir.join(format!("engine.log.{index}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OwnedPathKind {
    Absent,
    Symlink,
    RegularFile,
    Other,
}

/// Existence/type must use symlink_metadata only — never Path::exists()
/// (dangling symlink looks Absent to exists(), then create follows the link).
fn probe_owned_path(path: &Path) -> OwnedPathKind {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == ErrorKind::NotFound => OwnedPathKind::Absent,
        Err(_) => OwnedPathKind::Other,
        Ok(m) if m.file_type().is_symlink() => OwnedPathKind::Symlink,
        Ok(m) if m.file_type().is_file() => OwnedPathKind::RegularFile,
        Ok(_) => OwnedPathKind::Other,
    }
}

fn remove_owned_path(path: &Path) {
    match probe_owned_path(path) {
        OwnedPathKind::Absent => {}
        OwnedPathKind::Symlink | OwnedPathKind::RegularFile => {
            let _ = std::fs::remove_file(path);
        }
        OwnedPathKind::Other => {
            let _ = std::fs::remove_file(path);
            let _ = std::fs::remove_dir(path);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenPlan {
    Append,
    CreateFresh,
    /// Symlink / other / unsafe file: never treat as Absent (exists() trap).
    ReplaceFresh,
}

fn open_plan(kind: OwnedPathKind, is_safe_regular: bool) -> OpenPlan {
    match kind {
        OwnedPathKind::RegularFile if is_safe_regular => OpenPlan::Append,
        OwnedPathKind::Absent => OpenPlan::CreateFresh,
        OwnedPathKind::Symlink | OwnedPathKind::Other | OwnedPathKind::RegularFile => {
            OpenPlan::ReplaceFresh
        }
    }
}

fn is_safe_v1_engine_log(path: &Path, policy: EngineLogPolicy) -> bool {
    match probe_owned_path(path) {
        OwnedPathKind::RegularFile => {}
        _ => return false,
    }
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    if meta.len() > policy.max_bytes {
        return false;
    }
    // Size is capped by max_bytes — safe to read fully for exact allowlist validation.
    let contents = match std::fs::read(path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    if contents.len() as u64 > policy.max_bytes {
        return false;
    }
    let header = ENGINE_LOG_HEADER.as_bytes();
    if !contents.starts_with(header) {
        return false;
    }
    let diag_line = {
        let mut v = ENGINE_DIAG_LINE.as_bytes().to_vec();
        v.push(b'\n');
        v
    };
    let mut rest = &contents[header.len()..];
    while !rest.is_empty() {
        if rest.len() < diag_line.len() || &rest[..diag_line.len()] != diag_line.as_slice() {
            return false;
        }
        rest = &rest[diag_line.len()..];
    }
    true
}

fn reset_unsafe_owned_logs(dir: &Path, policy: EngineLogPolicy) {
    for i in 0..=policy.backup_count {
        let path = owned_log_path(dir, i);
        match probe_owned_path(&path) {
            OwnedPathKind::Absent => {}
            OwnedPathKind::RegularFile => {
                if !is_safe_v1_engine_log(&path, policy) {
                    remove_owned_path(&path);
                }
            }
            OwnedPathKind::Symlink | OwnedPathKind::Other => {
                remove_owned_path(&path);
            }
        }
    }
}

struct EngineLogWriter {
    dir: PathBuf,
    policy: EngineLogPolicy,
    file: Option<std::fs::File>,
    size: u64,
}

impl EngineLogWriter {
    fn create_fresh(path: &Path) -> Result<(std::fs::File, u64), ()> {
        remove_owned_path(path);
        if probe_owned_path(path) != OwnedPathKind::Absent {
            return Err(());
        }
        let mut created = std::fs::File::create(path).map_err(|_| ())?;
        created
            .write_all(ENGINE_LOG_HEADER.as_bytes())
            .map_err(|_| ())?;
        created.flush().map_err(|_| ())?;
        let file = std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .map_err(|_| ())?;
        Ok((file, ENGINE_LOG_HEADER.len() as u64))
    }

    fn open(dir: &Path, policy: EngineLogPolicy) -> Result<Self, ()> {
        if policy.max_bytes < ENGINE_LOG_HEADER.len() as u64 + ENGINE_DIAG_LINE.len() as u64 + 1 {
            // One record must never exceed the cap under the configured policy.
            return Err(());
        }
        std::fs::create_dir_all(dir).map_err(|_| ())?;
        reset_unsafe_owned_logs(dir, policy);
        let path = owned_log_path(dir, 0);
        let kind = probe_owned_path(&path);
        let safe = kind == OwnedPathKind::RegularFile && is_safe_v1_engine_log(&path, policy);
        let (file, size) = match open_plan(kind, safe) {
            OpenPlan::Append => {
                let size = std::fs::symlink_metadata(&path).map_err(|_| ())?.len();
                let file = std::fs::OpenOptions::new()
                    .append(true)
                    .open(&path)
                    .map_err(|_| ())?;
                (file, size)
            }
            OpenPlan::CreateFresh | OpenPlan::ReplaceFresh => Self::create_fresh(&path)?,
        };
        Ok(Self {
            dir: dir.to_path_buf(),
            policy,
            file: Some(file),
            size,
        })
    }

    fn rotate(&mut self) -> Result<(), ()> {
        // Close handle before rename (required on Windows).
        self.file = None;

        let last = owned_log_path(&self.dir, self.policy.backup_count);
        remove_owned_path(&last);
        for i in (1..self.policy.backup_count).rev() {
            let from = owned_log_path(&self.dir, i);
            let to = owned_log_path(&self.dir, i + 1);
            if probe_owned_path(&from) == OwnedPathKind::RegularFile {
                let _ = std::fs::rename(&from, &to);
            }
        }
        let current = owned_log_path(&self.dir, 0);
        let backup1 = owned_log_path(&self.dir, 1);
        if probe_owned_path(&current) == OwnedPathKind::RegularFile {
            std::fs::rename(&current, &backup1).map_err(|_| ())?;
        }
        let (file, size) = Self::create_fresh(&current)?;
        self.file = Some(file);
        self.size = size;
        Ok(())
    }

    fn append_diag(&mut self) -> Result<(), ()> {
        let record = format!("{ENGINE_DIAG_LINE}\n");
        let record_len = record.len() as u64;
        if record_len > self.policy.max_bytes {
            return Err(());
        }
        if self.size + record_len > self.policy.max_bytes {
            self.rotate()?;
        }
        if self.size + record_len > self.policy.max_bytes {
            return Err(());
        }
        let file = self.file.as_mut().ok_or(())?;
        file.write_all(record.as_bytes()).map_err(|_| ())?;
        file.flush().map_err(|_| ())?;
        self.size += record_len;
        Ok(())
    }
}

/// Fixed-memory exact allowlist matcher for `[PKB_DIAG_V1] REQUEST_FAILED`.
/// LF or CRLF terminators only; CR inside the marker is rejected.
struct DiagLineMatcher {
    matched: usize,
    seen_cr_after_match: bool,
    discard_until_nl: bool,
}

impl DiagLineMatcher {
    fn new() -> Self {
        Self {
            matched: 0,
            seen_cr_after_match: false,
            discard_until_nl: false,
        }
    }

    fn reset_line(&mut self) {
        self.matched = 0;
        self.seen_cr_after_match = false;
    }

    /// Returns true when one exact allowlisted line was recognized.
    fn feed(&mut self, byte: u8) -> bool {
        if self.discard_until_nl {
            if byte == b'\n' {
                self.discard_until_nl = false;
                self.reset_line();
            }
            return false;
        }

        let expected = ENGINE_DIAG_LINE.as_bytes();

        if self.seen_cr_after_match {
            // Only CRLF terminator: CR must be immediately followed by LF.
            if byte == b'\n' {
                self.reset_line();
                return true;
            }
            self.discard_until_nl = true;
            self.reset_line();
            return false;
        }

        if byte == b'\r' {
            if self.matched == expected.len() {
                // CRLF terminator only after full marker.
                self.seen_cr_after_match = true;
            } else {
                // CR inside marker (or before completion) — reject.
                self.discard_until_nl = true;
                self.reset_line();
            }
            return false;
        }

        if byte == b'\n' {
            let ok = self.matched == expected.len();
            self.reset_line();
            return ok;
        }

        if self.matched < expected.len() && byte == expected[self.matched] {
            self.matched += 1;
            return false;
        }

        self.discard_until_nl = true;
        self.reset_line();
        false
    }
}

/// Drain child stderr; persist only exact allowlisted diagnostics under a bounded log.
pub(crate) fn collect_engine_stderr<R: Read>(root: &Path, mut reader: R, policy: EngineLogPolicy) {
    let logs_dir = root.join("logs");
    let mut writer = EngineLogWriter::open(&logs_dir, policy).ok();
    let mut matcher = DiagLineMatcher::new();
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                for &b in &buf[..n] {
                    if matcher.feed(b) {
                        if let Some(w) = writer.as_mut() {
                            if w.append_diag().is_err() {
                                writer = None; // fail-closed persist; keep draining
                            }
                        }
                    }
                }
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod engine_tests;
