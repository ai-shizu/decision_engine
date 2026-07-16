# -*- coding: utf-8 -*-
"""E0b STEP 0 — Constitutional Guard: lock the networkless sterile state.

Test-only ratchet. Production code is never modified here.
Parent: docs/SPEC_E0B_STEP0_GUARD.md / docs/SPEC_E0B_KNOWLEDGE_GATEWAY_v3.md §12 STEP 0.

Scope T  = tree-wide network egress ban (src/python/**/*.py).
Scope E  = E0b-file-scoped strict ban (knowledge_gateway*.py / *e0b*.py only).
subprocess/ctypes are NOT in Scope T (legitimate local substrate).
bare asyncio is allowed; only asyncio.subprocess is denied.
"""
from __future__ import annotations

import ast
import fnmatch
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PYTHON_SRC = ROOT / "src" / "python"
KF_PATH = PYTHON_SRC / "core" / "knowledge_fetcher.py"

# ---------------------------------------------------------------------------
# Scope T — tree-wide network egress deny-set (§1.4)
# ---------------------------------------------------------------------------
DENY_NET_TOP = frozenset({
    "urllib",
    "socket",
    "requests",
    "httpx",
    "aiohttp",
    "ftplib",
    "websockets",
    "smtplib",
    "telnetlib",
    "poplib",
    "imaplib",
    "ssl",
    "xmlrpc",
    "webbrowser",
})
DENY_NET_SUBMODULE = frozenset({"http.client", "asyncio.subprocess"})

# ---------------------------------------------------------------------------
# Scope E — E0b-module strict cage (§1.4); reserved until E0b modules appear
# ---------------------------------------------------------------------------
DENY_E0B_EXTRA = frozenset({"subprocess", "ctypes", "multiprocessing"})
DENY_E0B_TOP = DENY_NET_TOP | DENY_E0B_EXTRA
DENY_E0B_SUBMODULE = DENY_NET_SUBMODULE

DENY_CALL_NAMES = frozenset({
    "__import__",
    "urlopen",
    "system",
    "popen",
    "Popen",
    "run",
    "call",
    "check_call",
    "check_output",
    "eval",
    "exec",
})

E0A_MSG = "Egress blocked by E0a strict lockdown."

E0B_BASENAME_GLOBS = ("knowledge_gateway*.py", "*e0b*.py")

# STEP 1.A: sanctioned Scope E modules (basename only). Unsanctioned *e0b*.py → RED.
SANCTIONED_E0B_MODULES = frozenset({"e0b_attestation.py", "e0b_intent.py"})


# ---------------------------------------------------------------------------
# AST helpers
# ---------------------------------------------------------------------------
def _call_name(node: ast.Call) -> str | None:
    func = node.func
    if isinstance(func, ast.Name):
        return func.id
    if isinstance(func, ast.Attribute):
        return func.attr
    return None


def _denied_imports(
    tree: ast.AST,
    *,
    deny_top: frozenset[str],
    deny_submodule: frozenset[str],
) -> list[str]:
    """Collect denied import forms (top-level, nested, from-variants, aliases)."""
    hits: list[str] = []
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                name = alias.name
                top = name.split(".", 1)[0]
                if top in deny_top or name in deny_top:
                    hits.append(f"import {name}")
                for sub in deny_submodule:
                    if name == sub or name.startswith(sub + "."):
                        hits.append(f"import {name}")
        elif isinstance(node, ast.ImportFrom):
            mod = node.module or ""
            names = [a.name for a in node.names]
            top = mod.split(".", 1)[0] if mod else ""
            if top in deny_top or mod in deny_top:
                hits.append(f"from {mod} import {', '.join(names)}")
            for sub in deny_submodule:
                if mod == sub or mod.startswith(sub + "."):
                    hits.append(f"from {mod} import {', '.join(names)}")
            # from http import client  /  from http import client as X
            if mod == "http" and "client" in names and "http.client" in deny_submodule:
                hits.append("from http import client")
            # from asyncio import subprocess
            if (
                mod == "asyncio"
                and "subprocess" in names
                and "asyncio.subprocess" in deny_submodule
            ):
                hits.append("from asyncio import subprocess")
    return hits


