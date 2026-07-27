# -*- coding: utf-8 -*-
"""BLACKBOX SIMULATOR structural contracts (docs/SPEC_BLACKBOX_SIMULATOR.md).

Four guard families, all read-only (sandbox-safe):

1. SPEC structural contract — the design document keeps the skeleton other
   artifacts reference (§19.2 discipline: structure is a contract).
2. Cross-language determinism anchor (BXS-I-06) — a stdlib-only Python
   mirror of Philox4x32-10 must reproduce the Random123 published
   known-answer vectors. The Rust side asserts the same vectors in
   `blackbox_sim/rng.rs`; both sides agreeing with the published constants
   is the PKBVEC01-style mutual pin.
3. Isolation-wall guards (BXS-I-11 / BXS-W-02) — the simulator core must
   not import profile/vault/LLM machinery (wall W-b: the game never reads
   the profile), and must not call libm transcendentals (float policy,
   SPEC §3.3). String scan includes comments deliberately (the same posture
   as the constitution's reqwest scan).
4. Compilation-reachability and totality guards (BXS-W-10 / BXS-I-18) — every
   source file must actually be declared in `mod.rs`, and every account must
   appear in the cash-flow section map. Both failures are silent in Rust:
   the first reports GREEN while skipping a file, the second only bites once
   someone replaces an exhaustive match with a wildcard.
5. Phase 3 record-lane guards — the published stimulus view must not name a
   sealed field (wall W-a), the sink port must stay write-only (wall W-b), and
   the vault v12 lane must stay append-only (eighth law). All three are the
   quiet kind: adding a field, adding a trait method or reaching for `UPDATE`
   compiles, passes, and silently dismantles the instrument.
6. Phase 4 fixture-blindness guard (SPEC §12 / BXS-I-23) — `bias.rs` (the
   estimator) and `phantom_bot.rs` (the calibration BOT) must never import
   each other; `calibration.rs` is the ONE file allowed to import both. A
   BOT tuned to what the estimator expects makes calibration tautological
   rather than a real recovery test — this guard keeps that impossible by
   construction, not by review discipline.
7. Phase 5 IPC boundary guards — `blackbox_arena` (new in P5) is the only
   module besides `db/` itself allowed to import `blackbox_sim` (wall W-b's
   asymmetry: the simulator must not read `db`/`analytics`, but `db` and the
   new orchestration module may read the simulator). The command-surface
   request types stay a closed, `deny_unknown_fields` vocabulary (§4.12a-2 /
   §15). And the P4→P5 handoff's explicit non-goal — no production caller
   for `bias::estimate_profile` or the three `Session` estimator accessors —
   is re-checked here, since P6's double gate (§11) is the only place that
   may open it.
"""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = ROOT / "docs" / "SPEC_BLACKBOX_SIMULATOR.md"
TAURI_SRC = ROOT / "apps" / "desktop" / "src-tauri" / "src"
SIM_DIR = TAURI_SRC / "blackbox_sim"
DB_DIR = TAURI_SRC / "db"
ARENA_DIR = TAURI_SRC / "blackbox_arena"

# ---------------------------------------------------------------------------
# 1. SPEC structural contract
# ---------------------------------------------------------------------------

REQUIRED_SPEC_MARKERS = [
    "## 0. 権威と裁定要請",
    "## 2. 三層アーキテクチャと隔離壁",
    "壁 W-a",
    "壁 W-b",
    "壁 W-c",
    "壁 W-d",
    "Philox4x32-10",
    "## 7. ゲーム FSM",
    "## 12. 校正 — PHANTOM-BOT 既知解ゲート",
    "BXS-I-01",
    "BXS-I-11",
    "BXS-W-03",
    "blackbox_profile.v1",
    "uncalibrated-instrument",
    # Phase 1: wall W-d made concrete, plus the traps that phase actually hit.
    "BXS-I-15",
    "BXS-I-16",
    "BXS-W-08",
    "BXS-W-10",
    "## 19. Phase 1 as-built",
    # Phase 2: FirmState, the settle loop, snapshots and the turn driver.
    "BXS-I-17",
    "BXS-I-18",
    "BXS-I-19",
    "BXS-W-11",
    "BXS-W-12",
    "BXS-W-14",
    "## 20. Phase 2 as-built",
    # Phase 3: stimulus planting, the sink port and the vault v12 record lane.
    "BXS-I-20",
    "BXS-I-21",
    "BXS-I-22",
    "BXS-W-16",
    "BXS-W-17",
    "BXS-W-18",
    "## 21. Phase 3 as-built",
    # Phase 4: the six-lane estimator and the PHANTOM-BOT calibration suite.
    "BXS-I-23",
    "BXS-I-24",
    "BXS-I-25",
    "BXS-W-19",
    "BXS-W-20",
    "BXS-W-21",
    "## 22. Phase 4 as-built",
    # Phase 5 (backend slice): the IPC command layer + vault persistence
    # wiring, with FE/flavor explicitly deferred to a follow-up ruling.
    "## 23. Phase 5 as-built",
    "blackbox_arena",
    "CollectingSink",
]


