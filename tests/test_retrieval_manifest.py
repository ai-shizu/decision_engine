# -*- coding: utf-8 -*-
"""Phase 4-A STEP 1 — RetrievalManifestV1 remediation contract tests."""
from __future__ import annotations

import ast
import json
import sys
from pathlib import Path
from unittest.mock import patch

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core.consultation_engine import ConsultationEngine  # noqa: E402
from core.retrieval_manifest import (  # noqa: E402
    CandidateStatus,
    ContextLane,
    LaneUsageV1,
    ReasonCode,
    RetrievalCandidateV1,
    RetrievalManifestPersistenceError,
    RetrievalManifestV1,
    SourceType,
    build_bounded_context_with_manifest,
    build_retrieval_manifest,
    canonical_manifest_json,
    compute_content_hash,
    compute_context_hash,
    compute_manifest_id,
    load_latest_retrieval_manifest,
    manifest_from_dict,
    manifest_to_dict,
    save_retrieval_manifest,
    validate_manifest,
)
from core.session_memory import (  # noqa: E402
    DYNAMIC_CONTEXT_CHAR_BUDGET,
    EXACT_TAIL_CHAR_BUDGET,
    MAX_ACTIVE_MEMORY_ATOMS,
    MAX_RETRIEVED_ATOMS,
    RETRIEVED_EVIDENCE_BUDGET,
    WORKING_MEMORY_CHAR_BUDGET,
    _assemble_context_units,
    _build_authoritative_current_unit,
    _format_atom_line,
    _select_atom_blocks,
    _select_atoms,
    _select_tail_turn_blocks,
    _working_memory_from_atoms,
    build_bounded_context,
    char_count,
    compile_atoms,
    normalize_text,
    transcript_turns_from_pairs,
)

# Frozen from commit 412c1c2 (pre-Phase-4-A HEAD) via legacy selection algorithm.
_GOLDEN_COMMIT = "412c1c2d9e02accb726909500faef75a5fe1d3c5"
_GOLDEN_SESSION = "phase4a-golden-session"
_GOLDEN_CONTEXT_HASH = "d3e1b6bb8dc5c1a44361db578a70e62c"
_GOLDEN_WM_IDS = (
    "d53c162d149ef76ad762dbbe279c5c47",
    "9748a242365fa40e8a4ff6c350ea3d03",
    "d9b5da0e10d018702fb8dd2a209e7ed9",
    "9dafa9feacc3f37602bee3e7309d85bd",
    "ec893a886008ce5cb30ca9f0e9dc260b",
    "44c11f5d734d65b956588798d953190b",
    "b7d2a366fcb27bed1766f7783285bec0",
    "26eafcf5235e6b92a4d0ac780549964b",
    "e16a3cbc764ad615d327efc6687fe33c",
    "7c633f84ca72d8e513d99181545833de",
    "753aa078a4f9f8a285049dfea522fff2",
    "9e17ebcbcd64b1654e86b219f16032a6",
    "96fd82a53d3a13eeeb700e936f77e2d7",
    "02008f252e18f333b94be2dab1e2a950",
    "81435a291d425d71c2987f2f01ac0484",
    "4e898247711000033e2fcab345935500",
    "b0aa0fa75e86a14cb1d32ab08158a2c3",
    "7eb471e137313ae3bae817900cd4c4ea",
    "bda22fdaea02120a6e1e755d133462f5",
    "6f0d6c9d65d673d0766f169f75e770c5",
    "d01cfb6c68c5da130ed62cbb19f776f6",
    "146a444d5ad9d00a62ebebe1c4770095",
)
_CONTRACT_SESSION = "phase4a-contract-session"
_CONTRACT_CONTEXT_HASH = "13c1ee67938ab0394636000b50f61672"
_CONTRACT_WM_IDS = (
    "3dca42d0d9b4f9b4ed9f9e55566de164",
    "2906edb7fca0355fcb083b79d81bd496",
    "793fc87f4728194fc464154fa1b0a058",
    "6b38f7aa3fcb14be8e3eec326e71e116",
    "43c54d1702daf280989c54648a5eb90b",
)
_CONTRACT_TRANSCRIPT = [
    ("面接官", "turn-000-statement about problem 0 and data 0%"),
    ("候補者", "turn-001-statement about problem 1 and data 3%"),
    ("面接官", "turn-002-statement about problem 2 and data 6%"),
    ("候補者", "turn-003-statement about problem 3 and data 9%"),
    ("面接官", "turn-004-statement about problem 4 and data 12%"),
    ("候補者", "turn-005-statement about problem 5 and data 15%"),
]


def _golden_transcript() -> list[tuple[str, str]]:
    transcript: list[tuple[str, str]] = []
    for i in range(30):
        role = "候補者" if i % 2 else "面接官"
        text = (
            f"turn-{i:03d}-statement about problem {i} and data {i * 3}% "
            f"with extra atomizable sentence {i}."
        )
        transcript.append((role, text))
    return transcript


def _contract_kwargs() -> dict:
    return {
        "session_id": _CONTRACT_SESSION,
        "transcript": list(_CONTRACT_TRANSCRIPT),
        "current_query": "turn-005-statement about problem 5 and data 15%",
        "runtime_identity": "ab" * 64,
    }


