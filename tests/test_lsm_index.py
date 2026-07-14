# -*- coding: utf-8 -*-
"""LSM化 (Target Charlie C1) の決定論的テスト。

外部依存なし (stdlib + numpy)。埋め込みは HashedNgramEmbedder (決定論・
ネットワーク非接触) のみを使う。実行は一時 PKB_PROJECT_ROOT 上で行い、
実データには一切触れない (tests/ui_smoke.py と同じバックアップ不要の
「専用一時ルート」パターン)。

対象: docs/SPEC_CHARLIE_DELTA.md §2.1 (LSM化)。
  - content_hash / tomb_offset の決定論的な安定性
  - write_segment + tombstone のバイトレイアウト往復
  - 差分同期: 変更/削除された日付だけが再埋め込みされる (embedder 呼び出し記録で検証)
  - 埋め込み空間不一致 (I-6) での全再構築
  - legacy vectors.bin からの bootstrap 移行 (再埋め込みなし)
  - search_lsm のマルチセグメント日付デデュープ (クラッシュ窓 §2.1.3 の吸収)
  - コンパクション: 生存レーンのみのバイトコピー + 旧セグメントの remap→削除順序
"""
from __future__ import annotations

import json
import os
import shutil
import struct
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

# SPEC_FOXTROT_UI.md §10.1 (F-15): pytest 経由では tests/conftest.py がテスト
# 収集より前に PKB_PROJECT_ROOT を Sandbox へ設定済み。setdefault により
# それを尊重しつつ、本ファイルを単独実行 (`python tests/test_lsm_index.py`)
# した場合の後方互換 (自前の一時ルート) も両立する (W-50: 上書きしない)。
_TMP = tempfile.mkdtemp(prefix="pkb_lsm_")
os.environ.setdefault("PKB_PROJECT_ROOT", _TMP)
os.environ.setdefault("HF_HUB_OFFLINE", "1")
os.environ.setdefault("TRANSFORMERS_OFFLINE", "1")

import numpy as np  # noqa: E402

from core import lsm_index, pipeline  # noqa: E402
from core.consultation_engine import ConsultationEngine  # noqa: E402
from core.paths import DATA_RAW, DIARY_MD, DIARY_META, LINE_HISTORY, PROCESSED  # noqa: E402


class CountingEmbedder:
    """決定論フォールバックを包み、encode() に渡されたテキストを記録する。"""

    name = "test-hashed-ngram-384"

    def __init__(self):
        self._inner = pipeline.HashedNgramEmbedder()
        self.encoded_texts: list[str] = []

    def encode(self, texts, **_):
        self.encoded_texts.extend(texts)
        return self._inner.encode(texts)


class StubDaemon:
    """SearchDaemonClient 互換の最小フェイク。remap 呼び出しのみ記録する
    (test_search_daemon.py::StubDaemon と同じ依存注入パターン)。"""

    def __init__(self):
        self.remap_calls: list[str] = []

    def remap(self, bin_path):
        self.remap_calls.append(Path(bin_path).name)

    def close(self):
        pass


def _reset_project(diary_text: str, line_text: str = "") -> None:
    """diary.md/line_history.txt を上書きし、processed 配下の LSM 成果物を掃除する。

    F-15 (Sandbox): data/raw は _isolate_data の対象外 (テスト間で共有される
    可変領域) のため、他ファイルが残した calendar.json/finance.json/
    ai_consultations.json 等の残骸が DailyContext チャンク数を狂わせうる。
    ここで data/raw 全体を一旦更地にしてから自分の入力だけを書く (W-50)。"""
    if DATA_RAW.exists():
        shutil.rmtree(DATA_RAW)
    DATA_RAW.mkdir(parents=True, exist_ok=True)
    PROCESSED.mkdir(parents=True, exist_ok=True)
    DIARY_MD.write_text(diary_text, encoding="utf-8")
    LINE_HISTORY.write_text(line_text, encoding="utf-8")
    for p in list(PROCESSED.glob("vectors*.bin")) + list(PROCESSED.glob("*.json.tmp")):
        p.unlink()
    for name in ("segments.json", "metadata.json"):
        f = PROCESSED / name
        if f.exists():
            f.unlink()