def test_spec_document_structure() -> None:
    assert SPEC.exists(), "SPEC_BLACKBOX_SIMULATOR.md missing"
    text = SPEC.read_text(encoding="utf-8")
    missing = [m for m in REQUIRED_SPEC_MARKERS if m not in text]
    assert not missing, f"SPEC skeleton markers missing: {missing}"


# ---------------------------------------------------------------------------
# 2. Philox4x32-10 cross-language anchor (stdlib only — no numpy)
# ---------------------------------------------------------------------------

_M0 = 0xD2511F53
_M1 = 0xCD9E8D57
_W0 = 0x9E3779B9
_W1 = 0xBB67AE85
_MASK = 0xFFFFFFFF


def _round(ctr: tuple[int, int, int, int], key: tuple[int, int]) -> tuple[int, int, int, int]:
    x0, x1, x2, x3 = ctr
    k0, k1 = key
    p0 = _M0 * x0
    p1 = _M1 * x2
    hi0, lo0 = (p0 >> 32) & _MASK, p0 & _MASK
    hi1, lo1 = (p1 >> 32) & _MASK, p1 & _MASK
    return ((hi1 ^ x1 ^ k0) & _MASK, lo1, (hi0 ^ x3 ^ k1) & _MASK, lo0)


def philox4x32_10(ctr: tuple[int, int, int, int], key: tuple[int, int]) -> tuple[int, int, int, int]:
    c, k = ctr, key
    for r in range(10):
        c = _round(c, k)
        if r < 9:
            k = ((k[0] + _W0) & _MASK, (k[1] + _W1) & _MASK)
    return c


def test_philox_known_answer_vectors() -> None:
    # Random123 published KAT vectors — identical constants are asserted by
    # blackbox_sim/rng.rs (mutual cross-language pin, BXS-I-06).
    assert philox4x32_10((0, 0, 0, 0), (0, 0)) == (
        0x6627E8D5,
        0xE169C58D,
        0xBC57AC4C,
        0x9B00DBD8,
    )
    assert philox4x32_10(
        (0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF), (0xFFFFFFFF, 0xFFFFFFFF)
    ) == (0x408F276D, 0x41C83B0E, 0xA20BC7C6, 0x6D5451FD)
    assert philox4x32_10(
        (0x243F6A88, 0x85A308D3, 0x13198A2E, 0x03707344), (0xA4093822, 0x299F31D0)
    ) == (0xD16CFE09, 0x94FDCCEB, 0x5001E420, 0x24126EA1)


# ---------------------------------------------------------------------------
# 3. Isolation-wall guards over the Rust sources
# ---------------------------------------------------------------------------

# Wall W-b (BXS-I-11): the simulator core must be structurally unable to read
# profile/vault/LLM state. Includes comments on purpose.
FORBIDDEN_IMPORT_TOKENS = [
    "use crate::analytics",
    "use crate::db",
    "use crate::llm",
    "use crate::coliseum",
    "use crate::rag",
    "use crate::knowledge",
    "VaultHandle",
]

# Float policy (SPEC §3.3 / BXS-W-02): libm transcendentals drift across
# platforms; only +,−,×,÷,sqrt and in-crate det_exp/det_ln are allowed.
FORBIDDEN_LIBM_TOKENS = [
    ".exp(",
    ".exp2(",
    ".exp_m1(",
    ".ln(",
    ".ln_1p(",
    ".log(",
    ".log2(",
    ".log10(",
    ".sin(",
    ".cos(",
    ".tan(",
    ".powf(",
    ".powi(",
]

REQUIRED_LINT_DENIES = [
    "clippy::unwrap_used",
    "clippy::expect_used",
    "clippy::panic",
    "clippy::indexing_slicing",
    "clippy::string_slice",
]


