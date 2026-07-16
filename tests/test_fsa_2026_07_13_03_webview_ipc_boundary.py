# -*- coding: utf-8 -*-
"""FSA-2026-07-13-03: WebView egress and explicit IPC boundary contracts."""
from __future__ import annotations

import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DESKTOP = ROOT / "apps" / "desktop"
TAURI = DESKTOP / "src-tauri"
FRONTEND = DESKTOP / "src"

EXPOSED_COMMANDS = {
    "engine_ready",
    "engine_health",
    "record_load",
    "record_save",
    "calendar_event_dates",
    "import_stats",
    "es_view",
    "consult",
    "calendar_sync_ics",
    "calendar_sync_apple",
    "import_line_single",
    "import_line_batch",
    "import_classify",
    "import_document",
    "settings_get",
    "settings_save_fixed",
    "settings_run_profiler",
    "oracle_payload",
    "oracle_report",
    "twin_forecast",
    "tensor_rebuild",
    "profile_source_code",
    "narrative_compile",
    "knowledge_fetch_pending",
    "knowledge_research",
    "knowledge_policy_get",
    "knowledge_policy_set",
    "probe_status",
    "probe_next",
    "probe_answer",
    "context_manifest_latest",
}


def _directives(raw: str) -> dict[str, tuple[str, ...]]:
    result: dict[str, tuple[str, ...]] = {}
    for part in raw.split(";"):
        tokens = part.strip().split()
        if tokens:
            assert tokens[0] not in result, f"duplicate CSP directive: {tokens[0]}"
            result[tokens[0]] = tuple(tokens[1:])
    return result


def test_01_production_csp_is_closed_and_has_no_network_sink() -> None:
    config = json.loads((TAURI / "tauri.conf.json").read_text(encoding="utf-8"))
    raw = config["app"]["security"]["csp"]
    assert isinstance(raw, str) and raw.strip()
    csp = _directives(raw)

    assert csp["default-src"] == ("'self'",)
    assert csp["script-src"] == ("'self'",)
    assert set(csp["connect-src"]) == {"ipc:", "http://ipc.localhost"}
    assert set(csp["style-src"]) == {"'self'", "'unsafe-inline'"}
    assert set(csp["img-src"]) == {"'self'", "data:"}
    assert csp["font-src"] == ("'self'",)
    for directive in (
        "media-src",
        "object-src",
        "frame-src",
        "child-src",
        "worker-src",
        "form-action",
        "base-uri",
    ):
        assert csp[directive] == ("'none'",), directive
    assert "*" not in raw
    assert "https:" not in raw
    assert "ws:" not in raw and "wss:" not in raw


def test_02_development_csp_is_explicit_and_localhost_only() -> None:
    config = json.loads((TAURI / "tauri.conf.json").read_text(encoding="utf-8"))
    raw = config["app"]["security"]["devCsp"]
    assert isinstance(raw, str) and raw.strip()
    csp = _directives(raw)
    assert csp["default-src"] == ("'self'",)
    assert set(csp["connect-src"]) == {
        "ipc:",
        "http://ipc.localhost",
        "http://localhost:1420",
        "ws://localhost:1420",
    }
    assert "*" not in raw
    assert "0.0.0.0" not in raw
    assert "https:" not in raw and "wss:" not in raw


def test_03_window_is_manually_created_with_three_fail_closed_handlers() -> None:
    config = json.loads((TAURI / "tauri.conf.json").read_text(encoding="utf-8"))
    windows = config["app"]["windows"]
    assert len(windows) == 1 and windows[0]["label"] == "main"
    assert windows[0]["create"] is False

    source = (TAURI / "src" / "lib.rs").read_text(encoding="utf-8")
    assert "WebviewWindowBuilder::from_config" in source
    assert ".on_navigation(" in source
    assert ".on_new_window(" in source
    assert "NewWindowResponse::Deny" in source
    assert ".on_download(" in source
    assert "DownloadEvent::Requested" in source


def test_04_capability_is_minimal_and_cannot_create_windows_or_webviews() -> None:
    capability = json.loads(
        (TAURI / "capabilities" / "default.json").read_text(encoding="utf-8")
    )
    assert capability["windows"] == ["main"]
    permissions = set(capability["permissions"])
    assert "core:default" not in permissions
    assert permissions == {
        "core:event:allow-listen",
        "core:event:allow-unlisten",
        "core:window:allow-close",
        "core:window:allow-minimize",
        "core:window:allow-start-dragging",
        "core:window:allow-toggle-maximize",
        "core:window:deny-create",
        "core:webview:deny-create-webview",
        "core:webview:deny-create-webview-window",
    }


