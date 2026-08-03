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

# Closed set matched by the generate_handler! extractor in test_05
# (any `commands::name` except `analytics::commands::*`).
EXPOSED_COMMANDS = {
    "assign_interview_turn_ids",
    "bxs_abort",
    "bxs_advance",
    "bxs_estimate_profile",
    "bxs_get_view",
    "bxs_latest_profile",
    "bxs_list_profiles",
    "bxs_load_generation",
    "bxs_start_campaign",
    "bxs_submit_decision",
    "bxs_take_flavor",
    "calendar_event_dates",
    "calendar_sync_apple",
    "calendar_sync_ics",
    "consult",
    "context_manifest_latest",
    "engine_health",
    "engine_ready",
    "es_list",
    "es_view",
    "fetch_apple_calendar_events",
    "import_classify",
    "import_document",
    "import_line_batch",
    "import_line_single",
    "import_stats",
    "knowledge_fetch_pending",
    "knowledge_policy_get",
    "knowledge_policy_set",
    "knowledge_research",
    "llm_warm",
    "narrative_compile",
    "ocr_recognize_layout",
    "oracle_payload",
    "oracle_report",
    "probe_answer",
    "probe_next",
    "probe_status",
    "profile_source_code",
    "record_load",
    "record_save",
    "seal_interview_evaluation",
    "seal_interview_evaluation_from_session",
    "seal_metacognitive_debrief",
    "seal_metacognitive_debrief_from_session",
    "settings_get",
    "settings_run_profiler",
    "settings_save_fixed",
    "tensor_rebuild",
    "twin_forecast",
}

# Subset owned exclusively by apps/desktop/src/lib/engine.ts (invokeEngine).
ENGINE_OWNED_COMMANDS = {
    "calendar_event_dates",
    "calendar_sync_apple",
    "calendar_sync_ics",
    "consult",
    "context_manifest_latest",
    "engine_health",
    "engine_ready",
    "es_list",
    "es_view",
    "import_classify",
    "import_document",
    "import_line_batch",
    "import_line_single",
    "import_stats",
    "knowledge_fetch_pending",
    "knowledge_policy_get",
    "knowledge_policy_set",
    "knowledge_research",
    "llm_warm",
    "narrative_compile",
    "oracle_payload",
    "oracle_report",
    "probe_answer",
    "probe_next",
    "probe_status",
    "profile_source_code",
    "record_load",
    "record_save",
    "settings_get",
    "settings_run_profiler",
    "settings_save_fixed",
    "tensor_rebuild",
    "twin_forecast",
}

# Post scope-down capability pin (T-6 案 B). Strings + scoped fs verb identifiers.
CORE_CAPABILITY_PERMISSIONS = {
    "core:event:allow-listen",
    "core:event:allow-unlisten",
    "core:window:allow-close",
    "core:window:allow-minimize",
    "core:window:allow-start-dragging",
    "core:window:allow-toggle-maximize",
    "core:window:deny-create",
    "core:webview:deny-create-webview",
    "core:webview:deny-create-webview-window",
    "dialog:default",
    "fs:allow-start-accessing-security-scoped-resource",
    "fs:allow-stop-accessing-security-scoped-resource",
    "fs:allow-mkdir",
    "fs:allow-write-file",
    "fs:allow-remove",
    "fs:allow-copy-file",
}

FORBIDDEN_CAPABILITY_PERMISSIONS = {
    "core:default",
    "fs:default",
    "fs:allow-appdata-write-recursive",
    "fs:allow-appdata-read-recursive",
}

FS_SCOPE_PATHS = {
    "fs:allow-mkdir": {
        "$APPDATA/imports",
        "$APPDATA/imports/**",
        "$APPDATA/models",
        "$APPDATA/models/**",
    },
    "fs:allow-write-file": {
        "$APPDATA/imports",
        "$APPDATA/imports/**",
        "$APPDATA/models",
        "$APPDATA/models/**",
    },
    "fs:allow-remove": {
        "$APPDATA/imports",
        "$APPDATA/imports/**",
    },
    "fs:allow-copy-file": {
        "$APPDATA/models",
        "$APPDATA/models/**",
    },
}


def _directives(raw: str) -> dict[str, tuple[str, ...]]:
    result: dict[str, tuple[str, ...]] = {}
    for part in raw.split(";"):
        tokens = part.strip().split()
        if tokens:
            assert tokens[0] not in result, f"duplicate CSP directive: {tokens[0]}"
            result[tokens[0]] = tuple(tokens[1:])
    return result


