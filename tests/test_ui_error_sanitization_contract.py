# -*- coding: utf-8 -*-
"""Finding 13 — UI catch sites must use operation-keyed sterile messages.

T-5 debt repayment (2026-07-28): rewritten as an inclusion-based contract
instead of a brittle fixed-count/fixed-order snapshot. The registry
(`uiErrorMessages.ts`) is allowed to grow (GD_ARENA / BXS_ARENA_* /
INTERVIEW_FALLBACK_* / RAG_* were added after the original Finding 13 pass)
without breaking this test, as long as:
  1. every REQUIRED_KEYS entry still exists in UI_ERROR_SPECS (inclusion,
     not exact count),
  2. every UI catch site that surfaces text to the user routes through
     uiErrorMessage("KEY") with a KEY that is actually registered,
  3. no catch site leaks exception internals into UI-facing state (String(err),
     .message, .stack, JSON.stringify, console.log(err), template-interpolated
     err/error) — console.error is exempt (§4.52a-3 requires it; it never
     reaches the rendered UI, only the developer console), and
  4. retry-policy tagging (retry-safe / verify-first) stays aligned with the
     Rust replay_policy for the keys that matter operationally.
"""
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

# Approved dedicated sanitizer (see lib/lineImportFeedback.ts) — never echoes
# raw exception text, only maps controlled `LINE_IMPORT:CODE` suffixes to
# fixed Japanese strings. Accepted as an alternative to uiErrorMessage() for
# the on-device LINE import fallback paths in ImportTab.tsx.
_APPROVED_SANITIZERS = ("uiErrorMessage(", "sterileLineImportFromUnknown(")

# Inclusion set — every key here MUST exist in UI_ERROR_SPECS. This is a
# floor, not a ceiling: new legitimate keys may be added without editing
# this test (see class docstring).
REQUIRED_KEYS = {
    "PROBE_STATUS_LOAD",
    "PROBE_NEXT",
    "PROBE_ANSWER",
    "RECORD_LOAD",
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
    "KNOWLEDGE_RESEARCH",
    "PROFILE_LOAD",
    "ORACLE_REPORT",
    "TWIN_FORECAST",
    "TENSOR_REBUILD",
    "SETTINGS_LOAD",
    "SETTINGS_SAVE",
    "PROFILER_RUN",
    "RAG_CHAT",
    "RAG_MODEL_NOT_LOADED",
    "INTERVIEW_FALLBACK_THINKING",
    "INTERVIEW_FALLBACK_MODEL_COLD",
    "INTERVIEW_FALLBACK_VAULT_LOCKED",
    "INTERVIEW_FALLBACK_TOO_LONG",
    "GD_ARENA",
    "BXS_ARENA_REJECTED",
    "BXS_ARENA_STATE",
    "BXS_ARENA_FAULT",
}

RETRY_SAFE = {
    "PROBE_STATUS_LOAD",
    "RECORD_LOAD",
    "PROFILE_LOAD",
    "TWIN_FORECAST",
    "SETTINGS_LOAD",
    "RAG_CHAT",
    "RAG_MODEL_NOT_LOADED",
    "INTERVIEW_FALLBACK_THINKING",
    "INTERVIEW_FALLBACK_MODEL_COLD",
    "INTERVIEW_FALLBACK_TOO_LONG",
    "BXS_ARENA_REJECTED",
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
    "KNOWLEDGE_RESEARCH",
    "ORACLE_REPORT",
    "TENSOR_REBUILD",
    "SETTINGS_SAVE",
    "PROFILER_RUN",
    "INTERVIEW_FALLBACK_VAULT_LOCKED",
    "GD_ARENA",
    "BXS_ARENA_STATE",
    "BXS_ARENA_FAULT",
}

assert REQUIRED_KEYS == RETRY_SAFE | VERIFY_FIRST, "every required key needs a retry-policy bucket"
assert not (RETRY_SAFE & VERIFY_FIRST), "a key cannot be both retry-safe and verify-first"

