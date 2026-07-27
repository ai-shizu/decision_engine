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
    # Phase 5-B: Coliseum BLACKBOX Arena FE + BooksView/ArenaLimitsView DTO expansion.
    "## 24. Phase 5-B as-built",
    "BooksView",
    "ArenaLimitsView",
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
    is ever the target of an UPDATE or a DELETE.

    Scope note: the v13 *profile* lane uses `INSERT OR REPLACE` intentionally
    (snapshot semantics for a derived estimate). This guard scans only the
    record tables (`blackbox_decisions` / `blackbox_stimuli`); the profile
    lane is out of scope here."""
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
    assert "pub(crate) const LATEST_SCHEMA_VERSION: i64 = 13;" in text, (
        "the schema head moved without updating this contract"
    )
    block = re.search(r"CREATE TABLE blackbox_campaigns \((.*?)\n\);", text, re.DOTALL)
    assert block is not None, "blackbox_campaigns DDL not found in migrations.rs"
    columns = re.findall(r"^\s*(\w+)\s+\w+", block.group(1), re.MULTILINE)
    assert "campaign_fingerprint" in columns, f"fingerprint column missing: {columns}"
    seedy = [column for column in columns if "seed" in column.lower()]
    assert not seedy, f"wall W-a breached at rest — seed persisted: {seedy}"


def test_vault_v13_profile_lane_seals_the_certificate_at_rest() -> None:
    """LAW-19 / R-7 at rest. The `CalibrationCertificate` type seal keeps a
    calibrated profile un-constructible; the v13 DDL must keep it unstorable
    too, so a future writer cannot route around the type system by inserting
    a row that claims calibration. The same DDL must expose no 6D projection
    column: the projection stays all N/A until calibration data exists, and a
    `score_*` column would be exactly the place an invented number lands."""
    text = (DB_DIR / "migrations.rs").read_text(encoding="utf-8")
    block = re.search(r"CREATE TABLE blackbox_profiles \((.*?)\n\);", text, re.DOTALL)
    assert block is not None, "blackbox_profiles DDL not found in migrations.rs"
    body = block.group(1)
    assert "CHECK (calibration = 'uncalibrated-instrument')" in body, (
        "the profile lane must pin the uncalibrated marker (LAW-19 / R-7)"
    )
    assert "CHECK (schema_version = 'blackbox_profile.v1')" in body, (
        "the profile lane must pin its schema literal"
    )

    forbidden = ("score", "projection", "tensor", "6d", "dimension", "certificate")
    for table in ("blackbox_profiles", "blackbox_profile_lanes", "blackbox_profile_sources"):
        table_block = re.search(rf"CREATE TABLE {table} \((.*?)\n\);", text, re.DOTALL)
        assert table_block is not None, f"{table} DDL not found in migrations.rs"
        columns = re.findall(r"^\s*(\w+)\s+(?:BLOB|TEXT|INTEGER)", table_block.group(1), re.MULTILINE)
        leaked = [c for c in columns if any(n in c.lower() for n in forbidden)]
        assert not leaked, f"{table} exposes a 6D/certificate surface (R-7): {leaked}"


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
    sits inside blackbox_sim's own `#[cfg(test)] mod tests` block.

    R-8 carve-out: `blackbox_arena` may expose a command-layer
    `estimate_profile` method that is shape-gated by
    `blackbox-profile-write` and must never call `bias::estimate_profile`
    or the Session accessors directly (it goes through `bridge::estimate_pooled`).
    """
    # Arena command surface is the R-8 gate, not a bias::estimate_profile caller.
    arena_allow = {"blackbox_arena/handle.rs", "blackbox_arena/commands.rs"}
    outside_offenders = []
    for path in sorted(TAURI_SRC.rglob("*.rs")):
        if "blackbox_sim" in path.parts:
            continue
        rel = path.relative_to(TAURI_SRC).as_posix()
        text = path.read_text(encoding="utf-8")
        if rel in arena_allow:
            # Still forbid Session accessor / bias estimator leaks through the gate.
            if re.search(
                r"\.events\(\)|\.refusals\(\)|\.pricing_trials\(\)|bias::estimate_profile",
                text,
            ):
                outside_offenders.append(rel)
            continue
        if ESTIMATOR_CALL_SITE_RE.search(text) or re.search(r"\bestimate_profile\(", text):
            outside_offenders.append(rel)
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


