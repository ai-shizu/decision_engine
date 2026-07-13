# -*- coding: utf-8 -*-
"""Phase 4-E E0a — emergency egress lockdown contracts (networkless)."""
from __future__ import annotations

import ast
import re
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
KF_PATH = ROOT / "src" / "python" / "core" / "knowledge_fetcher.py"
CE_PATH = ROOT / "src" / "python" / "core" / "consultation_engine.py"
FACADE_PATH = ROOT / "src" / "python" / "core" / "facade.py"

# Exact hard-fail message (must match production stubs once implemented).
E0A_MSG = "Egress blocked by E0a strict lockdown."

# Split identifiers so this file is not a greppable false-positive for production audits.
_TAG = "fetch" + "_query"
_OPEN = "<" + _TAG + ">"
_CLOSE = "</" + _TAG + ">"
_ALLOW_ENV = "PKB_ALLOW_" + "ONLINE_FETCH"

_DENY_TOP = frozenset({
    "urllib",
    "socket",
    "requests",
    "httpx",
    "aiohttp",
    "ftplib",
    "websockets",
    "subprocess",
    "ctypes",
})

_DENY_CALL_NAMES = frozenset({
    "__import__",
    "urlopen",
    "system",
    "popen",
    "Popen",
    "run",
    "call",
    "check_call",
    "check_output",
})


def _call_name(node: ast.Call) -> str | None:
    func = node.func
    if isinstance(func, ast.Name):
        return func.id
    if isinstance(func, ast.Attribute):
        return func.attr
    return None


def _denied_imports(tree: ast.AST) -> list[str]:
    """Collect denied import forms (top-level and nested/from variants)."""
    hits: list[str] = []
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                name = alias.name
                top = name.split(".", 1)[0]
                if top in _DENY_TOP or name in _DENY_TOP:
                    hits.append(f"import {name}")
                if name == "http.client" or name.startswith("http.client."):
                    hits.append(f"import {name}")
                if name == "asyncio.subprocess" or name.startswith(
                    "asyncio.subprocess."
                ):
                    hits.append(f"import {name}")
        elif isinstance(node, ast.ImportFrom):
            mod = node.module or ""
            names = [a.name for a in node.names]
            top = mod.split(".", 1)[0] if mod else ""
            if top in _DENY_TOP or mod in _DENY_TOP:
                hits.append(f"from {mod} import {', '.join(names)}")
            if mod == "http.client" or mod.startswith("http.client."):
                hits.append(f"from {mod} import {', '.join(names)}")
            # from http import client  /  from http import client as X
            if mod == "http" and "client" in names:
                hits.append("from http import client")
            if mod == "asyncio.subprocess" or mod.startswith("asyncio.subprocess."):
                hits.append(f"from {mod} import {', '.join(names)}")
            # from asyncio import subprocess
            if mod == "asyncio" and "subprocess" in names:
                hits.append("from asyncio import subprocess")
    return hits


def _has_dynamic_bypass(tree: ast.AST) -> list[str]:
    hits: list[str] = []
    for node in ast.walk(tree):
        if isinstance(node, ast.Call):
            name = _call_name(node)
            if name in _DENY_CALL_NAMES:
                hits.append(name or "?")
            if isinstance(node.func, ast.Attribute):
                if node.func.attr == "import_module":
                    hits.append("import_module")
                if node.func.attr in {"system", "popen"}:
                    hits.append(f"os.{node.func.attr}")
    return hits


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


def _assert_e0a_stubs(funcs: dict[str, ast.FunctionDef], required: dict[str, list[str]]) -> None:
    for name, args in required.items():
        assert name in funcs, f"missing required signature owner: {name}"
        fn = funcs[name]
        assert [a.arg for a in fn.args.args] == args, f"{name} args mismatch"
        assert _is_exact_e0a_raise(fn.body), f"{name} must be exact E0a raise"


def test_e0a_ast_physical_lockdown() -> None:
    """Test A: knowledge_fetcher + facade AST must hard-block all egress."""
    src = KF_PATH.read_text(encoding="utf-8")
    tree = ast.parse(src, filename=str(KF_PATH))
    assert isinstance(tree, ast.Module)

    denied = _denied_imports(tree)
    assert not denied, f"denied network/process imports present: {denied}"

    bypass = _has_dynamic_bypass(tree)
    assert not bypass, f"dynamic egress bypass constructs: {bypass}"

    funcs = _func_defs(tree)
    _assert_e0a_stubs(
        funcs,
        {
            "_http_get": ["url"],
            "default_online_fetcher": ["query"],
            "process_pending": ["fetcher"],
        },
    )
    assert funcs["process_pending"].args.defaults

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

    # facade public egress entrypoint: exact raise only (no docstring body).
    facade_src = FACADE_PATH.read_text(encoding="utf-8")
    facade_tree = ast.parse(facade_src, filename=str(FACADE_PATH))
    assert isinstance(facade_tree, ast.Module)
    facade_funcs = _func_defs(facade_tree)
    assert "fetch_pending_knowledge" in facade_funcs
    fp = facade_funcs["fetch_pending_knowledge"]
    assert [a.arg for a in fp.args.args] == []
    assert _is_exact_e0a_raise(fp.body), (
        "facade.fetch_pending_knowledge must be exact E0a raise"
    )


def test_e0a_effective_prompt_sterile(monkeypatch: pytest.MonkeyPatch) -> None:
    """Test B: effective consult prompt must not solicit outbound fetch tags."""
    monkeypatch.syspath_prepend(str(ROOT / "src" / "python"))
    from core.consultation_engine import SYSTEM_PROMPT, ConsultationEngine

    engine = ConsultationEngine()
    user_prompt = engine.build_prompt(
        "キャリアの次の一手を相談したい",
        diary_hits=[],
        knowledge_hits=[],
    )
    effective = f"{SYSTEM_PROMPT}\n{user_prompt}"
    lowered = effective.lower()

    assert _OPEN.lower() not in lowered
    assert _CLOSE.lower() not in lowered
    assert _TAG.lower() not in lowered

    for phrase in (
        "外部知識リクエスト",
        "検索クエリ",
        "取得はユーザーの明示許可時のみ",
        _ALLOW_ENV,
    ):
        assert phrase.lower() not in lowered, f"banned phrase present: {phrase}"

    for phrase in (
        "外部知識の取得要求",
        "ウェブ検索",
        "ネット検索",
        "オンラインで検索",
        "検索してよい",
        "クエリを出力",
    ):
        assert phrase not in effective, f"egress solicitation present: {phrase}"

    ce_src = CE_PATH.read_text(encoding="utf-8")
    ce_tree = ast.parse(ce_src, filename=str(CE_PATH))

    banned_names = {
        "extract_" + "fetch_queries",
        "queue_" + "fetch_queries",
    }
    for node in ast.walk(ce_tree):
        if isinstance(node, ast.ImportFrom):
            for alias in node.names:
                assert alias.name not in banned_names, alias.name
        if isinstance(node, ast.Name) and isinstance(node.ctx, ast.Load):
            assert node.id not in banned_names, node.id
        if isinstance(node, ast.Call):
            name = _call_name(node)
            assert name not in banned_names, name

    assert not re.search(re.escape(_OPEN), ce_src, flags=re.I)
    assert not re.search(re.escape(_CLOSE), ce_src, flags=re.I)
    assert "knowledge_hits" in ce_src or "sync_knowledge_index" in ce_src