def denied_imports_scope_t(tree: ast.AST) -> list[str]:
    return _denied_imports(tree, deny_top=DENY_NET_TOP, deny_submodule=DENY_NET_SUBMODULE)


def denied_imports_scope_e(tree: ast.AST) -> list[str]:
    return _denied_imports(tree, deny_top=DENY_E0B_TOP, deny_submodule=DENY_E0B_SUBMODULE)


def has_dynamic_bypass(tree: ast.AST) -> list[str]:
    """Detect dynamic egress bypass constructs (Scope E cage / canaries)."""
    hits: list[str] = []
    for node in ast.walk(tree):
        if isinstance(node, ast.Call):
            name = _call_name(node)
            if name in DENY_CALL_NAMES:
                hits.append(name or "?")
            if isinstance(node.func, ast.Attribute):
                if node.func.attr == "import_module":
                    hits.append("import_module")
                if node.func.attr in {"system", "popen"}:
                    hits.append(f"os.{node.func.attr}")
    return hits


def _is_e0b_scoped_basename(name: str) -> bool:
    return any(fnmatch.fnmatch(name, pat) for pat in E0B_BASENAME_GLOBS)


def iter_e0b_scoped_modules() -> list[Path]:
    """Enumerate Scope E target files under src/python/ (0 expected at STEP 0)."""
    found: list[Path] = []
    if not PYTHON_SRC.is_dir():
        return found
    for path in PYTHON_SRC.rglob("*.py"):
        if "__pycache__" in path.parts:
            continue
        if _is_e0b_scoped_basename(path.name):
            found.append(path)
    return found


def _func_defs(tree: ast.Module) -> dict[str, ast.FunctionDef]:
    return {
        n.name: n
        for n in tree.body
        if isinstance(n, ast.FunctionDef)
    }


def _is_exact_e0a_raise(body: list[ast.stmt]) -> bool:
    """Body must be exactly: raise NotImplementedError(<E0A_MSG>)."""
    if len(body) != 1 or not isinstance(body[0], ast.Raise):
        return False
    raise_node = body[0]
    if raise_node.cause is not None:
        return False
    exc = raise_node.exc
    if not isinstance(exc, ast.Call):
        return False
    if not isinstance(exc.func, ast.Name) or exc.func.id != "NotImplementedError":
        return False
    if len(exc.args) != 1 or exc.keywords:
        return False
    arg = exc.args[0]
    if not isinstance(arg, ast.Constant) or arg.value != E0A_MSG:
        return False
    return True


def _returns_false_only(body: list[ast.stmt]) -> bool:
    if len(body) != 1 or not isinstance(body[0], ast.Return):
        return False
    val = body[0].value
    return isinstance(val, ast.Constant) and val.value is False


# ---------------------------------------------------------------------------
# STEP 0.A — Detector canaries (prove detection power before production scan)
# ---------------------------------------------------------------------------
def test_detector_flags_synthetic_violation() -> None:
    """Canary: every synthetic egress form must be hit by Scope T / bypass detector."""
    cases: list[tuple[str, str]] = [
        ("import socket\n", "socket"),
        ("from urllib import request\n", "urllib"),
        ("import http.client\n", "http.client"),
        ("from asyncio import subprocess\n", "asyncio.subprocess"),
        ("__import__('os').system('x')\n", "__import__"),
        # Edge cases required by blueprint §2 STEP 0.A
        ("import a.b.c\nimport socket.AF_INET\n", "socket"),  # top-level extract
        ("from urllib.request import urlopen as u\n", "urllib"),  # alias
        ("from http import client\n", "http.client"),
        ("from http import client as hc\n", "http.client"),
        (
            "def f():\n    import requests\n",
            "requests",
        ),  # nested (function-body) import
        (
            "if True:\n    import aiohttp\n",
            "aiohttp",
        ),  # conditional import
        (
            "import importlib\nimportlib.import_module('socket')\n",
            "import_module",
        ),  # dynamic
    ]

    for src, label in cases:
        tree = ast.parse(src)
        import_hits = denied_imports_scope_t(tree)
        bypass_hits = has_dynamic_bypass(tree)
        combined = import_hits + bypass_hits
        assert combined, (
            f"detector missed synthetic violation ({label!r}):\n{src!r}"
        )