def _sim_sources() -> list[Path]:
    assert SIM_DIR.exists(), "blackbox_sim module directory missing"
    files = sorted(SIM_DIR.glob("*.rs"))
    assert files, "blackbox_sim module has no Rust sources"
    return files


def test_no_profile_dependency() -> None:
    offenders: list[str] = []
    for path in _sim_sources():
        text = path.read_text(encoding="utf-8")
        for token in FORBIDDEN_IMPORT_TOKENS:
            if token in text:
                offenders.append(f"{path.name}: {token}")
    assert not offenders, f"wall W-b breached (BXS-I-11): {offenders}"


def test_no_libm_transcendentals() -> None:
    offenders: list[str] = []
    for path in _sim_sources():
        text = path.read_text(encoding="utf-8")
        for token in FORBIDDEN_LIBM_TOKENS:
            if token in text:
                offenders.append(f"{path.name}: {token}")
    assert not offenders, f"float policy breached (BXS-W-02): {offenders}"


def test_zero_panic_lint_header_present() -> None:
    mod_rs = SIM_DIR / "mod.rs"
    assert mod_rs.exists(), "blackbox_sim/mod.rs missing"
    text = mod_rs.read_text(encoding="utf-8")
    missing = [lint for lint in REQUIRED_LINT_DENIES if lint not in text]
    assert not missing, f"Zero Panic deny header incomplete: {missing}"


def test_every_module_is_declared() -> None:
    """BXS-W-10: a file absent from mod.rs is never compiled, and `cargo test`
    reports GREEN while silently skipping it. Four Phase 0 files reached the
    Phase 1 handoff in exactly that state. This is the structural stop."""
    mod_rs = SIM_DIR / "mod.rs"
    declarations = mod_rs.read_text(encoding="utf-8")
    undeclared = [
        path.name
        for path in _sim_sources()
        if path.name != "mod.rs" and f"pub mod {path.stem};" not in declarations
    ]
    assert not undeclared, (
        f"blackbox_sim modules exist on disk but are never compiled: {undeclared}"
    )


def test_every_account_is_classified_for_cash_flow() -> None:
    """BXS-I-18: the cash-flow identity is a theorem only while the account →
    section map is total. Rust's exhaustive match enforces this at compile
    time; this guard states the same contract across files, so it still holds
    if someone reaches for a wildcard arm."""
    ledger = (SIM_DIR / "ledger.rs").read_text(encoding="utf-8")
    settle = (SIM_DIR / "settle.rs").read_text(encoding="utf-8")
    block = re.search(r"pub enum AccountCode \{(.*?)\n\}", ledger, re.DOTALL)
    assert block is not None, "AccountCode enum not found in ledger.rs"
    accounts = re.findall(r"^\s*([A-Z]\w*)\s*=\s*\d+,", block.group(1), re.MULTILINE)
    assert len(accounts) >= 20, f"AccountCode parse looks wrong: {accounts}"
    unclassified = [a for a in accounts if f"AccountCode::{a}" not in settle]
    assert not unclassified, (
        f"accounts missing from the cash-flow section map (BXS-I-18): {unclassified}"
    )


# ---------------------------------------------------------------------------
# 5. Phase 3 record-lane guards
# ---------------------------------------------------------------------------

# Wall W-a (BXS-I-20): these name the answer key. A stimulus is only a
# measurement while the subject cannot see the value being measured against,
# so `StimulusView` — the one type that reaches the UI and the LLM — must not
# carry any of them. Adding such a field compiles and reads like a feature.
SEALED_STIMULUS_FIELDS = [
    "reference_minor",
    "delta_micro",
    "good_state",
    "sunk_minor",
    "required_cash_minor",
    "arm",
]


def test_published_stimulus_view_names_no_sealed_field() -> None:
    text = (SIM_DIR / "stimulus.rs").read_text(encoding="utf-8")
    block = re.search(r"pub struct StimulusView \{(.*?)\n\}", text, re.DOTALL)
    assert block is not None, "StimulusView struct not found in stimulus.rs"
    body = block.group(1)
    leaked = [field for field in SEALED_STIMULUS_FIELDS if field in body]
    assert not leaked, f"wall W-a breached — sealed fields published (BXS-I-20): {leaked}"


