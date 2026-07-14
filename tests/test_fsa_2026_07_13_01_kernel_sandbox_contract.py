# -*- coding: utf-8 -*-
"""FSA-2026-07-13-01/02: production kernel-sandbox contracts."""
from __future__ import annotations

import plistlib
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
TAURI = ROOT / "apps" / "desktop" / "src-tauri"
RUST = TAURI / "src"
ENGINE = RUST / "engine.rs"
PATHS = RUST / "paths.rs"
SANDBOX = RUST / "os_sandbox.rs"
SANDBOX_TESTS = TAURI / "tests" / "os_sandbox_runtime.rs"
NATIVE_PROBE = RUST / "bin" / "pkb-sandbox-probe.rs"


def test_01_release_is_bundled_sidecar_only_and_dev_is_explicit() -> None:
    source = ENGINE.read_text(encoding="utf-8")
    assert "PKB_UNSAFE_DEV_ENGINE" in source
    assert "#[cfg(debug_assertions)]\nfn spawn_python_engine" in source
    assert "#[cfg(not(debug_assertions))]\nfn spawn_configured_engine" in source
    assert "release: 同梱エンジンなし" not in source
    assert "else if script.is_file()" not in source
    assert "spawn_python_engine" in source
    assert "spawn_bundled_engine" in source

    dev_script = (ROOT / "apps/desktop/scripts/run-tauri-dev.ps1").read_text(
        encoding="utf-8"
    )
    assert '$env:PKB_UNSAFE_DEV_ENGINE = "1"' in dev_script
    assert "UNSAFE DEVELOPMENT ENGINE" in dev_script

    paths = PATHS.read_text(encoding="utf-8")
    assert re.search(
        r'#\[cfg\(debug_assertions\)\]\s+pub fn project_root\(\) -> PathBuf \{'
        r'.*?env::var\("PKB_PROJECT_ROOT"\)',
        paths,
        re.DOTALL,
    )
    assert re.search(
        r'#\[cfg\(not\(debug_assertions\)\)\]\s+'
        r'pub fn project_root\(\) -> PathBuf \{\s+user_data_root\(\)',
        paths,
    )


def test_02_rust_has_one_fail_closed_kernel_sandbox_owner() -> None:
    assert SANDBOX.is_file()
    lib = (RUST / "lib.rs").read_text(encoding="utf-8")
    source = SANDBOX.read_text(encoding="utf-8")
    assert "mod os_sandbox;" in lib
    assert "spawn_kernel_sandboxed" in source
    assert "SandboxUnavailable" in source
    assert "fallback" not in source.lower()


def test_03_windows_uses_zero_capability_appcontainer_and_pid_owned_stdio() -> None:
    source = SANDBOX.read_text(encoding="utf-8")
    for token in (
        "CreateAppContainerProfile",
        "DeriveAppContainerSidFromAppContainerName",
        "PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES",
        "SECURITY_CAPABILITIES",
        "InitializeProcThreadAttributeList",
        "UpdateProcThreadAttribute",
        "CreateProcessW",
        "PROC_THREAD_ATTRIBUTE_HANDLE_LIST",
        "GetExitCodeProcess",
    ):
        assert token in source
    assert "CapabilityCount: 0" in source
    assert "internetClient" not in source
    assert "internetClientServer" not in source


def test_04_linux_seccomp_denies_only_ip_socket_families_and_checks_arch() -> None:
    source = SANDBOX.read_text(encoding="utf-8")
    for token in (
        "PR_SET_NO_NEW_PRIVS",
        "SECCOMP_MODE_FILTER",
        "SECCOMP_RET_ERRNO",
        "AUDIT_ARCH_X86_64",
        "AUDIT_ARCH_AARCH64",
        "AF_INET",
        "AF_INET6",
        "AF_UNIX",
    ):
        assert token in source
    assert "libc::SYS_socket" in source
    assert "libc::EACCES" in source


def test_05_macos_app_and_child_entitlements_have_no_network_capability() -> None:
    app = plistlib.loads((TAURI / "entitlements.plist").read_bytes())
    child = plistlib.loads((TAURI / "sidecar-entitlements.plist").read_bytes())

    assert app["com.apple.security.app-sandbox"] is True
    assert "com.apple.security.network.client" not in app
    assert "com.apple.security.network.server" not in app
    assert child == {
        "com.apple.security.app-sandbox": True,
        "com.apple.security.inherit": True,
    }


def test_06_macos_build_signs_and_verifies_every_embedded_executable() -> None:
    build = (ROOT / "apps/desktop/scripts/build-sidecar.sh").read_text(
        encoding="utf-8"
    )
    workflow = (ROOT / ".github/workflows/build-macos.yml").read_text(
        encoding="utf-8"
    )
    for source in (build, workflow):
        assert "sidecar-entitlements.plist" in source
        assert "codesign" in source
        assert "com.apple.security.network.client" in source
        assert "com.apple.security.network.server" in source
    assert "PKB_MACOS_SANDBOX_VERIFIED" in workflow


def test_07_native_socket_probe_runs_through_the_production_launcher() -> None:
    assert SANDBOX_TESTS.is_file()
    assert NATIVE_PROBE.is_file()
    runtime = SANDBOX_TESTS.read_text(encoding="utf-8")
    native = NATIVE_PROBE.read_text(encoding="utf-8")
    assert "native_tcp_socket_probe" in runtime
    assert "spawn_kernel_sandboxed" in runtime
    assert "PKB_KERNEL_SANDBOX_PROBE_CHILD" in runtime
    assert "AF_INET" in native
    assert "AF_INET6" in native
    assert "connect(" in native
    cargo = (TAURI / "Cargo.toml").read_text(encoding="utf-8")
    tauri_config = (TAURI / "tauri.conf.json").read_text(encoding="utf-8")
    assert 'default-run = "pkb-desktop"' in cargo
    assert "pkb-sandbox-probe" not in tauri_config


def test_08_engine_startup_propagates_sandbox_failure() -> None:
    source = (RUST / "lib.rs").read_text(encoding="utf-8")
    assert "async_runtime::spawn" not in source
    assert "block_on(manager.start" in source
    assert "PKB engine startup failed" not in source