def test_detector_allows_legitimate_local_substrate() -> None:
    """Negative canary: Scope T must NOT flag bare asyncio / subprocess / ctypes."""
    for src in (
        "import asyncio\n",
        "import subprocess\n",
        "import ctypes\n",
        "from asyncio import get_event_loop\n",
        "import subprocess as sp\n",
        "from ctypes import windll\n",
    ):
        tree = ast.parse(src)
        hits = denied_imports_scope_t(tree)
        assert not hits, (
            f"Scope T false-positive on legitimate local substrate:\n"
            f"  src={src!r}\n  hits={hits}"
        )
        # Dynamic bypass must also stay silent on bare imports (no calls).
        assert not has_dynamic_bypass(tree), (
            f"bypass detector false-positive on bare import:\n  src={src!r}"
        )


# ---------------------------------------------------------------------------
# STEP 0.B — Tree-wide sterile lock (Scope T → production src/python/)
# ---------------------------------------------------------------------------
def _iter_production_python() -> list[Path]:
    """src/python/**/*.py excluding __pycache__; tests/ is never under this tree."""
    files: list[Path] = []
    for path in PYTHON_SRC.rglob("*.py"):
        if "__pycache__" in path.parts:
            continue
        files.append(path)
    return files


def test_python_tree_has_no_network_egress_imports() -> None:
    """Scope T applied to all production Python: zero network egress imports.

    Negative check: offline_runtime.py bare asyncio must NOT be flagged.
    """
    violations: list[str] = []
    offline_runtime_seen = False
    for path in _iter_production_python():
        src = path.read_text(encoding="utf-8")
        try:
            tree = ast.parse(src, filename=str(path))
        except SyntaxError as exc:
            violations.append(f"{path}: SyntaxError: {exc}")
            continue
        hits = denied_imports_scope_t(tree)
        if hits:
            rel = path.relative_to(ROOT)
            for h in hits:
                violations.append(f"{rel}: {h}")
        if path.name == "offline_runtime.py":
            offline_runtime_seen = True
            # Explicit negative: bare asyncio must remain unflagged.
            assert not any("asyncio" in h and "subprocess" not in h for h in hits), (
                f"offline_runtime.py bare asyncio false-positive: {hits}"
            )

    assert offline_runtime_seen, "offline_runtime.py not found under src/python/"
    assert not violations, (
        "Scope T network egress imports found in production tree:\n"
        + "\n".join(violations)
    )


# ---------------------------------------------------------------------------
# STEP 0.C — E0a freeze inheritance + Scope E reserved cage
# ---------------------------------------------------------------------------
def test_e0a_stub_freeze_still_holds() -> None:
    """E0b dual-lock: knowledge_fetcher E0a stubs remain exact hard-fail."""
    src = KF_PATH.read_text(encoding="utf-8")
    tree = ast.parse(src, filename=str(KF_PATH))
    assert isinstance(tree, ast.Module)
    funcs = _func_defs(tree)

    for name, args in (
        ("_http_get", ["url"]),
        ("default_online_fetcher", ["query"]),
        ("process_pending", ["fetcher"]),
    ):
        assert name in funcs, f"missing E0a stub: {name}"
        fn = funcs[name]
        assert [a.arg for a in fn.args.args] == args, f"{name} args mismatch"
        assert _is_exact_e0a_raise(fn.body), (
            f"{name} must be exact raise NotImplementedError({E0A_MSG!r})"
        )
    assert funcs["process_pending"].args.defaults, (
        "process_pending must keep optional fetcher default"
    )

    allowed = funcs["online_fetch_allowed"]
    assert [a.arg for a in allowed.args.args] == []
    assert _returns_false_only(allowed.body), (
        "online_fetch_allowed must return False with no side effects"
    )
    for node in ast.walk(allowed):
        if isinstance(node, ast.Attribute) and node.attr in {"environ", "getenv"}:
            raise AssertionError("online_fetch_allowed must not read environment")
        if isinstance(node, ast.Call) and _call_name(node) == "getenv":
            raise AssertionError("online_fetch_allowed must not call getenv")