def test_the_sink_port_is_write_only() -> None:
    """BXS-I-21: wall W-b holds by the shape of the port, not by good manners.
    `DecisionSink` has exactly one method and it takes a batch and returns a
    receipt. The moment a read method appears the simulator can consult the
    vault, and the game starts reading the profile it is supposed to measure."""
    text = (SIM_DIR / "persist.rs").read_text(encoding="utf-8")
    block = re.search(r"pub trait DecisionSink \{(.*?)\n\}", text, re.DOTALL)
    assert block is not None, "DecisionSink trait not found in persist.rs"
    methods = re.findall(r"\bfn (\w+)", block.group(1))
    assert methods == ["persist"], (
        f"the sink port must stay write-only (BXS-I-21); found methods: {methods}"
    )


def test_the_vault_record_lane_is_append_only() -> None:
    """Eighth law / BXS-I-22: a record already written is not the repository's
    to revise. Re-flushing after a crash must be a no-op, which is why every
    write to the two record tables is `INSERT OR IGNORE` and why neither table
    is ever the target of an UPDATE or a DELETE."""
    repo = DB_DIR / "blackbox_repo.rs"
    assert repo.exists(), "db/blackbox_repo.rs missing"
    text = repo.read_text(encoding="utf-8")

    record_tables = ("blackbox_decisions", "blackbox_stimuli")
    mutations = [
        line.strip()
        for line in text.splitlines()
        if re.search(r"\b(UPDATE|DELETE FROM)\b", line)
        and any(table in line for table in record_tables)
    ]
    assert not mutations, f"records are not editable (BXS-I-22): {mutations}"

    inserts = re.findall(r"INSERT(?: OR IGNORE)? INTO (blackbox_\w+)", text)
    assert set(record_tables).issubset(set(inserts)), (
        f"expected inserts into both record tables, found: {inserts}"
    )
    unguarded = re.findall(r"INSERT INTO (blackbox_decisions|blackbox_stimuli)", text)
    assert not unguarded, (
        f"record inserts must be `INSERT OR IGNORE` so a retry is a no-op: {unguarded}"
    )


# ---------------------------------------------------------------------------
# 6. Phase 4 fixture-blindness guard
# ---------------------------------------------------------------------------


def test_bias_and_phantom_bot_never_import_each_other() -> None:
    """BXS-I-23 / SPEC §12: the estimator (`bias.rs`) and the calibration
    BOT (`phantom_bot.rs`) must have zero import edges between them in
    either direction. `calibration.rs` is the one file allowed to import
    both — that is where recovery is actually checked. A BOT written
    against the estimator's own constants (or an estimator quietly special
    -cased for the BOT's outputs) would make every calibration test
    tautological; this guard makes that impossible to introduce silently."""
    bias_text = (SIM_DIR / "bias.rs").read_text(encoding="utf-8")
    bot_text = (SIM_DIR / "phantom_bot.rs").read_text(encoding="utf-8")
    # Scoped to actual `use` edges (not doc comments describing the wall
    # itself, which legitimately name the other module by name).
    assert re.search(r"^use\s+\S*phantom_bot", bias_text, re.MULTILINE) is None, (
        "wall breached (BXS-I-23): bias.rs imports phantom_bot"
    )
    assert re.search(r"^use\s+\S*\bbias\b", bot_text, re.MULTILINE) is None, (
        "wall breached (BXS-I-23): phantom_bot.rs imports bias"
    )
    calibration_text = (SIM_DIR / "calibration.rs").read_text(encoding="utf-8")
    assert "bias::" in calibration_text or "use crate::blackbox_sim::bias" in calibration_text, (
        "calibration.rs must be the file that actually imports bias.rs"
    )
    assert "phantom_bot::" in calibration_text or "use crate::blackbox_sim::phantom_bot" in calibration_text, (
        "calibration.rs must be the file that actually imports phantom_bot.rs"
    )


def test_calibration_certificate_has_no_production_constructor() -> None:
    """BXS-I-24 / SPEC §11: `CalibrationCertificate` must stay uninstantiable
    outside tests (Commander's Option-A ruling, 2026-07-27) — no `#[cfg(test)]`
    workaround that leaks a constructor into a non-test path, and no second
    constructor added elsewhere that bypasses `test_only()`'s gate."""
    text = (SIM_DIR / "bias.rs").read_text(encoding="utf-8")
    block = re.search(r"impl CalibrationCertificate \{(.*?)\n\}", text, re.DOTALL)
    assert block is not None, "CalibrationCertificate impl block not found in bias.rs"
    body = block.group(1)
    fns = re.findall(r"pub(?:\(crate\))?\s+fn (\w+)", body)
    assert fns == ["test_only"], (
        f"CalibrationCertificate must expose exactly one, test-gated constructor: {fns}"
    )
    assert "#[cfg(test)]\n    pub(crate) fn test_only" in text, (
        "test_only() must be #[cfg(test)]-gated, not reachable from production"
    )