# ---------------------------------------------------------------------------
# 8. Phase 5-B frontend guards
# ---------------------------------------------------------------------------

FRONTEND_SRC = ROOT / "apps" / "desktop" / "src"
BXS_INVOKE_OWNER = FRONTEND_SRC / "lib" / "blackboxArena.ts"
BXS_FE_GLOBS = (
    FRONTEND_SRC / "lib" / "parseBlackboxArena.ts",
    FRONTEND_SRC / "lib" / "blackboxArena.ts",
    FRONTEND_SRC / "lib" / "blackboxIntent.ts",
    FRONTEND_SRC / "lib" / "blackboxArenaReducer.ts",
    FRONTEND_SRC / "lib" / "blackboxDraftIntent.ts",
    FRONTEND_SRC / "lib" / "blackboxUiError.ts",
    FRONTEND_SRC / "components" / "consult" / "coliseum" / "BlackboxArena.tsx",
    FRONTEND_SRC / "components" / "consult" / "coliseum" / "BlackboxSetupPanel.tsx",
    FRONTEND_SRC / "components" / "consult" / "coliseum" / "BlackboxMarketRail.tsx",
    FRONTEND_SRC / "components" / "consult" / "coliseum" / "BlackboxBooksPanel.tsx",
    FRONTEND_SRC / "components" / "consult" / "coliseum" / "BlackboxStimulusDeck.tsx",
    FRONTEND_SRC / "components" / "consult" / "coliseum" / "BlackboxCommandConsole.tsx",
    FRONTEND_SRC / "components" / "consult" / "coliseum" / "BlackboxTurnLog.tsx",
)

BXS_FE_FORBIDDEN = [
    r"\bfetch\s*\(",
    r"\blocalStorage\b",
    r"\bMath\.random\b",
    r"\bDate\.now\b",
    r"\sas any\b",
    r"dangerouslySetInnerHTML",
]

# Numeric bounds must come from ArenaLimitsView, never FE literals matching
# firm::MAX_PRICE_MINOR / action::MAX_ACTION_AMOUNT_MINOR etc.
BXS_LIMIT_LITERALS = [
    r"\b10_000_000\b",
    r"\b10000000\b",
    r"\b1_000_000_000_000\b",
    r"\b1000000000000\b",
]


def test_bxs_invoke_owned_only_by_blackbox_arena_ts() -> None:
    """bxs_* invoke calls may live only in lib/blackboxArena.ts (closed owner)."""
    assert BXS_INVOKE_OWNER.is_file(), "lib/blackboxArena.ts missing"
    offenders: list[str] = []
    for path in FRONTEND_SRC.rglob("*"):
        if path.suffix not in {".ts", ".tsx"}:
            continue
        if path.resolve() == BXS_INVOKE_OWNER.resolve():
            continue
        text = path.read_text(encoding="utf-8")
        if re.search(r'invoke\s*<[^>]*>\s*\(\s*"bxs_', text) or re.search(
            r'"bxs_(start_campaign|get_view|submit_decision|advance|abort|load_generation)"',
            text,
        ):
            # Type/command name mentions in parsers/docs are fine; only invoke owners matter.
            if "invoke" in text and "bxs_" in text:
                if re.search(r"\binvoke\s*<", text) and "bxs_" in text:
                    offenders.append(path.relative_to(ROOT).as_posix())
    assert not offenders, f"bxs_ invoke escaped blackboxArena.ts: {offenders}"