# Verify-first keys carry a standard "確認してから再実行" template. The
# interview-fallback keys are conversational filler for a stalled/cold LLM,
# not operation-result reports, so they are exempt from the template wording
# (they still must be tagged verify-first / retry-safe correctly, see above).
VERIFY_FIRST_WORDING_EXEMPT = {"INTERVIEW_FALLBACK_VAULT_LOCKED"}

# Finding 2 replay_policy RetryOnceAfterRestart cmds (subset used by UI keys).
RETRY_SAFE_CMDS = {
    "probe.status",
    "record.load",
    "settings.get",
    "oracle.payload",
    "twin.forecast",
    "profile.source_code",
}

# Per-file inclusion floor: keys that MUST appear in at least one UI-error
# catch of that file. This replaces the old exact ordered-list/count
# equality (brittle against legitimate refactors that add/merge catches).
REQUIRED_CATCH_KEYS = {
    "ProbeTab.tsx": {"PROBE_STATUS_LOAD", "PROBE_NEXT", "PROBE_ANSWER"},
    "RecordTab.tsx": {"RECORD_LOAD", "RECORD_SAVE"},
    "InterviewTab.tsx": {"INTERVIEW_RESPONSE", "NARRATIVE_COMPILE"},
    "ConsultTab.tsx": {"ROMANCE_ANALYSIS", "CONSULT_RESPONSE"},
    "ImportTab.tsx": {"ICS_SYNC", "APPLE_CALENDAR_SYNC", "DOCUMENT_IMPORT", "KNOWLEDGE_FETCH"},
    "ProfileTab.tsx": {"PROFILE_LOAD", "ORACLE_REPORT", "TWIN_FORECAST", "TENSOR_REBUILD", "PROFILER_RUN"},
    "SettingsTab.tsx": {"SETTINGS_SAVE"},
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
    """Catches that surface user-visible error text — ignore best-effort/
    silent swallows and non-error informational fallbacks (e.g. a catch that
    only sets statusKind("info") to keep working offline is not an "error"
    catch and is not required to route through the sterile registry, since
    it never carries an error message at all).
    """
    out: list[str] = []
    for block in _catch_blocks(src):
        if any(tok in block for tok in _APPROVED_SANITIZERS):
            out.append(block)
            continue
        if _BANNED_STRING_ERR in block or _BANNED_STRING_ERROR in block:
            out.append(block)
            continue
        if any(tok in block for tok in ("setError(", "pushImportLog(", "setNarrativeResult(")):
            out.append(block)
            continue
        if 'setStatusKind("error")' in block or re.search(r'kind:\s*"error"', block):
            out.append(block)
            continue
    return out


def test_required_keys_exist_in_registry() -> None:
    assert HELPER.is_file(), "uiErrorMessages.ts missing"
    src = HELPER.read_text(encoding="utf-8")
    keys = set(re.findall(r"^\s{2}([A-Z0-9_]+):\s*\{", src, flags=re.M))
    missing = REQUIRED_KEYS - keys
    assert not missing, f"required keys missing from UI_ERROR_SPECS: {sorted(missing)}"


def test_no_string_err_in_target_components() -> None:
    for name in TARGET_FILES:
        src = _read(name)
        assert _BANNED_STRING_ERR not in src, f"{name} still has {_BANNED_STRING_ERR}"
        assert _BANNED_STRING_ERROR not in src, f"{name} still has {_BANNED_STRING_ERROR}"


def test_catch_blocks_do_not_leak_exception_fields() -> None:
    """Finding 13 (§16.4.1) bans leaking exception *values* into UI-facing
    state/DOM/aria: String(err)/.message/.stack/JSON.stringify/template
    interpolation/console.log(err). It does NOT ban console.error — §4.52a-3
    (a separate, non-negotiable rule born from a multi-day debugging
    incident, see .cursorrules "LLMトークン予算" + AI_SKILLS.md LAW-06)
    *requires* every FE catch to console.error the raw error before showing
    sterile text ("ログは生・表示は無菌" — log raw, display sterile). The two
    rules are complementary, not in tension: console.error never reaches the
    rendered UI, so it cannot violate Finding 13.
    """
    banned_in_catch = [
        ".message",
        ".stack",
        "JSON.stringify",
        "console.log",
        "${err}",
        "${error}",
        "`${err",
        "`${error",
    ]
    for name in TARGET_FILES:
        for block in _catch_blocks(_read(name)):
            for token in banned_in_catch:
                assert token not in block, f"{name} catch contains {token!r}"


def test_catch_sites_use_registered_keys() -> None:
    registry_src = HELPER.read_text(encoding="utf-8")
    registered = set(re.findall(r"^\s{2}([A-Z0-9_]+):\s*\{", registry_src, flags=re.M))
    for name in TARGET_FILES:
        src = _read(name)
        for block in _ui_error_catches(src):
            if "sterileLineImportFromUnknown(" in block and "uiErrorMessage(" not in block:
                # Dedicated Finding-13-compliant sanitizer (lib/lineImportFeedback.ts);
                # does not use the operation-keyed registry by design.
                continue
            found = re.findall(r'uiErrorMessage\(\s*"([A-Z0-9_]+)"\s*\)', block)
            assert found, f"{name} catch missing uiErrorMessage fixed key:\n{block}"
            for key in found:
                assert key in registered, f"{name} catch uses unregistered key {key!r}"


def test_required_catch_keys_present_per_file() -> None:
    for name, required in REQUIRED_CATCH_KEYS.items():
        src = _read(name)
        found_keys: set[str] = set()
        for block in _ui_error_catches(src):
            found_keys.update(re.findall(r'uiErrorMessage\(\s*"([A-Z0-9_]+)"\s*\)', block))
        missing = required - found_keys
        assert not missing, f"{name}: missing required catch keys {sorted(missing)} (found {sorted(found_keys)})"


def test_import_error_catch_has_no_file_name() -> None:
    """No catch block in ImportTab.tsx may reference file.name/file?.name —
    capture `const fileLabel = file.name` before the try and reuse it."""
    src = _read("ImportTab.tsx")
    for block in _catch_blocks(src):
        assert "file.name" not in block, f"ImportTab.tsx catch still references file.name:\n{block}"
        assert "file?.name" not in block


def test_helper_api_and_no_error_argument() -> None:
    assert HELPER.is_file(), "uiErrorMessages.ts missing"
    src = HELPER.read_text(encoding="utf-8")
    # Must not accept an error/exception argument.
    assert re.search(r"function\s+uiErrorMessage\s*\(\s*code\s*:", src)
    assert not re.search(r"function\s+uiErrorMessage\s*\([^)]*err", src)
    assert not re.search(r"function\s+uiErrorMessage\s*\([^)]*error", src)
    keys = re.findall(r"^\s{2}([A-Z0-9_]+):\s*\{", src, flags=re.M)
    assert len(keys) == len(set(keys)), "duplicate keys in UI_ERROR_SPECS"
    assert REQUIRED_KEYS.issubset(set(keys)), "registry regressed: required keys missing"


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
        assert f"{key}:" in helper
        assert re.search(
            rf"{key}:\s*\{{[^}}]*retryPolicy:\s*\"verify-first\"",
            helper,
            flags=re.S,
        ), f"{key} must be verify-first"
        if key in VERIFY_FIRST_WORDING_EXEMPT:
            continue
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
    assert messages, "no messages found in UI_ERROR_SPECS"
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
    # RecordTab uses the sys-log--err terminal styling instead of error-text.
    assert "sys-log--err" in record

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
    assert "payload.message" in interview or "payload?.message" in interview
    import_tab = _read("ImportTab.tsx")
    assert "res.message" in import_tab or "summary.message" in import_tab