def test_e0b_scoped_modules_absent_or_caged() -> None:
    """STEP 1.A: e0b_attestation.py をサンクションし、Scope E cage を実働させた.

    Unsanctioned *e0b*.py / knowledge_gateway*.py must not appear.
    Sanctioned modules (when present) are still subject to DENY_E0B.
    """
    scoped = iter_e0b_scoped_modules()
    # (a) Only SANCTIONED_E0B_MODULES may exist under Scope E globs.
    unexpected = [p for p in scoped if p.name not in SANCTIONED_E0B_MODULES]
    assert unexpected == [], (
        "unsanctioned E0b-scoped modules found: "
        + ", ".join(str(p.relative_to(ROOT)) for p in unexpected)
    )
    # (b) Cage body — DENY_E0B applies to every scoped module (incl. sanctioned).
    cage_violations: list[str] = []
    for path in scoped:
        src = path.read_text(encoding="utf-8")
        tree = ast.parse(src, filename=str(path))
        for h in denied_imports_scope_e(tree):
            cage_violations.append(f"{path.relative_to(ROOT)}: import {h}")
        for h in has_dynamic_bypass(tree):
            cage_violations.append(f"{path.relative_to(ROOT)}: bypass {h}")
    assert not cage_violations, (
        "Scope E DENY_E0B violations:\n" + "\n".join(cage_violations)
    )


# ---------------------------------------------------------------------------
# STEP 0.D — Fail-closed: E0b egress IPC / facade entrypoints unwired
# ---------------------------------------------------------------------------
ENGINE_STDIO = PYTHON_SRC / "engine_stdio.py"
FACADE_PATH = PYTHON_SRC / "core" / "facade.py"

E0B_IPC_COMMANDS = frozenset({
    "knowledge.intent.build",
    "knowledge.research",
    "knowledge.integrate",
})

# Public facade names that would constitute E0b egress entrypoints (deny list).
# fetch_pending_knowledge is the E0a raise-stub and is explicitly allowed.
E0B_FACADE_EGRESS_NAMES = frozenset({
    "intent_build",
    "build_intent",
    "knowledge_intent_build",
    "research",
    "knowledge_research",
    "start_research",
    "integrate",
    "knowledge_integrate",
    "integrate_knowledge",
    "run_research",
    "fetch_knowledge",  # distinct from fetch_pending_knowledge (E0a stub)
})


def _dispatch_string_constants(tree: ast.Module) -> list[str]:
    """Collect ast.Constant(str) values inside the dispatch() function body only.

    Comments/docstrings outside dispatch, and module-level strings, are ignored
    so that documentation mentions cannot false-positive.
    """
    dispatch_fn: ast.FunctionDef | None = None
    for node in tree.body:
        if isinstance(node, ast.FunctionDef) and node.name == "dispatch":
            dispatch_fn = node
            break
    assert dispatch_fn is not None, "engine_stdio.dispatch not found"
    found: list[str] = []
    for node in ast.walk(dispatch_fn):
        if isinstance(node, ast.Constant) and isinstance(node.value, str):
            found.append(node.value)
    return found


def test_no_e0b_ipc_command_wired() -> None:
    """Fail-closed: E0b IPC command strings must not exist in dispatch body.

    INVERSION OBLIGATION (STEP 5/8): when E0b commands are wired, do NOT delete
    this test — rewrite it to the affirmative form that the commands exist AND
    always pass through attestation + dual-run gate. That rewrite is a separate
    STEP under commander ACK; STEP 0 must keep the negative (unwired) contract.
    """
    src = ENGINE_STDIO.read_text(encoding="utf-8")
    tree = ast.parse(src, filename=str(ENGINE_STDIO))
    assert isinstance(tree, ast.Module)
    constants = _dispatch_string_constants(tree)
    hits = sorted(E0B_IPC_COMMANDS.intersection(constants))
    assert not hits, (
        "E0b IPC commands must not be wired in dispatch yet; found: "
        + ", ".join(hits)
    )


