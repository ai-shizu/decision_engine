# -*- coding: utf-8 -*-
"""Flavor Layer structural contracts (docs/SPEC_FLAVOR_LAYER.md v2 / F-1).

Read-only scans (sandbox-safe). Guards FLV-I-02 / I-03 / I-08 / I-10 and the
§3.4 ban on Deserialize/serde(from|try_from) for VerifiedFlavor.
"""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TAURI = ROOT / "apps" / "desktop" / "src-tauri"
DESKTOP = ROOT / "apps" / "desktop"
CARGO_TOML = TAURI / "Cargo.toml"
FLAVOR_DIR = TAURI / "src" / "flavor"
LIB_RS = TAURI / "src" / "lib.rs"
SPEC = ROOT / "docs" / "SPEC_FLAVOR_LAYER.md"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _features_table() -> str:
    text = _read(CARGO_TOML)
    m = re.search(r"\[features\](.*?)(\n\[|\Z)", text, re.S)
    assert m is not None, "[features] table missing"
    return m.group(1)


def test_spec_placement_is_top_level_flavor_not_llm() -> None:
    """SSOT: §3.3 must point at flavor/, not llm/flavor/."""
    text = _read(SPEC)
    assert "flavor/" in text
    # The corrected tree fence must not still advertise llm/flavor/ as the home.
    assert "llm/flavor/" not in text.split("### 3.3")[1].split("### 3.4")[0]


def test_flavor_layer_not_in_default_features() -> None:
    body = _features_table()
    assert re.search(r"^flavor-layer\s*=\s*\[\s*\]", body, re.M), (
        "flavor-layer = [] must exist and must not depend on pocket-brain"
    )
    default = re.search(r"^default\s*=\s*\[(.*?)\]", body, re.M | re.S)
    if default is not None:
        assert "flavor-layer" not in default.group(1), (
            "flavor-layer must not be in default features (FLV-R-4)"
        )


def test_flavor_layer_does_not_depend_on_pocket_brain() -> None:
    """FLV-I-08 adjacency: flavor-layer must not pull pocket-brain."""
    body = _features_table()
    m = re.search(r"^flavor-layer\s*=\s*\[(.*?)\]", body, re.M | re.S)
    assert m is not None
    assert "pocket-brain" not in m.group(1)


def test_blackbox_sim_still_independent_of_pocket_brain() -> None:
    """FLV-I-08: blackbox-sim must not gain a pocket-brain dependency."""
    body = _features_table()
    m = re.search(r"^blackbox-sim\s*=\s*\[(.*?)\]", body, re.M | re.S)
    assert m is not None
    assert "pocket-brain" not in m.group(1)
    assert "flavor-layer" not in m.group(1)


def test_lib_rs_gates_flavor_module() -> None:
    text = _read(LIB_RS)
    assert 'feature = "flavor-layer"' in text
    assert "mod flavor" in text
    # Must NOT be nested under the pocket-brain llm cfg block as the only home.
    assert (FLAVOR_DIR / "mod.rs").is_file()
    assert not (TAURI / "src" / "llm" / "flavor").exists()


def test_flv_i_02_slot_value_has_no_numeric_or_string_payload() -> None:
    """FLV-I-02: SlotValue variants must not hold numeric or String payloads."""
    text = _read(FLAVOR_DIR / "request.rs")
    # Extract the SlotValue enum body.
    m = re.search(r"pub enum SlotValue\s*\{(.*?)\n\}", text, re.S)
    assert m is not None, "SlotValue enum missing"
    body = m.group(1)
    # No tuple/struct variants with payloads (qualitative unit variants only).
    assert "(" not in body, f"SlotValue must not hold payloads: {body!r}"
    assert "String" not in body
    for ty in ("i64", "u64", "i32", "u32", "u16", "f64", "f32", "isize", "usize"):
        assert ty not in body, f"SlotValue must not mention {ty}"
    # Ban a real construction path, not a doc comment that names the trap.
    assert not re.search(r"\bfn\s+from_quantized\b", text)
    assert not re.search(r"\bimpl\b[\s\S]*\bfrom_quantized\b", text)


