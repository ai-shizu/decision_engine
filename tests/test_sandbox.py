# -*- coding: utf-8 -*-
"""Target Sandbox 回帰ガード (SPEC_FOXTROT_UI.md §10.1 / F-15)。

conftest.py が PKB_PROJECT_ROOT を一時ディレクトリへ差し替えていることの
二重の証明と、core/paths.py 経由を強制する静的トリップワイヤ。
"""
from __future__ import annotations

import os
import pathlib
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))


def test_sandbox_active() -> None:
    """conftest.py が既に PKB_PROJECT_ROOT を Sandbox へ差し替え済みであり、
    core.paths.PROJECT_ROOT/ES_DIR がリポジトリ実ルートを指していないことの
    二重の証明 (F-15a)。"""
    import core.paths as P

    assert str(P.PROJECT_ROOT) == os.environ["PKB_PROJECT_ROOT"], (
        "core.paths.PROJECT_ROOT が conftest の Sandbox と一致しない — "
        "core import が conftest より前に発生した疑い"
    )

    repo_root = pathlib.Path(__file__).resolve().parents[1]
    assert P.PROJECT_ROOT != repo_root, "PROJECT_ROOT がリポジトリ実ルートを指している (Sandbox未適用)"
    assert P.ES_DIR != repo_root / "data" / "es", "ES_DIR がリポジトリ実 data/es を指している"
    print("  sandbox active: PROJECT_ROOT/ES_DIR are isolated from repo root OK")


# core/paths.py 自身は "data" 文字列を含む正当な源であり除外する。
_EXCLUDED_FILES = {"paths.py"}
_LITERAL_WRITE_RE = re.compile(r"""(?:open|Path)\(\s*['"]data[/\\]""")


def test_no_literal_data_writes() -> None:
    """静的トリップワイヤ (W-50 の系): src/python/core 配下の .py に
    `open("data/...")` / `Path("data/...")` のようなリテラル data/ パスの
    直書きが無いことを検査する。全てのデータパスは core/paths.py の定数
    (PKB_PROJECT_ROOT 経由で束縛される) を経由せねばならない — 直書きは
    Sandbox を迂回して本番 data/ を触りうる構造的な穴になる。

    ※ このガードは字面ベースの簡易検査であり将来誤検出しうる。誤検出が
    頻発する場合は対象を書き込み系呼び出しへ絞るか、test_sandbox_active を
    主ガードとして本テストを簡素化してよい (削除はしない)。
    """
    core_dir = ROOT / "src" / "python" / "core"
    assert core_dir.is_dir(), f"core ディレクトリが見つからない: {core_dir}"

    offenders: list[str] = []
    for path in sorted(core_dir.glob("*.py")):
        if path.name in _EXCLUDED_FILES:
            continue
        text = path.read_text(encoding="utf-8")
        for lineno, line in enumerate(text.splitlines(), start=1):
            if _LITERAL_WRITE_RE.search(line):
                offenders.append(f"{path.name}:{lineno}: {line.strip()}")

    assert not offenders, (
        "リテラル data/ パス直書きを検出 (paths.py 経由を強制):\n" + "\n".join(offenders)
    )
    print("  no literal data/ path writes outside paths.py (static tripwire) OK")

