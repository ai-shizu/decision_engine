//! Fault-injection tests for IPC at-most-once recovery (Finding 2).
//! Fake only: std::sync + serde_json. No real Python / spawn / data.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use super::*;

// ---------------------------------------------------------------------------
// Fake seam
// ---------------------------------------------------------------------------

#[allow(dead_code)]
enum RecvStep {
    Line(String),
    FailTransport(TransportFailure),
    FailProtocolUtf8,
    Block(mpsc::Receiver<()>),
}

struct FakeConnection {
    recv_script: VecDeque<RecvStep>,
    send_results: VecDeque<Result<(), TransportFailure>>,
    sent: Arc<Mutex<Vec<String>>>,
    alive: bool,
    killed: Arc<AtomicBool>,
}

impl EngineConnection for FakeConnection {
    fn send_line(&mut self, line: &str) -> Result<(), TransportFailure> {
        self.sent.lock().unwrap().push(line.to_string());
        self.send_results.pop_front().unwrap_or(Ok(()))
    }

    fn recv_line(&mut self) -> Result<String, InvokeError> {
        match self.recv_script.pop_front() {
            Some(RecvStep::Line(s)) => Ok(s),
            Some(RecvStep::FailTransport(t)) => Err(InvokeError::Transport(t)),
            Some(RecvStep::FailProtocolUtf8) => {
                Err(InvokeError::Protocol(ProtocolFailure::InvalidUtf8))
            }
            Some(RecvStep::Block(rx)) => {
                let _ = rx.recv();
                match self.recv_script.pop_front() {
                    Some(RecvStep::Line(s)) => Ok(s),
                    Some(RecvStep::FailTransport(t)) => Err(InvokeError::Transport(t)),
                    Some(RecvStep::FailProtocolUtf8) => {
                        Err(InvokeError::Protocol(ProtocolFailure::InvalidUtf8))
                    }
                    Some(RecvStep::Block(_)) => {
                        panic!("nested Block not supported in fake script")
                    }
                    None => Err(InvokeError::Transport(TransportFailure::Eof)),
                }
            }
            None => Err(InvokeError::Transport(TransportFailure::Eof)),
        }
    }

    fn is_alive(&mut self) -> bool {
        self.alive
    }

    fn kill(&mut self) {
        self.killed.store(true, Ordering::SeqCst);
        self.alive = false;
    }
}

struct FakeConnector {
    queue: Mutex<VecDeque<Result<FakeConnection, String>>>,
    connects: Arc<AtomicUsize>,
}

impl EngineConnector for FakeConnector {
    fn connect(&self) -> Result<Box<dyn EngineConnection>, String> {
        self.connects.fetch_add(1, Ordering::SeqCst);
        let next = self
            .queue
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err("FakeConnector: no more connections".to_string()));
        match next {
            Ok(conn) => Ok(Box::new(conn)),
            Err(e) => Err(e),
        }
    }
}

fn okline(id: u64) -> String {
    json!({ "id": id, "ok": true, "result": {} }).to_string()
}

fn remote_line(id: u64, error: &str) -> String {
    json!({ "id": id, "ok": false, "error": error }).to_string()
}

fn event_line(id: u64, event: &str, message: &str) -> String {
    json!({ "id": id, "event": event, "message": message }).to_string()
}

fn count_cmd(sent: &Mutex<Vec<String>>, cmd: &str) -> usize {
    sent.lock()
        .unwrap()
        .iter()
        .filter(|line| {
            serde_json::from_str::<Value>(line)
                .ok()
                .and_then(|v| v.get("cmd").and_then(|c| c.as_str()).map(|s| s == cmd))
                .unwrap_or(false)
        })
        .count()
}

fn parse_sent(line: &str) -> Value {
    serde_json::from_str(line).expect("sent line must be JSON")
}

fn boot_with(
    conns: Vec<Result<FakeConnection, String>>,
    connects: Arc<AtomicUsize>,
) -> Arc<EngineManager> {
    let connector = FakeConnector {
        queue: Mutex::new(VecDeque::from(conns)),
        connects,
    };
    let mgr = EngineManager::with_connector(Box::new(connector));
    mgr.boot().expect("boot");
    assert!(mgr.is_ready());
    mgr
}

fn make_conn(
    recv: Vec<RecvStep>,
    send_results: Vec<Result<(), TransportFailure>>,
    sent: Arc<Mutex<Vec<String>>>,
    killed: Arc<AtomicBool>,
) -> FakeConnection {
    FakeConnection {
        recv_script: VecDeque::from(recv),
        send_results: VecDeque::from(send_results),
        sent,
        alive: true,
        killed,
    }
}

// ---------------------------------------------------------------------------
// Tests 1–8: NoReplay after transport/protocol failure
// ---------------------------------------------------------------------------