def test_flv_i_03_no_into_or_from_slot_value_for_authority_types() -> None:
    """FLV-I-03: no Into<SlotValue> / From<...> for SlotValue in the crate src."""
    src_root = TAURI / "src"
    patterns = [
        re.compile(r"impl\s+Into\s*<\s*SlotValue\s*>"),
        re.compile(r"impl\s+From\s*<[^>]+>\s+for\s+SlotValue"),
        re.compile(r"impl\s+TryFrom\s*<[^>]+>\s+for\s+SlotValue"),
    ]
    offenders: list[str] = []
    for path in sorted(src_root.rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        for pat in patterns:
            if pat.search(text):
                offenders.append(path.relative_to(src_root).as_posix())
                break
    assert not offenders, f"authority→SlotValue conversions found: {offenders}"


def test_flv_i_10_flavor_request_has_no_numeric_fields() -> None:
    """FLV-I-10: FlavorRequest / FlavorSlot structs must not declare numeric fields."""
    text = _read(FLAVOR_DIR / "request.rs")
    for struct_name in ("FlavorRequest", "FlavorSlot"):
        m = re.search(rf"pub struct {struct_name}\s*\{{(.*?)\n\}}", text, re.S)
        assert m is not None, f"{struct_name} missing"
        body = m.group(1)
        for ty in ("i64", "u64", "i32", "u32", "u16", "i16", "f64", "f32", "isize", "usize"):
            # Field type appearance (not method return types outside the struct).
            assert not re.search(rf":\s*{ty}\b", body), (
                f"{struct_name} must not have numeric field of type {ty}: {body!r}"
            )


def test_verified_flavor_has_no_deserialize_or_serde_from() -> None:
    """SPEC §3.4: VerifiedFlavor must not derive Deserialize or use serde(from/try_from)."""
    text = _read(FLAVOR_DIR / "verified.rs")
    # Strip line comments so ban-list prose cannot false-positive.
    code = re.sub(r"//.*?$", "", text, flags=re.M)
    code = re.sub(r"/\*.*?\*/", "", code, flags=re.S)
    # Derive / attribute surface on the type itself.
    m = re.search(
        r"((?:#\[[^\]]+\]\s*)*)pub struct VerifiedFlavor\s*\{",
        code,
        re.S,
    )
    assert m is not None, "VerifiedFlavor struct missing"
    attrs = m.group(1)
    assert "Deserialize" not in attrs, "VerifiedFlavor must not derive Deserialize"
    assert not re.search(r"serde\s*\(\s*from\s*=", code)
    assert not re.search(r"serde\s*\(\s*try_from\s*=", code)
    assert not re.search(r"impl\s+Deserialize\b", code)
    # Hand-written Serialize is required.
    assert re.search(r"impl\s+Serialize\s+for\s+VerifiedFlavor", text)


def test_two_key_modules_exist() -> None:
    for name in ("checked.rs", "verified.rs", "wire.rs", "policy.rs", "request.rs", "mod.rs"):
        assert (FLAVOR_DIR / name).is_file(), f"missing {name}"
    mod = _read(FLAVOR_DIR / "mod.rs")
    assert "#![deny(unsafe_code)]" in mod


def test_flv_i_15_p1_population_is_complete() -> None:
    """FLV-I-15 / T-1b+T-1c: nine live P-1 tests, not dead names (LAW-23).

    T-1b closed silent deletion of `fn p1_N_`. T-1c also requires:
    - `#[test]` immediately precedes each `fn p1_N_` (whitespace only between)
    - no `#[ignore]` anywhere in policy.rs (permanent-skip occlusion)
    """
    text = _read(FLAVOR_DIR / "policy.rs")
    assert "#[ignore]" not in text, (
        "policy.rs must not contain #[ignore] (permanent-skip hole)"
    )
    # doc comment may sit above #[test]; #[test] must abut fn (no other attrs).
    required = [f"p1_{i}_" for i in range(1, 10)]
    missing = [
        prefix
        for prefix in required
        if not re.search(rf"#\[test\]\s+fn {re.escape(prefix)}", text)
    ]
    assert not missing, (
        f"P-1 population incomplete or not live #[test] — missing: {missing}"
    )


def _feature_deps(name: str) -> str:
    body = _features_table()
    m = re.search(rf"^{re.escape(name)}\s*=\s*\[(.*?)\]", body, re.M | re.S)
    assert m is not None, f"feature {name!r} missing from [features]"
    return m.group(1)


def test_flv_i_12_flavor_live_not_in_default() -> None:
    """FLV-I-12: flavor-live exists off-default; reverse edges stay closed."""
    body = _features_table()

    # 1) flavor-live exists and pulls the three required features.
    live = _feature_deps("flavor-live")
    for req in ("flavor-layer", "pocket-brain", "blackbox-sim"):
        assert req in live, f"flavor-live must include {req!r}; got [{live}]"

    # 2) If a default key exists, flavor-live must not be in it (dormant when absent).
    default = re.search(r"^default\s*=\s*\[(.*?)\]", body, re.M | re.S)
    if default is not None:
        assert "flavor-live" not in default.group(1), (
            "flavor-live must not be in default features (FLV-R-4 / FLV-I-12)"
        )

    # 3) Reverse edge ban: blackbox-sim must not pull LLM / flavor features.
    bbs = _feature_deps("blackbox-sim")
    for banned in ("pocket-brain", "flavor-layer", "flavor-live"):
        assert banned not in bbs, (
            f"blackbox-sim must not depend on {banned!r} (FLV-I-08 / one-way); got [{bbs}]"
        )

    # 4) flavor-layer stays pure (no pocket-brain / flavor-live).
    layer = _feature_deps("flavor-layer")
    for banned in ("pocket-brain", "flavor-live"):
        assert banned not in layer, (
            f"flavor-layer must not depend on {banned!r} (FLV-I-08); got [{layer}]"
        )


def test_flv_i_07_no_retry_on_guard_failure() -> None:
    """関所 B / FLV-I-07: flavor_gen must not retry after verify/decide failure."""
    path = TAURI / "src" / "llm" / "flavor_gen.rs"
    assert path.is_file(), "llm/flavor_gen.rs must exist (single file, not llm/flavor/)"
    text = _read(path)
    # Strip line comments so ban-list prose cannot false-positive.
    code = re.sub(r"//.*?$", "", text, flags=re.M)
    code = re.sub(r"/\*.*?\*/", "", code, flags=re.S)
    assert not re.search(r"\bretry\b", code, re.I), "retry identifier forbidden in flavor_gen"
    assert not re.search(r"\bloop\b", code), "loop-based regeneration forbidden in flavor_gen"
    assert not re.search(r"\bwhile\b", code), "while-based regeneration forbidden in flavor_gen"


def test_flv_i_15_generation_path_never_calls_v1_empty() -> None:
    """関所 C / FLV-I-15: generation path must use for_template, never v1_empty."""
    path = TAURI / "src" / "llm" / "flavor_gen.rs"
    assert path.is_file(), "llm/flavor_gen.rs must exist"
    text = _read(path)
    code = re.sub(r"//.*?$", "", text, flags=re.M)
    code = re.sub(r"/\*.*?\*/", "", code, flags=re.S)
    assert "v1_empty" not in code, (
        "flavor_gen must not call FlavorPolicy::v1_empty (FLV-R-9 / FLV-I-15)"
    )


def test_flv_i_14_flavor_slot_has_no_queue() -> None:
    """FLV-I-14: ambient slot must not buffer requests (no VecDeque / queue)."""
    path = TAURI / "src" / "blackbox_arena" / "flavor_slot.rs"
    assert path.is_file(), "blackbox_arena/flavor_slot.rs must exist"
    text = _read(path)
    code = re.sub(r"//.*?$", "", text, flags=re.M)
    code = re.sub(r"/\*.*?\*/", "", code, flags=re.S)
    assert "VecDeque" not in code, "VecDeque queue buffer forbidden in flavor_slot"
    assert not re.search(r"\bVec\s*<", code), "Vec buffer forbidden in flavor_slot"
    assert "mpsc::" not in code and "sync_channel" not in code, (
        "channel buffering forbidden in flavor_slot"
    )


def test_flavor_gen_has_no_allow_dead_code() -> None:
    """T-4: wiring must drop #![allow(dead_code)] (AI_SKILLS §20-17 C-2)."""
    path = TAURI / "src" / "llm" / "flavor_gen.rs"
    text = _read(path)
    assert "allow(dead_code)" not in text, (
        "flavor_gen must not retain allow(dead_code) after arena wiring"
    )


def test_advance_view_has_no_flavor_fields() -> None:
    """A-4: flavor must not piggy-back on AdvanceView."""
    path = TAURI / "src" / "blackbox_arena" / "view.rs"
    text = _read(path)
    m = re.search(r"pub\(crate\) struct AdvanceView\s*\{(.*?)\n\}", text, re.S)
    assert m is not None, "AdvanceView struct missing"
    body = m.group(1).lower()
    for banned in ("flavor", "verified", "prose", "ambient"):
        assert banned not in body, f"AdvanceView must not carry flavor field ({banned})"


def test_flv_i_01_a1_deletability_script_exists() -> None:
    """FLV-I-01 / §13: A-1 proof is a shell script (not nested cargo-in-test)."""
    path = ROOT / "scripts" / "flavor_a1_deletability.sh"
    assert path.is_file(), "scripts/flavor_a1_deletability.sh must exist for T-7 wiring"
    text = _read(path)
    # Strip comments: the script may *mention* the ban without nesting cargo.
    code = re.sub(r"#.*?$", "", text, flags=re.M)
    assert "cargo test" not in code, "A-1 script must not nest cargo test"
    assert "A-1-b" in text and "empty" in text.lower(), "A-1-b empty-series guard required"
    assert "A-1-e" in text and "accepted" in text, "A-1-e accepted-count guard required"
    assert "A-1-f" in text and "attempts" in text, "A-1-f attempts guard required for live arm"
    assert "diff" in code, "A-1-c byte compare via diff required"
    assert "flavor-a1-digest" in text, "must drive the digest dump binary"
    assert "--arm canned" in text and "--arm none" in text, "A-1-2 and A-1-2b arms required"
    assert "--arm live" in text, "A-1-3 live arm required (T-8 proposition b)"


# ---------------------------------------------------------------------------
# F-3 T-5 — 関所 E / F / G (LAW-23 frozen literals)
# ---------------------------------------------------------------------------

# 関所 F — visual distinction (SPEC §6). Frozen before FE implementation.
FLAVOR_CSS_CLASS = "bxs-flavor"
FLAVOR_MARK_CLASS = "bxs-flavor-mark"
FACT_CSS_CLASS = "bxs-books"  # Books panel = Fact surface
FLAVOR_SLOT_TSX = (
    DESKTOP / "src" / "components" / "consult" / "coliseum" / "BlackboxFlavorSlot.tsx"
)
FACT_BOOKS_TSX = (
    DESKTOP / "src" / "components" / "consult" / "coliseum" / "BlackboxBooksPanel.tsx"
)

# 関所 E — P-4 forbidden patterns; scan ONLY flavor-string consumers.
FLAVOR_FE_PATHS = (FLAVOR_SLOT_TSX,)
P4_FORBIDDEN = [
    r"\bNumber\s*\(",
    r"\bparseFloat\b",
    r"\bparseInt\b",
    r"\bMath\.",
    r"/\s*\\d",  # digit-class regex literals
    r"\.match\s*\(",
    r"dangerouslySetInnerHTML",
]


def _strip_ts_comments(text: str) -> str:
    code = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    code = re.sub(r"//.*?$", "", code, flags=re.M)
    return code


def _rust_fn_body(src: str, fn_name: str) -> str:
    """Extract the body of `fn {fn_name}(...) { ... }` via brace matching."""
    m = re.search(rf"\bfn\s+{re.escape(fn_name)}\s*\(", src)
    assert m is not None, f"fn {fn_name} not found"
    brace = src.find("{", m.end())
    assert brace != -1, f"fn {fn_name}: opening brace missing"
    depth = 0
    for i in range(brace, len(src)):
        ch = src[i]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return src[brace + 1 : i]
    raise AssertionError(f"fn {fn_name}: unbalanced braces")


def test_flv_i_11_flavor_slot_is_visually_distinct() -> None:
    """関所 F / FLV-I-11: flavor uses a CSS class distinct from Fact books."""
    assert FLAVOR_CSS_CLASS != FACT_CSS_CLASS
    flavor = _read(FLAVOR_SLOT_TSX)
    books = _read(FACT_BOOKS_TSX)
    assert f'className="{FLAVOR_CSS_CLASS}"' in flavor, (
        f"flavor slot must use frozen class {FLAVOR_CSS_CLASS!r}"
    )
    assert f'className="{FLAVOR_MARK_CLASS}"' in flavor, (
        f"flavor mark must use frozen class {FLAVOR_MARK_CLASS!r}"
    )
    assert f'className="{FACT_CSS_CLASS}"' in books, (
        f"Fact books panel must keep class {FACT_CSS_CLASS!r}"
    )
    assert f'className="{FACT_CSS_CLASS}"' not in flavor, (
        "flavor slot must not reuse Fact books class"
    )
    assert f'className="{FLAVOR_CSS_CLASS}"' not in books, (
        "Fact books must not use flavor class"
    )
    css = _read(DESKTOP / "src" / "App.css")
    assert f".{FLAVOR_CSS_CLASS}" in css, "App.css must define .bxs-flavor"
    assert "font-style: italic" in css.split(f".{FLAVOR_CSS_CLASS}", 1)[1][:400]


def test_flv_i_05_fe_never_parses_flavor_text() -> None:
    """関所 E / FLV-I-05: P-4 patterns absent from flavor FE paths only."""
    for path in FLAVOR_FE_PATHS:
        assert path.is_file(), f"flavor FE path missing: {path}"
        code = _strip_ts_comments(_read(path))
        # Unary plus coercing the flavor prop (e.g. +text).
        assert not re.search(r"\{\s*\+\s*text\s*\}", code), (
            f"{path.name}: unary + on flavor text forbidden"
        )
        for pattern in P4_FORBIDDEN:
            assert re.search(pattern, code) is None, (
                f"{path.name}: P-4 forbidden pattern {pattern!r}"
            )


def test_flv_i_14_turn_path_never_blocks_on_model() -> None:
    """関所 G / FLV-I-14: kick_ambient_flavor must not call generate sync."""
    path = TAURI / "src" / "blackbox_arena" / "handle.rs"
    text = _read(path)
    body = _rust_fn_body(text, "kick_ambient_flavor")
    code = re.sub(r"//.*?$", "", body, flags=re.M)
    code = re.sub(r"/\*.*?\*/", "", code, flags=re.S)
    for banned in (
        "flavor_gen::generate",
        "request_generate",
        "deliver_completion",
        "LlmHandle::generate",
        ".generate(",
    ):
        assert banned not in code, (
            f"kick_ambient_flavor must not invoke {banned!r} (A-4 / 関所 G)"
        )
    assert "begin_request" in code, "kick must still admit via begin_request"


def test_flv_i_14_delivery_never_runs_model_on_sim_worker() -> None:
    """関所 H / FLV-I-14: deliver_ambient_flavor must not call the model."""
    path = TAURI / "src" / "blackbox_arena" / "handle.rs"
    text = _read(path)
    body = _rust_fn_body(text, "deliver_ambient_flavor")
    code = re.sub(r"//.*?$", "", body, flags=re.M)
    code = re.sub(r"/\*.*?\*/", "", code, flags=re.S)
    for banned in (
        "LlmHandle::generate",
        ".generate(",
        "flavor_gen::generate",
        "deliver_completion",
        "generate_chat_text",
    ):
        assert banned not in code, (
            f"deliver_ambient_flavor must not invoke {banned!r} (関所 H)"
        )
    # Positive companion: owned-clone relay onto the LLM worker.
    assert "enqueue_flavor_generate" in code, (
        "deliver_ambient_flavor must enqueue onto LlmHandle (関所 H companion)"
    )
    assert "AmbientFlavorReady" in code or "flavor_tx" in code


def test_flv_i_17_flavor_gate_workflow_drives_probes() -> None:
    """FLV-I-17: seal probes and A-1 must be wired in flavor-gate.yml."""
    path = ROOT / ".github" / "workflows" / "flavor-gate.yml"
    assert path.is_file(), "flavor-gate.yml must exist (FLV-I-17 / T-7)"
    text = _read(path)
    # Jobs that turn paper guards into running electricity.
    for job in (
        "seal-probe:",
        "doctest-fences:",
        "test-floors:",
        "feature-one-way:",
        "a1-deletability:",
        "flavor-live-absent:",
    ):
        assert job in text, f"flavor-gate.yml missing job {job!r}"
    # Probe drivers (not merely names in comments).
    assert "flavor_seal_probe_verified" in text
    assert "flavor_seal_probe_checked" in text
    assert "error\\[E0451\\]" in text or "error[E0451]" in text or "E0451" in text
    assert "E0603" in text
    assert "flavor_a1_deletability.sh" in text
    assert "accepted=" in text
    # Doctests must not be silently excluded (FLV-W-11).
    assert "--doc" in text
    assert not re.search(
        r"cargo test[^\n]*--features flavor-layer[^\n]*--doc[^\n]*--lib",
        text,
    ), "doctest-fences must not pass --lib (excludes fences)"
    # Floors / ignored guard (ok. alone is insufficient).
    assert 'IGNORED" -eq 0' in text or "IGNORED" in text
    # Positive companion required (FLV-W-06): cfg-free rustc must succeed.
    assert "Positive companion" in text or "positive companion" in text.lower()