def _phase3b_reference_build_bounded_context(
    *,
    session_id: str,
    transcript: list[tuple[str, str]],
    current_query: str,
) -> tuple[str, object]:
    """Legacy algorithm frozen at commit 412c1c2 (observability-free)."""
    from core.session_memory import CURRENT_TURN_CHAR_BUDGET

    query = normalize_text(current_query)
    turns = transcript_turns_from_pairs(transcript, session_id)
    current_turn_index = len(turns)
    exclude_turn_index: int | None = None
    units: list[tuple[str, str]] = []
    if query:
        header, body, exclude_turn_index = _build_authoritative_current_unit(
            query, turns, transcript, CURRENT_TURN_CHAR_BUDGET,
        )
        units.append((header, body))
    tail_blocks = _select_tail_turn_blocks(
        transcript,
        session_id,
        EXACT_TAIL_CHAR_BUDGET,
        exclude_turn_index=exclude_turn_index,
    )
    if tail_blocks:
        units.append(("# Recent transcript", "\n\n".join(tail_blocks)))
    atoms = compile_atoms(transcript, session_id)
    if exclude_turn_index is not None:
        atoms = [a for a in atoms if a.source.turn_index != exclude_turn_index]
    active_goal_ids = tuple(
        a.atom_id for a in atoms if a.kind == "goal" and a.status == "active"
    )
    wm_atoms = _select_atoms(
        atoms,
        query,
        current_turn_index,
        active_goal_ids,
        MAX_ACTIVE_MEMORY_ATOMS,
        WORKING_MEMORY_CHAR_BUDGET,
    )
    wm_lines = _select_atom_blocks(wm_atoms, WORKING_MEMORY_CHAR_BUDGET)
    if wm_lines:
        units.append(("# Working memory", "\n".join(wm_lines)))
    retrieved = _select_atoms(
        atoms,
        query,
        current_turn_index,
        active_goal_ids,
        MAX_RETRIEVED_ATOMS,
        RETRIEVED_EVIDENCE_BUDGET,
    )
    wm_ids = {a.atom_id for a in wm_atoms}
    retrieved = [a for a in retrieved if a.atom_id not in wm_ids]
    rv_lines = _select_atom_blocks(retrieved, RETRIEVED_EVIDENCE_BUDGET)
    if rv_lines:
        units.append(("# Retrieved evidence", "\n".join(rv_lines)))
    context = _assemble_context_units(units, DYNAMIC_CONTEXT_CHAR_BUDGET)
    included_atoms = [
        a for a in wm_atoms + retrieved if _format_atom_line(a) in context
    ]
    wm = _working_memory_from_atoms(
        included_atoms,
        char_count(context),
        session_id,
        len(transcript),
    )
    return context, wm


class FailIfCalledBackend:
    def generate(self, *args, **kwargs):
        raise AssertionError("generate must not be called")

    def generate_structured(self, *args, **kwargs):
        raise AssertionError("generate_structured must not be called")


def _minimal_candidate(**overrides) -> RetrievalCandidateV1:
    base = dict(
        candidate_id="CURRENT:abc",
        document_id="abc",
        content_hash=compute_content_hash("x"),
        source_type=SourceType.CURRENT_QUERY,
        lane=ContextLane.CURRENT,
        char_count=1,
        included_chars=1,
        status=CandidateStatus.ACCEPTED,
        reason_code=ReasonCode.ACCEPTED_REQUIRED_CURRENT,
        selection_rank=None,
        source_index=None,
        speaker_alias=None,
        memory_kind=None,
    )
    base.update(overrides)
    return RetrievalCandidateV1(**base)


def _minimal_lane(**overrides) -> LaneUsageV1:
    base = dict(
        lane=ContextLane.CURRENT,
        budget_chars=2400,
        used_chars=0,
        formatting_chars=0,
        accepted_count=0,
        rejected_count=0,
        deduplicated_count=0,
    )
    base.update(overrides)
    return LaneUsageV1(**base)


def _minimal_manifest(**overrides) -> RetrievalManifestV1:
    cand = _minimal_candidate()
    lane = _minimal_lane(used_chars=1, formatting_chars=0, accepted_count=1)
    lanes = (
        lane,
        _minimal_lane(lane=ContextLane.RECENT_TRANSCRIPT, budget_chars=3600),
        _minimal_lane(lane=ContextLane.WORKING_MEMORY, budget_chars=4000),
        _minimal_lane(lane=ContextLane.RETRIEVED_EVIDENCE, budget_chars=2000),
    )
    factory_kwargs = dict(
        session_id="s",
        transcript_version=1,
        query_hash="a" * 32,
        context_hash="b" * 32,
        prompt_version="pv1",
        runtime_identity="ab" * 64,
        used_chars=1,
        formatting_overhead_chars=0,
        candidates=(cand,),
        lane_usage=lanes,
    )
    factory_kwargs.update(overrides)
    return build_retrieval_manifest(**factory_kwargs)


def _manifest_constructor_kwargs(manifest: RetrievalManifestV1) -> dict:
    return dict(
        schema=manifest.schema,
        manifest_id=manifest.manifest_id,
        session_id=manifest.session_id,
        transcript_version=manifest.transcript_version,
        query_hash=manifest.query_hash,
        context_hash=manifest.context_hash,
        policy_version=manifest.policy_version,
        prompt_version=manifest.prompt_version,
        runtime_identity=manifest.runtime_identity,
        total_budget_chars=manifest.total_budget_chars,
        used_chars=manifest.used_chars,
        formatting_overhead_chars=manifest.formatting_overhead_chars,
        candidates=manifest.candidates,
        lane_usage=manifest.lane_usage,
    )


# ------------------------------------------------------------------ v2 strict boundary RED


def test_direct_constructor_rejects_wrong_manifest_id() -> None:
    manifest = _minimal_manifest()
    kwargs = _manifest_constructor_kwargs(manifest)
    kwargs["manifest_id"] = "f" * 32
    with pytest.raises(ValueError, match="manifest_id mismatch"):
        RetrievalManifestV1(**kwargs)


def test_direct_constructor_rejects_float_total_budget_chars() -> None:
    manifest = _minimal_manifest()
    kwargs = _manifest_constructor_kwargs(manifest)
    kwargs["total_budget_chars"] = 12000.0
    with pytest.raises(ValueError, match="total_budget_chars must be int"):
        RetrievalManifestV1(**kwargs)


def test_direct_constructor_rejects_bool_total_budget_chars() -> None:
    manifest = _minimal_manifest()
    kwargs = _manifest_constructor_kwargs(manifest)
    kwargs["total_budget_chars"] = True
    with pytest.raises(ValueError, match="total_budget_chars must be int"):
        RetrievalManifestV1(**kwargs)


def test_direct_constructor_rejects_list_memory_kind() -> None:
    with pytest.raises(ValueError, match="memory_kind must be str or None"):
        _minimal_candidate(memory_kind=[])


def test_direct_constructor_rejects_list_speaker_alias() -> None:
    with pytest.raises(ValueError, match="speaker_alias must be str or None"):
        _minimal_candidate(speaker_alias=[])


def test_direct_constructor_rejects_list_candidates() -> None:
    manifest = _minimal_manifest()
    kwargs = _manifest_constructor_kwargs(manifest)
    kwargs["candidates"] = []
    with pytest.raises(ValueError, match="candidates must be tuple"):
        RetrievalManifestV1(**kwargs)