def test_facade_has_no_e0b_egress_entrypoint() -> None:
    """Fail-closed: facade must expose no E0b intent/research/integrate egress APIs.

    E0a fetch_pending_knowledge (exact raise stub) remains allowed.
    INVERSION OBLIGATION (STEP 5/8): same as test_no_e0b_ipc_command_wired —
    rewrite to affirmative gated contract; do not merely delete.
    """
    src = FACADE_PATH.read_text(encoding="utf-8")
    tree = ast.parse(src, filename=str(FACADE_PATH))
    assert isinstance(tree, ast.Module)
    funcs = _func_defs(tree)

    # E0a stub must still be present as exact raise (allowed exception).
    assert "fetch_pending_knowledge" in funcs
    assert _is_exact_e0a_raise(funcs["fetch_pending_knowledge"].body), (
        "fetch_pending_knowledge must remain exact E0a raise stub"
    )

    banned = sorted(name for name in funcs if name in E0B_FACADE_EGRESS_NAMES)
    assert not banned, (
        "E0b egress facade entrypoints must not exist yet; found: "
        + ", ".join(banned)
    )


# ---------------------------------------------------------------------------
# STEP 0.E — Rust side: no HTTP client linked (text scan; cargo not invoked)
# ---------------------------------------------------------------------------
TAURI_DIR = ROOT / "apps" / "desktop" / "src-tauri"
CARGO_TOML = TAURI_DIR / "Cargo.toml"
RUST_SRC = TAURI_DIR / "src"

HTTP_CLIENT_CRATES = frozenset({
    "reqwest",
    "hyper",
    "isahc",
    "ureq",
    "curl",
    "surf",
})

# Outbound network symbols in .rs (egress clients / sockets). Sandbox capability
# stripping uses WinSock/AF_INET — those are NOT in this deny list.
RUST_OUTBOUND_PATTERNS = (
    "reqwest::",
    "TcpStream",
    "tokio::net",
    "hyper::",
    "UdpSocket",
    "isahc::",
    "ureq::",
    "surf::",
)

# Explicit allowlist substrings: sandbox capability denial / feature decls only.
# These must never cause a hit when present alone (negative canary).
RUST_SANDBOX_ALLOW_SUBSTRINGS = frozenset({
    "Win32_Networking_WinSock",
    "windows_sys::Win32::Networking::WinSock",
})


def _cargo_toml_direct_deps(text: str) -> set[str]:
    """Collect crate names appearing as direct dependency keys in Cargo.toml.

    Matches lines like `reqwest = "..."` / `reqwest = { ... }` inside any
    [dependencies] / [target.*.dependencies] / [build-dependencies] section.
    Does not parse Cargo.lock (transitive deps from tauri are out of scope).
    """
    deps: set[str] = set()
    in_deps = False
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            section = stripped[1:-1]
            in_deps = (
                section == "dependencies"
                or section.endswith(".dependencies")
                or section == "build-dependencies"
            )
            continue
        if not in_deps or not stripped or stripped.startswith("#"):
            continue
        # crate = "ver"  or  crate = { ... }
        if "=" in stripped:
            name = stripped.split("=", 1)[0].strip().strip('"')
            if name:
                deps.add(name)
    return deps


def rust_outbound_hits(text: str) -> list[str]:
    """Return outbound-network symbol hits; sandbox allowlist substrings alone are OK."""
    hits: list[str] = []
    for pat in RUST_OUTBOUND_PATTERNS:
        if pat in text:
            hits.append(pat)
    return hits


def test_rust_detector_canary() -> None:
    """Canary: reqwest:: egress hits; WinSock sandbox feature line does not."""
    assert rust_outbound_hits("reqwest::get(...)") == ["reqwest::"], (
        "detector must flag synthetic reqwest:: egress"
    )
    for allow in RUST_SANDBOX_ALLOW_SUBSTRINGS:
        assert rust_outbound_hits(allow) == [], (
            f"detector must NOT flag sandbox allowlist substring: {allow!r}"
        )
    allow_line = 'windows-sys = { features = ["Win32_Networking_WinSock"] }'
    assert rust_outbound_hits(allow_line) == [], (
        "detector must NOT flag Win32_Networking_WinSock sandbox feature line"
    )
    sandbox_import = "use windows_sys::Win32::Networking::WinSock::{socket, connect};"
    assert rust_outbound_hits(sandbox_import) == [], (
        "detector must NOT flag WinSock sandbox capability-probe imports"
    )