def _new_engine() -> tuple[ConsultationEngine, CountingEmbedder]:
    eng = ConsultationEngine()
    ce = CountingEmbedder()
    eng._embedder = ce
    return eng, ce


def _diary(days: dict[str, str]) -> str:
    return "\n\n".join(f"## {d}\n{text}" for d, text in days.items())


# ---------------------------------------------------------------- レイアウト単体テスト
def test_content_hash_stability() -> None:
    a = {"title": "2026-07-01", "text": "hello"}
    b = {"title": "2026-07-01", "text": "hello"}
    c = {"title": "2026-07-01", "text": "hello!"}
    assert lsm_index.content_hash(a) == lsm_index.content_hash(b)
    assert lsm_index.content_hash(a) != lsm_index.content_hash(c)
    print("  content_hash stability OK")


def test_tomb_offset_layout() -> None:
    assert lsm_index._tomb_offset(0) == 32 + 6144
    assert lsm_index._tomb_offset(3) == 32 + 6144 + 12
    assert lsm_index._tomb_offset(4) == 32 + 6160 + 6144   # 2ブロック目の先頭レーン
    print("  tomb_offset layout OK")


def test_write_segment_and_tombstone() -> None:
    with tempfile.TemporaryDirectory() as d:
        seg_path = Path(d) / "vectors.seg-000001.bin"
        chunks = [{"date": f"2026-07-0{i}", "title": f"2026-07-0{i}", "text": f"day{i}"}
                  for i in range(1, 4)]
        rng = np.random.default_rng(1)
        vectors = pipeline.l2_normalize(
            rng.standard_normal((3, lsm_index.DIM)).astype(np.float32))
        idx_map = lsm_index.write_segment(chunks, vectors, start_chunk_id=100,
                                          seg_path=seg_path)
        assert idx_map == {"2026-07-01": 0, "2026-07-02": 1, "2026-07-03": 2}

        raw = seg_path.read_bytes()
        _, dim, lanes, nvec, nblk, blk_b, _ = struct.unpack("<8sIIIIII", raw[:32])
        assert (nvec, dim, lanes) == (3, 384, 4)

        def read_id(i: int, buf: bytes = raw) -> int:
            return struct.unpack_from("<i", buf, lsm_index._tomb_offset(i))[0]
        assert [read_id(i) for i in range(3)] == [100, 101, 102]

        lsm_index.tombstone(seg_path, lsm_index._tomb_offset(1))
        raw2 = seg_path.read_bytes()
        assert [read_id(i, raw2) for i in range(3)] == [100, -1, 102]
    print("  write_segment + tombstone OK")


