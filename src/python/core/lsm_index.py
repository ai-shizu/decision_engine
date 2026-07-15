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
import hmac
import json
import os
import re
import stat
import struct
import tempfile
from pathlib import Path

import numpy as np

from . import pipeline
from .canonicalization import canonicalize_json, canonicalize_text
from .durable_persistence import (
    durable_atomic_write,
    durable_atomic_write_text,
    read_json_file,
)
from .paths import DIARY_BIN, DIARY_META, LSM_MANIFEST, PROCESSED
from .score_ranking import score_order_key

DIM = pipeline.DIM
LANES = pipeline.LANES
BLOCK_BYTES = pipeline.BLOCK_BYTES
HEADER_BYTES = 32
_TOMB = struct.pack("<i", -1)

# セグメント内オフセット定数 (FileHeader 32B + data[384][4] の後に chunk_ids[4])
_IDS_OFFSET_IN_BLOCK = DIM * LANES * 4   # 6144

MANIFEST_FORMAT = "pkbseg.v2"
_MANIFEST_KEYS = frozenset({"format", "embedder_id", "next_chunk_id", "segments", "days"})
_SEGMENT_KEYS = frozenset({"file", "live", "dead", "payload_hash"})
_DAY_KEYS = frozenset({
    "chunk_id", "segment", "index_in_segment", "tomb_offset", "content_hash",
})
_SEGMENT_NAME = re.compile(r"\Avectors\.seg-[0-9]{6}\.bin\Z")
_PAYLOAD_HASH = re.compile(r"\Ablake2b-256:[0-9a-f]{64}\Z")
_CONTENT_HASH = re.compile(r"\Ablake2b:[0-9a-f]{32}\Z")


class DerivedStoreIntegrityError(ValueError):
    """An LSM manifest or one of its owned segment payloads is untrusted."""


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
    canonical = canonicalize_text(text).encode("utf-8")
    return "blake2b:" + hashlib.blake2b(canonical, digest_size=16).hexdigest()


# ---------------------------------------------------------------- マニフェスト I/O
def _empty_manifest(eid: str) -> dict:
    return {"format": MANIFEST_FORMAT, "embedder_id": eid, "next_chunk_id": 0,
             "segments": [], "days": {}}


def segment_payload_hash(path: Path) -> str:
    """Return the manifest hash for one physical segment payload."""
    digest = hashlib.blake2b(digest_size=32)
    try:
        with Path(path).open("rb") as handle:
            while chunk := handle.read(1024 * 1024):
                digest.update(chunk)
    except OSError as exc:
        raise DerivedStoreIntegrityError("segment payload cannot be read") from exc
    return "blake2b-256:" + digest.hexdigest()


def _owned_segment_path(name: str, *, require_exists: bool = True) -> Path:
    if type(name) is not str or not _SEGMENT_NAME.fullmatch(name):
        raise DerivedStoreIntegrityError("segment basename is not owned")
    relative = Path(name)
    if relative.is_absolute() or relative.name != name:
        raise DerivedStoreIntegrityError("segment path must be a direct child basename")

    try:
        owned_root = PROCESSED.resolve(strict=True)
    except OSError as exc:
        raise DerivedStoreIntegrityError("owned segment directory is unavailable") from exc
    candidate = PROCESSED / name
    if candidate.is_symlink():
        raise DerivedStoreIntegrityError("segment symlinks are forbidden")
    try:
        resolved = candidate.resolve(strict=require_exists)
    except (OSError, RuntimeError) as exc:
        raise DerivedStoreIntegrityError("declared segment is missing") from exc
    if resolved.parent != owned_root or not resolved.is_relative_to(owned_root):
        raise DerivedStoreIntegrityError("segment path escapes the owned directory")
    if require_exists and not resolved.is_file():
        raise DerivedStoreIntegrityError("declared segment is not a regular file")
    return resolved


def _strict_nonnegative_int(value, field: str) -> int:
    if type(value) is not int or value < 0:
        raise DerivedStoreIntegrityError(f"{field} must be a non-negative integer")
    return value