def test_refusal_classifier_is_an_allowlist_not_a_catchall() -> None:
    """BXS-I-25: the Commander's refusal-telemetry ruling (2026-07-27) scoped
    logging to "valid intent, business-rule refused, under an active
    stimulus" — not every `CompileError`. `classify_refusal` must stay a
    `Some(..) => ...` allowlist ending in a `_ => None` catch-none, so a
    newly added `CompileError` variant defaults to unlogged rather than
    silently joining the escalation/pressure signal."""
    text = (SIM_DIR / "director.rs").read_text(encoding="utf-8")
    block = re.search(
        r"fn classify_refusal\(err: &CompileError\) -> Option<RefusalReason> \{(.*?)\n\}",
        text,
        re.DOTALL,
    )
    assert block is not None, "classify_refusal not found in director.rs"
    body = block.group(1)
    assert re.search(r"_\s*=>\s*None", body), (
        "classify_refusal must end in a `_ => None` catch-none (BXS-I-25)"
    )


def test_vault_v12_stores_no_campaign_seed() -> None:
    """Wall W-a again, at rest. The Genesis seed regenerates every true value in
    the campaign, so persisting it would put the answer key inside the same
    database the analysis lane reads. The campaign row keys on the fingerprint,
    which identifies a campaign without being able to reconstruct it."""
    text = (DB_DIR / "migrations.rs").read_text(encoding="utf-8")
    assert "pub(crate) const LATEST_SCHEMA_VERSION: i64 = 12;" in text, (
        "vault v12 is not the latest schema version"
    )
    block = re.search(r"CREATE TABLE blackbox_campaigns \((.*?)\n\);", text, re.DOTALL)
    assert block is not None, "blackbox_campaigns DDL not found in migrations.rs"
    columns = re.findall(r"^\s*(\w+)\s+\w+", block.group(1), re.MULTILINE)
    assert "campaign_fingerprint" in columns, f"fingerprint column missing: {columns}"
    seedy = [column for column in columns if "seed" in column.lower()]
    assert not seedy, f"wall W-a breached at rest — seed persisted: {seedy}"


# ---------------------------------------------------------------------------
# 7. Phase 5 IPC boundary guards
# ---------------------------------------------------------------------------


def test_only_blackbox_arena_and_db_import_the_simulator() -> None:
    """Wall W-b's asymmetry, extended for Phase 5: BXS-I-11 (above) already
    forbids `blackbox_sim` from reading `db`/`analytics`. The reverse edge
    (something reading `blackbox_sim`) is allowed but must stay narrow —
    only `db/` (the vault worker's flush/load dispatch, `blackbox_repo.rs`'s
    persistence lane) and `blackbox_arena/` (the ONE new module the Phase 5
    plan names as allowed to import both `blackbox_sim` and `db`, same
    precedent as `calibration.rs` importing both `bias` and `phantom_bot`).
    No other module — analytics, llm, coliseum, rag, knowledge, UI-facing
    command layers — may reach into the simulator directly; everything else
    must go through `blackbox_arena`'s command surface."""
    importers = []
    for path in sorted(TAURI_SRC.rglob("*.rs")):
        if "blackbox_sim" in path.parts:
            continue
        text = path.read_text(encoding="utf-8")
        if re.search(r"\bcrate::blackbox_sim\b", text):
            importers.append(path)
    assert importers, "expected at least db/ and blackbox_arena/ to import blackbox_sim"
    offenders = [
        p.relative_to(TAURI_SRC).as_posix()
        for p in importers
        if "blackbox_arena" not in p.parts and "db" not in p.parts
    ]
    assert not offenders, (
        f"blackbox_sim imported outside blackbox_arena/db (wall W-b asymmetry): {offenders}"
    )
    assert any("blackbox_arena" in p.parts for p in importers), (
        "blackbox_arena must exist and actually import blackbox_sim"
    )


# Request-facing (Deserialize) structs/enums: derive line, optional serde
# attribute line(s), then the item keyword and name.
REQUEST_TYPE_RE = re.compile(
    r"#\[derive\(([^)]*)\)\]\n((?:#\[serde\([^\]]*\)\]\n)*)pub\(crate\)\s+(struct|enum)\s+(\w+)"
)


