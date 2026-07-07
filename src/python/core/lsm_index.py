#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
core/lsm_index.py — PKBVEC01 の LSM 化 (Target Charlie / C1)
==================================================================
DailyContext インデックスの再構築を O(全履歴) から O(変更日数) へ縮める。

設計の核心 (docs/SPEC_CHARLIE_DELTA.md §2.1.0):
  - セグメントファイル (vectors.seg-NNNNNN.bin) の中身は PKBVEC01 形式のまま
    1 バイトも変えない。C++ 側 (search_engine.cpp) は完全無変更で流用できる。
    バージョニングは本モジュールのマニフェスト (segments.json) 側でのみ行う。
  - コンパクション = 生存レーンのバイトコピー + マニフェスト更新。埋め込み
    モデルは一切ロードしない (ベクトルは不変データ)。
  - chunk_id はセグメントをまたぐグローバル ID (manifest["next_chunk_id"] で採番)。
    metadata.json は全セグメント共通の「生存チャンク台帳」として共有する。

全処理は stdlib + numpy (pipeline.py と同じ扱い。numpy はベクトル演算のみに
使い、決定論的判定ロジック自体は分岐なしの純粋なデータ変換)。
"""

from __future__ import annotations

import hashlib
import json
import os
import struct
from pathlib import Path

import numpy as np

from . import pipeline
from .paths import DIARY_BIN, DIARY_META, LSM_MANIFEST, PROCESSED

DIM = pipeline.DIM
LANES = pipeline.LANES
BLOCK_BYTES = pipeline.BLOCK_BYTES
HEADER_BYTES = 32
_TOMB = struct.pack("<i", -1)

# セグメント内オフセット定数 (FileHeader 32B + data[384][4] の後に chunk_ids[4])
_IDS_OFFSET_IN_BLOCK = DIM * LANES * 4   # 6144


# ---------------------------------------------------------------- embedder_id
def embedder_id(embedder) -> str:
    """埋め込み空間の同一性判定キー (SPEC 不変条件 I-6)。モデル名 + 次元。"""
    name = getattr(embedder, "name", type(embedder).__name__)
    return f"{name}#{DIM}"


# ---------------------------------------------------------------- content_hash
def content_hash(chunk: dict) -> str:
    """その日の embed 対象テキストそのもの (title+text) をハッシュする。

    build_index / write_segment はどちらも f"{title}\\n{text}" を埋め込み入力に
    使う。この文字列が変わらない限り埋め込みベクトルは変わらないので、
    個々のソース (日記/LINE/家計簿/予定/相談) を列挙して結合する方式より
    頑健 — ソース追加を content_hash 側で追従し忘れる事故 (罠 T-3) が
    構造的に起きない。
    """
    text = f"{chunk.get('title', '')}\n{chunk.get('text', '')}"
    return "blake2b:" + hashlib.blake2b(text.encode("utf-8"), digest_size=16).hexdigest()


# ---------------------------------------------------------------- マニフェスト I/O
def _empty_manifest(eid: str) -> dict:
    return {"format": "pkbseg.v1", "embedder_id": eid, "next_chunk_id": 0,
             "segments": [], "days": {}}


def _tomb_offset(index_in_segment: int) -> int:
    """セグメント内 index-th チャンクの chunk_id フィールドの絶対バイトオフセット。"""
    block, lane = divmod(index_in_segment, LANES)
    return HEADER_BYTES + block * BLOCK_BYTES + _IDS_OFFSET_IN_BLOCK + lane * 4


def _bootstrap_from_legacy(eid: str) -> dict:
    """既存 vectors.bin + metadata.json (非LSM) を seg-000001 として採用する。

    vectors.bin は【コピー】する (rename しない)。core/cli.py の検証ツールが
    VECTORS_BIN (= DIARY_BIN) を直接参照しており、legacy パスを壊さないため。
    再埋め込みは不要 — 既存バイナリの chunk_id 採番をそのままグローバル ID
    として引き継ぐ。
    """
    if not (DIARY_BIN.exists() and DIARY_META.exists()):
        return _empty_manifest(eid)
    meta = json.loads(DIARY_META.read_text(encoding="utf-8"))
    chunks = meta.get("chunks", [])
    if not chunks:
        return _empty_manifest(eid)

    seg_name = "vectors.seg-000001.bin"
    seg_path = PROCESSED / seg_name
    if not seg_path.exists():
        seg_path.write_bytes(DIARY_BIN.read_bytes())

    ids = sorted(c["id"] for c in chunks)
    days: dict[str, dict] = {}
    for c in chunks:
        idx = c["id"]  # legacy build は 0-based 連番なので index_in_segment == id
        days[c["date"]] = {
            "chunk_id": idx, "segment": seg_name, "index_in_segment": idx,
            "tomb_offset": _tomb_offset(idx), "content_hash": content_hash(c),
        }
    return {
        "format": "pkbseg.v1", "embedder_id": eid,
        "next_chunk_id": max(ids) + 1,
        "segments": [{"file": seg_name, "live": len(chunks), "dead": 0}],
        "days": days,
    }


def load_manifest(eid: str) -> dict:
    if LSM_MANIFEST.exists():
        m = json.loads(LSM_MANIFEST.read_text(encoding="utf-8"))
        if m.get("embedder_id") == eid:
            return m
        # 埋め込み空間の不一致 (I-6): 全再構築を強制する空マニフェストを返す。
        # 呼び出し側 (sync_diary_index_lsm) がこれを見て全日付を「変更」として扱う。
        return _empty_manifest(eid)
    return _bootstrap_from_legacy(eid)


def atomic_save_manifest(manifest: dict) -> None:
    LSM_MANIFEST.parent.mkdir(parents=True, exist_ok=True)
    tmp = LSM_MANIFEST.with_suffix(".json.tmp")
    tmp.write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")
    os.replace(tmp, LSM_MANIFEST)


# ---------------------------------------------------------------- 共有 metadata.json
def _load_meta() -> dict:
    if DIARY_META.exists():
        return json.loads(DIARY_META.read_text(encoding="utf-8"))
    return {"format": pipeline.MAGIC.decode(), "dim": DIM, "lanes": LANES,
            "normalized": True, "similarity": "dot_product (== cosine)",
            "lsm": True, "chunks": []}


_META_SIZE_LIMIT_BYTES = 200 * 1024 * 1024  # IMP-1 トリップワイヤ (docs/AI_SKILLS.md §14)


def _save_meta(meta: dict, chunks_by_id: dict[int, dict]) -> None:
    meta["chunks"] = [chunks_by_id[i] for i in sorted(chunks_by_id)]
    meta["num_vectors"] = len(chunks_by_id)
    meta["lsm"] = True
    serialized = json.dumps(meta, ensure_ascii=False, indent=2)
    size = len(serialized.encode("utf-8"))
    if size > _META_SIZE_LIMIT_BYTES:
        raise RuntimeError(
            f"metadata.json のシリアライズサイズが上限を超過"
            f" ({size / 1e6:.1f}MB > {_META_SIZE_LIMIT_BYTES / 1e6:.0f}MB) — "
            "書き込みを中止した。台帳の異常肥大 (import 重複・セッション橋渡し"
            "の増幅など) を疑え。詳細は docs/AI_SKILLS.md §14 (IMP-1) を参照。"
        )
    DIARY_META.parent.mkdir(parents=True, exist_ok=True)
    tmp = DIARY_META.with_suffix(".json.tmp")
    tmp.write_text(serialized, encoding="utf-8")
    os.replace(tmp, DIARY_META)


# ---------------------------------------------------------------- 墓標
def tombstone(seg_path: Path, tomb_offset: int) -> None:
    """chunk_id フィールドを in-place で -1 に書き換える (PKBVEC01 の墓標規約)。

    mmap 中のファイルは Windows で truncate 不可だが、サイズを変えない
    in-place 書き込みは問題ない。ただし daemon が read-only mmap で保持して
    いる場合、その OS 依存のフォールト済みページ可視性は保証されない
    (罠 T-1 相当) — 呼び出し側が直後に該当セグメントへ remap すること。
    """
    with open(seg_path, "r+b") as f:
        f.seek(tomb_offset)
        f.write(_TOMB)


# ---------------------------------------------------------------- セグメント書き出し
def write_segment(chunks: list[dict], vectors: np.ndarray, start_chunk_id: int,
                  seg_path: Path) -> dict[str, int]:
    """新セグメントを書き出し、日付 -> index_in_segment の対応を返す。

    index_in_segment を明示的に返す (chunk_id からの逆算に依存しない) 理由:
    墓標書きで生存エントリが間引かれた後、セグメント先頭の chunk_id が
    "最小値" ではなくなり得る。min(生存chunk_id) を基点とみなす実装は
    その時点で誤ったオフセットを算出する (compaction 時に踏んだ設計ミスの
    教訓)。tomb_offset / compaction の読み出しは必ずこの index を経由すること。

    pipeline.to_aosoa は 0-based のローカル ID (0..n-1) を割り当てる。
    ここではそれをグローバル ID へシフトするだけで、to_aosoa/write_binary の
    実装には一切触れない (Bravo/既存 pipeline の凍結を守る)。
    """
    aosoa, local_ids = pipeline.to_aosoa(vectors)
    global_ids = local_ids.copy()
    mask = global_ids >= 0
    global_ids[mask] += start_chunk_id
    pipeline.write_binary(aosoa, global_ids, len(chunks), seg_path)

    index_in_segment: dict[str, int] = {}
    for i, c in enumerate(chunks):
        index_in_segment[c["date"]] = i
    return index_in_segment


# ---------------------------------------------------------------- コンパクション
def maybe_compact(manifest: dict, *, release_fn=None, max_segments: int = 8,
                  dead_ratio: float = 0.5) -> bool:
    """発火条件: セグメント数 > max_segments、またはいずれかで dead 比率超過。
    生存レーンをバイトコピーして 1 セグメントへ集約する (再埋め込みなし)。
    manifest は呼び出し側が atomic_save_manifest で確定させる。

    release_fn(path) が渡されれば、旧セグメントの unlink 前に必ず呼ぶ
    (デーモンが mmap 保持中のファイルは Windows で削除できない — remap
    してからでないと unlink は PermissionError の時限爆弾になる。§2.1.5 手順4)。
    """
    segs = manifest["segments"]
    needs = len(segs) > max_segments or any(
        s["dead"] / max(s["live"] + s["dead"], 1) > dead_ratio for s in segs)
    if not needs or not segs:
        return False

    # manifest["days"] が生存の唯一の真実 (ファイル側の墓標書きが遅延していても
    # manifest が正 — §2.1.5 の規約)。
    live_by_segment: dict[str, list[str]] = {}
    for date, rec in manifest["days"].items():
        live_by_segment.setdefault(rec["segment"], []).append(date)

    old_segment_names = [s["file"] for s in segs]
    entries: list[tuple[str, int]] = []   # (date, chunk_id) 生存順
    vectors: list[np.ndarray] = []
    for seg in segs:
        dates = live_by_segment.get(seg["file"], [])
        if not dates:
            continue
        raw = (PROCESSED / seg["file"]).read_bytes()
        _, dim, lanes, _nvec, nblk, blk_b, _ = struct.unpack("<8sIIIIII", raw[:32])
        blocks = np.frombuffer(raw[32:], dtype=np.uint8).reshape(nblk, blk_b)
        data = blocks[:, :dim * lanes * 4].copy().view(np.float32).reshape(nblk, dim, lanes)
        vecs_in_seg = data.transpose(0, 2, 1).reshape(-1, dim)   # (nblk*4, dim)
        for date in dates:
            rec = manifest["days"][date]
            idx = rec["index_in_segment"]   # 明示フィールド (chunk_id からの逆算はしない)
            vectors.append(vecs_in_seg[idx])
            entries.append((date, rec["chunk_id"]))

    if not entries:
        return False

    new_vectors = np.stack(vectors).astype(np.float32)
    aosoa, local_ids = pipeline.to_aosoa(new_vectors)
    # コンパクション後も chunk_id はグローバル値を保持する (compact は「詰め直し」
    # であって「採番し直し」ではない — 参照している他モジュールへの影響をゼロにする)
    global_ids = np.full(local_ids.shape, -1, dtype=np.int32)
    flat = global_ids.reshape(-1)
    for i, (_, cid) in enumerate(entries):
        flat[i] = cid
    global_ids = flat.reshape(local_ids.shape)

    new_seg_name = _next_segment_name(manifest)
    new_seg_path = PROCESSED / new_seg_name
    pipeline.write_binary(aosoa, global_ids, len(entries), new_seg_path)

    for i, (date, cid) in enumerate(entries):
        manifest["days"][date] = {
            "chunk_id": cid, "segment": new_seg_name, "index_in_segment": i,
            "tomb_offset": _tomb_offset(i),
            "content_hash": manifest["days"][date]["content_hash"],
        }
    manifest["segments"] = [s for s in segs if s["file"] not in old_segment_names]
    manifest["segments"].append({"file": new_seg_name, "live": len(entries), "dead": 0})

    for name in old_segment_names:
        p = PROCESSED / name
        if release_fn is not None:
            release_fn(p)          # remap してから削除 (Windows PermissionError 回避)
        p.unlink(missing_ok=True)
    return True


def _next_segment_name(manifest: dict) -> str:
    existing = {s["file"] for s in manifest["segments"]}
    n = 1
    while True:
        name = f"vectors.seg-{n:06d}.bin"
        if name not in existing:
            return name
        n += 1


# ---------------------------------------------------------------- 差分同期オーケストレーション
def sync_diary_index_lsm(engine, force: bool = False) -> bool:
    """diary.md 等の変更日数だけ再埋め込みし、セグメントを追記する。

    engine は ConsultationEngine 相当 (embedder プロパティ・
    _release_index_mapping(path) を持つダックタイピング。テストではフェイクで
    差し替え可能)。手順は SPEC §2.1.3 と完全に一致させること:
      新セグメント確定 (manifest+meta) → 墓標 → remap → コンパクション判定。
    この順序を変えるな (クラッシュ耐性と Windows remap 安全性の両方の根拠)。
    """
    eid = embedder_id(engine.embedder)
    needs_persist = not LSM_MANIFEST.exists()   # bootstrap/空マニフェストは即永続化する
    manifest = load_manifest(eid)
    if manifest["embedder_id"] != eid:
        # 埋め込み空間の不一致 (不変条件 I-6): 全再構築を強制する。
        for seg in manifest.get("segments", []):
            engine._release_index_mapping(PROCESSED / seg["file"])
        manifest = _empty_manifest(eid)
        force = True
        needs_persist = True
    if needs_persist:
        # bootstrap 結果 (legacy 由来 or 空) をここで書き出さないと、次回呼び出しが
        # 毎回 metadata.json の全走査から作り直す羽目になり、読み取りパスに
        # O(全履歴) が残ってしまう (idempotent boot の欠落 — 実装中に発見した罠)。
        atomic_save_manifest(manifest)

    daily = pipeline.load_chunks()
    by_date = {c["date"]: c for c in daily}

    changed: list[tuple[dict, str]] = []
    for d, c in by_date.items():
        h = content_hash(c)
        rec = manifest["days"].get(d)
        if force or rec is None or rec["content_hash"] != h:
            changed.append((c, h))
    deleted_dates = sorted(set(manifest["days"]) - set(by_date))

    if not changed and not deleted_dates:
        return False

    meta = _load_meta()
    chunks_by_id = {c["id"]: c for c in meta.get("chunks", [])}

    to_tombstone: list[dict] = []
    for c, _h in changed:
        old = manifest["days"].get(c["date"])
        if old is not None:
            to_tombstone.append(old)
            chunks_by_id.pop(old["chunk_id"], None)
    for d in deleted_dates:
        old = manifest["days"][d]
        to_tombstone.append(old)
        chunks_by_id.pop(old["chunk_id"], None)

    new_seg_name = None
    if changed:
        texts = [f"{c['title']}\n{c['text']}" for c, _ in changed]
        vectors = pipeline.l2_normalize(
            np.asarray(engine.embedder.encode(texts), dtype=np.float32))
        start_id = manifest["next_chunk_id"]
        new_chunks = []
        for i, (c, _h) in enumerate(changed):
            nc = dict(c)
            nc["id"] = start_id + i
            new_chunks.append(nc)
        new_seg_name = _next_segment_name(manifest)
        new_seg_path = PROCESSED / new_seg_name
        index_map = write_segment(new_chunks, vectors, start_id, new_seg_path)
        manifest["next_chunk_id"] = start_id + len(new_chunks)
        manifest["segments"].append(
            {"file": new_seg_name, "live": len(new_chunks), "dead": 0})
        for i, (c, h) in enumerate(changed):
            idx = index_map[c["date"]]
            manifest["days"][c["date"]] = {
                "chunk_id": start_id + i, "segment": new_seg_name,
                "index_in_segment": idx, "tomb_offset": _tomb_offset(idx),
                "content_hash": h,
            }
            chunks_by_id[start_id + i] = new_chunks[i]
    for d in deleted_dates:
        del manifest["days"][d]

    # 1. metadata → manifest の順で確定 (manifest がメタに存在しない chunk_id を
    #    指す瞬間を作らない)
    meta["embedder"] = getattr(engine.embedder, "name", type(engine.embedder).__name__)
    meta["source"] = ("diary.md + line_history.txt + calendar.json "
                      "+ ai_consultations.json + finance.json (DailyContext, LSM)")
    _save_meta(meta, chunks_by_id)
    atomic_save_manifest(manifest)

    # 2. 墓標書き (manifest 確定後。ここでクラッシュしても旧エントリが manifest
    #    上は既に非生存なので、検索側の日付デデュープが重複を吸収する)
    dead_bump: dict[str, int] = {}
    for rec in to_tombstone:
        tombstone(PROCESSED / rec["segment"], rec["tomb_offset"])
        dead_bump[rec["segment"]] = dead_bump.get(rec["segment"], 0) + 1
    if dead_bump:
        for seg in manifest["segments"]:
            if seg["file"] in dead_bump:
                seg["live"] -= dead_bump[seg["file"]]
                seg["dead"] += dead_bump[seg["file"]]
        atomic_save_manifest(manifest)

    # 3. remap: 墓標を書いた旧セグメント + 新セグメント (デーモンの mmap 解放)
    touched = {rec["segment"] for rec in to_tombstone}
    if new_seg_name:
        touched.add(new_seg_name)
    for seg_name in touched:
        engine._release_index_mapping(PROCESSED / seg_name)

    # 4. コンパクション判定 (best-effort。旧セグメント unlink 前に remap を挟む)
    if maybe_compact(manifest,
                     release_fn=lambda p: engine._release_index_mapping(p)):
        atomic_save_manifest(manifest)
    return True


# ---------------------------------------------------------------- マルチセグメント検索
def search_lsm(engine, qvec, top_k: int = 3) -> list[dict]:
    """全セグメントに対して既存 search_index() を無改造のまま呼び、日付
    デデュープしてマージする (C1 マイルストーン: C++ 側は完全無変更)。

    manifest が存在しない (LSM 未移行) 環境では呼び出し側が legacy パスへ
    分岐すること — この関数はマニフェストの存在を前提にする。
    """
    eid = embedder_id(engine.embedder)
    manifest = load_manifest(eid)
    hits: list[dict] = []
    for seg in manifest.get("segments", []):
        seg_path = PROCESSED / seg["file"]
        if not seg_path.exists():
            continue
        hits.extend(engine.search_index(seg_path, DIARY_META, qvec, top_k))
    # 日付デデュープ: 同一日付の重複ヒットは score 最大の 1 件のみ残す
    # (クラッシュ窓 §2.1.3 と、墓標書き遅延の両方をここで吸収する)
    best: dict[str, dict] = {}
    for h in hits:
        d = h.get("date")
        if d is None:
            continue
        if d not in best or h["score"] > best[d]["score"]:
            best[d] = h
    ranked = sorted(best.values(), key=lambda h: -h["score"])
    return ranked[:top_k]