def _preflight_manifest(manifest: dict) -> dict[str, Path]:
    """Validate every manifest reference before any segment is consumed."""
    if type(manifest) is not dict or set(manifest) != _MANIFEST_KEYS:
        raise DerivedStoreIntegrityError("manifest schema keys are invalid")
    if manifest.get("format") != MANIFEST_FORMAT:
        raise DerivedStoreIntegrityError("manifest format is unsupported")
    if type(manifest.get("embedder_id")) is not str or not manifest["embedder_id"]:
        raise DerivedStoreIntegrityError("manifest embedder_id is invalid")
    _strict_nonnegative_int(manifest.get("next_chunk_id"), "next_chunk_id")
    segments = manifest.get("segments")
    days = manifest.get("days")
    if type(segments) is not list or type(days) is not dict:
        raise DerivedStoreIntegrityError("manifest collections are invalid")

    paths: dict[str, Path] = {}
    for segment in segments:
        if type(segment) is not dict or set(segment) != _SEGMENT_KEYS:
            raise DerivedStoreIntegrityError("segment record schema is invalid")
        name = segment.get("file")
        if type(name) is not str:
            raise DerivedStoreIntegrityError("segment basename is invalid")
        if name in paths:
            raise DerivedStoreIntegrityError("duplicate segment basename")
        _strict_nonnegative_int(segment.get("live"), "segment.live")
        _strict_nonnegative_int(segment.get("dead"), "segment.dead")
        recorded_hash = segment.get("payload_hash")
        if type(recorded_hash) is not str or not _PAYLOAD_HASH.fullmatch(recorded_hash):
            raise DerivedStoreIntegrityError("segment payload_hash is invalid")
        path = _owned_segment_path(name)
        actual_hash = segment_payload_hash(path)
        if not hmac.compare_digest(recorded_hash, actual_hash):
            raise DerivedStoreIntegrityError("segment payload hash mismatch")
        paths[name] = path

    for date_key, record in days.items():
        if type(date_key) is not str or type(record) is not dict or set(record) != _DAY_KEYS:
            raise DerivedStoreIntegrityError("day record schema is invalid")
        if record.get("segment") not in paths:
            raise DerivedStoreIntegrityError("day record references an unknown segment")
        chunk_id = _strict_nonnegative_int(record.get("chunk_id"), "day.chunk_id")
        index = _strict_nonnegative_int(
            record.get("index_in_segment"), "day.index_in_segment"
        )
        tomb_offset = _strict_nonnegative_int(record.get("tomb_offset"), "day.tomb_offset")
        if chunk_id >= manifest["next_chunk_id"] or tomb_offset != _tomb_offset(index):
            raise DerivedStoreIntegrityError("day record identity or offset is invalid")
        chash = record.get("content_hash")
        if type(chash) is not str or not _CONTENT_HASH.fullmatch(chash):
            raise DerivedStoreIntegrityError("day content_hash is invalid")
    return paths