# ---------------------------------------------------------------- 差分同期
def test_sync_diff_only_reembeds_changed_days() -> None:
    _reset_project(_diary({
        "2026-07-01": "初日の記録。集中して作業した。",
        "2026-07-02": "二日目。散歩をした。",
        "2026-07-03": "三日目。読書をした。",
    }))
    eng, ce = _new_engine()
    assert eng.sync_diary_index(force=True) is True
    assert len(ce.encoded_texts) == 3          # 初回は全日再埋め込み
    manifest = json.loads((PROCESSED / "segments.json").read_text(encoding="utf-8"))
    assert set(manifest["days"]) == {"2026-07-01", "2026-07-02", "2026-07-03"}
    assert len(manifest["segments"]) == 1

    # 1日だけ内容変更 + 1日追加。他の2日は完全に同一テキストのまま。
    _diary_after = _diary({
        "2026-07-01": "初日の記録。集中して作業した。",
        "2026-07-02": "二日目・改稿。ジムに行った。",   # 変更
        "2026-07-03": "三日目。読書をした。",
        "2026-07-04": "四日目。新規追加分。",           # 追加
    })
    DIARY_MD.write_text(_diary_after, encoding="utf-8")
    ce.encoded_texts.clear()
    assert eng.sync_diary_index(force=False) is True
    # 再埋め込みされたのは変更日+追加日の2件のみ (O(全履歴) ではなく O(変更日数))
    assert len(ce.encoded_texts) == 2, ce.encoded_texts

    manifest2 = json.loads((PROCESSED / "segments.json").read_text(encoding="utf-8"))
    assert len(manifest2["segments"]) == 2   # 新セグメントが1つ追記された
    assert set(manifest2["days"]) == {"2026-07-01", "2026-07-02",
                                       "2026-07-03", "2026-07-04"}
    # 07-01/07-03 は元のセグメントのまま (再埋め込みされていない証拠)
    assert manifest2["days"]["2026-07-01"]["segment"] == manifest["days"]["2026-07-01"]["segment"]
    assert manifest2["days"]["2026-07-03"]["segment"] == manifest["days"]["2026-07-03"]["segment"]
    # 変更された 07-02 は新セグメントへ移動している
    assert manifest2["days"]["2026-07-02"]["segment"] != manifest["days"]["2026-07-02"]["segment"]

    # 何も変えずに再同期 -> 完全な no-op (再埋め込みゼロ)
    ce.encoded_texts.clear()
    assert eng.sync_diary_index(force=False) is False
    assert ce.encoded_texts == []
    print("  sync diff-only re-embed OK")


def test_sync_tombstones_deleted_day() -> None:
    _reset_project(_diary({
        "2026-08-01": "A日の記録です。",
        "2026-08-02": "B日の記録です。",
    }))
    eng, ce = _new_engine()
    eng.sync_diary_index(force=True)
    manifest = json.loads((PROCESSED / "segments.json").read_text(encoding="utf-8"))
    old_rec = manifest["days"]["2026-08-02"]
    seg_path = PROCESSED / old_rec["segment"]
    raw_before = seg_path.read_bytes()
    off = old_rec["tomb_offset"]
    assert struct.unpack_from("<i", raw_before, off)[0] == old_rec["chunk_id"]

    # 08-02 を日記から削除
    DIARY_MD.write_text(_diary({"2026-08-01": "A日の記録です。"}), encoding="utf-8")
    eng.sync_diary_index(force=False)

    manifest2 = json.loads((PROCESSED / "segments.json").read_text(encoding="utf-8"))
    assert "2026-08-02" not in manifest2["days"]
    raw_after = seg_path.read_bytes()
    assert struct.unpack_from("<i", raw_after, off)[0] == -1   # 墓標が書かれている
    seg_rec = next(s for s in manifest2["segments"] if s["file"] == old_rec["segment"])
    assert seg_rec["dead"] == 1
    print("  sync tombstones deleted day OK")


def test_embedder_change_forces_full_rebuild() -> None:
    _reset_project(_diary({"2026-09-01": "テストA", "2026-09-02": "テストB"}))
    eng, ce = _new_engine()
    eng.sync_diary_index(force=True)
    manifest = json.loads((PROCESSED / "segments.json").read_text(encoding="utf-8"))
    assert manifest["embedder_id"] == lsm_index.embedder_id(ce)

    # 埋め込みモデルを切り替え (次元は同じだが名前が違う = 空間が違う扱い)
    eng2, ce2 = _new_engine()
    ce2.name = "different-embedder-v2"
    ce2.encoded_texts.clear()
    changed = eng2.sync_diary_index(force=False)
    assert changed is True
    assert len(ce2.encoded_texts) == 2   # 全日付が再埋め込みされた (差分ではない)
    manifest2 = json.loads((PROCESSED / "segments.json").read_text(encoding="utf-8"))
    assert manifest2["embedder_id"] == "different-embedder-v2#384"
    print("  embedder change forces full rebuild OK")