def test_05_generic_rust_dispatch_is_removed_and_command_set_is_closed() -> None:
    commands = (TAURI / "src" / "commands.rs").read_text(encoding="utf-8")
    lib = (TAURI / "src" / "lib.rs").read_text(encoding="utf-8")
    assert "pkb_invoke" not in commands
    assert "cmd: String" not in commands
    assert "Option<Value>" not in commands
    assert "pub async fn pkb_invoke" not in commands

    handler_match = re.search(
        r"tauri::generate_handler!\[(?P<body>.*?)\]\)", lib, re.DOTALL
    )
    assert handler_match is not None
    registered = set(re.findall(r"commands::([a-z0-9_]+)", handler_match["body"]))
    assert registered == EXPOSED_COMMANDS


def test_06_renderer_inputs_use_strict_command_specific_rust_schemas() -> None:
    contract_path = TAURI / "src" / "ipc_contract.rs"
    assert contract_path.is_file()
    source = contract_path.read_text(encoding="utf-8")
    assert source.count("#[serde(deny_unknown_fields)]") >= 15
    assert "serde_json::Value" not in source
    assert "pub trait ValidateRequest" in source
    assert "enum ConsultMode" in source
    assert "enum ImportDestination" in source
    assert "enum CalendarMergeMode" in source

    commands = (TAURI / "src" / "commands.rs").read_text(encoding="utf-8")
    assert "request.validate()?" in commands
    assert "serde_json::to_value(request)" in commands


def test_07_frontend_has_no_generic_command_or_typed_invoke_cast() -> None:
    engine = (FRONTEND / "lib" / "engine.ts").read_text(encoding="utf-8")
    assert "pkbInvoke" not in engine
    assert '"pkb_invoke"' not in engine
    assert not re.search(r"invoke\s*<\s*(?!unknown\b)[^>]+>", engine)
    assert "invoke<unknown>" in engine
    assert "parseEngineResponse" in engine

    parsed_calls = re.findall(
        r'invokeEngine\s*\(\s*"([a-z0-9_]+)"\s*,\s*(parse[A-Za-z0-9_]+)',
        engine,
    )
    assert {command for command, _parser in parsed_calls} == EXPOSED_COMMANDS
    assert all(parser.startswith("parse") for _command, parser in parsed_calls)
    assert not re.search(r'invoke\s*<\s*unknown\s*>\s*\(\s*"', engine)


def test_08_tauri_invoke_has_one_frontend_owner() -> None:
    owners: list[Path] = []
    for path in FRONTEND.rglob("*"):
        if path.suffix not in {".ts", ".tsx"}:
            continue
        source = path.read_text(encoding="utf-8")
        if "@tauri-apps/api/core" in source or re.search(r"\binvoke\s*<", source):
            owners.append(path.relative_to(ROOT))
    assert owners == [Path("apps/desktop/src/lib/engine.ts")]


def test_09_engine_events_are_unknown_then_strictly_parsed() -> None:
    for name in ("ConsultTab.tsx", "ImportTab.tsx", "InterviewTab.tsx"):
        source = (FRONTEND / "components" / name).read_text(encoding="utf-8")
        assert "listen<unknown>" in source, name
        assert "parseEngineEvent" in source, name
        assert "listen<EngineEvent>" not in source, name


def test_10_frontend_has_no_external_resource_or_dynamic_html_sink() -> None:
    corpus = (DESKTOP / "index.html").read_text(encoding="utf-8")
    for path in FRONTEND.rglob("*"):
        if path.suffix in {".ts", ".tsx", ".css", ".html"}:
            corpus += "\n" + path.read_text(encoding="utf-8")
    for pattern in (
        r"https?://",
        r"\bfetch\s*\(",
        r"\bWebSocket\s*\(",
        r"\bEventSource\s*\(",
        r"\bsendBeacon\s*\(",
        r"dangerouslySetInnerHTML",
        r"<iframe\b",
    ):
        assert re.search(pattern, corpus, re.IGNORECASE) is None, pattern


def test_11_navigation_policy_is_a_pure_allowlist_not_a_prefix_check() -> None:
    policy_path = TAURI / "src" / "webview_policy.rs"
    assert policy_path.is_file()
    source = policy_path.read_text(encoding="utf-8")
    assert "pub fn navigation_allowed" in source
    assert "url.username().is_empty()" in source
    assert "url.password().is_none()" in source
    assert "url.port().is_none()" in source
    assert "starts_with" not in source
    assert "ends_with" not in source


def test_12_no_untyped_engine_wrapper_is_imported_by_components() -> None:
    offenders: list[str] = []
    for path in FRONTEND.rglob("*.tsx"):
        source = path.read_text(encoding="utf-8")
        if re.search(r"\bpkbInvoke\b", source):
            offenders.append(str(path.relative_to(ROOT)))
    assert offenders == []