def _snapshot_verified_segments(
    manifest: dict,
    verified_paths: dict[str, Path],
    snapshot_root: Path,
) -> dict[str, Path]:
    """Copy and re-hash each segment from one open handle for search use.

    The returned paths contain the exact bytes whose digest was compared with
    the manifest.  Search backends may safely reopen these private snapshots;
    they never reopen the mutable manifest-owned segment path.
    """
    snapshots: dict[str, Path] = {}
    read_flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0)
    read_flags |= getattr(os, "O_NOFOLLOW", 0)
    write_flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    write_flags |= getattr(os, "O_CLOEXEC", 0)
    write_flags |= getattr(os, "O_NOFOLLOW", 0)

    for segment in manifest["segments"]:
        name = segment["file"]
        source_path = verified_paths[name]
        snapshot_path = snapshot_root / name
        digest = hashlib.blake2b(digest_size=32)
        try:
            source_fd = os.open(source_path, read_flags)
            try:
                if not stat.S_ISREG(os.fstat(source_fd).st_mode):
                    raise DerivedStoreIntegrityError(
                        "declared segment changed file type before search"
                    )
                snapshot_fd = os.open(snapshot_path, write_flags, 0o600)
                try:
                    with os.fdopen(source_fd, "rb") as source:
                        source_fd = -1
                        with os.fdopen(snapshot_fd, "wb") as snapshot:
                            snapshot_fd = -1
                            while chunk := source.read(1024 * 1024):
                                digest.update(chunk)
                                snapshot.write(chunk)
                finally:
                    if snapshot_fd >= 0:
                        os.close(snapshot_fd)
            finally:
                if source_fd >= 0:
                    os.close(source_fd)
        except DerivedStoreIntegrityError:
            raise
        except OSError as exc:
            raise DerivedStoreIntegrityError(
                "segment snapshot could not be created"
            ) from exc

        actual_hash = "blake2b-256:" + digest.hexdigest()
        if not hmac.compare_digest(segment["payload_hash"], actual_hash):
            raise DerivedStoreIntegrityError(
                "segment payload changed between preflight and search"
            )
        snapshots[name] = snapshot_path
    return snapshots


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
        durable_atomic_write(seg_path, DIARY_BIN.read_bytes())

    ids = sorted(c["id"] for c in chunks)
    days: dict[str, dict] = {}
    for c in chunks:
        idx = c["id"]  # legacy build は 0-based 連番なので index_in_segment == id
        days[c["date"]] = {
            "chunk_id": idx, "segment": seg_name, "index_in_segment": idx,
            "tomb_offset": _tomb_offset(idx), "content_hash": content_hash(c),
        }
    return {
        "format": MANIFEST_FORMAT, "embedder_id": eid,
        "next_chunk_id": max(ids) + 1,
        "segments": [{
            "file": seg_name,
            "live": len(chunks),
            "dead": 0,
            "payload_hash": segment_payload_hash(seg_path),
        }],
        "days": days,
    }


def load_manifest(eid: str) -> dict:
    try:
        manifest = read_json_file(LSM_MANIFEST)
    except FileNotFoundError:
        manifest = _bootstrap_from_legacy(eid)
    _preflight_manifest(manifest)
    return manifest