def test_blackbox_fe_has_no_forbidden_tokens() -> None:
    missing = [p.as_posix() for p in BXS_FE_GLOBS if not p.is_file()]
    assert not missing, f"BLACKBOX FE files missing: {missing}"
    for path in BXS_FE_GLOBS:
        text = path.read_text(encoding="utf-8")
        for pattern in BXS_FE_FORBIDDEN:
            assert re.search(pattern, text) is None, f"{path.name}: forbidden {pattern}"


def test_blackbox_fe_has_no_forced_default_builder() -> None:
    """Director-only timeout default must not be constructible on the FE."""
    pattern = re.compile(r"forced_default|ForcedDefault|buildForcedDefault")
    for path in BXS_FE_GLOBS:
        text = path.read_text(encoding="utf-8")
        # Strip block and line comments so documentation may name the ban.
        stripped = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
        stripped = re.sub(r"//.*?$", "", stripped, flags=re.M)
        assert pattern.search(stripped) is None, (
            f"{path.name}: timeout-default intent must not appear in FE code"
        )


def test_blackbox_fe_does_not_hardcode_arena_limits() -> None:
    """Limits constants are co-shipped on ObservationView — FE must not invent them."""
    # Boundary tests may mention the published numbers as fixtures; production
    # FE modules under src/ must not.
    for path in BXS_FE_GLOBS:
        if path.name.endswith(".test.ts"):
            continue
        text = path.read_text(encoding="utf-8")
        for pattern in BXS_LIMIT_LITERALS:
            assert re.search(pattern, text) is None, (
                f"{path.name}: hardcoded limit literal {pattern}"
            )


def test_observation_view_ships_books_and_limits() -> None:
    view_text = (ARENA_DIR / "view.rs").read_text(encoding="utf-8")
    assert "struct BooksView" in view_text
    assert "struct ArenaLimitsView" in view_text
    assert "pub books: BooksView" in view_text
    assert "pub limits: ArenaLimitsView" in view_text
    assert "cash_minor: i64" not in view_text.split("struct ObservationView")[1].split("}")[0], (
        "ObservationView must not keep a duplicate cash_minor beside books"
    )
    assert 'rename_all = "camelCase"' in view_text


# ---------------------------------------------------------------------------
# 9. Phase 6-A / R-8 profile-write two-factor gate
# ---------------------------------------------------------------------------

CARGO_TOML = TAURI_SRC.parent / "Cargo.toml"
CI_WRITE_GATE = ROOT / ".github" / "workflows" / "blackbox-profile-write-gate.yml"
PROFILE_WRITE_NOT_READY = "BLACKBOX_PROFILE_WRITE_NOT_READY"


def test_blackbox_profile_write_is_not_a_default_feature() -> None:
    """R-8: blackbox-profile-write must never ride along on default builds."""
    text = CARGO_TOML.read_text(encoding="utf-8")
    # Isolate the [features] table.
    features = re.search(r"\[features\](.*?)(\n\[|\Z)", text, re.S)
    assert features is not None, "[features] table missing from Cargo.toml"
    body = features.group(1)
    assert re.search(
        r'^blackbox-profile-write\s*=\s*\["blackbox-sim"\]', body, re.M
    ), "blackbox-profile-write must depend on blackbox-sim"
    default = re.search(r"^default\s*=\s*\[(.*?)\]", body, re.M | re.S)
    if default is not None:
        assert "blackbox-profile-write" not in default.group(1), (
            "blackbox-profile-write must not be in default features"
        )


