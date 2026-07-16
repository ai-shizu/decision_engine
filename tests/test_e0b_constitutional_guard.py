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
    """Scope E reserved cage: zero E0b modules now; DENY_E0B applies if any appear."""
    scoped = iter_e0b_scoped_modules()
    # (a) STEP 0 proof: E0b body not yet implemented.
    assert scoped == [], (
        "E0b-scoped modules must be absent at STEP 0; found: "
        + ", ".join(str(p.relative_to(ROOT)) for p in scoped)
    )
    # (b) Reserved cage body — empty loop today; auto-enforces when modules appear.
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


def test_rust_has_no_http_client_dependency() -> None:
    """Cargo.toml must not directly depend on HTTP client crates (= sterile build).

    INVERSION OBLIGATION (dependency-intro STEP): do not delete — rewrite to the
    affirmative contract that reqwest enters with default-features=false and
    features limited to rustls-tls,stream (no gzip/native-tls/socks). Separate
    STEP under commander ACK; STEP 0 keeps the negative (unlinked) contract.
    """
    text = CARGO_TOML.read_text(encoding="utf-8")
    deps = _cargo_toml_direct_deps(text)
    banned = sorted(HTTP_CLIENT_CRATES.intersection(deps))
    assert not banned, (
        "HTTP client crates must not be direct Cargo.toml dependencies; found: "
        + ", ".join(banned)
    )


def test_rust_src_has_no_outbound_network_calls() -> None:
    """src-tauri/src/**/*.rs must not contain outbound network client/socket symbols.

    WinSock / AF_INET usage in os_sandbox.rs and pkb-sandbox-probe.rs is AppContainer
    / seccomp capability stripping (denial), not egress — those symbols are outside
    RUST_OUTBOUND_PATTERNS by design.
    """
    assert RUST_SRC.is_dir(), f"missing Rust src tree: {RUST_SRC}"
    violations: list[str] = []
    for path in sorted(RUST_SRC.rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        hits = rust_outbound_hits(text)
        if hits:
            rel = path.relative_to(ROOT)
            violations.append(f"{rel}: {', '.join(hits)}")
    assert not violations, (
        "Outbound network symbols found in Rust sources:\n"
        + "\n".join(violations)
    )