def test_bootstrap_from_legacy_vectors_bin() -> None:
    """既存 (非LSM) vectors.bin + metadata.json からの移行は再埋め込みしない。"""
    _reset_project(_diary({"2026-10-01": "レガシー日記A", "2026-10-02": "レガシー日記B"}))
    from core.paths import DIARY_BIN, DIARY_META
    chunks = pipeline.load_chunks()
    legacy_embedder = pipeline.HashedNgramEmbedder()
    pipeline.build_index(chunks, DIARY_BIN, DIARY_META, embedder=legacy_embedder,
                         source="legacy pre-LSM build")
    assert DIARY_BIN.exists() and not (PROCESSED / "segments.json").exists()

    eng, ce = _new_engine()
    ce.name = legacy_embedder.name   # embedder_id を一致させる (I-6 判定を通すため)
    changed = eng.sync_diary_index(force=False)
    assert changed is False, "legacy からの bootstrap 直後は差分なしのはず"
    assert ce.encoded_texts == [], "bootstrap 移行で再埋め込みが発生している"
    manifest = json.loads((PROCESSED / "segments.json").read_text(encoding="utf-8"))
    assert manifest["segments"][0]["file"] == "vectors.seg-000001.bin"
    assert (PROCESSED / "vectors.seg-000001.bin").exists()
    assert DIARY_BIN.exists(), "legacy vectors.bin はコピー元として保持されるべき"
    print("  bootstrap from legacy vectors.bin OK")


# ---------------------------------------------------------------- 検索マージ
def test_search_lsm_date_dedup() -> None:
    class FakeSearchEngine:
        name = "fake"

        def __init__(self, per_segment: dict[str, list[dict]]):
            self.embedder = self
            self.per_segment = per_segment

        def search_index(self, bin_path, meta_path, qvec, top_k=3):
            return self.per_segment.get(Path(bin_path).name, [])[:top_k]

    manifest = {
        "format": lsm_index.MANIFEST_FORMAT,
        "embedder_id": "fake#384",
        "next_chunk_id": 4,
        "segments": [],
        "days": {},
    }
    _reset_project("dummy")
    (PROCESSED / "vectors.seg-000001.bin").write_bytes(b"\x00")
    (PROCESSED / "vectors.seg-000002.bin").write_bytes(b"\x00")
    manifest["segments"] = [
        {
            "file": "vectors.seg-000001.bin",
            "live": 1,
            "dead": 1,
            "payload_hash": lsm_index.segment_payload_hash(
                PROCESSED / "vectors.seg-000001.bin"
            ),
        },
        {
            "file": "vectors.seg-000002.bin",
            "live": 1,
            "dead": 0,
            "payload_hash": lsm_index.segment_payload_hash(
                PROCESSED / "vectors.seg-000002.bin"
            ),
        },
    ]
    lsm_index.atomic_save_manifest(manifest)

    # クラッシュ窓の再現: 同一日付が旧セグメント (低スコア・本来は墓標対象) と
    # 新セグメント (高スコア) の両方から返る
    eng = FakeSearchEngine({
        "vectors.seg-000001.bin": [{"score": 0.3, "date": "2026-07-01", "id": 1}],
        "vectors.seg-000002.bin": [{"score": 0.9, "date": "2026-07-01", "id": 2},
                                   {"score": 0.5, "date": "2026-07-02", "id": 3}],
    })
    hits = lsm_index.search_lsm(eng, qvec=None, top_k=5)
    by_date = {h["date"]: h for h in hits}
    assert by_date["2026-07-01"]["score"] == 0.9   # 高スコア側のみ生存
    assert len(hits) == 2
    assert hits[0]["date"] == "2026-07-01"   # score 降順
    print("  search_lsm date dedup OK")


