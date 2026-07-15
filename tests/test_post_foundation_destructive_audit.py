"""Post-Foundation RED contracts for cross-boundary and mathematical flaws.

These tests intentionally describe security properties that the current
production implementation does not satisfy.  They are audit evidence only.
"""
from __future__ import annotations

import hashlib
import json
import math
import os
import re
import subprocess
import sys
import time
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core import dynamic_ordinal_rasch as dor  # noqa: E402
from core import lsm_index, retrieval_manifest, state_chain  # noqa: E402
from core.probe_funnel import PROBE_QUESTION_BANK  # noqa: E402
from core.retrieval_manifest import (  # noqa: E402
    CandidateStatus,
    ContextLane,
    LaneUsageV1,
    ReasonCode,
    RetrievalCandidateV1,
    RetrievalManifestPersistenceError,
    SourceType,
    build_retrieval_manifest,
    compute_content_hash,
    load_latest_retrieval_manifest,
    manifest_to_dict,
    save_retrieval_manifest,
)
from core.runtime_identity import (  # noqa: E402
    CanonicalRuntimeIdentity,
    hash_file_sha256,
    numeric_runtime_version,
)


_RUNTIME_COMPONENTS = {
    "gguf_hash": "11" * 32,
    "llama_server_hash": "22" * 32,
    "embedding_model_version": "embedder@1",
    "prompt_text": "system\nuser",
    "json_schema": {
        "type": "object",
        "properties": {"answer": {"type": "string"}},
        "required": ["answer"],
        "additionalProperties": False,
    },
    "generation_params": {"temperature": 0.0, "seed": 7},
    "numeric_runtime_version": "python-test|libm-test",
    "ordinal_rasch_artifact_hash": "33" * 32,
}
_RUNTIME_ID = "ab" * 64


def _runtime_identity(**overrides: object) -> CanonicalRuntimeIdentity:
    components = dict(_RUNTIME_COMPONENTS)
    components.update(overrides)
    return CanonicalRuntimeIdentity.from_components(**components)