def test_direct_constructor_rejects_list_lane_usage() -> None:
    manifest = _minimal_manifest()
    kwargs = _manifest_constructor_kwargs(manifest)
    kwargs["lane_usage"] = []
    with pytest.raises(ValueError, match="lane_usage must be tuple"):
        RetrievalManifestV1(**kwargs)


def test_direct_constructor_rejects_object_candidate_element() -> None:
    manifest = _minimal_manifest()
    kwargs = _manifest_constructor_kwargs(manifest)
    kwargs["candidates"] = (object(),)
    with pytest.raises(ValueError, match="candidate element must be RetrievalCandidateV1"):
        RetrievalManifestV1(**kwargs)


def test_direct_constructor_rejects_object_lane_element() -> None:
    manifest = _minimal_manifest()
    kwargs = _manifest_constructor_kwargs(manifest)
    kwargs["lane_usage"] = (object(),) + manifest.lane_usage[1:]
    with pytest.raises(ValueError, match="lane_usage element must be LaneUsageV1"):
        RetrievalManifestV1(**kwargs)


def test_factory_rejects_model_hash_none() -> None:
    manifest = _minimal_manifest()
    kwargs = _manifest_constructor_kwargs(manifest)
    with pytest.raises(ValueError, match="canonical runtime identity"):
        build_retrieval_manifest(
            session_id=manifest.session_id,
            transcript_version=manifest.transcript_version,
            query_hash=manifest.query_hash,
            context_hash=manifest.context_hash,
            prompt_version=manifest.prompt_version,
            model_hash=None,
            used_chars=manifest.used_chars,
            formatting_overhead_chars=manifest.formatting_overhead_chars,
            candidates=manifest.candidates,
            lane_usage=manifest.lane_usage,
        )


def test_factory_rejects_model_hash_false() -> None:
    manifest = _minimal_manifest()
    with pytest.raises(ValueError, match="canonical runtime identity"):
        build_retrieval_manifest(
            session_id=manifest.session_id,
            transcript_version=manifest.transcript_version,
            query_hash=manifest.query_hash,
            context_hash=manifest.context_hash,
            prompt_version=manifest.prompt_version,
            model_hash=False,
            used_chars=manifest.used_chars,
            formatting_overhead_chars=manifest.formatting_overhead_chars,
            candidates=manifest.candidates,
            lane_usage=manifest.lane_usage,
        )


def test_factory_rejects_invalid_model_hash() -> None:
    manifest = _minimal_manifest()
    with pytest.raises(ValueError, match="canonical runtime identity"):
        build_retrieval_manifest(
            session_id=manifest.session_id,
            transcript_version=manifest.transcript_version,
            query_hash=manifest.query_hash,
            context_hash=manifest.context_hash,
            prompt_version=manifest.prompt_version,
            model_hash="INVALID",
            used_chars=manifest.used_chars,
            formatting_overhead_chars=manifest.formatting_overhead_chars,
            candidates=manifest.candidates,
            lane_usage=manifest.lane_usage,
        )


def test_factory_constructs_manifest_exactly_once() -> None:
    manifest = _minimal_manifest()
    call_count = 0
    real_init = RetrievalManifestV1.__init__

    def counting_init(self, *args, **kwargs):
        nonlocal call_count
        call_count += 1
        return real_init(self, *args, **kwargs)

    with patch.object(RetrievalManifestV1, "__init__", counting_init):
        build_retrieval_manifest(
            session_id=manifest.session_id,
            transcript_version=manifest.transcript_version,
            query_hash=manifest.query_hash,
            context_hash=manifest.context_hash,
            prompt_version=manifest.prompt_version,
            runtime_identity=manifest.runtime_identity,
            used_chars=manifest.used_chars,
            formatting_overhead_chars=manifest.formatting_overhead_chars,
            candidates=manifest.candidates,
            lane_usage=manifest.lane_usage,
        )
    assert call_count == 1


def test_factory_source_has_no_placeholder_manifest_probe() -> None:
    source = (ROOT / "src" / "python" / "core" / "retrieval_manifest.py").read_text(
        encoding="utf-8",
    )
    tree = ast.parse(source)
    forbidden = '"0" * 32'
    for node in ast.walk(tree):
        if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Mult):
            left = ast.unparse(node.left) if hasattr(ast, "unparse") else ""
            right = ast.unparse(node.right) if hasattr(ast, "unparse") else ""
            if left == '"0"' and right == "32":
                pytest.fail("placeholder manifest_id probe pattern found")
    assert forbidden not in source.replace(" ", "")


def test_from_dict_rejects_model_hash_none() -> None:
    manifest = _minimal_manifest()
    data = manifest_to_dict(manifest)
    data["model_hash"] = None
    with pytest.raises(ValueError, match="canonical runtime identity"):
        manifest_from_dict(data)


def test_from_dict_rejects_float_total_budget_chars() -> None:
    manifest = _minimal_manifest()
    data = manifest_to_dict(manifest)
    data["total_budget_chars"] = 12000.0
    with pytest.raises(ValueError, match="total_budget_chars must be int"):
        manifest_from_dict(data)


def test_from_dict_rejects_list_memory_kind() -> None:
    manifest = _minimal_manifest()
    data = manifest_to_dict(manifest)
    data["candidates"] = list(data["candidates"])
    data["candidates"][0] = dict(data["candidates"][0])
    data["candidates"][0]["memory_kind"] = []
    with pytest.raises(ValueError, match="memory_kind must be str or None"):
        manifest_from_dict(data)


def test_from_dict_rejects_list_speaker_alias() -> None:
    manifest = _minimal_manifest()
    data = manifest_to_dict(manifest)
    data["candidates"] = list(data["candidates"])
    data["candidates"][0] = dict(data["candidates"][0])
    data["candidates"][0]["speaker_alias"] = []
    with pytest.raises(ValueError, match="speaker_alias must be str or None"):
        manifest_from_dict(data)


def test_from_dict_rejects_dict_candidates() -> None:
    manifest = _minimal_manifest()
    data = manifest_to_dict(manifest)
    data["candidates"] = {}
    with pytest.raises(ValueError, match="candidates must be list"):
        manifest_from_dict(data)