#[test]
fn record_save_not_resent_after_write_failure() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let conn1 = make_conn(
        vec![],
        vec![Err(TransportFailure::Write)],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let conn2 = make_conn(
        vec![RecvStep::Line(okline(99))],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(vec![Ok(conn1), Ok(conn2)], Arc::clone(&connects));
    let err = mgr
        .invoke_blocking("record.save", json!({"date": "2026-07-12"}), None)
        .expect_err("must fail");
    assert_eq!(err, MSG_OUTCOME_UNKNOWN);
    assert_eq!(count_cmd(&sent, "record.save"), 1);
    assert_eq!(connects.load(Ordering::SeqCst), 2);
}

#[test]
fn consult_not_resent_after_read_failure() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let conn1 = make_conn(
        vec![RecvStep::FailTransport(TransportFailure::Read)],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let conn2 = make_conn(
        vec![RecvStep::Line(okline(99))],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(vec![Ok(conn1), Ok(conn2)], Arc::clone(&connects));
    let err = mgr
        .invoke_blocking("consult", json!({"query": "q"}), None)
        .expect_err("must fail");
    assert_eq!(err, MSG_OUTCOME_UNKNOWN);
    assert_eq!(count_cmd(&sent, "consult"), 1);
    assert_eq!(connects.load(Ordering::SeqCst), 2);
}

#[test]
fn probe_answer_not_resent_after_eof() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let conn1 = make_conn(
        vec![RecvStep::FailTransport(TransportFailure::Eof)],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let conn2 = make_conn(
        vec![RecvStep::Line(okline(99))],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(vec![Ok(conn1), Ok(conn2)], Arc::clone(&connects));
    let err = mgr
        .invoke_blocking(
            "probe.answer",
            json!({"session_id": "s", "question_id": "q", "answer": "a", "today": "2026-07-12"}),
            None,
        )
        .expect_err("must fail");
    assert_eq!(err, MSG_OUTCOME_UNKNOWN);
    assert_eq!(count_cmd(&sent, "probe.answer"), 1);
    assert_eq!(connects.load(Ordering::SeqCst), 2);
}

#[test]
fn calendar_sync_not_resent_after_partial_write() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    // send Ok (request reached) then recv Eof (response lost)
    let conn1 = make_conn(
        vec![RecvStep::FailTransport(TransportFailure::Eof)],
        vec![Ok(())],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let conn2 = make_conn(
        vec![RecvStep::Line(okline(99))],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(vec![Ok(conn1), Ok(conn2)], Arc::clone(&connects));
    let err = mgr
        .invoke_blocking("calendar.sync", json!({"source": "ics"}), None)
        .expect_err("must fail");
    assert_eq!(err, MSG_OUTCOME_UNKNOWN);
    assert_eq!(count_cmd(&sent, "calendar.sync"), 1);
    assert_eq!(connects.load(Ordering::SeqCst), 2);
}

#[test]
fn import_line_not_resent_after_protocol_failure() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let conn1 = make_conn(
        vec![RecvStep::Line("not json".to_string())],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let conn2 = make_conn(
        vec![RecvStep::Line(okline(99))],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(vec![Ok(conn1), Ok(conn2)], Arc::clone(&connects));
    let err = mgr
        .invoke_blocking("import.line", json!({"content": "x"}), None)
        .expect_err("must fail");
    assert_eq!(err, MSG_OUTCOME_UNKNOWN);
    assert_eq!(count_cmd(&sent, "import.line"), 1);
    assert_eq!(connects.load(Ordering::SeqCst), 2);
}

#[test]
fn noreplay_still_restarts_exactly_once() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let conn1 = make_conn(
        vec![],
        vec![Err(TransportFailure::Write)],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let conn2 = make_conn(
        vec![RecvStep::Line(okline(99))],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(vec![Ok(conn1), Ok(conn2)], Arc::clone(&connects));
    let _ = mgr.invoke_blocking("record.save", json!({}), None);
    assert_eq!(connects.load(Ordering::SeqCst), 2);
}

#[test]
fn noreplay_returns_outcome_unknown_after_restart_success() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let conn1 = make_conn(
        vec![],
        vec![Err(TransportFailure::Write)],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let conn2 = make_conn(
        vec![RecvStep::Line(okline(99))],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(vec![Ok(conn1), Ok(conn2)], Arc::clone(&connects));
    let err = mgr
        .invoke_blocking("record.save", json!({}), None)
        .expect_err("must fail");
    assert_eq!(err, MSG_OUTCOME_UNKNOWN);
}

#[test]
fn noreplay_restart_failure_no_resend() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let conn1 = make_conn(
        vec![],
        vec![Err(TransportFailure::Write)],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(
        vec![Ok(conn1), Err("spawn失敗".to_string())],
        Arc::clone(&connects),
    );
    let err = mgr
        .invoke_blocking("record.save", json!({}), None)
        .expect_err("must fail");
    assert_eq!(err, MSG_ENGINE_UNAVAILABLE);
    assert_eq!(count_cmd(&sent, "record.save"), 1);
    assert_eq!(connects.load(Ordering::SeqCst), 2);
}

// ---------------------------------------------------------------------------
// Tests 9–10: RetryOnceAfterRestart
// ---------------------------------------------------------------------------

#[test]
fn retry_safe_resent_exactly_once_after_restart() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let conn1 = make_conn(
        vec![RecvStep::FailTransport(TransportFailure::Eof)],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let conn2 = make_conn(
        vec![RecvStep::Line(okline(2))],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(vec![Ok(conn1), Ok(conn2)], Arc::clone(&connects));
    let ok = mgr
        .invoke_blocking("record.load", json!({"date": "2026-07-12"}), None)
        .expect("ok");
    assert_eq!(ok, json!({}));
    assert_eq!(count_cmd(&sent, "record.load"), 2);
    assert_eq!(connects.load(Ordering::SeqCst), 2);
}

#[test]
fn retry_safe_second_failure_no_third_attempt() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let conn1 = make_conn(
        vec![RecvStep::FailTransport(TransportFailure::Eof)],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let conn2 = make_conn(
        vec![RecvStep::FailTransport(TransportFailure::Eof)],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let conn3 = make_conn(
        vec![RecvStep::Line(okline(99))],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(
        vec![Ok(conn1), Ok(conn2), Ok(conn3)],
        Arc::clone(&connects),
    );
    let err = mgr
        .invoke_blocking("record.load", json!({"date": "2026-07-12"}), None)
        .expect_err("must fail");
    assert_eq!(err, MSG_RETRY_FAILED);
    assert_eq!(count_cmd(&sent, "record.load"), 2);
    assert_eq!(connects.load(Ordering::SeqCst), 2);
}

// ---------------------------------------------------------------------------
// Tests 11–12: Remote — no restart / no retry
// ---------------------------------------------------------------------------

#[test]
fn remote_error_no_restart() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let conn1 = make_conn(
        vec![RecvStep::Line(remote_line(1, "ValueError: x"))],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(vec![Ok(conn1)], Arc::clone(&connects));
    let err = mgr
        .invoke_blocking("record.save", json!({}), None)
        .expect_err("remote");
    assert_eq!(err, "ValueError: x");
    assert_eq!(count_cmd(&sent, "record.save"), 1);
    assert_eq!(connects.load(Ordering::SeqCst), 1);
}

#[test]
fn remote_error_no_retry() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let conn1 = make_conn(
        vec![RecvStep::Line(remote_line(1, "ValueError: x"))],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(vec![Ok(conn1)], Arc::clone(&connects));
    let err = mgr
        .invoke_blocking("record.load", json!({"date": "2026-07-12"}), None)
        .expect_err("remote");
    assert_eq!(err, "ValueError: x");
    assert_eq!(count_cmd(&sent, "record.load"), 1);
    assert_eq!(connects.load(Ordering::SeqCst), 1);
}

// ---------------------------------------------------------------------------
// Tests 13–15: unknown / shutdown NoReplay
// ---------------------------------------------------------------------------

#[test]
fn unknown_command_is_noreplay() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let conn1 = make_conn(
        vec![RecvStep::FailTransport(TransportFailure::Eof)],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let conn2 = make_conn(
        vec![RecvStep::Line(okline(99))],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(vec![Ok(conn1), Ok(conn2)], Arc::clone(&connects));
    let err = mgr
        .invoke_blocking("totally.unknown", json!({}), None)
        .expect_err("must fail");
    assert_eq!(err, MSG_OUTCOME_UNKNOWN);
    assert_eq!(count_cmd(&sent, "totally.unknown"), 1);
    assert_eq!(connects.load(Ordering::SeqCst), 2);
}

#[test]
fn new_command_default_noreplay_unit() {
    assert_eq!(replay_policy("future.cmd"), ReplayPolicy::NoReplay);
}

#[test]
fn shutdown_cmd_not_resent() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let conn1 = make_conn(
        vec![RecvStep::FailTransport(TransportFailure::Eof)],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let conn2 = make_conn(
        vec![RecvStep::Line(okline(99))],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(vec![Ok(conn1), Ok(conn2)], Arc::clone(&connects));
    let err = mgr
        .invoke_blocking("shutdown", json!({}), None)
        .expect_err("must fail");
    assert_eq!(err, MSG_OUTCOME_UNKNOWN);
    assert_eq!(count_cmd(&sent, "shutdown"), 1);
}

// ---------------------------------------------------------------------------
// Tests 16–19: retry identity + no secret leakage
// ---------------------------------------------------------------------------

#[test]
fn retry_preserves_params_and_cid() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let params = json!({"date": "2026-07-12"});
    let conn1 = make_conn(
        vec![RecvStep::FailTransport(TransportFailure::Eof)],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let conn2 = make_conn(
        vec![RecvStep::Line(okline(2))],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(vec![Ok(conn1), Ok(conn2)], Arc::clone(&connects));
    let _ = mgr
        .invoke_blocking("record.load", params.clone(), Some(7))
        .expect("ok");
    let lines = sent.lock().unwrap().clone();
    assert_eq!(lines.len(), 2);
    let a = parse_sent(&lines[0]);
    let b = parse_sent(&lines[1]);
    assert_eq!(a.get("params"), Some(&params));
    assert_eq!(b.get("params"), Some(&params));
    assert_eq!(a.get("cid"), Some(&json!(7)));
    assert_eq!(b.get("cid"), Some(&json!(7)));
}

#[test]
fn retry_uses_fresh_request_id() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let conn1 = make_conn(
        vec![RecvStep::FailTransport(TransportFailure::Eof)],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let conn2 = make_conn(
        vec![RecvStep::Line(okline(2))],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(vec![Ok(conn1), Ok(conn2)], Arc::clone(&connects));
    let _ = mgr
        .invoke_blocking("record.load", json!({"date": "2026-07-12"}), None)
        .expect("ok");
    let lines = sent.lock().unwrap().clone();
    assert_eq!(lines.len(), 2);
    let id0 = parse_sent(&lines[0])
        .get("id")
        .and_then(|v| v.as_u64())
        .unwrap();
    let id1 = parse_sent(&lines[1])
        .get("id")
        .and_then(|v| v.as_u64())
        .unwrap();
    assert_ne!(id0, id1);
    assert!(id1 > id0);
}

#[test]
fn outcome_unknown_error_carries_no_params() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let conn1 = make_conn(
        vec![],
        vec![Err(TransportFailure::Write)],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let conn2 = make_conn(
        vec![RecvStep::Line(okline(99))],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(vec![Ok(conn1), Ok(conn2)], Arc::clone(&connects));
    let err = mgr
        .invoke_blocking("record.save", json!({"secret": "SECRET_MARKER"}), None)
        .expect_err("must fail");
    assert_eq!(err, MSG_OUTCOME_UNKNOWN);
    assert!(!err.contains("SECRET_MARKER"));
}

#[test]
fn restart_failure_error_carries_no_params() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let conn1 = make_conn(
        vec![],
        vec![Err(TransportFailure::Write)],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(
        vec![Ok(conn1), Err("spawn失敗".to_string())],
        Arc::clone(&connects),
    );
    let err = mgr
        .invoke_blocking("record.save", json!({"secret": "SECRET_MARKER"}), None)
        .expect_err("must fail");
    assert_eq!(err, MSG_ENGINE_UNAVAILABLE);
    assert!(!err.contains("SECRET_MARKER"));
}

// ---------------------------------------------------------------------------
// Tests 20–23: happy path + policy table + typed remote
// ---------------------------------------------------------------------------

#[test]
fn safe_read_normal_path_single_attempt() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let conn1 = make_conn(
        vec![RecvStep::Line(okline(1))],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(vec![Ok(conn1)], Arc::clone(&connects));
    let ok = mgr
        .invoke_blocking("record.load", json!({"date": "2026-07-12"}), None)
        .expect("ok");
    assert_eq!(ok, json!({}));
    assert_eq!(count_cmd(&sent, "record.load"), 1);
    assert_eq!(connects.load(Ordering::SeqCst), 1);
}

#[test]
fn full_policy_table_matches_ruling() {
    let retry_once: &[&str] = &[
        "health",
        "settings.get",
        "record.load",
        "calendar.event_dates",
        "import.stats",
        "es.view",
        "import.classify",
        "oracle.payload",
        "twin.forecast",
        "profile.source_code",
        "probe.status",
        "context.manifest.latest",
    ];
    let no_replay: &[&str] = &[
        "settings.save_fixed",
        "record.save",
        "consult",
        "knowledge.fetch_pending",
        "calendar.sync",
        "import.line",
        "import.document",
        "settings.run_profiler",
        "narrative.compile",
        "oracle.report",
        "tensor.rebuild",
        "probe.next",
        "probe.answer",
        "shutdown",
    ];
    assert_eq!(retry_once.len() + no_replay.len(), 26);
    for cmd in retry_once {
        assert_eq!(
            replay_policy(cmd),
            ReplayPolicy::RetryOnceAfterRestart,
            "{cmd}"
        );
    }
    for cmd in no_replay {
        assert_eq!(replay_policy(cmd), ReplayPolicy::NoReplay, "{cmd}");
    }
}

#[test]
fn invoke_sync_has_no_allow_restart_param() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let conn1 = make_conn(
        vec![RecvStep::Line(okline(1))],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(vec![Ok(conn1)], Arc::clone(&connects));
    let params = json!({"date": "2026-07-12"});
    // 3-arg invoke_sync: compile success is the proof (_allow_restart gone)
    let ok = mgr
        .invoke_sync("record.load", &params, None)
        .expect("ok");
    assert_eq!(ok, json!({}));
}

#[test]
fn remote_message_with_pipe_words_does_not_restart() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let conn1 = make_conn(
        vec![RecvStep::Line(remote_line(1, "...書き込み失敗..."))],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(vec![Ok(conn1)], Arc::clone(&connects));
    let err = mgr
        .invoke_blocking("record.load", json!({"date": "2026-07-12"}), None)
        .expect_err("remote");
    assert_eq!(err, "...書き込み失敗...");
    assert_eq!(connects.load(Ordering::SeqCst), 1);
}

// ---------------------------------------------------------------------------
// Tests 24–27: event sink, cid, lock, startup/shutdown
// ---------------------------------------------------------------------------

#[test]
fn event_lines_forwarded_then_final_ok() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let events: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let events_sink = Arc::clone(&events);
    let conn1 = make_conn(
        vec![
            RecvStep::Line(event_line(1, "status", "working")),
            RecvStep::Line(event_line(1, "chunk", "tok")),
            RecvStep::Line(okline(1)),
        ],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let connector = FakeConnector {
        queue: Mutex::new(VecDeque::from(vec![Ok(conn1)])),
        connects: Arc::clone(&connects),
    };
    let mgr = EngineManager::with_connector(Box::new(connector));
    mgr.install_event_sink(Box::new(move |v: &Value| {
        events_sink.lock().unwrap().push(v.clone());
    }));
    mgr.boot().expect("boot");
    let ok = mgr
        .invoke_blocking("consult", json!({"query": "q"}), None)
        .expect("ok");
    assert_eq!(ok, json!({}));
    let ev = events.lock().unwrap().clone();
    assert_eq!(ev.len(), 2);
    assert_eq!(ev[0].get("event").and_then(|v| v.as_str()), Some("status"));
    assert_eq!(ev[1].get("event").and_then(|v| v.as_str()), Some("chunk"));
}

#[test]
fn cid_stamped_on_request_envelope() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let conn1 = make_conn(
        vec![RecvStep::Line(okline(1))],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(vec![Ok(conn1)], Arc::clone(&connects));
    let _ = mgr
        .invoke_blocking("health", json!({}), Some(9))
        .expect("ok");
    let line = sent.lock().unwrap()[0].clone();
    let v = parse_sent(&line);
    assert_eq!(v.get("cid"), Some(&json!(9)));

    let sent2 = Arc::new(Mutex::new(Vec::new()));
    let killed2 = Arc::new(AtomicBool::new(false));
    let connects2 = Arc::new(AtomicUsize::new(0));
    let conn2 = make_conn(
        vec![RecvStep::Line(okline(2))],
        vec![],
        Arc::clone(&sent2),
        Arc::clone(&killed2),
    );
    let mgr2 = boot_with(vec![Ok(conn2)], Arc::clone(&connects2));
    let _ = mgr2
        .invoke_blocking("health", json!({}), None)
        .expect("ok");
    let line2 = sent2.lock().unwrap()[0].clone();
    let v2 = parse_sent(&line2);
    assert_eq!(v2.get("cid"), Some(&Value::Null));
}

#[test]
fn requests_serialized_under_connection_lock() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = mpsc::channel();
    let conn1 = make_conn(
        vec![
            RecvStep::Block(rx),
            RecvStep::Line(okline(1)),
            RecvStep::Line(okline(2)),
        ],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(vec![Ok(conn1)], Arc::clone(&connects));
    let mgr_a = Arc::clone(&mgr);
    let mgr_b = Arc::clone(&mgr);
    let t1 = thread::spawn(move || {
        mgr_a
            .invoke_blocking("record.load", json!({"date": "2026-07-01"}), None)
            .expect("t1 ok")
    });
    // Wait until first request has been written (lock held in recv Block)
    for _ in 0..200 {
        if sent.lock().unwrap().len() >= 1 {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        sent.lock().unwrap().len(),
        1,
        "second request must not send before unlock"
    );
    let t2 = thread::spawn(move || {
        mgr_b
            .invoke_blocking("record.load", json!({"date": "2026-07-02"}), None)
            .expect("t2 ok")
    });
    thread::sleep(Duration::from_millis(30));
    assert_eq!(
        sent.lock().unwrap().len(),
        1,
        "still one while first blocked"
    );
    tx.send(()).expect("unblock");
    t1.join().expect("t1");
    t2.join().expect("t2");
    assert_eq!(sent.lock().unwrap().len(), 2);
}

#[test]
fn startup_and_shutdown_normal_path() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let killed = Arc::new(AtomicBool::new(false));
    let connects = Arc::new(AtomicUsize::new(0));
    let conn1 = make_conn(
        vec![RecvStep::Line(okline(1))],
        vec![],
        Arc::clone(&sent),
        Arc::clone(&killed),
    );
    let mgr = boot_with(vec![Ok(conn1)], Arc::clone(&connects));
    assert!(mgr.is_ready());
    mgr.shutdown();
    assert!(!mgr.is_ready());
    assert!(killed.load(Ordering::SeqCst));
    assert_eq!(count_cmd(&sent, "shutdown"), 1);
}

// ---------------------------------------------------------------------------
// Finding 12 — finite sterile engine.log (OS temp dir only)
// ---------------------------------------------------------------------------

use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;

static ELOG_TMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn elog_temp_root() -> PathBuf {
    let n = ELOG_TMP_SEQ.fetch_add(1, Ordering::SeqCst);
    let p = std::env::temp_dir().join(format!(
        "pkb_elog_{}_{}_{}",
        std::process::id(),
        n,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).expect("temp root");
    p
}

fn tiny_policy() -> EngineLogPolicy {
    // Header (20) + one diag record (30) = 50; room for exactly two diags before rotate.
    EngineLogPolicy {
        max_bytes: 80,
        backup_count: 2,
    }
}

fn read_log(root: &std::path::Path, name: &str) -> String {
    fs::read_to_string(root.join("logs").join(name)).unwrap_or_default()
}

fn owned_names() -> [&'static str; 3] {
    ["engine.log", "engine.log.1", "engine.log.2"]
}

#[test]
fn elog_lf_diag_only_persisted() {
    let root = elog_temp_root();
    let input = format!("{ENGINE_DIAG_LINE}\nother noise\n");
    collect_engine_stderr(&root, Cursor::new(input.into_bytes()), tiny_policy());
    let body = read_log(&root, "engine.log");
    assert!(body.starts_with(ENGINE_LOG_HEADER));
    assert_eq!(body.matches(ENGINE_DIAG_LINE).count(), 1);
    assert!(!body.contains("other noise"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn elog_crlf_normalized_to_lf() {
    let root = elog_temp_root();
    let input = format!("{ENGINE_DIAG_LINE}\r\n");
    collect_engine_stderr(&root, Cursor::new(input.into_bytes()), tiny_policy());
    let body = read_log(&root, "engine.log");
    assert!(body.contains(&format!("{ENGINE_DIAG_LINE}\n")));
    assert!(!body.contains("\r"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn elog_traceback_and_secret_not_persisted() {
    let root = elog_temp_root();
    let secret = "SECRET_DIARY_xyz";
    let input = format!(
        "Traceback (most recent call last):\n  File \"engine_stdio.py\", line 1\nValueError: {secret}\n{ENGINE_DIAG_LINE}\n"
    );
    collect_engine_stderr(&root, Cursor::new(input.into_bytes()), tiny_policy());
    let body = read_log(&root, "engine.log");
    assert!(!body.contains("Traceback"));
    assert!(!body.contains(secret));
    assert!(!body.contains("engine_stdio.py"));
    assert_eq!(body.matches(ENGINE_DIAG_LINE).count(), 1);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn elog_allowlist_prefix_with_secret_rejected() {
    let root = elog_temp_root();
    let input = format!("{ENGINE_DIAG_LINE} SECRET_QUERY\n{ENGINE_DIAG_LINE}\n");
    collect_engine_stderr(&root, Cursor::new(input.into_bytes()), tiny_policy());
    let body = read_log(&root, "engine.log");
    assert!(!body.contains("SECRET_QUERY"));
    assert_eq!(body.matches(ENGINE_DIAG_LINE).count(), 1);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn elog_allowlist_suffix_rejected() {
    let root = elog_temp_root();
    let input = format!("xx{ENGINE_DIAG_LINE}\n{ENGINE_DIAG_LINE}\n");
    collect_engine_stderr(&root, Cursor::new(input.into_bytes()), tiny_policy());
    let body = read_log(&root, "engine.log");
    assert_eq!(body.matches(ENGINE_DIAG_LINE).count(), 1);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn elog_invalid_utf8_discarded() {
    let root = elog_temp_root();
    let mut input = Vec::new();
    input.extend_from_slice(&[0xff, 0xfe, b'\n']);
    input.extend_from_slice(ENGINE_DIAG_LINE.as_bytes());
    input.push(b'\n');
    collect_engine_stderr(&root, Cursor::new(input), tiny_policy());
    let raw = fs::read(root.join("logs").join("engine.log")).expect("log");
    assert!(raw.starts_with(ENGINE_LOG_HEADER.as_bytes()));
    assert!(!raw.contains(&0xff));
    assert_eq!(
        String::from_utf8_lossy(&raw)
            .matches(ENGINE_DIAG_LINE)
            .count(),
        1
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn elog_huge_nonl_then_valid_diag() {
    let root = elog_temp_root();
    let mut input = vec![b'A'; 200_000];
    input.push(b'\n');
    input.extend_from_slice(ENGINE_DIAG_LINE.as_bytes());
    input.push(b'\n');
    collect_engine_stderr(&root, Cursor::new(input), tiny_policy());
    let body = read_log(&root, "engine.log");
    assert!(!body.contains("AAA"));
    assert_eq!(body.matches(ENGINE_DIAG_LINE).count(), 1);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn elog_tiny_policy_fills_until_cap_then_rotates() {
    let root = elog_temp_root();
    let policy = tiny_policy();
    // Two diags fit in 80 with header; third forces rotate.
    let input = format!(
        "{ENGINE_DIAG_LINE}\n{ENGINE_DIAG_LINE}\n{ENGINE_DIAG_LINE}\n"
    );
    collect_engine_stderr(&root, Cursor::new(input.into_bytes()), policy);
    let current = read_log(&root, "engine.log");
    let backup1 = read_log(&root, "engine.log.1");
    assert!(current.starts_with(ENGINE_LOG_HEADER));
    assert!(backup1.starts_with(ENGINE_LOG_HEADER));
    assert_eq!(current.matches(ENGINE_DIAG_LINE).count(), 1);
    assert_eq!(backup1.matches(ENGINE_DIAG_LINE).count(), 2);
    for name in owned_names() {
        let path = root.join("logs").join(name);
        if path.exists() {
            let len = fs::metadata(&path).unwrap().len();
            assert!(len <= policy.max_bytes, "{name} len={len}");
        }
    }
    assert!(!root.join("logs").join("engine.log.3").exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn elog_backup_count_capped() {
    let root = elog_temp_root();
    let policy = tiny_policy();
    let mut input = String::new();
    for _ in 0..12 {
        input.push_str(ENGINE_DIAG_LINE);
        input.push('\n');
    }
    collect_engine_stderr(&root, Cursor::new(input.into_bytes()), policy);
    assert!(root.join("logs").join("engine.log").exists());
    assert!(root.join("logs").join("engine.log.1").exists());
    assert!(root.join("logs").join("engine.log.2").exists());
    assert!(!root.join("logs").join("engine.log.3").exists());
    for name in owned_names() {
        let path = root.join("logs").join(name);
        let len = fs::metadata(&path).unwrap().len();
        assert!(len <= policy.max_bytes, "{name} len={len}");
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.starts_with(ENGINE_LOG_HEADER));
        assert!(!body.contains("SECRET"));
        assert!(!body.contains("Traceback"));
        assert!(!body.contains("query"));
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn elog_legacy_raw_not_copied_to_backup() {
    let root = elog_temp_root();
    let logs = root.join("logs");
    fs::create_dir_all(&logs).unwrap();
    let legacy = "Traceback\nSECRET_LEGACY_BODY\n/path/to/diary.md\n";
    fs::write(logs.join("engine.log"), legacy).unwrap();
    collect_engine_stderr(
        &root,
        Cursor::new(format!("{ENGINE_DIAG_LINE}\n").into_bytes()),
        tiny_policy(),
    );
    let current = read_log(&root, "engine.log");
    assert!(current.starts_with(ENGINE_LOG_HEADER));
    assert!(!current.contains("SECRET_LEGACY"));
    assert!(!current.contains("Traceback"));
    for name in ["engine.log.1", "engine.log.2"] {
        if root.join("logs").join(name).exists() {
            let b = read_log(&root, name);
            assert!(!b.contains("SECRET_LEGACY"));
            assert!(!b.contains("Traceback"));
        }
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn elog_v1_reinit_appends() {
    let root = elog_temp_root();
    collect_engine_stderr(
        &root,
        Cursor::new(format!("{ENGINE_DIAG_LINE}\n").into_bytes()),
        tiny_policy(),
    );
    collect_engine_stderr(
        &root,
        Cursor::new(format!("{ENGINE_DIAG_LINE}\n").into_bytes()),
        tiny_policy(),
    );
    let body = read_log(&root, "engine.log");
    assert!(body.starts_with(ENGINE_LOG_HEADER));
    assert_eq!(body.matches(ENGINE_DIAG_LINE).count(), 2);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn elog_unknown_file_untouched() {
    let root = elog_temp_root();
    let logs = root.join("logs");
    fs::create_dir_all(&logs).unwrap();
    let marker = "keep-me-unrelated";
    fs::write(logs.join("other.log"), marker).unwrap();
    fs::write(logs.join("notes.txt"), "hello").unwrap();
    collect_engine_stderr(
        &root,
        Cursor::new(format!("{ENGINE_DIAG_LINE}\n").into_bytes()),
        tiny_policy(),
    );
    assert_eq!(fs::read_to_string(logs.join("other.log")).unwrap(), marker);
    assert_eq!(fs::read_to_string(logs.join("notes.txt")).unwrap(), "hello");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn elog_logger_open_failure_still_drains() {
    let root = elog_temp_root();
    // Make `logs` a file so create_dir_all / open fails → persist disabled.
    fs::write(root.join("logs"), b"not-a-directory").unwrap();
    let mut input = Vec::new();
    for _ in 0..1000 {
        input.extend_from_slice(b"noise line\n");
    }
    input.extend_from_slice(ENGINE_DIAG_LINE.as_bytes());
    input.push(b'\n');
    // Must return (drain complete) without hanging.
    collect_engine_stderr(&root, Cursor::new(input), tiny_policy());
    assert!(root.join("logs").is_file());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn elog_header_plus_secret_body_is_reset_not_kept() {
    // P1: V1 header alone must not make a poisoned body "safe".
    let root = elog_temp_root();
    let logs = root.join("logs");
    fs::create_dir_all(&logs).unwrap();
    let secret = "SECRET_AFTER_HEADER_xyzzy";
    fs::write(
        logs.join("engine.log"),
        format!("{ENGINE_LOG_HEADER}{secret}\n"),
    )
    .unwrap();
    collect_engine_stderr(
        &root,
        Cursor::new(format!("{ENGINE_DIAG_LINE}\n").into_bytes()),
        tiny_policy(),
    );
    let body = read_log(&root, "engine.log");
    assert!(body.starts_with(ENGINE_LOG_HEADER));
    assert!(!body.contains(secret), "poisoned body must not be retained");
    assert_eq!(body.matches(ENGINE_DIAG_LINE).count(), 1);
    for name in ["engine.log.1", "engine.log.2"] {
        if root.join("logs").join(name).exists() {
            let b = read_log(&root, name);
            assert!(!b.contains(secret));
        }
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn elog_open_plan_symlink_never_equals_absent() {
    // P1: dangling symlink must not be classified like Absent (exists() trap).
    assert_eq!(
        open_plan(OwnedPathKind::Symlink, false),
        OpenPlan::ReplaceFresh
    );
    assert_eq!(
        open_plan(OwnedPathKind::Absent, false),
        OpenPlan::CreateFresh
    );
    assert_ne!(
        open_plan(OwnedPathKind::Symlink, false),
        open_plan(OwnedPathKind::Absent, false)
    );
    assert_eq!(
        open_plan(OwnedPathKind::Other, false),
        OpenPlan::ReplaceFresh
    );
}

#[test]
fn elog_finding12_source_has_no_path_exists() {
    let src = include_str!("engine.rs");
    let start = src
        .find("Finding 12 — finite sterile engine diagnostic log")
        .expect("Finding 12 section");
    let section = &src[start..];
    assert!(
        !section.contains(".exists()"),
        "engine-log code must not use Path::exists (dangling symlink trap)"
    );
    assert!(
        section.contains("symlink_metadata"),
        "engine-log existence must use symlink_metadata"
    );
}

#[test]
fn elog_directory_named_engine_log_replaced_with_v1_file() {
    let root = elog_temp_root();
    let logs = root.join("logs");
    fs::create_dir_all(logs.join("engine.log")).unwrap();
    collect_engine_stderr(
        &root,
        Cursor::new(format!("{ENGINE_DIAG_LINE}\n").into_bytes()),
        tiny_policy(),
    );
    let meta = std::fs::symlink_metadata(logs.join("engine.log")).expect("engine.log");
    assert!(meta.file_type().is_file());
    assert!(!meta.file_type().is_symlink());
    let body = read_log(&root, "engine.log");
    assert!(body.starts_with(ENGINE_LOG_HEADER));
    assert_eq!(body.matches(ENGINE_DIAG_LINE).count(), 1);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn elog_dangling_symlink_not_followed_on_create() {
    // P1 runtime: remove dangling symlink via symlink_metadata; do not create target.
    let root = elog_temp_root();
    let logs = root.join("logs");
    fs::create_dir_all(&logs).unwrap();
    let target = logs.join("dangling_target_MUST_NOT_EXIST");
    let link = logs.join("engine.log");

    let mut ready = false;
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_file(&target, &link).is_ok() {
            ready = true;
        } else {
            let decoy = logs.join("decoy_target");
            fs::write(&decoy, b"x").unwrap();
            let _ = fs::remove_file(&link);
            if std::os::windows::fs::symlink_file(&decoy, &link).is_ok() {
                fs::remove_file(&decoy).unwrap();
                ready = true;
            }
        }
    }
    #[cfg(unix)]
    {
        if std::os::unix::fs::symlink(&target, &link).is_ok() {
            ready = true;
        } else {
            let decoy = logs.join("decoy_target");
            fs::write(&decoy, b"x").unwrap();
            let _ = fs::remove_file(&link);
            if std::os::unix::fs::symlink(&decoy, &link).is_ok() {
                fs::remove_file(&decoy).unwrap();
                ready = true;
            }
        }
    }

    if !ready {
        // Host lacks symlink privilege (common on Windows without Developer Mode).
        // Portable contracts above still cover Symlink≠Absent and no Path::exists.
        let _ = fs::remove_dir_all(&root);
        return;
    }

    assert!(
        std::fs::symlink_metadata(&link)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false),
        "precondition: engine.log must be a symlink"
    );
    assert!(
        std::fs::symlink_metadata(&target).is_err(),
        "precondition: dangling target absent"
    );
    collect_engine_stderr(
        &root,
        Cursor::new(format!("{ENGINE_DIAG_LINE}\n").into_bytes()),
        tiny_policy(),
    );
    assert!(
        std::fs::symlink_metadata(&target).is_err(),
        "create must not materialize dangling symlink target"
    );
    let meta = std::fs::symlink_metadata(logs.join("engine.log")).expect("engine.log");
    assert!(meta.file_type().is_file());
    assert!(!meta.file_type().is_symlink());
    let body = read_log(&root, "engine.log");
    assert!(body.starts_with(ENGINE_LOG_HEADER));
    assert_eq!(body.matches(ENGINE_DIAG_LINE).count(), 1);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn elog_cr_inside_marker_rejected_crlf_end_ok() {
    // P2: CR only allowed as CRLF terminator after full marker; mid-marker CR rejected.
    let root = elog_temp_root();
    let mut input = Vec::new();
    // Inject CR inside the allowlist marker.
    let marker = ENGINE_DIAG_LINE.as_bytes();
    input.extend_from_slice(&marker[..8]);
    input.push(b'\r');
    input.extend_from_slice(&marker[8..]);
    input.push(b'\n');
    // Valid CRLF-terminated diag must still be accepted.
    input.extend_from_slice(marker);
    input.extend_from_slice(b"\r\n");
    collect_engine_stderr(&root, Cursor::new(input), tiny_policy());
    let body = read_log(&root, "engine.log");
    assert_eq!(body.matches(ENGINE_DIAG_LINE).count(), 1);
    assert!(!body.contains('\r'));
    let _ = fs::remove_dir_all(&root);
}