def atomic_save_manifest(manifest: dict) -> None:
    _preflight_manifest(manifest)
    durable_atomic_write_text(
        LSM_MANIFEST,
        canonicalize_json(manifest),
    )


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
    serialized = canonicalize_json(meta)
    size = len(serialized.encode("utf-8"))
    if size > _META_SIZE_LIMIT_BYTES:
        raise RuntimeError(
            f"metadata.json のシリアライズサイズが上限を超過"
            f" ({size / 1e6:.1f}MB > {_META_SIZE_LIMIT_BYTES / 1e6:.0f}MB) — "
            "書き込みを中止した。台帳の異常肥大 (import 重複・セッション橋渡し"
            "の増幅など) を疑え。詳細は docs/AI_SKILLS.md §14 (IMP-1) を参照。"
        )
    durable_atomic_write_text(DIARY_META, serialized)


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
    verified_paths = _preflight_manifest(manifest)
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
        raw = verified_paths[seg["file"]].read_bytes()
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
    new_seg_path = _owned_segment_path(new_seg_name, require_exists=False)
    pipeline.write_binary(aosoa, global_ids, len(entries), new_seg_path)

    for i, (date, cid) in enumerate(entries):
        manifest["days"][date] = {
            "chunk_id": cid, "segment": new_seg_name, "index_in_segment": i,
            "tomb_offset": _tomb_offset(i),
            "content_hash": manifest["days"][date]["content_hash"],
        }
    manifest["segments"] = [s for s in segs if s["file"] not in old_segment_names]
    manifest["segments"].append({
        "file": new_seg_name,
        "live": len(entries),
        "dead": 0,
        "payload_hash": segment_payload_hash(new_seg_path),
    })

    for name in old_segment_names:
        p = verified_paths[name]
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
        verified_paths = _preflight_manifest(manifest)
        for seg in manifest["segments"]:
            engine._release_index_mapping(verified_paths[seg["file"]])
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
        new_seg_path = _owned_segment_path(new_seg_name, require_exists=False)
        index_map = write_segment(new_chunks, vectors, start_id, new_seg_path)
        manifest["next_chunk_id"] = start_id + len(new_chunks)
        manifest["segments"].append({
            "file": new_seg_name,
            "live": len(new_chunks),
            "dead": 0,
            "payload_hash": segment_payload_hash(new_seg_path),
        })
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
    verified_paths = _preflight_manifest(manifest)
    for rec in to_tombstone:
        tombstone(verified_paths[rec["segment"]], rec["tomb_offset"])
        dead_bump[rec["segment"]] = dead_bump.get(rec["segment"], 0) + 1
    if dead_bump:
        for seg in manifest["segments"]:
            if seg["file"] in dead_bump:
                seg["live"] -= dead_bump[seg["file"]]
                seg["dead"] += dead_bump[seg["file"]]
                seg["payload_hash"] = segment_payload_hash(
                    verified_paths[seg["file"]]
                )
        atomic_save_manifest(manifest)

    # 3. remap: 墓標を書いた旧セグメント + 新セグメント (デーモンの mmap 解放)
    touched = {rec["segment"] for rec in to_tombstone}
    if new_seg_name:
        touched.add(new_seg_name)
    for seg_name in touched:
        engine._release_index_mapping(_owned_segment_path(seg_name))

    # 4. コンパクション判定 (best-effort。旧セグメント unlink 前に remap を挟む)
    if maybe_compact(manifest,
                     release_fn=lambda p: engine._release_index_mapping(p)):
        atomic_save_manifest(manifest)
    return True


# ---------------------------------------------------------------- マルチセグメント検索
def search_lsm(engine, qvec, top_k: int = 3) -> list[dict]:
    """検査済みsnapshotを介して各segmentを検索し、日付dedupしてmergeする。

    manifest が存在しない (LSM 未移行) 環境では呼び出し側が legacy パスへ
    分岐すること — この関数はマニフェストの存在を前提にする。
    """
    eid = embedder_id(engine.embedder)
    manifest = load_manifest(eid)
    if manifest["embedder_id"] != eid:
        raise DerivedStoreIntegrityError("manifest embedder identity mismatch")
    verified_paths = _preflight_manifest(manifest)
    hits: list[dict] = []
    with tempfile.TemporaryDirectory(
        prefix=".lsm-search-",
        dir=PROCESSED,
    ) as snapshot_dir:
        snapshot_paths = _snapshot_verified_segments(
            manifest,
            verified_paths,
            Path(snapshot_dir),
        )
        try:
            for seg in manifest["segments"]:
                seg_path = snapshot_paths[seg["file"]]
                hits.extend(engine.search_index(seg_path, DIARY_META, qvec, top_k))
        finally:
            release_mapping = getattr(engine, "_release_index_mapping", None)
            if callable(release_mapping):
                for seg_path in snapshot_paths.values():
                    release_mapping(seg_path)
    # 日付デデュープ: 同一日付の重複ヒットは score 最大の 1 件のみ残す
    # (クラッシュ窓 §2.1.3 と、墓標書き遅延の両方をここで吸収する)
    best: dict[str, dict] = {}
    for h in hits:
        d = h.get("date")
        if d is None:
            continue
        hit_key = score_order_key(h["score"], h.get("id", h.get("chunk_id", -1)))
        if d not in best or hit_key < score_order_key(
                best[d]["score"], best[d].get("id", best[d].get("chunk_id", -1))):
            best[d] = h
    ranked = sorted(
        best.values(),
        key=lambda h: score_order_key(
            h["score"], h.get("id", h.get("chunk_id", -1))),
    )
    return ranked[:top_k]