def test_from_dict_rejects_dict_lane_usage() -> None:
    manifest = _minimal_manifest()
    data = manifest_to_dict(manifest)
    data["lane_usage"] = {}
    with pytest.raises(ValueError, match="lane_usage must be list"):
        manifest_from_dict(data)


def test_from_dict_rejects_non_dict_candidate_item() -> None:
    manifest = _minimal_manifest()
    data = manifest_to_dict(manifest)
    data["candidates"] = ["not-a-dict"]
    with pytest.raises(ValueError, match="candidate item must be dict"):
        manifest_from_dict(data)


def test_from_dict_rejects_non_dict_lane_item() -> None:
    manifest = _minimal_manifest()
    data = manifest_to_dict(manifest)
    data["lane_usage"] = ["not-a-dict"]
    with pytest.raises(ValueError, match="lane_usage item must be dict"):
        manifest_from_dict(data)


def test_compiler_rejects_model_hash_none() -> None:
    with pytest.raises(ValueError, match="canonical runtime identity"):
        build_bounded_context_with_manifest(
            **_contract_kwargs(),
            model_hash=None,
        )


def test_compiler_rejects_model_hash_false() -> None:
    with pytest.raises(ValueError, match="canonical runtime identity"):
        build_bounded_context_with_manifest(
            **_contract_kwargs(),
            model_hash=False,
        )


def test_compiler_does_not_normalize_invalid_model_hash() -> None:
    with pytest.raises(ValueError, match="canonical runtime identity"):
        build_bounded_context_with_manifest(
            **_contract_kwargs(),
            model_hash="INVALID",
        )


# ------------------------------------------------------------------ 3.1 legacy selection


def test_retrieved_deduplication_does_not_refill_lower_ranked_atoms() -> None:
    transcript = _golden_transcript()
    query = transcript[-1][1]
    context, _, manifest = build_bounded_context_with_manifest(
        session_id=_GOLDEN_SESSION,
        transcript=transcript,
        current_query=query,
        runtime_identity="ab" * 64,
    )
    assert "# Retrieved evidence" not in context
    rv = [c for c in manifest.candidates if c.lane == ContextLane.RETRIEVED_EVIDENCE]
    deduped = [c for c in rv if c.status == CandidateStatus.DEDUPLICATED]
    accepted = [c for c in rv if c.status == CandidateStatus.ACCEPTED]
    assert len(deduped) == MAX_RETRIEVED_ATOMS
    assert len(accepted) == 0
    assert all(c.reason_code == ReasonCode.DEDUPLICATED_HIGHER_LANE for c in deduped)
    low_priority = [
        c for c in rv
        if c.status == CandidateStatus.REJECTED
        and c.reason_code == ReasonCode.REJECTED_LOW_PRIORITY
    ]
    assert low_priority


def test_context_matches_pre_phase4a_golden_fixture() -> None:
    transcript = _golden_transcript()
    query = transcript[-1][1]
    context, wm = build_bounded_context(
        session_id=_GOLDEN_SESSION,
        transcript=transcript,
        current_query=query,
        runtime_identity="ab" * 64,
    )
    from core.retrieval_manifest import compute_context_hash

    assert compute_context_hash(context) == _GOLDEN_CONTEXT_HASH
    assert wm.supporting_atom_ids == _GOLDEN_WM_IDS
    legacy_ctx, legacy_wm = _phase3b_reference_build_bounded_context(
        session_id=_GOLDEN_SESSION,
        transcript=transcript,
        current_query=query,
    )
    assert context == legacy_ctx
    assert wm == legacy_wm


# ------------------------------------------------------------------ 3.2 privacy


def test_manifest_masks_unknown_real_name_role() -> None:
    real_name = "山田太郎"
    transcript = [(real_name, "相談内容について話します")]
    _, _, manifest = build_bounded_context_with_manifest(
        session_id="privacy-session",
        transcript=transcript,
        current_query="相談内容について話します",
        runtime_identity="ab" * 64,
    )
    blob = json.dumps(manifest_to_dict(manifest), ensure_ascii=False)
    assert real_name not in blob
    turn_cands = [
        c for c in manifest.candidates
        if c.source_type == SourceType.TRANSCRIPT_TURN
    ]
    assert turn_cands
    assert all(c.speaker_alias is None for c in turn_cands)
    for cand in manifest.candidates:
        assert real_name not in cand.document_id
        assert real_name not in cand.content_hash
        assert real_name not in cand.candidate_id


# ------------------------------------------------------------------ 3.3 dataclass strictness


def test_candidate_rejects_bool_char_count() -> None:
    with pytest.raises(ValueError, match="char_count must be int"):
        _minimal_candidate(char_count=True)


def test_candidate_rejects_float_included_chars() -> None:
    with pytest.raises(ValueError, match="included_chars must be int"):
        _minimal_candidate(included_chars=1.5)


def test_candidate_rejects_string_selection_rank() -> None:
    with pytest.raises(ValueError, match="selection_rank must be int"):
        _minimal_candidate(selection_rank="0")


def test_lane_rejects_negative_budget_chars() -> None:
    with pytest.raises(ValueError, match="budget_chars must be nonnegative"):
        _minimal_lane(budget_chars=-1)


def test_lane_rejects_negative_formatting_chars() -> None:
    with pytest.raises(ValueError, match="formatting_chars must be nonnegative"):
        _minimal_lane(formatting_chars=-3)


def test_lane_rejects_wrong_fixed_budget_for_recent_transcript() -> None:
    with pytest.raises(ValueError, match="budget_chars mismatch"):
        _minimal_lane(lane=ContextLane.RECENT_TRANSCRIPT, budget_chars=2400)


def test_manifest_rejects_duplicated_lane() -> None:
    lanes = (
        _minimal_lane(used_chars=1, formatting_chars=0, accepted_count=1),
        _minimal_lane(lane=ContextLane.CURRENT, budget_chars=2400),
        _minimal_lane(lane=ContextLane.WORKING_MEMORY, budget_chars=4000),
        _minimal_lane(lane=ContextLane.RETRIEVED_EVIDENCE, budget_chars=2000),
    )
    with pytest.raises(ValueError, match="lane_usage order mismatch"):
        build_retrieval_manifest(
            session_id="s",
            transcript_version=1,
            query_hash="a" * 32,
            context_hash="b" * 32,
            prompt_version="pv1",
            runtime_identity="ab" * 64,
            used_chars=1,
            formatting_overhead_chars=0,
            candidates=(_minimal_candidate(),),
            lane_usage=lanes,
        )


