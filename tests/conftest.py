# -*- coding: utf-8 -*-
"""pytest 共通設定 — Target Sandbox (SPEC_FOXTROT_UI.md §10.1 / F-15)。

【急所】core/paths.py の全パス定数は import 時に確定する (束縛済み Path)。
env を後から立てても既 import の定数は動かない。pytest は conftest.py を
テスト収集 (= 各 test_*.py の import) より前に読むため、モジュール最上部
(いかなる `from core...` import よりも前) で PKB_PROJECT_ROOT を Sandbox へ
差し替えることで、以後の全 core.paths.* 定数を Sandbox 配下へ束縛できる。

PKB_DATA_DIR は新設しない — PKB_PROJECT_ROOT が唯一の真実の源 (F-15b)。
本ファイル自身は core を import しない (import順の事故を避ける。paths の
確認は tests/test_sandbox.py 側で行う)。
"""
from __future__ import annotations

import atexit
import os
import pathlib
import shutil
import tempfile

# ---------------------------------------------------------------- F-15a
_SANDBOX = pathlib.Path(tempfile.mkdtemp(prefix="pkb_test_"))
os.environ["PKB_PROJECT_ROOT"] = str(_SANDBOX)
# Tests must never depend on a workstation's DPAPI state, and non-Windows
# runners intentionally require a managed key provider.  A fixed sandbox-only
# root makes process-restart and cross-process state-chain contracts portable.
os.environ["PKB_IDENTITY_ROOT_KEY_HEX"] = "51" * 32

_SKELETON = (
    "data/raw",
    "data/es",
    "data/knowledge",
    "data/processed",
    "data/records/interviews",
    "build",
    "models",
)
for _rel in _SKELETON:
    (_SANDBOX / _rel).mkdir(parents=True, exist_ok=True)

atexit.register(lambda: shutil.rmtree(_SANDBOX, ignore_errors=True))

import pytest  # noqa: E402 (env を立てた後の import なので core 由来の事故はない)

# ---------------------------------------------------------------- W-50
# 各テストの前後で可変領域を空にし、実行順に依存する相互汚染を断つ。
# data/raw はテスト自身が自前で seed する対象であり、ここでは触らない
# (SPEC §10.1 施工構造の明示的な対象4領域のみ)。
_ISOLATED_SUBDIRS = ("data/es", "data/knowledge", "data/records/interviews", "data/processed")


def _reset_isolated_dirs() -> None:
    for _rel in _ISOLATED_SUBDIRS:
        _d = _SANDBOX / _rel
        shutil.rmtree(_d, ignore_errors=True)
        _d.mkdir(parents=True, exist_ok=True)


@pytest.fixture(autouse=True)
def _isolate_data():
    """F-15 / W-50: 各テストの前後で Sandbox の可変領域を空にする。"""
    _reset_isolated_dirs()
    yield
    _reset_isolated_dirs()
