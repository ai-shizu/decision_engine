# -*- coding: utf-8 -*-
"""Cross-boundary golden: real Python tensor_profile/interview_report output ->
TS parseConsultResponse acceptance proof. No persistence, no LLM, no core edits —
calls the actual backend aggregation functions under test."""
import json
import sys
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core.interview_report import SCHEMA_VERSION, synthesize_latency  # noqa: E402
from core.session_memory import transcript_turns_from_pairs  # noqa: E402
from core.tensor_profile import (  # noqa: E402
    aggregate_profile,
    report_tensor_field,
    transcript_hash,
)

SESSION_ID = "boundary-tensor-golden-session"

TRANSCRIPT = [
    ("面接官", "opening question"),
    ("候補者", "I will clarify the objective first."),
    ("面接官", "follow-up question"),
    ("候補者", "Then I will decompose the problem into smaller parts."),
]

turns = transcript_turns_from_pairs(TRANSCRIPT, SESSION_ID)
turn_by_index = {t["turn_index"]: t for t in turns}

proposals = [
    {
        "dimension_id": "problem_structuring",
        "indicator_id": "ps_clarify_objective",
        "level": 3,
        "turn_id": turn_by_index[1]["turn_id"],
        "turn_index": 1,
        "speaker_alias": "candidate",
        "quote": "clarify the objective",
    },
    {
        "dimension_id": "problem_structuring",
        "indicator_id": "ps_decompose",
        "level": 4,
        "turn_id": turn_by_index[3]["turn_id"],
        "turn_index": 3,
        "speaker_alias": "candidate",
        "quote": "decompose the problem",
    },
]

th = transcript_hash(turns)
profile = aggregate_profile(
    SESSION_ID, th, "golden-model", "tensor_profile.v1", turns, proposals,
)

report = {
    "schema": SCHEMA_VERSION,
    "date": datetime.now().isoformat(timespec="seconds"),
    "config": {},
    "metrics": [{"axis": "論理性", "score": 80, "evidence": "strong reasoning shown"}],
    "summary": "golden summary",
    "latency": synthesize_latency([]),
    "simulated": True,
    "tensor_profile": report_tensor_field(profile),
}

payload = {
    "query": "candidate answer",
    "mode": "interview_sim",
    "answer": "interviewer reply",
    "report": report,
}

Path(sys.argv[1]).write_text(
    json.dumps(payload, ensure_ascii=False), encoding="utf-8",
)
