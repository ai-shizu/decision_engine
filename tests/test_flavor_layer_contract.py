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