def test_manifest_rejects_missing_lane() -> None:
    lanes = (_minimal_lane(used_chars=1, formatting_chars=0, accepted_count=1),)
    with pytest.raises(ValueError, match="lane_usage must have 4 entries"):
        build_retrieval_manifest(
            session_id="s",
            transcript_version=1,
            query_hash="a" * 32,
            context_hash="b" * 32,
            prompt_version="pv1",
            runtime_identity="ab" * 64,
            used_chars=1,
            formatting_overhead_chars=0,
            candidates=(_minimal_candidate(),),
            lane_usage=lanes,
        )


def test_manifest_rejects_lane_order_violation() -> None:
    lanes = (
        _minimal_lane(lane=ContextLane.RECENT_TRANSCRIPT, budget_chars=3600),
        _minimal_lane(used_chars=1, formatting_chars=0, accepted_count=1),
        _minimal_lane(lane=ContextLane.WORKING_MEMORY, budget_chars=4000),
        _minimal_lane(lane=ContextLane.RETRIEVED_EVIDENCE, budget_chars=2000),
    )
    with pytest.raises(ValueError, match="lane_usage order mismatch"):
        build_retrieval_manifest(
            session_id="s",
            transcript_version=1,
            query_hash="a" * 32,
            context_hash="b" * 32,
            prompt_version="pv1",
            runtime_identity="ab" * 64,
            used_chars=1,
            formatting_overhead_chars=0,
            candidates=(_minimal_candidate(),),
            lane_usage=lanes,
        )


def test_lane_rejects_used_chars_above_lane_budget() -> None:
    with pytest.raises(ValueError, match="lane included content exceeds budget"):
        _minimal_lane(used_chars=5000, formatting_chars=0)


def test_manifest_rejects_lane_accepted_count_mismatch() -> None:
    lanes = (
        _minimal_lane(used_chars=1, formatting_chars=0, accepted_count=99),
        _minimal_lane(lane=ContextLane.RECENT_TRANSCRIPT, budget_chars=3600),
        _minimal_lane(lane=ContextLane.WORKING_MEMORY, budget_chars=4000),
        _minimal_lane(lane=ContextLane.RETRIEVED_EVIDENCE, budget_chars=2000),
    )
    with pytest.raises(ValueError, match="lane accepted_count mismatch"):
        build_retrieval_manifest(
            session_id="s",
            transcript_version=1,
            query_hash="a" * 32,
            context_hash="b" * 32,
            prompt_version="pv1",
            runtime_identity="ab" * 64,
            used_chars=1,
            formatting_overhead_chars=0,
            candidates=(_minimal_candidate(),),
            lane_usage=lanes,
        )


def test_candidate_rejects_arbitrary_memory_kind() -> None:
    with pytest.raises(ValueError, match="memory_kind not allowed"):
        _minimal_candidate(memory_kind="fantasy_kind")


def test_candidate_rejects_non_hex_content_hash() -> None:
    with pytest.raises(ValueError, match="content_hash must be 32-char lowercase hex"):
        _minimal_candidate(content_hash="not-a-hash")


def test_candidate_rejects_uppercase_content_hash() -> None:
    with pytest.raises(ValueError, match="content_hash must be 32-char lowercase hex"):
        _minimal_candidate(content_hash=("a" * 31 + "A"))


def test_candidate_rejects_31_digit_content_hash() -> None:
    with pytest.raises(ValueError, match="content_hash must be 32-char lowercase hex"):
        _minimal_candidate(content_hash="a" * 31)


def test_candidate_rejects_33_digit_content_hash() -> None:
    with pytest.raises(ValueError, match="content_hash must be 32-char lowercase hex"):
        _minimal_candidate(content_hash="a" * 33)


def test_manifest_rejects_model_hash_none() -> None:
    manifest = _minimal_manifest()
    data = manifest_to_dict(manifest)
    data["model_hash"] = None
    with pytest.raises(ValueError, match="canonical runtime identity"):
        manifest_from_dict(data)


def test_candidate_rejects_status_reason_mismatch() -> None:
    with pytest.raises(ValueError, match="status/reason mismatch"):
        _minimal_candidate(
            status=CandidateStatus.ACCEPTED,
            reason_code=ReasonCode.REJECTED_BUDGET_LIMIT,
        )


def test_latest_pointer_rejects_extra_key(tmp_path, monkeypatch) -> None:
    from core import paths

    manifest_dir = tmp_path / "retrieval_manifests"
    monkeypatch.setattr(paths, "RETRIEVAL_MANIFESTS_DIR", manifest_dir)
    monkeypatch.setattr(
        paths, "LATEST_RETRIEVAL_MANIFEST", manifest_dir / "latest.json",
    )
    manifest_dir.mkdir(parents=True)
    (manifest_dir / "latest.json").write_text(
        json.dumps({"manifest_id": "a" * 32, "extra": 1}),
        encoding="utf-8",
    )
    with pytest.raises(ValueError, match="latest pointer key mismatch"):
        load_latest_retrieval_manifest()


def test_latest_pointer_rejects_path_traversal(tmp_path, monkeypatch) -> None:
    from core import paths

    manifest_dir = tmp_path / "retrieval_manifests"
    monkeypatch.setattr(paths, "RETRIEVAL_MANIFESTS_DIR", manifest_dir)
    monkeypatch.setattr(
        paths, "LATEST_RETRIEVAL_MANIFEST", manifest_dir / "latest.json",
    )
    manifest_dir.mkdir(parents=True)
    (manifest_dir / "latest.json").write_text(
        json.dumps({"manifest_id": "../" + "a" * 30}),
        encoding="utf-8",
    )
    with pytest.raises(ValueError, match="manifest_id must be 32-char lowercase hex"):
        load_latest_retrieval_manifest()