def _patch_manifest_store(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> Path:
    from core import paths

    manifest_dir = tmp_path / "retrieval_manifests"
    latest = manifest_dir / "latest.json"
    monkeypatch.setattr(paths, "RETRIEVAL_MANIFESTS_DIR", manifest_dir)
    monkeypatch.setattr(paths, "LATEST_RETRIEVAL_MANIFEST", latest)
    retrieval_manifest._TRUSTED_STATE_HEADS.pop(str(latest.resolve()), None)
    return manifest_dir


def _manifest(
    *,
    session_id: str,
    session_genesis_id: str,
    sequence_number: int,
    parent_hash: str,
    marker: str,
):
    candidate = RetrievalCandidateV1(
        candidate_id=f"CURRENT:{marker}",
        document_id=marker,
        content_hash=compute_content_hash(marker),
        source_type=SourceType.CURRENT_QUERY,
        lane=ContextLane.CURRENT,
        char_count=len(marker),
        included_chars=len(marker),
        status=CandidateStatus.ACCEPTED,
        reason_code=ReasonCode.ACCEPTED_REQUIRED_CURRENT,
        selection_rank=None,
        source_index=None,
        speaker_alias=None,
        memory_kind=None,
    )
    lane_usage = tuple(
        LaneUsageV1(
            lane=lane,
            budget_chars=retrieval_manifest.LANE_FIXED_BUDGETS[lane],
            used_chars=len(marker) if lane is ContextLane.CURRENT else 0,
            formatting_chars=0,
            accepted_count=1 if lane is ContextLane.CURRENT else 0,
            rejected_count=0,
            deduplicated_count=0,
        )
        for lane in (
            ContextLane.CURRENT,
            ContextLane.RECENT_TRANSCRIPT,
            ContextLane.WORKING_MEMORY,
            ContextLane.RETRIEVED_EVIDENCE,
        )
    )
    return build_retrieval_manifest(
        parent_hash=parent_hash,
        sequence_number=sequence_number,
        session_genesis_id=session_genesis_id,
        session_id=session_id,
        transcript_version=sequence_number,
        query_hash=hashlib.blake2b(marker.encode(), digest_size=16).hexdigest(),
        context_hash=hashlib.blake2b(
            f"context:{marker}".encode(), digest_size=16
        ).hexdigest(),
        prompt_version="post-foundation-audit",
        runtime_identity=_RUNTIME_ID,
        used_chars=len(marker),
        formatting_overhead_chars=0,
        candidates=(candidate,),
        lane_usage=lane_usage,
    )


def test_runtime_identity_binds_exact_prompt_bytes() -> None:
    first_prompt = "system\nPreserve  two spaces.\n\nIndented:\n  value"
    second_prompt = "system\nPreserve two spaces.\nIndented:\n value"

    assert first_prompt.encode("utf-8") != second_prompt.encode("utf-8")
    first = _runtime_identity(prompt_text=first_prompt)
    second = _runtime_identity(prompt_text=second_prompt)

    assert first.digest != second.digest, (
        "operationally distinct prompt bytes collapsed to one runtime identity"
    )


def test_runtime_identity_does_not_rewrite_json_schema_regexes() -> None:
    exact_double_space = {
        "type": "string",
        "pattern": r"^a  b$",
    }
    exact_single_space = {
        "type": "string",
        "pattern": r"^a b$",
    }

    first = _runtime_identity(json_schema=exact_double_space)
    second = _runtime_identity(json_schema=exact_single_space)

    assert first.digest != second.digest, (
        "schemas with different accepted languages share one runtime identity"
    )


def test_runtime_artifact_hash_cache_revalidates_actual_bytes(tmp_path: Path) -> None:
    artifact = tmp_path / "model.gguf"
    artifact.write_bytes(b"A" * 64)
    initial_stat = artifact.stat()
    first = hash_file_sha256(artifact)

    artifact.write_bytes(b"B" * 64)
    os.utime(
        artifact,
        ns=(initial_stat.st_atime_ns, initial_stat.st_mtime_ns),
    )
    replaced_stat = artifact.stat()
    assert replaced_stat.st_size == initial_stat.st_size
    assert replaced_stat.st_mtime_ns == initial_stat.st_mtime_ns

    second = hash_file_sha256(artifact)
    assert first != second, "same-size, restored-mtime replacement reused a stale hash"


def test_live_probe_bank_is_covered_by_the_rasch_artifact() -> None:
    model = dor.DynamicOrdinalRaschFilter()
    artifact_ids = {item.item_id for item in model.items}
    production_ids = {question.id for question in PROBE_QUESTION_BANK}

    assert production_ids <= artifact_ids, (
        "the live PROBE bank cannot be observed or selected by the Rasch model; "
        f"missing={sorted(production_ids - artifact_ids)}"
    )


def test_artifact_eig_policy_is_not_an_all_item_tie() -> None:
    model = dor.DynamicOrdinalRaschFilter()
    posterior = model.initial_posterior()
    quantized = {
        item.item_id: math.floor(
            model.expected_information_gain(posterior, item_id=item.item_id)
            * 1_000_000
            + 0.5
        )
        for item in model.items
    }

    assert len(set(quantized.values())) > 1, (
        "all versioned items have the same response function, so EIG always "
        f"degenerates to the item-id tie-break: {quantized}"
    )


def test_declared_rasch_model_obeys_adjacent_category_logit_invariant() -> None:
    model = dor.DynamicOrdinalRaschFilter()
    low_theta = -1.0
    high_theta = 1.0
    low = model.response_probabilities(item_id="anchor-item", ability=low_theta)
    high = model.response_probabilities(item_id="anchor-item", ability=high_theta)

    observed_change = math.log(high[1] / high[0]) - math.log(low[1] / low[0])
    expected_rasch_change = high_theta - low_theta

    assert observed_change == pytest.approx(expected_rasch_change, abs=1e-12), (
        "adjacent-category Rasch requires log(P_r/P_{r-1}) to have unit slope; "
        f"observed slope change={observed_change}"
    )


def test_ephemeral_state_key_restart_can_bootstrap_a_new_chain(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from core.secure_identity import identity_root_key

    _patch_manifest_store(tmp_path, monkeypatch)
    identity_root_key.cache_clear()
    old_genesis = hashlib.sha512(b"old-process").hexdigest()
    old = _manifest(
        session_id="old-session",
        session_genesis_id=old_genesis,
        sequence_number=1,
        parent_hash=state_chain.genesis_parent_hash(old_genesis),
        marker="old",
    )
    save_retrieval_manifest(old)

    # A restarted process reloads the managed root key and loses its
    # process-local trusted-head acceleration cache.
    identity_root_key.cache_clear()
    retrieval_manifest._TRUSTED_STATE_HEADS.pop(
        retrieval_manifest._store_key(),
        None,
    )
    from core.facade import latest_context_manifest

    restarted_view = latest_context_manifest()
    assert restarted_view["manifest"]["manifest_id"] == old.manifest_id
    new_genesis = hashlib.sha512(b"new-process").hexdigest()
    new = _manifest(
        session_id="new-session",
        session_genesis_id=new_genesis,
        sequence_number=1,
        parent_hash=state_chain.genesis_parent_hash(new_genesis),
        marker="new",
    )

    try:
        try:
            save_retrieval_manifest(new)
        except RetrievalManifestPersistenceError as exc:
            pytest.fail(
                f"valid process restart is permanently poisoned by stale state: {exc}"
            )

        loaded = load_latest_retrieval_manifest(
            expected_session_head=new_genesis,
            expected_sequence_number=1,
        )
        assert loaded is not None and loaded.manifest_id == new.manifest_id
    finally:
        identity_root_key.cache_clear()


def test_manifest_atomic_writer_never_follows_a_precreated_hardlink(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    manifest_dir = _patch_manifest_store(tmp_path, monkeypatch)
    manifest_dir.mkdir(parents=True)
    genesis = hashlib.sha512(b"hardlink-session").hexdigest()
    manifest = _manifest(
        session_id="hardlink-session",
        session_genesis_id=genesis,
        sequence_number=1,
        parent_hash=state_chain.genesis_parent_hash(genesis),
        marker="hardlink",
    )
    victim = tmp_path / "unrelated-evidence.bin"
    sentinel = b"DO-NOT-OVERWRITE"
    victim.write_bytes(sentinel)
    target = manifest_dir / f"{manifest.manifest_id}.json"
    predictable_temp = target.with_suffix(".json.tmp")
    os.link(victim, predictable_temp)

    try:
        save_retrieval_manifest(manifest)
    except RetrievalManifestPersistenceError:
        pass

    assert victim.read_bytes() == sentinel, (
        "predictable manifest temp followed a hardlink and overwrote unrelated bytes"
    )


def test_concurrent_state_children_cannot_both_commit(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from core import paths
    from core.secure_identity import identity_root_key

    project_root = tmp_path / "shared-project"
    manifest_dir = project_root / "data" / "processed" / "retrieval_manifests"
    latest = manifest_dir / "latest.json"
    monkeypatch.setattr(paths, "RETRIEVAL_MANIFESTS_DIR", manifest_dir)
    monkeypatch.setattr(paths, "LATEST_RETRIEVAL_MANIFEST", latest)
    retrieval_manifest._TRUSTED_STATE_HEADS.pop(str(latest.resolve()), None)
    identity_root_key.cache_clear()
    genesis = hashlib.sha512(b"concurrent-session").hexdigest()
    root = _manifest(
        session_id="concurrent-session",
        session_genesis_id=genesis,
        sequence_number=1,
        parent_hash=state_chain.genesis_parent_hash(genesis),
        marker="root",
    )
    save_retrieval_manifest(root)
    children = tuple(
        _manifest(
            session_id="concurrent-session",
            session_genesis_id=genesis,
            sequence_number=2,
            parent_hash=root.manifest_id,
            marker=marker,
        )
        for marker in ("child-a", "child-b")
    )

    payload_paths: list[Path] = []
    for index, child in enumerate(children):
        payload_path = tmp_path / f"child-{index}.json"
        payload_path.write_text(
            json.dumps(manifest_to_dict(child), ensure_ascii=False),
            encoding="utf-8",
        )
        payload_paths.append(payload_path)

    start_signal = tmp_path / "start.signal"
    ready_signals = [tmp_path / f"ready-{index}.signal" for index in range(2)]
    child_program = r"""
import json
import sys
import time
from pathlib import Path

from core import retrieval_manifest
from core.retrieval_manifest import (
    RetrievalManifestPersistenceError,
    manifest_from_dict,
    save_retrieval_manifest,
)

manifest = manifest_from_dict(
    json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
)
start_signal = Path(sys.argv[2])
ready_signal = Path(sys.argv[3])

original_write = retrieval_manifest._atomic_write_text
delayed = [False]

def delayed_manifest_write(path, text):
    if Path(path).name != "latest.json" and not delayed[0]:
        delayed[0] = True
        time.sleep(0.5)
    return original_write(path, text)

retrieval_manifest._atomic_write_text = delayed_manifest_write
ready_signal.write_text("ready\n", encoding="utf-8")
deadline = time.monotonic() + 10.0
while not start_signal.exists():
    if time.monotonic() >= deadline:
        raise TimeoutError("start signal was not published")
    time.sleep(0.005)
try:
    save_retrieval_manifest(manifest)
except RetrievalManifestPersistenceError:
    print("rejected")
else:
    print("saved")
"""
    child_env = os.environ.copy()
    child_env["PKB_PROJECT_ROOT"] = str(project_root)
    python_path = str(ROOT / "src" / "python")
    if child_env.get("PYTHONPATH"):
        python_path += os.pathsep + child_env["PYTHONPATH"]
    child_env["PYTHONPATH"] = python_path

    processes: list[subprocess.Popen[str]] = []
    outcomes: list[str] = []
    try:
        for payload, ready_signal in zip(payload_paths, ready_signals, strict=True):
            processes.append(
                subprocess.Popen(
                    [
                        sys.executable,
                        "-c",
                        child_program,
                        str(payload),
                        str(start_signal),
                        str(ready_signal),
                    ],
                    cwd=ROOT,
                    env=child_env,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    text=True,
                )
            )
        ready_deadline = time.monotonic() + 10.0
        while not all(signal.exists() for signal in ready_signals):
            if any(process.poll() is not None for process in processes):
                pytest.fail("concurrent child exited before publishing readiness")
            if time.monotonic() >= ready_deadline:
                pytest.fail("concurrent children did not become ready")
            time.sleep(0.005)
        start_signal.write_text("go\n", encoding="utf-8")
        for process in processes:
            try:
                stdout, stderr = process.communicate(timeout=15)
            except subprocess.TimeoutExpired:
                process.kill()
                stdout, stderr = process.communicate()
                pytest.fail(
                    f"concurrent child timed out: stdout={stdout!r} stderr={stderr!r}"
                )
            assert process.returncode == 0, stderr
            outcomes.append(stdout.strip())
    finally:
        for process in processes:
            if process.poll() is None:
                process.kill()
                process.communicate()
        identity_root_key.cache_clear()

    assert sorted(outcomes) == ["rejected", "saved"], (
        "two children of the same authenticated head both committed, forking "
        f"the state chain: {outcomes}"
    )


class _CaptureSearchEngine:
    name = "post-foundation-capture"

    def __init__(self) -> None:
        self.embedder = self
        self.payloads: list[bytes] = []

    def search_index(self, bin_path, meta_path, qvec, top_k=3):
        del meta_path, qvec, top_k
        self.payloads.append(Path(bin_path).read_bytes())
        return []


def test_lsm_preflight_binds_the_verified_inode_to_search(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    processed = tmp_path / "processed"
    processed.mkdir()
    segment = processed / "vectors.seg-000001.bin"
    original_payload = b"verified-segment"
    forged_payload = b"forged-after-preflight"
    segment.write_bytes(original_payload)
    replacement = processed / "replacement.bin"
    replacement.write_bytes(forged_payload)
    engine = _CaptureSearchEngine()
    manifest_path = processed / "segments.json"
    metadata_path = processed / "metadata.json"
    manifest_path.write_text(
        json.dumps(
            {
                "format": lsm_index.MANIFEST_FORMAT,
                "embedder_id": lsm_index.embedder_id(engine.embedder),
                "next_chunk_id": 1,
                "segments": [
                    {
                        "file": segment.name,
                        "live": 1,
                        "dead": 0,
                        "payload_hash": lsm_index.segment_payload_hash(segment),
                    }
                ],
                "days": {},
            }
        ),
        encoding="utf-8",
    )
    metadata_path.write_text("{}", encoding="utf-8")
    monkeypatch.setattr(lsm_index, "PROCESSED", processed)
    monkeypatch.setattr(lsm_index, "LSM_MANIFEST", manifest_path)
    monkeypatch.setattr(lsm_index, "DIARY_META", metadata_path)

    real_hash = lsm_index.segment_payload_hash
    hash_calls = 0

    def hash_then_swap(path: Path) -> str:
        nonlocal hash_calls
        digest = real_hash(path)
        hash_calls += 1
        if hash_calls == 2:
            os.replace(replacement, path)
        return digest

    monkeypatch.setattr(lsm_index, "segment_payload_hash", hash_then_swap)

    try:
        lsm_index.search_lsm(engine, qvec=None)
    except lsm_index.DerivedStoreIntegrityError:
        pass

    assert forged_payload not in engine.payloads, (
        "search reopened the segment path after preflight and consumed swapped bytes"
    )


def test_rust_stdout_queue_is_bounded_at_the_producer() -> None:
    source = (ROOT / "apps" / "desktop" / "src-tauri" / "src" / "engine.rs").read_text(
        encoding="utf-8"
    )
    start = source.index("fn spawn_stdout_worker")
    end = source.index("\nimpl ProcessConnection", start)
    worker = source[start:end]

    assert "mpsc::channel::<Result<String, InvokeError>>()" not in worker, (
        "the stdout producer can enqueue unlimited short lines before the "
        "consumer-side message cap runs"
    )
    assert "IPC_MAX_MESSAGES_PER_REQUEST" in worker


def test_ipc_schema_text_limit_fits_inside_transport_request_frame() -> None:
    contract_source = (
        ROOT / "apps" / "desktop" / "src-tauri" / "src" / "ipc_contract.rs"
    ).read_text(encoding="utf-8")
    engine_source = (
        ROOT / "apps" / "desktop" / "src-tauri" / "src" / "engine.rs"
    ).read_text(encoding="utf-8")
    domain_match = re.search(
        r"const MAX_TEXT_BYTES: usize = (\d+) \* 1024 \* 1024;",
        contract_source,
    )
    transport_match = re.search(
        r"IPC_MAX_REQUEST_LINE_BYTES: usize = (\d+) \* 1024 \* 1024;",
        engine_source,
    )
    assert domain_match is not None and transport_match is not None
    domain_mebibytes = int(domain_match.group(1))
    transport_mebibytes = int(transport_match.group(1))

    assert domain_mebibytes < transport_mebibytes, (
        "schema-valid text can never cross the smaller IPC frame once JSON "
        f"overhead is added: domain={domain_mebibytes}MiB, "
        f"transport={transport_mebibytes}MiB"
    )


def test_bit_level_numeric_contract_pins_libm_and_rounding_mode() -> None:
    artifact = json.loads(dor._ARTIFACT_PATH.read_text(encoding="utf-8"))
    numeric_contract = artifact["numeric_contract"]
    runtime = numeric_runtime_version()

    assert "libm" in numeric_contract and "rounding_mode" in numeric_contract
    assert "libm-" in runtime and "rounding-" in runtime
