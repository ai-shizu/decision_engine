# -*- coding: utf-8 -*-
"""Finding 13 — UI catch sites must use operation-keyed sterile messages."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
COMP = ROOT / "apps" / "desktop" / "src" / "components"
LIB = ROOT / "apps" / "desktop" / "src" / "lib"
HELPER = LIB / "uiErrorMessages.ts"
ENGINE_RS = ROOT / "apps" / "desktop" / "src-tauri" / "src" / "engine.rs"

TARGET_FILES = [
    "ProbeTab.tsx",
    "RecordTab.tsx",
    "InterviewTab.tsx",
    "ConsultTab.tsx",
    "ImportTab.tsx",
    "ProfileTab.tsx",
    "SettingsTab.tsx",
]

# Split banned tokens so this test file itself is not a false positive for tree greps.
_S = "String"
_BANNED_STRING_ERR = _S + "(err)"
_BANNED_STRING_ERROR = _S + "(error)"

EXPECTED_CATCH_KEYS = {
    "ProbeTab.tsx": [
        "PROBE_STATUS_LOAD",
        "PROBE_STATUS_LOAD",
        "PROBE_NEXT",
        "PROBE_ANSWER",
    ],
    "RecordTab.tsx": ["RECORD_LOAD", "RECORD_SAVE"],
    "InterviewTab.tsx": ["INTERVIEW_RESPONSE", "NARRATIVE_COMPILE"],
    "ConsultTab.tsx": ["ROMANCE_ANALYSIS", "CONSULT_RESPONSE"],
    "ImportTab.tsx": [
        "LINE_IMPORT",
        "ICS_SYNC",
        "APPLE_CALENDAR_SYNC",
        "DOCUMENT_IMPORT",
        "KNOWLEDGE_FETCH",
    ],
    "ProfileTab.tsx": [
        "PROFILE_LOAD",
        "ORACLE_REPORT",
        "TWIN_FORECAST",
        "TENSOR_REBUILD",
    ],
    "SettingsTab.tsx": ["SETTINGS_LOAD", "SETTINGS_SAVE", "PROFILER_RUN"],
}

RETRY_SAFE = {
    "PROBE_STATUS_LOAD",
    "RECORD_LOAD",
    "PROFILE_LOAD",
    "TWIN_FORECAST",
    "SETTINGS_LOAD",
}

VERIFY_FIRST = {
    "PROBE_NEXT",
    "PROBE_ANSWER",
    "RECORD_SAVE",
    "INTERVIEW_RESPONSE",
    "NARRATIVE_COMPILE",
    "CONSULT_RESPONSE",
    "ROMANCE_ANALYSIS",
    "LINE_IMPORT",
    "ICS_SYNC",
    "APPLE_CALENDAR_SYNC",
    "DOCUMENT_IMPORT",
    "KNOWLEDGE_FETCH",
    "ORACLE_REPORT",
    "TENSOR_REBUILD",
    "SETTINGS_SAVE",
    "PROFILER_RUN",
}

# Finding 2 replay_policy RetryOnceAfterRestart cmds (subset used by UI keys).
RETRY_SAFE_CMDS = {
    "probe.status",
    "record.load",
    "settings.get",
    "oracle.payload",
    "twin.forecast",
    "profile.source_code",
}


def _read(name: str) -> str:
    return (COMP / name).read_text(encoding="utf-8")


def _catch_blocks(src: str) -> list[str]:
    """Extract catch { ... } / catch (x) { ... } bodies (brace-balanced)."""
    blocks: list[str] = []
    for m in re.finditer(r"catch\s*(?:\([^)]*\))?\s*\{", src):
        start = m.end()
        depth = 1
        i = start
        while i < len(src) and depth:
            if src[i] == "{":
                depth += 1
            elif src[i] == "}":
                depth -= 1
            i += 1
        blocks.append(src[start : i - 1])
    return blocks


def _ui_error_catches(src: str) -> list[str]:
    """IPC/UI error catches only — ignore empty/best-effort swallows."""
    out: list[str] = []
    for block in _catch_blocks(src):
        if "uiErrorMessage" in block or "String(err)" in block or "String(error)" in block:
            out.append(block)
            continue
        if any(
            tok in block
            for tok in (
                "setError(",
                "setStatus(",
                "pushImportLog(",
                "setSaveNotice(",
                "setNarrativeResult(",
                "setLoadError(",
            )
        ):
            out.append(block)
    return out


def test_twenty_two_catches_exist() -> None:
    total = 0
    for name, keys in EXPECTED_CATCH_KEYS.items():
        blocks = _ui_error_catches(_read(name))
        assert len(blocks) == len(keys), f"{name}: expected {len(keys)} catches, got {len(blocks)}"
        total += len(blocks)
    assert total == 22


def test_no_string_err_in_target_components() -> None:
    for name in TARGET_FILES:
        src = _read(name)
        assert _BANNED_STRING_ERR not in src, f"{name} still has {_BANNED_STRING_ERR}"
        assert _BANNED_STRING_ERROR not in src, f"{name} still has {_BANNED_STRING_ERROR}"


def test_catch_blocks_do_not_leak_exception_fields() -> None:
    banned_in_catch = [
        ".message",
        ".stack",
        "JSON.stringify",
        "console.log",
        "console.error",
        "${err}",
        "${error}",
        "`${err",
        "`${error",
    ]
    for name in TARGET_FILES:
        for block in _ui_error_catches(_read(name)):
            for token in banned_in_catch:
                assert token not in block, f"{name} catch contains {token!r}"


def test_all_catches_use_fixed_keys() -> None:
    for name, keys in EXPECTED_CATCH_KEYS.items():
        blocks = _ui_error_catches(_read(name))
        found: list[str] = []
        for block in blocks:
            m = re.search(r'uiErrorMessage\(\s*"([A-Z0-9_]+)"\s*\)', block)
            assert m, f"{name} catch missing uiErrorMessage fixed key:\n{block}"
            found.append(m.group(1))
        assert found == keys, f"{name}: keys {found} != {keys}"


def test_import_error_catch_has_no_file_name() -> None:
    src = _read("ImportTab.tsx")
    for block in _ui_error_catches(src):
        assert "file.name" not in block
        assert "file?.name" not in block


def test_helper_api_and_exact_twenty_one_keys() -> None:
    assert HELPER.is_file(), "uiErrorMessages.ts missing"
    src = HELPER.read_text(encoding="utf-8")
    # Must not accept an error/exception argument.
    assert re.search(r"function\s+uiErrorMessage\s*\(\s*code\s*:", src)
    assert not re.search(r"function\s+uiErrorMessage\s*\([^)]*err", src)
    assert not re.search(r"function\s+uiErrorMessage\s*\([^)]*error", src)
    keys = re.findall(r"^\s{2}([A-Z0-9_]+):\s*\{", src, flags=re.M)
    assert len(keys) == 21, f"expected 21 keys, got {len(keys)}: {keys}"
    assert set(keys) == RETRY_SAFE | VERIFY_FIRST


def test_retry_policy_aligns_with_rust_replay_policy() -> None:
    helper = HELPER.read_text(encoding="utf-8")
    rust = ENGINE_RS.read_text(encoding="utf-8")
    # Retry-safe UI keys must correspond to RetryOnceAfterRestart cmds present in Rust.
    for cmd in RETRY_SAFE_CMDS:
        assert f'"{cmd}"' in rust or f"| \"{cmd}\"" in rust or cmd in rust
    m = re.search(
        r"\|\s*\"context\.manifest\.latest\"\s*=>\s*ReplayPolicy::RetryOnceAfterRestart",
        rust,
    )
    assert m, "replay_policy RetryOnceAfterRestart arm missing"
    for key in RETRY_SAFE:
        assert f"{key}:" in helper
        assert re.search(
            rf"{key}:\s*\{{[^}}]*retryPolicy:\s*\"retry-safe\"",
            helper,
            flags=re.S,
        ), f"{key} must be retry-safe"
    for key in VERIFY_FIRST:
        assert re.search(
            rf"{key}:\s*\{{[^}}]*retryPolicy:\s*\"verify-first\"",
            helper,
            flags=re.S,
        ), f"{key} must be verify-first"
        # Extract message for this key
        mm = re.search(
            rf'{key}:\s*\{{\s*message:\s*"([^"]+)"',
            helper,
        )
        assert mm, f"message missing for {key}"
        assert "必要な場合だけ再実行" in mm.group(1), f"{key} verify-first wording"


def test_fixed_messages_have_no_path_json_exception_tokens() -> None:
    helper = HELPER.read_text(encoding="utf-8")
    messages = re.findall(r'message:\s*"([^"]+)"', helper)
    assert len(messages) == 21
    banned = [
        "ValueError",
        "RuntimeError",
        "Traceback",
        "Error:",
        "\\\\",
        "/",
        ".py",
        ".tsx",
        "{",
        "}",
        "exception",
        "Exception",
    ]
    # Allow Japanese 『。』 only; path-like tokens banned.
    for msg in messages:
        for b in banned:
            if b in ("/", "\\\\"):
                # Absolute / relative path patterns
                assert not re.search(r"[A-Za-z]:\\", msg)
                assert not re.search(r"(?:^|[\s\"'])/(?:Users|home|tmp|var)/", msg)
                continue
            assert b not in msg, f"banned token {b!r} in {msg!r}"


def test_error_display_roles_and_classes() -> None:
    probe = _read("ProbeTab.tsx")
    assert 'role="alert"' in probe
    assert "error-text" in probe

    record = _read("RecordTab.tsx")
    assert re.search(r'role=\{[^}]*["\']alert["\']', record) or 'role="alert"' in record
    assert "error-text" in record

    profile = _read("ProfileTab.tsx")
    assert 'role="alert"' in profile
    assert "error-text" in profile

    settings = _read("SettingsTab.tsx")
    assert 'role="alert"' in settings
    assert "error-text" in settings

    interview = _read("InterviewTab.tsx")
    assert re.search(r'role=\{[^}]*["\']alert["\']', interview) or 'role="alert"' in interview
    assert re.search(r'role=\{[^}]*["\']status["\']', interview) or 'role="status"' in interview
    assert "statusKind" in interview

    consult = _read("ConsultTab.tsx")
    assert re.search(r'role=\{[^}]*["\']alert["\']', consult) or 'role="alert"' in consult
    assert re.search(r'role=\{[^}]*["\']status["\']', consult) or 'role="status"' in consult
    assert "statusKind" in consult

    import_tab = _read("ImportTab.tsx")
    assert 'role="log"' in import_tab
    assert 'aria-live="polite"' in import_tab
    assert 'aria-relevant="additions text"' in import_tab


def test_record_error_notice_not_auto_hidden() -> None:
    src = _read("RecordTab.tsx")
    # Success may use 3000ms timer; error path must not schedule hide.
    assert "3000" in src
    # showNotice must gate timer on success only.
    assert re.search(
        r"kind\s*===\s*[\"']success[\"'][\s\S]{0,120}setTimeout|setTimeout[\s\S]{0,120}kind\s*===\s*[\"']success[\"']",
        src,
    ) or re.search(
        r"if\s*\(\s*kind\s*===\s*[\"']success[\"']\s*\)[\s\S]{0,200}setTimeout",
        src,
    )


def test_context_observatory_fixed_error_preserved() -> None:
    mfs = (LIB / "manifestFetchState.ts").read_text(encoding="utf-8")
    assert "MANIFEST_FETCH_ERROR_MESSAGE" in mfs
    assert "コンテキストマニフェストの取得または検証に失敗しました" in mfs
    # REQUEST_FAILURE must not carry exception payload fields.
    assert "REQUEST_FAILURE" in mfs
    coc = (COMP / "ContextObservatoryContainer.tsx").read_text(encoding="utf-8")
    assert "MANIFEST_FETCH_ERROR_MESSAGE" in coc or "errorMessage" in coc


def test_finding12_files_untouched_by_this_contract_scope() -> None:
    # This finding must not require edits to Finding 12 production files.
    # Presence-only: engine_stdio still has sterile diag; engine.rs still has collector.
    stdio = (ROOT / "src" / "python" / "engine_stdio.py").read_text(encoding="utf-8")
    assert "PKB_DIAG_V1" in stdio
    assert "traceback.print_exc" not in stdio


def test_backend_status_and_success_message_paths_retained() -> None:
    # Do not delete payload.message / res.message success-progress paths.
    interview = _read("InterviewTab.tsx")
    consult = _read("ConsultTab.tsx")
    assert "payload.message" in interview or "payload?.message" in interview
    assert "payload.message" in consult or "payload?.message" in consult
    import_tab = _read("ImportTab.tsx")
    assert "res.message" in import_tab or "summary.message" in import_tab