def test_latest_pointer_rejects_dot_in_manifest_id(tmp_path, monkeypatch) -> None:
    from core import paths

    manifest_dir = tmp_path / "retrieval_manifests"
    monkeypatch.setattr(paths, "RETRIEVAL_MANIFESTS_DIR", manifest_dir)
    monkeypatch.setattr(
        paths, "LATEST_RETRIEVAL_MANIFEST", manifest_dir / "latest.json",
    )
    manifest_dir.mkdir(parents=True)
    (manifest_dir / "latest.json").write_text(
        json.dumps({"manifest_id": "a" * 31 + "."}),
        encoding="utf-8",
    )
    with pytest.raises(ValueError, match="manifest_id must be 32-char lowercase hex"):
        load_latest_retrieval_manifest()


def test_load_latest_rejects_corrupt_immutable_manifest(tmp_path, monkeypatch) -> None:
    from core import paths

    manifest_dir = tmp_path / "retrieval_manifests"
    monkeypatch.setattr(paths, "RETRIEVAL_MANIFESTS_DIR", manifest_dir)
    monkeypatch.setattr(
        paths, "LATEST_RETRIEVAL_MANIFEST", manifest_dir / "latest.json",
    )
    manifest_id = "c" * 32
    manifest_dir.mkdir(parents=True)
    (manifest_dir / "latest.json").write_text(
        json.dumps({"manifest_id": manifest_id}),
        encoding="utf-8",
    )
    (manifest_dir / f"{manifest_id}.json").write_text('{"broken": true}', encoding="utf-8")
    with pytest.raises(ValueError):
        load_latest_retrieval_manifest()


# ------------------------------------------------------------------ v3 persistence boundary


def _tampered_alias_dict(manifest: RetrievalManifestV1, alias: str) -> dict:
    data = manifest_to_dict(manifest)
    data["candidates"] = list(data["candidates"])
    data["candidates"][0] = dict(data["candidates"][0])
    data["candidates"][0]["speaker_alias"] = alias
    return data


def test_from_dict_rejects_unapproved_persisted_alias() -> None:
    manifest = _minimal_manifest()
    data = _tampered_alias_dict(manifest, "山田太郎")
    with pytest.raises(ValueError, match="speaker_alias not approved"):
        manifest_from_dict(data)


def test_load_latest_rejects_manifest_with_real_name_alias(
    tmp_path, monkeypatch,
) -> None:
    from core import paths

    manifest_dir = tmp_path / "retrieval_manifests"
    monkeypatch.setattr(paths, "RETRIEVAL_MANIFESTS_DIR", manifest_dir)
    monkeypatch.setattr(
        paths, "LATEST_RETRIEVAL_MANIFEST", manifest_dir / "latest.json",
    )
    manifest = _minimal_manifest()
    save_retrieval_manifest(manifest)
    manifest_path = manifest_dir / f"{manifest.manifest_id}.json"
    before = manifest_path.read_text(encoding="utf-8")
    data = json.loads(before)
    data["candidates"][0]["speaker_alias"] = "山田太郎"
    manifest_path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    after = manifest_path.read_text(encoding="utf-8")
    assert "山田太郎" in after
    with pytest.raises(ValueError, match="speaker_alias not approved"):
        load_latest_retrieval_manifest()
    assert manifest_path.read_text(encoding="utf-8") == after


def test_latest_read_does_not_sanitize_invalid_alias(
    tmp_path, monkeypatch,
) -> None:
    from core import paths

    manifest_dir = tmp_path / "retrieval_manifests"
    monkeypatch.setattr(paths, "RETRIEVAL_MANIFESTS_DIR", manifest_dir)
    monkeypatch.setattr(
        paths, "LATEST_RETRIEVAL_MANIFEST", manifest_dir / "latest.json",
    )
    manifest = _minimal_manifest()
    save_retrieval_manifest(manifest)
    manifest_path = manifest_dir / f"{manifest.manifest_id}.json"
    data = json.loads(manifest_path.read_text(encoding="utf-8"))
    data["candidates"][0]["speaker_alias"] = "山田太郎"
    manifest_path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    with pytest.raises(ValueError):
        load_latest_retrieval_manifest()
    assert data["candidates"][0]["speaker_alias"] == "山田太郎"


def test_from_dict_accepts_verified_contact_alias() -> None:
    cand = _minimal_candidate(speaker_alias="C-01234567")
    lanes = (
        _minimal_lane(used_chars=1, formatting_chars=0, accepted_count=1),
        _minimal_lane(lane=ContextLane.RECENT_TRANSCRIPT, budget_chars=3600),
        _minimal_lane(lane=ContextLane.WORKING_MEMORY, budget_chars=4000),
        _minimal_lane(lane=ContextLane.RETRIEVED_EVIDENCE, budget_chars=2000),
    )
    manifest = build_retrieval_manifest(
        session_id="s",
        transcript_version=1,
        query_hash="a" * 32,
        context_hash="b" * 32,
        prompt_version="pv1",
        runtime_identity="ab" * 64,
        used_chars=1,
        formatting_overhead_chars=0,
        candidates=(cand,),
        lane_usage=lanes,
    )
    round_tripped = manifest_from_dict(manifest_to_dict(manifest))
    assert round_tripped.candidates[0].speaker_alias == "C-01234567"


def test_from_dict_accepts_fixed_internal_alias() -> None:
    cand = _minimal_candidate(speaker_alias="面接官")
    lanes = (
        _minimal_lane(used_chars=1, formatting_chars=0, accepted_count=1),
        _minimal_lane(lane=ContextLane.RECENT_TRANSCRIPT, budget_chars=3600),
        _minimal_lane(lane=ContextLane.WORKING_MEMORY, budget_chars=4000),
        _minimal_lane(lane=ContextLane.RETRIEVED_EVIDENCE, budget_chars=2000),
    )
    manifest = build_retrieval_manifest(
        session_id="s",
        transcript_version=1,
        query_hash="a" * 32,
        context_hash="b" * 32,
        prompt_version="pv1",
        runtime_identity="ab" * 64,
        used_chars=1,
        formatting_overhead_chars=0,
        candidates=(cand,),
        lane_usage=lanes,
    )
    round_tripped = manifest_from_dict(manifest_to_dict(manifest))
    assert round_tripped.candidates[0].speaker_alias == "面接官"


def test_from_dict_accepts_none_alias() -> None:
    manifest = _minimal_manifest()
    round_tripped = manifest_from_dict(manifest_to_dict(manifest))
    assert round_tripped.candidates[0].speaker_alias is None


