#!/usr/bin/env python3
"""T4-D: Full JSON Schema (draft 2020-12 subset) + policy/decision field sync."""
from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any


def fail(msg: str) -> None:
    print(f"validate_ios_policy: RED: {msg}", file=sys.stderr)
    raise SystemExit(1)


def type_ok(val: Any, typ: Any) -> bool:
    if isinstance(typ, list):
        return any(type_ok(val, t) for t in typ)
    if typ == "object":
        return isinstance(val, dict)
    if typ == "array":
        return isinstance(val, list)
    if typ == "string":
        return isinstance(val, str)
    if typ == "integer":
        return isinstance(val, int) and not isinstance(val, bool)
    if typ == "boolean":
        return isinstance(val, bool)
    if typ == "null":
        return val is None
    if typ == "number":
        return (isinstance(val, (int, float)) and not isinstance(val, bool))
    return True


def validate(instance: Any, schema: dict, path: str = "$") -> None:
    if "const" in schema and instance != schema["const"]:
        fail(f"{path}: const expected {schema['const']!r}, got {instance!r}")
    if "enum" in schema and instance not in schema["enum"]:
        fail(f"{path}: enum {schema['enum']}, got {instance!r}")
    if "type" in schema and not type_ok(instance, schema["type"]):
        fail(f"{path}: type {schema['type']}, got {type(instance).__name__}")

    if isinstance(instance, dict):
        req = schema.get("required", [])
        for key in req:
            if key not in instance:
                fail(f"{path}: missing required {key}")
        if schema.get("additionalProperties") is False:
            allowed = set(schema.get("properties", {}))
            extra = set(instance) - allowed
            if extra:
                fail(f"{path}: additionalProperties not allowed: {sorted(extra)}")
        props = schema.get("properties", {})
        for key, val in instance.items():
            if key in props:
                validate(val, props[key], f"{path}.{key}")

    if isinstance(instance, list):
        if "minItems" in schema and len(instance) < schema["minItems"]:
            fail(f"{path}: minItems {schema['minItems']}, got {len(instance)}")
        if "maxItems" in schema and len(instance) > schema["maxItems"]:
            fail(f"{path}: maxItems {schema['maxItems']}, got {len(instance)}")
        if schema.get("uniqueItems"):
            # JSON-serializable uniqueness
            seen = []
            for i, item in enumerate(instance):
                key = json.dumps(item, sort_keys=True, ensure_ascii=False)
                if key in seen:
                    fail(f"{path}: uniqueItems violated at index {i}: {item!r}")
                seen.append(key)
        item_schema = schema.get("items")
        if isinstance(item_schema, dict):
            for i, item in enumerate(instance):
                validate(item, item_schema, f"{path}[{i}]")

    if isinstance(instance, str):
        if "minLength" in schema and len(instance) < schema["minLength"]:
            fail(f"{path}: minLength")
        if "pattern" in schema:
            import re
            if re.fullmatch(schema["pattern"], instance) is None:
                fail(f"{path}: pattern {schema['pattern']} mismatch for {instance!r}")

    if isinstance(instance, int) and not isinstance(instance, bool):
        if "minimum" in schema and instance < schema["minimum"]:
            fail(f"{path}: minimum {schema['minimum']}")


def sync_decision(policy: dict, root: Path) -> None:
    elig_path = root / policy["eligibility"]["decision_record"]
    expo_path = root / policy["export_compliance"]["decision_record"]
    if not elig_path.is_file():
        fail(f"missing eligibility decision: {elig_path}")
    if not expo_path.is_file():
        fail(f"missing export-compliance decision: {expo_path}")
    elig = json.loads(elig_path.read_text(encoding="utf-8"))
    expo = json.loads(expo_path.read_text(encoding="utf-8"))

    for field in ("status", "option", "decision_id", "approver", "approved_at"):
        if policy["eligibility"].get(field) != elig.get(field):
            fail(
                f"eligibility.{field} policy={policy['eligibility'].get(field)!r} "
                f"decision={elig.get(field)!r}"
            )
    for field in (
        "status",
        "decision_id",
        "approver",
        "jurisdiction",
        "crypto_inventory_digest",
        "its_app_uses_non_exempt_encryption",
    ):
        if policy["export_compliance"].get(field) != expo.get(field):
            fail(
                f"export_compliance.{field} policy="
                f"{policy['export_compliance'].get(field)!r} decision={expo.get(field)!r}"
            )

    # Exact 13 unique predecessor names (schema already uniqueItems; restate)
    checks = policy["quality_predecessor_checks"]
    if len(checks) != 13 or len(set(checks)) != 13:
        fail("quality_predecessor_checks must be exactly 13 unique names")
    print("decision sync: GREEN (all eligibility/export fields match)")


def main() -> None:
    if len(sys.argv) != 2:
        fail("usage: validate_ios_policy.py <ios-release.policy.json>")
    path = Path(sys.argv[1])
    schema_path = path.with_name("ios-release.policy.schema.json")
    data = json.loads(path.read_text(encoding="utf-8"))
    schema = json.loads(schema_path.read_text(encoding="utf-8"))
    validate(data, schema)
    # Repo root: .../ios/policy/file -> parents[5] == repo
    # policy(0) ios(1) src-tauri(2) desktop(3) apps(4) repo(5)
    root = path.resolve().parents[5]
    sync_decision(data, root)

    # Domain consts beyond schema
    if data["features_exact"] != ["pocket-brain", "secure-vault", "flavor-live"]:
        fail("features_exact mismatch")
    if data["required_info_plist_keys"] != [
        "NSFaceIDUsageDescription",
        "NSCalendarsFullAccessUsageDescription",
        "ITSAppUsesNonExemptEncryption",
    ]:
        fail("required_info_plist_keys mismatch")
    if data["eligibility"]["status"] == "UNAPPROVED" and data["eligibility"]["option"] is not None:
        fail("eligibility.option must be null while UNAPPROVED")
    if (
        data["export_compliance"]["status"] == "UNAPPROVED"
        and data["export_compliance"]["its_app_uses_non_exempt_encryption"] is not None
    ):
        fail("export_compliance boolean must be null while UNAPPROVED")

    print("validate_ios_policy: GREEN (schema + sync)")
    print(f"  product_name={data['product_name']}")
    print(f"  product_app_name={data['product_app_name']}")
    print(f"  xcodegen_version={data['xcodegen_version']}")
    print(f"  eligibility.status={data['eligibility']['status']}")
    print(f"  export_compliance.status={data['export_compliance']['status']}")
    print(f"  predecessor_checks={len(data['quality_predecessor_checks'])} unique")


if __name__ == "__main__":
    main()