FORBIDDEN_REQWEST_FEATURES = frozenset({
    "gzip", "brotli", "deflate", "native-tls", "default-tls", "socks", "cookies",
})
# STEP 5.A (feature-isolation update): the BASE reqwest dependency must carry
# NO TLS provider at all — only "stream". TLS enters exclusively through the
# opt-in [features] egress-live = ["reqwest/<tls-feature>"] alias, never in the
# base [dependencies] line. This keeps `cargo build`/`cargo test` (no flags)
# free of any C-toolchain-requiring crypto backend (aws-lc-sys / ring).
SAFE_REQWEST_BASE_FEATURE_SUPERSET = frozenset({"stream"})
# reqwest 0.13.4's actual rustls-family feature is named "rustls" (not
# "rustls-tls" as in the blueprint's original prose) — verified via
# `cargo add --dry-run` feature listing during STEP 5.A. native-tls is never
# an acceptable substitute (explicitly forbidden by the blueprint).
ALLOWED_EGRESS_LIVE_TLS_FEATURES = frozenset({"rustls", "rustls-no-provider"})
OTHER_HTTP_CLIENT_CRATES = frozenset({"hyper", "isahc", "ureq", "curl", "surf"})


def _reqwest_dep_line(text: str) -> str | None:
    """Return the raw Cargo.toml line declaring the direct `reqwest` dependency.

    Assumes single-line inline-table or string form (the form this repo uses for
    all other pinned deps). Returns None if no such line exists in a
    [dependencies]-like section.
    """
    in_deps = False
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            section = stripped[1:-1]
            in_deps = (
                section == "dependencies"
                or section.endswith(".dependencies")
                or section == "build-dependencies"
            )
            continue
        if not in_deps or not stripped or stripped.startswith("#"):
            continue
        if "=" in stripped:
            name = stripped.split("=", 1)[0].strip().strip('"')
            if name == "reqwest":
                return stripped
    return None


def _extract_toml_string_list(line: str, key: str) -> set[str] | None:
    """Extract a `key = ["a", "b"]` string-list value from a single TOML line.

    Matches `key` only at a token boundary (start-of-string or preceded by a
    non-identifier char) so `features` does not match inside `default-features`.
    """
    pattern = re.compile(r"(?<![\w-])" + re.escape(key) + r"\s*=")
    m = pattern.search(line)
    if m is None:
        return None
    rest = line[m.end():].lstrip()
    if not rest.startswith("["):
        return None
    end = rest.find("]")
    if end == -1:
        return None
    inner = rest[1:end]
    items = {tok.strip().strip('"').strip("'") for tok in inner.split(",") if tok.strip()}
    return items


def _cargo_features_section(text: str) -> dict[str, str]:
    """Return {feature_name: raw_line} for every entry under [features]."""
    out: dict[str, str] = {}
    in_features = False
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            in_features = stripped[1:-1] == "features"
            continue
        if not in_features or not stripped or stripped.startswith("#"):
            continue
        if "=" in stripped:
            name = stripped.split("=", 1)[0].strip().strip('"')
            if name:
                out[name] = stripped
    return out


def reqwest_config_report(line: str | None) -> dict[str, object]:
    """Pure evaluation of a candidate BASE `reqwest = {...}` dependency line.

    STEP 5.A (feature-isolation): the base line must carry default-features=false
    and a features set that is a subset of SAFE_REQWEST_BASE_FEATURE_SUPERSET
    ({"stream"}) — i.e. NO TLS provider feature and no forbidden feature.
    Returns a dict with keys: present, default_features_false, features (set|None),
    forbidden_features (set), safe (bool). Shared by the real Cargo.toml
    assertion and the synthetic-canary check.
    """
    if line is None:
        return {
            "present": False,
            "default_features_false": False,
            "features": None,
            "forbidden_features": set(),
            "safe": False,
        }
    default_features_false = "default-features" in line and "false" in line.split(
        "default-features", 1
    )[1].split(",", 1)[0]
    features = _extract_toml_string_list(line, "features")
    forbidden = (features or set()) & FORBIDDEN_REQWEST_FEATURES
    safe = bool(
        default_features_false
        and features is not None
        and features <= SAFE_REQWEST_BASE_FEATURE_SUPERSET
        and not forbidden
    )
    return {
        "present": True,
        "default_features_false": default_features_false,
        "features": features,
        "forbidden_features": forbidden,
        "safe": safe,
    }


