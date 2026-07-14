use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use pkb_desktop_lib::os_sandbox::spawn_kernel_sandboxed;

#[test]
fn sandbox_construction_failure_is_a_hard_error() {
    let data_root = std::env::temp_dir().join(format!(
        "pkb-kernel-sandbox-hard-fail-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&data_root).expect("create hard-fail data root");
    let mut command = Command::new(data_root.join("missing-engine-binary"));
    command
        .env_clear()
        .current_dir(&data_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let result = spawn_kernel_sandboxed(command, &data_root);
    assert!(result.is_err(), "missing sandbox target was started");
}

#[test]
fn native_tcp_socket_probe() {
    let listener_v4 = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("bind adversarial IPv4 listener");
    let listener_v6 = std::net::TcpListener::bind((std::net::Ipv6Addr::LOCALHOST, 0))
        .expect("bind adversarial IPv6 listener");
    listener_v4
        .set_nonblocking(true)
        .expect("make IPv4 listener nonblocking");
    listener_v6
        .set_nonblocking(true)
        .expect("make IPv6 listener nonblocking");
    let data_root =
        std::env::temp_dir().join(format!("pkb-kernel-sandbox-probe-{}", std::process::id()));
    std::fs::create_dir_all(&data_root).expect("create probe data root");

    let mut command = Command::new(env!("CARGO_BIN_EXE_pkb-sandbox-probe"));
    command
        .env_clear()
        .env("PKB_KERNEL_SANDBOX_PROBE_CHILD", "1")
        .env(
            "PKB_KERNEL_SANDBOX_PROBE_PORT_V4",
            listener_v4
                .local_addr()
                .expect("IPv4 listener address")
                .port()
                .to_string(),
        )
        .env(
            "PKB_KERNEL_SANDBOX_PROBE_PORT_V6",
            listener_v6
                .local_addr()
                .expect("IPv6 listener address")
                .port()
                .to_string(),
        )
        .current_dir(&data_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in ["SystemRoot", "WINDIR", "LANG", "LC_ALL", "TZ"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }

    let process = spawn_kernel_sandboxed(command, &data_root)
        .expect("production kernel sandbox must be available");
    let pkb_desktop_lib::os_sandbox::SandboxedProcess {
        mut child,
        stdin,
        mut stdout,
        mut stderr,
    } = process;
    drop(stdin);
    let stdout_worker = std::thread::spawn(move || {
        let mut text = String::new();
        stdout.read_to_string(&mut text).expect("read probe stdout");
        text
    });
    let stderr_worker = std::thread::spawn(move || {
        let mut text = String::new();
        stderr.read_to_string(&mut text).expect("read probe stderr");
        text
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll probe child") {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().expect("terminate timed-out probe child");
            let _ = child.wait();
            panic!("kernel sandbox probe exceeded its fixed deadline");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = stdout_worker.join().expect("join probe stdout reader");
    let stderr = stderr_worker.join().expect("join probe stderr reader");
    for (name, listener) in [("IPv4", &listener_v4), ("IPv6", &listener_v6)] {
        match listener.accept() {
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Ok(_) => panic!("{name} bytes reached the parent listener"),
            Err(error) => panic!("inspect {name} listener: {error}"),
        }
    }
    assert!(
        status.success(),
        "native socket probe escaped the kernel boundary (status: {status:?}, code: {:?})\nstdout: {stdout}\nstderr: {stderr}",
        status.code()
    );
}