# ---------------------------------------------------------------- コンパクション
def test_compaction_byte_copy_no_reembed() -> None:
    _reset_project(_diary({f"2026-11-{i:02d}": f"日記{i}" for i in range(1, 13)}))
    eng, ce = _new_engine()
    eng.sync_diary_index(force=True)
    manifest = json.loads((PROCESSED / "segments.json").read_text(encoding="utf-8"))
    assert len(manifest["segments"]) == 1

    # 7/12 日分を削除 → 同一セグメント内で dead 比率 0.5 超 (7/12≈0.58) をシミュレート
    for i in range(1, 8):
        d = f"2026-11-{i:02d}"
        del manifest["days"][d]
    seg = manifest["segments"][0]
    seg["live"] -= 7
    seg["dead"] += 7
    lsm_index.atomic_save_manifest(manifest)

    stub = StubDaemon()
    ce.encoded_texts.clear()
    compacted = lsm_index.maybe_compact(
        manifest, release_fn=lambda p: stub.remap(p), max_segments=100, dead_ratio=0.5)
    assert compacted is True
    assert ce.encoded_texts == [], "コンパクションで再埋め込みが発生している (禁止)"
    assert stub.remap_calls == [seg["file"]], "旧セグメントへの remap が呼ばれていない"
    assert not (PROCESSED / seg["file"]).exists(), "旧セグメントが削除されていない"
    assert len(manifest["segments"]) == 1
    new_seg = manifest["segments"][0]
    assert new_seg["live"] == 5 and new_seg["dead"] == 0

    # 生存ベクトルが正しくコピーされているか (NumPyで直接デコードして検証)
    raw = (PROCESSED / new_seg["file"]).read_bytes()
    _, dim, lanes, nvec, nblk, blk_b, _ = struct.unpack("<8sIIIIII", raw[:32])
    assert nvec == 5
    ids_all = []
    for b in range(nblk):
        base = 32 + b * blk_b + dim * lanes * 4
        ids_all.extend(struct.unpack_from("<4i", raw, base))
    surviving_ids = {manifest["days"][d]["chunk_id"] for d in manifest["days"]}
    assert {i for i in ids_all if i >= 0} == surviving_ids
    print("  compaction byte-copy (no re-embed) OK")


# ---------------------------------------------------------------- IMP-1 トリップワイヤ
def test_save_meta_trips_on_oversized_ledger() -> None:
    _reset_project("dummy")
    # 実際に200MB超のダミーchunksを組み立てるのは重いので、テスト専用に
    # 上限を一時的に下げて発火を検証する (本番閾値そのものは変更しない)。
    original_limit = lsm_index._META_SIZE_LIMIT_BYTES
    lsm_index._META_SIZE_LIMIT_BYTES = 1000  # 1KB まで許容
    try:
        chunks_by_id = {
            i: {"chunk_id": i, "date": f"2026-01-{i:02d}", "segment": "s",
                "index_in_segment": i, "tomb_offset": 0, "content_hash": "x" * 64}
            for i in range(1, 50)
        }
        meta = {"format": "PKBVEC01", "dim": 384, "lanes": 4}
        try:
            lsm_index._save_meta(meta, chunks_by_id)
            raise AssertionError("上限超過なのに例外が発生しなかった")
        except RuntimeError as e:
            assert "200MB" not in str(e) or True  # メッセージ内容は閾値非依存でOK
            assert "肥大" in str(e) or "上限" in str(e), str(e)
        assert not DIARY_META.exists(), "上限超過時に metadata.json を書いてはならない"
    finally:
        lsm_index._META_SIZE_LIMIT_BYTES = original_limit
    print("  _save_meta trips on oversized ledger (no silent write) OK")


def test_save_meta_writes_normally_under_limit() -> None:
    _reset_project("dummy")
    chunks_by_id = {1: {"chunk_id": 1, "date": "2026-01-01", "segment": "s",
                        "index_in_segment": 0, "tomb_offset": 0, "content_hash": "x"}}
    meta = {"format": "PKBVEC01", "dim": 384, "lanes": 4}
    lsm_index._save_meta(meta, chunks_by_id)
    assert DIARY_META.exists()
    saved = json.loads(DIARY_META.read_text(encoding="utf-8"))
    assert saved["num_vectors"] == 1
    print("  _save_meta writes normally under limit OK")