def egress_live_feature_report(line: str | None) -> dict[str, object]:
    """Pure evaluation of the `egress-live = [...]` Cargo [features] entry.

    Must reference exactly one `reqwest/<tls-feature>` activation where
    <tls-feature> is rustls-family (never native-tls/default-tls), and must
    not smuggle in any forbidden decompression/cookie feature via this alias.
    """
    if line is None:
        return {"present": False, "reqwest_tls_features": set(), "safe": False}
    items = _extract_toml_string_list(line, "egress-live")
    if items is None:
        return {"present": False, "reqwest_tls_features": set(), "safe": False}
    reqwest_tls_features = {
        item.split("/", 1)[1] for item in items if item.startswith("reqwest/")
    }
    forbidden = reqwest_tls_features & (FORBIDDEN_REQWEST_FEATURES | {"native-tls", "default-tls"})
    safe = bool(
        reqwest_tls_features
        and reqwest_tls_features <= ALLOWED_EGRESS_LIVE_TLS_FEATURES
        and not forbidden
    )
    return {
        "present": True,
        "reqwest_tls_features": reqwest_tls_features,
        "safe": safe,
    }


def test_rust_reqwest_safe_config() -> None:
    """STEP 5.A affirmative contract (feature-isolation): the BASE reqwest
    dependency carries ONLY `stream` (no TLS provider at all, so the default
    `cargo build`/`cargo test` never needs a C toolchain); TLS is isolated
    behind the opt-in `egress-live = ["reqwest/<rustls-family>"]` feature alias,
    which itself must never enable native-tls/default-tls/decompression/cookies.
    Also asserts no *other* HTTP client crate sneaks in.

    INVERSION OBLIGATION lineage: this replaces test_rust_has_no_http_client_dependency
    per the STEP 0.E docstring's inversion obligation, at commander-ACK'd STEP 5.A
    (feature-isolation revision).
    """
    text = CARGO_TOML.read_text(encoding="utf-8")
    deps = _cargo_toml_direct_deps(text)
    assert "reqwest" in deps, "reqwest must be a direct Cargo.toml dependency"

    other_clients = sorted(OTHER_HTTP_CLIENT_CRATES.intersection(deps))
    assert not other_clients, (
        "Only reqwest may be a direct HTTP client dependency; found: "
        + ", ".join(other_clients)
    )

    line = _reqwest_dep_line(text)
    report = reqwest_config_report(line)
    assert report["present"], "reqwest dependency line not found in Cargo.toml"
    assert report["default_features_false"], (
        "reqwest must declare default-features = false"
    )
    features = report["features"]
    assert features is not None, "reqwest base features list must be explicit"
    assert features <= SAFE_REQWEST_BASE_FEATURE_SUPERSET, (
        f"reqwest BASE features must be a subset of {SAFE_REQWEST_BASE_FEATURE_SUPERSET} "
        f"(TLS must be isolated behind egress-live); found: {features}"
    )
    assert not report["forbidden_features"], (
        "reqwest must not enable decompression/native-tls/socks/cookies features; "
        f"found: {report['forbidden_features']}"
    )
    assert report["safe"], "reqwest_config_report must report safe=True"

    features_section = _cargo_features_section(text)
    egress_line = features_section.get("egress-live")
    live_report = egress_live_feature_report(egress_line)
    assert live_report["present"], "egress-live feature alias must be defined in [features]"
    assert live_report["safe"], (
        "egress-live must activate exactly a rustls-family reqwest TLS feature "
        f"with no forbidden features; found: {live_report['reqwest_tls_features']}"
    )