def test_insert_profile_call_sites_are_write_gated() -> None:
    """R-8 shape defence: insert_profile is defined only under the write feature."""
    repo = (DB_DIR / "blackbox_repo.rs").read_text(encoding="utf-8")
    assert re.search(
        r'#\[cfg\(feature = "blackbox-profile-write"\)\]\s*\n'
        r"pub\(crate\) fn insert_profile",
        repo,
    ), 'insert_profile must be #[cfg(feature = "blackbox-profile-write")]'
    # No production call expression may exist outside a write-gated region.
    # Doc comments / string mentions are ignored — shape defence is about
    # callable sites, not prose.
    call_re = re.compile(r"\binsert_profile\s*\(")
    for path in sorted(TAURI_SRC.rglob("*.rs")):
        if path.name == "blackbox_repo.rs":
            continue
        text = path.read_text(encoding="utf-8")
        if not call_re.search(text):
            continue
        lines = text.splitlines()
        for i, line in enumerate(lines):
            stripped = line.lstrip()
            if stripped.startswith("//"):
                continue
            if not call_re.search(line):
                continue
            window = "\n".join(lines[max(0, i - 30) : i + 1])
            assert 'feature = "blackbox-profile-write"' in window, (
                f"{path.relative_to(TAURI_SRC).as_posix()}:{i + 1}: "
                "insert_profile call lacks write-feature cfg in preceding window"
            )


def test_ci_calibration_job_is_required_predecessor_of_write_build() -> None:
    """R-8 factor B: write-gated CI job must `needs: calibration`."""
    assert CI_WRITE_GATE.is_file(), f"missing {CI_WRITE_GATE}"
    text = CI_WRITE_GATE.read_text(encoding="utf-8")
    assert "blackbox_sim::calibration" in text, "calibration job must run the suite"
    assert "needs: calibration" in text, "write-gated job must depend on calibration"
    assert "blackbox-profile-write" in text
    # C-1: `--lib` pins a single result line; refuse the tail-of-binaries trap.
    assert "--lib" in text
    assert 'RESULT_LINES' in text or 'test "$RESULT_LINES" -eq 1' in text
    # Lower-bound guard against empty-green: the workflow itself encodes >= 14.
    assert "-ge 14" in text or "PASSED" in text


def test_profile_write_not_ready_error_code_is_defined() -> None:
    """Non-flag builds refuse with a stable wire code (EGRESS_LIVE_NOT_READY twin)."""
    view = (ARENA_DIR / "view.rs").read_text(encoding="utf-8")
    assert PROFILE_WRITE_NOT_READY in view, (
        f"SimUiErrorCode must rename to {PROFILE_WRITE_NOT_READY}"
    )
    assert "BlackboxProfileWriteNotReady" in view
    fe = (FRONTEND_SRC / "lib" / "parseBlackboxArena.ts").read_text(encoding="utf-8")
    assert PROFILE_WRITE_NOT_READY in fe


# ---------------------------------------------------------------------------
# 10. Phase 6-A / R-9 outlets + BXS-I-26 non-recirculation
# ---------------------------------------------------------------------------

BXS_I26_MARKERS = (
    "LoadedProfile",
    "BlackboxOutletSnapshot",
    "get_latest_profile",
    "format_consult_block",
    "bxs_latest_profile",
    "blackbox_profile_outlet",
)

# Legal R-9 outlets (+ vault read plumbing). Everything else is deny.
BXS_I26_ALLOWLIST = {
    "db/blackbox_repo.rs",
    "db/blackbox_profile_outlet.rs",
    "db/worker.rs",
    "db/mod.rs",
    "db/migrations.rs",
    "llm/consult_context.rs",
    "llm/commands_sim.rs",
    "blackbox_arena/commands.rs",
    "lib.rs",
}

# G-1: mentor-band carriers. Touching these without review = contract RED.
# Allowlist is the four legal consumers of the mentor bundle (consult /
# debrief / rag-consult) plus the module that defines the fields/helpers.
BXS_I26_MENTOR_MARKERS = (
    "blackbox_block",
    "blackbox_available",
    "append_mentor_sections",
    "load_mentor_context",
)
BXS_I26_MENTOR_ALLOWLIST = {
    "llm/consult_context.rs",
    "llm/commands_consult.rs",
    "llm/commands_sim.rs",
    "rag/commands_rag.rs",
}

BXS_I26_BLACKBOX_SECTION_CALLEES = {
    "build_static_prefix",
    "_consult_interview_sim",
    "_consult_gd_sim",
}