@pytest.mark.parametrize("bad_input", [None, 123, {}, object()])
def test_validate_manifest_rejects_non_manifest_exact_type(bad_input) -> None:
    with pytest.raises(ValueError, match="manifest must be RetrievalManifestV1"):
        validate_manifest(bad_input)


def test_validate_manifest_recomputes_manifest_id() -> None:
    manifest = _minimal_manifest()
    object.__setattr__(manifest, "manifest_id", "f" * 32)
    with pytest.raises(ValueError, match="manifest_id mismatch"):
        validate_manifest(manifest)


def test_manifest_to_dict_rejects_non_manifest() -> None:
    with pytest.raises(ValueError, match="manifest must be RetrievalManifestV1"):
        manifest_to_dict(123)


def test_save_retrieval_manifest_rejects_non_manifest() -> None:
    with pytest.raises(ValueError, match="manifest must be RetrievalManifestV1"):
        save_retrieval_manifest(123)


def test_manifest_to_dict_rejects_tampered_manifest_id() -> None:
    manifest = _minimal_manifest()
    object.__setattr__(manifest, "manifest_id", "f" * 32)
    with pytest.raises(ValueError, match="manifest_id mismatch"):
        manifest_to_dict(manifest)


def test_save_retrieval_manifest_rejects_tampered_manifest_id(
    tmp_path, monkeypatch,
) -> None:
    from core import paths

    manifest_dir = tmp_path / "retrieval_manifests"
    monkeypatch.setattr(paths, "RETRIEVAL_MANIFESTS_DIR", manifest_dir)
    monkeypatch.setattr(
        paths, "LATEST_RETRIEVAL_MANIFEST", manifest_dir / "latest.json",
    )
    manifest = _minimal_manifest()
    object.__setattr__(manifest, "manifest_id", "f" * 32)
    with pytest.raises(ValueError, match="manifest_id mismatch"):
        save_retrieval_manifest(manifest)


# ------------------------------------------------------------------ v4 pointer-payload binding


def _manifest_pair() -> tuple[RetrievalManifestV1, RetrievalManifestV1]:
    cand = _minimal_candidate()
    lanes = (
        _minimal_lane(used_chars=1, formatting_chars=0, accepted_count=1),
        _minimal_lane(lane=ContextLane.RECENT_TRANSCRIPT, budget_chars=3600),
        _minimal_lane(lane=ContextLane.WORKING_MEMORY, budget_chars=4000),
        _minimal_lane(lane=ContextLane.RETRIEVED_EVIDENCE, budget_chars=2000),
    )
    manifest_a = build_retrieval_manifest(
        session_id="session-a",
        transcript_version=1,
        query_hash="a" * 32,
        context_hash="a" * 32,
        prompt_version="pv1",
        runtime_identity="ab" * 64,
        used_chars=1,
        formatting_overhead_chars=0,
        candidates=(cand,),
        lane_usage=lanes,
    )
    manifest_b = build_retrieval_manifest(
        session_id="session-b",
        transcript_version=2,
        query_hash="b" * 32,
        context_hash="c" * 32,
        prompt_version="pv1",
        runtime_identity="ab" * 64,
        used_chars=1,
        formatting_overhead_chars=0,
        candidates=(cand,),
        lane_usage=lanes,
    )
    assert manifest_a.manifest_id != manifest_b.manifest_id
    return manifest_a, manifest_b