def test_rust_reqwest_unsafe_config_canary() -> None:
    """Canary: synthetic unsafe configs must be judged unsafe on both axes."""
    unsafe_base = 'reqwest = { version = "0.13", features = ["gzip"] }'
    report = reqwest_config_report(unsafe_base)
    assert report["present"] is True
    assert not report["safe"], "gzip-enabling reqwest base config must be flagged unsafe"
    assert "gzip" in report["forbidden_features"]

    missing_line = None
    assert reqwest_config_report(missing_line)["safe"] is False

    # A base line that (mis-)includes a TLS feature directly must be unsafe.
    tls_in_base = 'reqwest = { version = "0.13", default-features = false, features = ["stream", "rustls"] }'
    assert reqwest_config_report(tls_in_base)["safe"] is False

    # egress-live canaries: native-tls forbidden; unknown TLS feature forbidden.
    assert egress_live_feature_report('egress-live = ["reqwest/native-tls"]')["safe"] is False
    assert egress_live_feature_report('egress-live = ["reqwest/gzip"]')["safe"] is False
    assert egress_live_feature_report('egress-live = ["reqwest/rustls"]')["safe"] is True
    assert egress_live_feature_report(None)["safe"] is False


# EGRESS_ALLOWED_FILES: the single-file ratchet for STEP 5 (§1.4 blueprint).
# reqwest:: may appear ONLY in these basenames; every other outbound-network
# pattern (raw sockets, other HTTP client crates) stays forbidden everywhere,
# including inside the allowed file itself.
EGRESS_ALLOWED_FILES = frozenset({"net_gateway.rs"})
RUST_EGRESS_SCOPED_PATTERNS = ("reqwest::",)
RUST_ALWAYS_FORBIDDEN_PATTERNS = (
    "TcpStream",
    "tokio::net",
    "hyper::",
    "UdpSocket",
    "isahc::",
    "ureq::",
    "surf::",
)


def rust_outbound_hits_scoped(basename: str, text: str) -> list[str]:
    """Scoped outbound-network detector (STEP 5.A reversal).

    Always-forbidden patterns are denied in every file, including net_gateway.rs.
    Egress-scoped patterns (reqwest::) are denied everywhere EXCEPT basenames in
    EGRESS_ALLOWED_FILES.
    """
    hits: list[str] = []
    for pat in RUST_ALWAYS_FORBIDDEN_PATTERNS:
        if pat in text:
            hits.append(pat)
    if basename not in EGRESS_ALLOWED_FILES:
        for pat in RUST_EGRESS_SCOPED_PATTERNS:
            if pat in text:
                hits.append(pat)
    return hits


def test_rust_outbound_scoped_detector_canary() -> None:
    """Canary: reqwest:: violates outside net_gateway.rs, is allowed inside it;
    always-forbidden patterns violate even inside net_gateway.rs."""
    assert rust_outbound_hits_scoped("commands.rs", "reqwest::get(x)") == ["reqwest::"]
    assert rust_outbound_hits_scoped("net_gateway.rs", "reqwest::get(x)") == []
    assert rust_outbound_hits_scoped("net_gateway.rs", "tokio::net::TcpStream") == [
        "TcpStream",
        "tokio::net",
    ]
    for allow in RUST_SANDBOX_ALLOW_SUBSTRINGS:
        assert rust_outbound_hits_scoped("os_sandbox.rs", allow) == []


def test_rust_src_has_no_outbound_network_calls_scoped() -> None:
    """src-tauri/src/**/*.rs: reqwest:: confined to net_gateway.rs; all raw-socket
    and other-HTTP-client symbols remain forbidden in every file (incl. net_gateway.rs).

    INVERSION OBLIGATION lineage: this replaces test_rust_src_has_no_outbound_network_calls
    per the STEP 0.E docstring's inversion obligation, at commander-ACK'd STEP 5.A.
    """
    assert RUST_SRC.is_dir(), f"missing Rust src tree: {RUST_SRC}"
    violations: list[str] = []
    for path in sorted(RUST_SRC.rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        hits = rust_outbound_hits_scoped(path.name, text)
        if hits:
            rel = path.relative_to(ROOT)
            violations.append(f"{rel}: {', '.join(hits)}")
    assert not violations, (
        "Outbound network symbols found in Rust sources (scoped check):\n"
        + "\n".join(violations)
    )
