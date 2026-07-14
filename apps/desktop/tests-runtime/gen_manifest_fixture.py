# -*- coding: utf-8 -*-
"""STAGE 5 cross-boundary golden: Python 実出力 -> TS parser 受理の証明。永続化なし。"""
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core.retrieval_manifest import (  # noqa: E402
    build_bounded_context_with_manifest,
    manifest_to_dict,
)
from core.state_chain import genesis_parent_hash  # noqa: E402

SESSION_GENESIS_ID = "cd" * 64

_, _, manifest = build_bounded_context_with_manifest(
    parent_hash=genesis_parent_hash(SESSION_GENESIS_ID),
    sequence_number=1,
    session_genesis_id=SESSION_GENESIS_ID,
    session_id="boundary-golden-session",
    transcript=[
        ("面接官", "turn-000-statement about problem 0 and data 0%"),
        ("candidate", "turn-001-statement about problem 1 and data 3%"),
    ],
    current_query="turn-001-statement about problem 1 and data 3%",
    runtime_identity="ab" * 64,
)
payload = {"manifest": manifest_to_dict(manifest), "reason": None}
Path(sys.argv[1]).write_text(
    json.dumps(payload, ensure_ascii=False), encoding="utf-8",
)