def test_bxs_i26_profile_types_stay_off_deny_surfaces() -> None:
    """BXS-I-26: interview discussion / GD / es_review / sim core / arena handle
    must not grow a private profile reader. Allowlisted R-9 outlets only."""
    py_core = ROOT / "src" / "python" / "core"
    es_path = py_core / "es_manager.py"
    if es_path.is_file():
        text = es_path.read_text(encoding="utf-8")
        for marker in BXS_I26_MARKERS:
            assert marker not in text, f"es_manager.py must not mention {marker}"

    offenders: list[str] = []
    for path in sorted(TAURI_SRC.rglob("*.rs")):
        rel = path.relative_to(TAURI_SRC).as_posix()
        if rel in BXS_I26_ALLOWLIST:
            continue
        text = path.read_text(encoding="utf-8")
        if rel.startswith("blackbox_sim/"):
            for marker in (
                "get_latest_profile",
                "BlackboxOutletSnapshot",
                "format_consult_block",
                "blackbox_profile_outlet",
            ):
                if marker in text:
                    offenders.append(f"{rel}:{marker}")
            continue
        for marker in BXS_I26_MARKERS:
            if marker in text:
                offenders.append(f"{rel}:{marker}")
    assert not offenders, f"BXS-I-26 breached — profile outlet leak: {offenders}"


def test_bxs_i26_mentor_band_consumers_are_allowlisted() -> None:
    """G-1: blackbox_block / mentor helpers may only appear in the four legal
    mentor-band files. A new `append_mentor_sections` caller is contract RED
    even if it never names LoadedProfile."""
    offenders: list[str] = []
    for path in sorted(TAURI_SRC.rglob("*.rs")):
        rel = path.relative_to(TAURI_SRC).as_posix()
        if rel in BXS_I26_MENTOR_ALLOWLIST:
            continue
        text = path.read_text(encoding="utf-8")
        for marker in BXS_I26_MENTOR_MARKERS:
            if marker in text:
                offenders.append(f"{rel}:{marker}")
    assert not offenders, (
        f"BXS-I-26 mentor-band leak (new consumer needs review): {offenders}"
    )
    for required in BXS_I26_MENTOR_ALLOWLIST:
        assert (TAURI_SRC / required).is_file(), f"missing mentor allowlist file {required}"


def test_bxs_i26_commands_sim_blackbox_only_inside_is_debrief() -> None:
    """G-1: every `blackbox_block` use in commands_sim.rs must sit inside an
    `if is_debrief { ... }` arm — never the live-interview else branch."""
    text = (TAURI_SRC / "llm" / "commands_sim.rs").read_text(encoding="utf-8")
    assert "blackbox_block" in text

    def brace_spans_for(predicate_src: str) -> list[tuple[int, int]]:
        spans: list[tuple[int, int]] = []
        for m in re.finditer(re.escape(predicate_src), text):
            # Find the `{` that opens this if-arm.
            i = m.end()
            while i < len(text) and text[i] != "{":
                i += 1
            if i >= len(text):
                continue
            depth = 0
            for j in range(i, len(text)):
                if text[j] == "{":
                    depth += 1
                elif text[j] == "}":
                    depth -= 1
                    if depth == 0:
                        spans.append((i, j + 1))
                        break
        return spans

    debrief_spans = brace_spans_for("if is_debrief")
    assert debrief_spans, "expected at least one `if is_debrief` block"
    for m in re.finditer(r"blackbox_block", text):
        pos = m.start()
        assert any(start <= pos < end for start, end in debrief_spans), (
            f"blackbox_block at offset {pos} is outside every is_debrief block"
        )


def _python_method_callers(source: str, needle: str) -> set[str]:
    """Return the set of `def name` methods that contain `needle` in their body.

    Indent-based: a method at class indent (4 spaces) owns subsequent lines
    until the next def at the same indent. Mirrors the awk enumeration used
    in the step-6 audit (G-2).
    """
    callers: set[str] = set()
    current: str | None = None
    for line in source.splitlines():
        def_match = re.match(r"^    def (\w+)\(", line)
        if def_match:
            current = def_match.group(1)
            continue
        if current is not None and needle in line:
            callers.add(current)
    return callers