def _permission_identifiers(permissions: list[object]) -> set[str]:
    ids: set[str] = set()
    for entry in permissions:
        if isinstance(entry, str):
            ids.add(entry)
        elif isinstance(entry, dict):
            ident = entry.get("identifier")
            assert isinstance(ident, str) and ident, entry
            ids.add(ident)
        else:
            raise AssertionError(f"unexpected permission entry: {entry!r}")
    return ids


def _scoped_paths(permissions: list[object], identifier: str) -> set[str]:
    paths: set[str] = set()
    for entry in permissions:
        if isinstance(entry, dict) and entry.get("identifier") == identifier:
            for item in entry.get("allow") or []:
                assert isinstance(item, dict) and "path" in item, item
                paths.add(item["path"])
    return paths


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
    permissions = capability["permissions"]
    assert isinstance(permissions, list)
    ids = _permission_identifiers(permissions)
    assert ids == CORE_CAPABILITY_PERMISSIONS
    assert FORBIDDEN_CAPABILITY_PERMISSIONS.isdisjoint(ids)
    for ident, expected_paths in FS_SCOPE_PATHS.items():
        assert _scoped_paths(permissions, ident) == expected_paths, ident


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
    registered = set(
        re.findall(r"(?<!analytics::)commands::([a-z0-9_]+)", handler_match["body"])
    )
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
    owned = {command for command, _parser in parsed_calls}
    assert owned == ENGINE_OWNED_COMMANDS
    assert owned <= EXPOSED_COMMANDS
    assert all(parser.startswith("parse") for _command, parser in parsed_calls)
    assert not re.search(r'invoke\s*<\s*unknown\s*>\s*\(\s*"', engine)


def test_08_tauri_invoke_has_closed_frontend_owners() -> None:
    owners: list[Path] = []
    for path in FRONTEND.rglob("*"):
        if path.suffix not in {".ts", ".tsx"}:
            continue
        source = path.read_text(encoding="utf-8")
        if "@tauri-apps/api/core" in source or re.search(r"\binvoke\s*<", source):
            owners.append(path.relative_to(ROOT))
    expected = [
        Path("apps/desktop/src/lib/blackboxArena.ts"),
        Path("apps/desktop/src/lib/engine.ts"),
        Path("apps/desktop/src/lib/haptics.ts"),
        Path("apps/desktop/src/lib/llm.ts"),
        Path("apps/desktop/src/lib/modelSetup.ts"),
        Path("apps/desktop/src/lib/pocketBrain/api.ts"),
        Path("apps/desktop/src/lib/pocketBrain/invoke.ts"),
        Path("apps/desktop/src/lib/vault.ts"),
    ]
    assert sorted(owners) == sorted(expected)


def test_09_engine_events_are_unknown_then_strictly_parsed() -> None:
    # ConsultTab moved to pocketBrain consult_with_oracle_context (no pkb-engine-event).
    for name in ("ImportTab.tsx", "InterviewTab.tsx"):
        source = (FRONTEND / "components" / name).read_text(encoding="utf-8")
        assert "listen<unknown>" in source, name
        assert "parseEngineEvent" in source, name
        assert "listen<EngineEvent>" not in source, name


def test_10_frontend_has_no_external_resource_or_dynamic_html_sink() -> None:
    corpus = (DESKTOP / "index.html").read_text(encoding="utf-8")
    for path in FRONTEND.rglob("*"):
        if path.suffix in {".ts", ".tsx", ".css", ".html"}:
            corpus += "\n" + path.read_text(encoding="utf-8")
    # data: URLs (incl. inline SVG xmlns=http://www.w3.org/...) are not network sinks.
    scrubbed = re.sub(r"url\(\s*[\"']?data:[^)]+\)", "", corpus, flags=re.IGNORECASE)
    scrubbed = re.sub(r"\bdata:[^\s\"')]+", "", scrubbed, flags=re.IGNORECASE)
    for pattern in (
        r"https?://",
        r"\bfetch\s*\(",
        r"\bWebSocket\s*\(",
        r"\bEventSource\s*\(",
        r"\bsendBeacon\s*\(",
        r"dangerouslySetInnerHTML",
        r"<iframe\b",
    ):
        assert re.search(pattern, scrubbed, re.IGNORECASE) is None, pattern


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