def test_blackbox_arena_request_types_are_closed() -> None:
    """SPEC §15 / §4.12a-2: every request-facing (`Deserialize`) type on the
    command surface is a closed vocabulary — structs are
    `#[serde(deny_unknown_fields)]` with `camelCase` fields, enums have
    `snake_case` variants (closed by construction: an unrecognized variant
    string simply fails to parse). No `serde_json::Value` escape hatch
    anywhere on this boundary."""
    assert ARENA_DIR.exists(), "blackbox_arena module directory missing"
    view_text = (ARENA_DIR / "view.rs").read_text(encoding="utf-8")
    commands_text = (ARENA_DIR / "commands.rs").read_text(encoding="utf-8")

    def _non_comment_lines(text: str) -> str:
        return "\n".join(
            line for line in text.splitlines() if not line.strip().startswith("//")
        )

    assert "serde_json::Value" not in _non_comment_lines(view_text), (
        "view.rs must not use serde_json::Value"
    )
    assert "serde_json::Value" not in _non_comment_lines(commands_text), (
        "commands.rs must not use serde_json::Value"
    )

    matches = REQUEST_TYPE_RE.findall(view_text)
    deserialize_items = [m for m in matches if "Deserialize" in m[0]]
    assert deserialize_items, "expected at least one Deserialize-able request type in view.rs"

    offenders = []
    for _derive, serde_attrs, kind, name in deserialize_items:
        if kind == "struct":
            if "deny_unknown_fields" not in serde_attrs or "camelCase" not in serde_attrs:
                offenders.append(name)
        else:  # enum
            if "snake_case" not in serde_attrs:
                offenders.append(name)
    assert not offenders, f"request types not closed per §4.12a-2/§15: {offenders}"


# Estimator double-gate (SPEC §11): `estimate_profile` calls always pass a
# reference as the first argument, which distinguishes call sites from the
# `pub fn estimate_profile(` definition line itself.
ESTIMATOR_CALL_SITE_RE = re.compile(
    r"\.events\(\)|\.refusals\(\)|\.pricing_trials\(\)|\bestimate_profile\(&"
)


def _line_of(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def test_estimator_double_gate_has_no_production_caller() -> None:
    """P4→P5 handoff's explicit non-goal (SPEC §11/§17): `bias::estimate_profile`
    and the three `Session` accessors it depends on (`events`/`refusals`/
    `pricing_trials`) must have zero call sites reachable from production —
    only P6's double gate (calibration GREEN AND commander ruling) may open
    that path. `pub(super)` visibility already makes the accessors
    uncallable from outside `blackbox_sim/` at compile time; this guard
    checks the source directly so a visibility relaxation does not silently
    widen the gate, and additionally checks that every in-module call site
    sits inside blackbox_sim's own `#[cfg(test)] mod tests` block."""
    outside_offenders = []
    for path in sorted(TAURI_SRC.rglob("*.rs")):
        if "blackbox_sim" in path.parts:
            continue
        text = path.read_text(encoding="utf-8")
        if ESTIMATOR_CALL_SITE_RE.search(text) or re.search(r"\bestimate_profile\(", text):
            outside_offenders.append(path.relative_to(TAURI_SRC).as_posix())
    assert not outside_offenders, (
        f"estimator double-gate breached — production caller outside blackbox_sim/: {outside_offenders}"
    )

    calibration_text = (SIM_DIR / "calibration.rs").read_text(encoding="utf-8")
    code_lines = [
        line
        for line in calibration_text.splitlines()
        if line.strip() and not line.strip().startswith("//")
    ]
    assert code_lines[:2] == ["#[cfg(test)]", "mod tests {"], (
        "calibration.rs must be #[cfg(test)]-only end to end (BXS-I-24's uninstantiable "
        "certificate must never become reachable from a non-test module)"
    )

    for filename in ("director.rs", "bias.rs"):
        text = (SIM_DIR / filename).read_text(encoding="utf-8")
        mod_match = re.search(r"\n#\[cfg\(test\)\]\nmod tests \{", text)
        assert mod_match is not None, f"{filename}: no #[cfg(test)] mod tests block found"
        mod_start_line = _line_of(text, mod_match.start())
        early = [
            _line_of(text, m.start())
            for m in ESTIMATOR_CALL_SITE_RE.finditer(text)
            if _line_of(text, m.start()) <= mod_start_line
        ]
        assert not early, (
            f"{filename}: estimator call site(s) at line(s) {early} sit outside "
            f"#[cfg(test)] mod tests (starts at line {mod_start_line})"
        )
