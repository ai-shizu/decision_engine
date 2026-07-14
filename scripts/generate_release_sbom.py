#!/usr/bin/env python3
"""Generate a deterministic CycloneDX inventory from the three release locks."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import tomllib
from typing import Any
from urllib.parse import quote


def _component(
    ecosystem: str,
    name: str,
    version: str,
    hashes: list[dict[str, str]] | None = None,
    properties: list[dict[str, str]] | None = None,
) -> dict[str, Any]:
    normalized = name.replace("_", "-") if ecosystem == "pypi" else name
    reference = f"pkg:{ecosystem}/{quote(normalized, safe='/')}@{version}"
    output: dict[str, Any] = {
        "bom-ref": reference,
        "name": name,
        "purl": reference,
        "type": "library",
        "version": version,
    }
    if hashes:
        output["hashes"] = sorted(hashes, key=lambda item: (item["alg"], item["content"]))
    if properties:
        output["properties"] = sorted(properties, key=lambda item: (item["name"], item["value"]))
    return output


def _cargo_components(root: Path) -> list[dict[str, Any]]:
    lock = tomllib.loads((root / "apps/desktop/src-tauri/Cargo.lock").read_text(encoding="utf-8"))
    output = []
    for package in lock.get("package", []):
        checksum = package.get("checksum")
        hashes = [{"alg": "SHA-256", "content": checksum}] if checksum else None
        output.append(_component("cargo", package["name"], package["version"], hashes))
    return output


def _npm_name(path: str, package: dict[str, Any]) -> str:
    if package.get("name"):
        return str(package["name"])
    tail = path.rsplit("node_modules/", 1)[-1]
    if not tail or tail == path:
        raise ValueError("npm lock entry has no stable package name")
    return tail


def _npm_components(root: Path) -> list[dict[str, Any]]:
    lock = json.loads((root / "apps/desktop/package-lock.json").read_bytes())
    if lock.get("lockfileVersion") != 3 or not isinstance(lock.get("packages"), dict):
        raise ValueError("unsupported npm lock shape")
    output = []
    for path, package in lock["packages"].items():
        if not path:
            continue
        integrity = package.get("integrity")
        hashes = None
        if integrity:
            algorithm, separator, content = integrity.partition("-")
            if separator != "-" or algorithm.lower() not in {"sha256", "sha384", "sha512"}:
                raise ValueError("unsupported npm integrity shape")
            hashes = [{"alg": algorithm.upper().replace("SHA", "SHA-"), "content": content}]
        output.append(_component("npm", _npm_name(path, package), package["version"], hashes))
    return output


def _python_components(root: Path) -> list[dict[str, Any]]:
    lines = (root / "apps/desktop/requirements-sidecar.lock").read_text(encoding="utf-8").splitlines()
    output: list[dict[str, Any]] = []
    current: dict[str, Any] | None = None
    for raw in lines:
        stripped = raw.strip()
        if not stripped or stripped.startswith("#"):
            continue
        if not raw[:1].isspace():
            if current is not None:
                output.append(current)
            declaration = stripped.removesuffix("\\").rstrip()
            match = re.fullmatch(r"([A-Za-z0-9_.-]+)==([^;\s]+)(?:;\s*(.+))?", declaration)
            if match is None:
                raise ValueError("unsupported Python lock declaration")
            name, version, marker = match.groups()
            properties = (
                [{"name": "pkb:environment-marker", "value": marker}] if marker else None
            )
            current = _component("pypi", name, version, properties=properties)
            current["hashes"] = []
            continue
        if current is None:
            raise ValueError("orphan Python hash line")
        hashes = re.findall(r"--hash=sha256:([0-9a-f]{64})", stripped)
        if len(hashes) != 1:
            raise ValueError("Python lock line must contain one SHA-256")
        current["hashes"].append({"alg": "SHA-256", "content": hashes[0]})
    if current is not None:
        output.append(current)
    if not output or any(not component.get("hashes") for component in output):
        raise ValueError("Python lock is incomplete")
    for component in output:
        component["hashes"].sort(key=lambda item: item["content"])
    return output


def build_sbom(root: Path) -> bytes:
    components = _cargo_components(root) + _npm_components(root) + _python_components(root)
    unique: dict[str, dict[str, Any]] = {}
    for component in components:
        reference = component["bom-ref"]
        existing = unique.get(reference)
        if existing is not None and existing != component:
            raise ValueError("conflicting lock entries share one package identity")
        unique[reference] = component
    payload = {
        "bomFormat": "CycloneDX",
        "components": [unique[key] for key in sorted(unique)],
        "metadata": {
            "component": {
                "bom-ref": "pkg:generic/pkb@0.1.0",
                "name": "PKB",
                "type": "application",
                "version": "0.1.0",
            },
            "properties": [
                {"name": "pkb:node-version", "value": "24.18.0"},
                {"name": "pkb:python-version", "value": "3.12.10"},
                {"name": "pkb:rust-version", "value": "1.96.1"},
            ],
        },
        "specVersion": "1.5",
        "version": 1,
    }
    return json.dumps(payload, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8") + b"\n"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    root = args.root.resolve(strict=True)
    output = args.output.absolute()
    encoded = build_sbom(root)
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_name(f".{output.name}.{os.getpid()}.tmp")
    with temporary.open("xb") as handle:
        handle.write(encoded)
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temporary, output)


if __name__ == "__main__":
    main()