def test_load_latest_rejects_pointer_payload_id_substitution(
    tmp_path, monkeypatch,
) -> None:
    from core import paths

    manifest_a, manifest_b = _manifest_pair()
    manifest_dir = tmp_path / "retrieval_manifests"
    monkeypatch.setattr(paths, "RETRIEVAL_MANIFESTS_DIR", manifest_dir)
    monkeypatch.setattr(
        paths, "LATEST_RETRIEVAL_MANIFEST", manifest_dir / "latest.json",
    )
    manifest_dir.mkdir(parents=True)
    latest_path = manifest_dir / "latest.json"
    manifest_path = manifest_dir / f"{manifest_a.manifest_id}.json"
    latest_path.write_text(
        json.dumps({"manifest_id": manifest_a.manifest_id}, ensure_ascii=False, indent=2)
        + "\n",
        encoding="utf-8",
    )
    manifest_path.write_text(
        json.dumps(manifest_to_dict(manifest_b), ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    latest_before = latest_path.read_bytes()
    manifest_before = manifest_path.read_bytes()
    with pytest.raises(ValueError, match="latest pointer manifest_id mismatch"):
        load_latest_retrieval_manifest()
    assert latest_path.read_bytes() == latest_before
    assert manifest_path.read_bytes() == manifest_before
    assert list(manifest_dir.glob("*.tmp")) == []


def test_save_rejects_existing_file_with_different_embedded_manifest_id(
    tmp_path, monkeypatch,
) -> None:
    from core import paths

    manifest_a, manifest_b = _manifest_pair()
    manifest_dir = tmp_path / "retrieval_manifests"
    monkeypatch.setattr(paths, "RETRIEVAL_MANIFESTS_DIR", manifest_dir)
    monkeypatch.setattr(
        paths, "LATEST_RETRIEVAL_MANIFEST", manifest_dir / "latest.json",
    )
    manifest_dir.mkdir(parents=True)
    latest_path = manifest_dir / "latest.json"
    manifest_path = manifest_dir / f"{manifest_a.manifest_id}.json"
    manifest_path.write_text(
        json.dumps(manifest_to_dict(manifest_b), ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    file_before = manifest_path.read_bytes()
    assert not latest_path.exists()
    with pytest.raises(
        RetrievalManifestPersistenceError,
        match="retrieval manifest persistence failed",
    ):
        save_retrieval_manifest(manifest_a)
    assert manifest_path.read_bytes() == file_before
    assert not latest_path.exists()
    assert list(manifest_dir.glob("*.tmp")) == []


# ------------------------------------------------------------------ 3.4 reason transition


def test_current_turn_repeated_in_tail_is_deduplicated_higher_lane() -> None:
    _, _, manifest = build_bounded_context_with_manifest(**_contract_kwargs())
    current = [
        c for c in manifest.candidates
        if c.lane == ContextLane.CURRENT
    ]
    assert len(current) == 1
    assert current[0].status in (
        CandidateStatus.ACCEPTED,
        CandidateStatus.ACCEPTED_TRUNCATED,
    )
    tail = [
        c for c in manifest.candidates
        if c.lane == ContextLane.RECENT_TRANSCRIPT
        and c.document_id == current[0].document_id
    ]
    assert len(tail) == 1
    assert tail[0].status == CandidateStatus.DEDUPLICATED
    assert tail[0].reason_code == ReasonCode.DEDUPLICATED_HIGHER_LANE
    assert tail[0].included_chars == 0
    assert tail[0].reason_code != ReasonCode.REJECTED_SUPERSEDED


# ------------------------------------------------------------------ 3.5 lane accounting


def test_each_lane_owns_its_actual_formatting_overhead() -> None:
    context, _, manifest = build_bounded_context_with_manifest(**_contract_kwargs())
    assert manifest.used_chars == char_count(context)
    for lane_usage in manifest.lane_usage:
        lane_cands = [
            c for c in manifest.candidates if c.lane == lane_usage.lane
        ]
        included = sum(c.included_chars for c in lane_cands)
        assert lane_usage.used_chars == included + lane_usage.formatting_chars
    non_empty = [lu for lu in manifest.lane_usage if lu.used_chars > 0]
    assert len(non_empty) >= 2
    assert non_empty[0].formatting_chars > 0
    if len(non_empty) > 1:
        assert non_empty[1].formatting_chars > 0
    assert not (
        non_empty
        and non_empty[0].formatting_chars == manifest.formatting_overhead_chars
        and all(lu.formatting_chars == 0 for lu in non_empty[1:])
    )


# ------------------------------------------------------------------ 3.6 true DI


def test_actual_consultation_engine_context_path_never_calls_backend(
    tmp_path, monkeypatch,
) -> None:
    from core import paths

    manifest_dir = tmp_path / "retrieval_manifests"
    monkeypatch.setattr(paths, "RETRIEVAL_MANIFESTS_DIR", manifest_dir)
    monkeypatch.setattr(
        paths, "LATEST_RETRIEVAL_MANIFEST", manifest_dir / "latest.json",
    )
    engine = ConsultationEngine()
    engine._backend = FailIfCalledBackend()
    state = {
        "transcript": list(_CONTRACT_TRANSCRIPT),
        "config": {},
        "canonical_runtime_identity": "ab" * 64,
    }
    context = engine._bounded_context(
        state,
        "turn-005-statement about problem 5 and data 15%",
        mode="interview_sim",
    )
    assert context
    loaded = load_latest_retrieval_manifest()
    assert loaded is not None
    assert loaded.context_hash == compute_context_hash(context)


# ------------------------------------------------------------------ retained contract tests


def test_manifest_deterministic() -> None:
    _, _, m1 = build_bounded_context_with_manifest(**_contract_kwargs())
    _, _, m2 = build_bounded_context_with_manifest(**_contract_kwargs())
    assert manifest_to_dict(m1) == manifest_to_dict(m2)
    assert m1.manifest_id == m2.manifest_id


def test_every_candidate_has_terminal_decision() -> None:
    _, _, manifest = build_bounded_context_with_manifest(**_contract_kwargs())
    for cand in manifest.candidates:
        assert cand.status in CandidateStatus
        assert cand.reason_code in ReasonCode
        assert cand.candidate_id
        assert cand.document_id
        assert cand.content_hash


def test_manifest_budget_accounting() -> None:
    context, _, manifest = build_bounded_context_with_manifest(**_contract_kwargs())
    assert manifest.total_budget_chars == 12_000
    assert manifest.used_chars == char_count(context)
    included_sum = sum(c.included_chars for c in manifest.candidates)
    assert included_sum + manifest.formatting_overhead_chars == manifest.used_chars
    lane_sum = sum(l.used_chars for l in manifest.lane_usage)
    assert lane_sum == manifest.used_chars


def test_status_reason_compatibility_is_strict() -> None:
    _, _, manifest = build_bounded_context_with_manifest(**_contract_kwargs())
    validate_manifest(manifest)
    bad = manifest_to_dict(manifest)
    bad["candidates"] = list(bad["candidates"])
    bad["candidates"][0] = dict(bad["candidates"][0])
    bad["candidates"][0]["status"] = CandidateStatus.ACCEPTED.value
    bad["candidates"][0]["reason_code"] = ReasonCode.REJECTED_BUDGET_LIMIT.value
    with pytest.raises(ValueError):
        manifest_from_dict(bad)


def test_context_hash_matches_context() -> None:
    context, _, manifest = build_bounded_context_with_manifest(**_contract_kwargs())
    from core.retrieval_manifest import compute_context_hash

    assert manifest.context_hash == compute_context_hash(context)


def test_manifest_contains_no_raw_text_or_real_names() -> None:
    _, _, manifest = build_bounded_context_with_manifest(**_contract_kwargs())
    blob = json.dumps(manifest_to_dict(manifest), ensure_ascii=False)
    for forbidden in ("canonical_text", "turn-005", "候補者", "quote"):
        assert forbidden not in blob


def test_contract_context_matches_pre_phase4a_golden() -> None:
    context, wm = build_bounded_context(**_contract_kwargs())
    from core.retrieval_manifest import compute_context_hash

    assert compute_context_hash(context) == _CONTRACT_CONTEXT_HASH
    assert wm.supporting_atom_ids == _CONTRACT_WM_IDS


def test_compute_manifest_id_excludes_self() -> None:
    _, _, manifest = build_bounded_context_with_manifest(**_contract_kwargs())
    mid = compute_manifest_id(manifest)
    assert mid == manifest.manifest_id
    assert len(mid) == 32


def test_save_manifest_is_idempotent(tmp_path, monkeypatch) -> None:
    from core import paths

    manifest_dir = tmp_path / "retrieval_manifests"
    monkeypatch.setattr(paths, "RETRIEVAL_MANIFESTS_DIR", manifest_dir)
    monkeypatch.setattr(
        paths, "LATEST_RETRIEVAL_MANIFEST", manifest_dir / "latest.json",
    )
    _, _, manifest = build_bounded_context_with_manifest(**_contract_kwargs())
    save_retrieval_manifest(manifest)
    first = (manifest_dir / f"{manifest.manifest_id}.json").read_text(encoding="utf-8")
    save_retrieval_manifest(manifest)
    second = (manifest_dir / f"{manifest.manifest_id}.json").read_text(encoding="utf-8")
    assert first == second