def _python_method_body(source: str, method: str) -> str:
    lines = source.splitlines()
    start = None
    for i, line in enumerate(lines):
        if re.match(rf"^    def {re.escape(method)}\(", line):
            start = i
            break
    assert start is not None, f"method {method} not found"
    body: list[str] = []
    for line in lines[start + 1 :]:
        if re.match(r"^    def \w+\(", line):
            break
        body.append(line)
    return "\n".join(body)


def test_bxs_i26_python_blackbox_section_callers_are_structural() -> None:
    """G-2: every `_blackbox_section()` call must live in the legal callee set;
    `_consult_es_review` must not call `build_static_prefix`."""
    ce = (ROOT / "src" / "python" / "core" / "consultation_engine.py").read_text(
        encoding="utf-8"
    )
    callers = _python_method_callers(ce, "_blackbox_section()")
    assert callers == BXS_I26_BLACKBOX_SECTION_CALLEES, (
        f"_blackbox_section() callers={sorted(callers)} "
        f"expected={sorted(BXS_I26_BLACKBOX_SECTION_CALLEES)}"
    )
    es_body = _python_method_body(ce, "_consult_es_review")
    assert "build_static_prefix" not in es_body, (
        "_consult_es_review must not call build_static_prefix"
    )
    assert "_blackbox_section()" not in es_body, (
        "_consult_es_review must not call _blackbox_section()"
    )


def test_bxs_i26_handle_does_not_feed_profile_into_session() -> None:
    """W-b / BXS-I-26: arena handle may estimate/write, but must not pass a
    LoadedProfile into Session::start / difficulty / stimulus selection."""
    text = (ARENA_DIR / "handle.rs").read_text(encoding="utf-8")
    for marker in (
        "LoadedProfile",
        "BlackboxOutletSnapshot",
        "get_latest_profile",
        "format_consult_block",
        "BlackboxProfileView",
        "blackbox_profile_outlet",
        "blackbox_block",
        "append_mentor_sections",
        "load_mentor_context",
    ):
        assert marker not in text, f"handle.rs must not reference {marker}"
    assert "Session::start" in text


def test_r9_outlets_go_through_single_accessor() -> None:
    """R-9 shape: consult/講評/UI format via outlet module; SQL only in repo."""
    outlet = (DB_DIR / "blackbox_profile_outlet.rs").read_text(encoding="utf-8")
    # Formatter must not open SQL (doc references to get_latest_profile are OK).
    code_lines = [
        ln
        for ln in outlet.splitlines()
        if ln.strip() and not ln.lstrip().startswith("//") and not ln.lstrip().startswith("//!")
    ]
    code = "\n".join(code_lines)
    assert "SELECT " not in code
    assert ".query" not in code
    assert "format_consult_block" in outlet
    assert "未測定" in outlet
    consult = (TAURI_SRC / "llm" / "consult_context.rs").read_text(encoding="utf-8")
    assert "format_consult_block" in consult
    assert "blackbox_latest_profile" in consult
    commands = (ARENA_DIR / "commands.rs").read_text(encoding="utf-8")
    assert "bxs_latest_profile" in commands
    assert "list_profiles" in (DB_DIR / "blackbox_repo.rs").read_text(encoding="utf-8")
    panel = FRONTEND_SRC / "components" / "consult" / "BlackboxProfilePanel.tsx"
    assert panel.is_file()
    arena = (
        FRONTEND_SRC / "components" / "consult" / "coliseum" / "BlackboxArena.tsx"
    ).read_text(encoding="utf-8")
    assert "bxs_latest_profile" not in arena
    assert "BlackboxProfilePanel" not in arena
    owner = (FRONTEND_SRC / "lib" / "blackboxArena.ts").read_text(encoding="utf-8")
    assert "bxs_latest_profile" in owner
