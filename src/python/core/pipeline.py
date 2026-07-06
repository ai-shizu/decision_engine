#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
PKB (Personal Knowledge Base) ベクトル化パイプライン
=====================================================
data/raw/diary.md を読み込み(無ければダミー生成)、384次元に埋め込み、
ARM NEON 向け 4-lane AoSoA レイアウトのバイナリ (vectors.bin) と
metadata.json を出力する。

バイナリレイアウト (little-endian, C++ 側 search_engine.cpp と完全同期):

  FileHeader (32 bytes):
    char     magic[8]      = "PKBVEC01"
    uint32   dim           = 384
    uint32   lanes         = 4
    uint32   num_vectors   (実チャンク数)
    uint32   num_blocks    = ceil(num_vectors / 4)
    uint32   block_bytes   = 384*4*4 + 4*4 = 6160
    uint32   reserved      = 0

  VectorBlock × num_blocks (各 6160 bytes):
    float    data[384][4]   # dim-major / lane-minor (AoSoA)
    int32    chunk_ids[4]   # パディングレーンは -1

全ベクトルは L2 正規化済み。内積 = コサイン類似度。
"""

from __future__ import annotations

import hashlib
import json
import struct
import sys
import time
from datetime import date, timedelta

import numpy as np

from .paths import (
    DIARY_BIN,
    DIARY_META,
    DIARY_MD,
    METADATA_JSON,
    OUT_DIR,
    QUERY_BIN,
    RAW_DIARY,
    VECTORS_BIN,
)

DIM = 384
LANES = 4
MAGIC = b"PKBVEC01"
BLOCK_BYTES = DIM * LANES * 4 + LANES * 4  # 6160
SAMPLE_QUERY = "最近の体調と運動習慣、集中力の変化について"

# ---------------------------------------------------------------- ダミー日記生成
_MOODS = ["晴れやか", "淡々", "焦り気味", "充実", "疲労感あり", "前向き", "モヤモヤ"]
_WORKS = [
    "ベクトル検索エンジンのNEON最適化を進めた。メモリレイアウトの見直しで内積計算が体感で速くなった。",
    "OpenMPのスレッド分割戦略を検証。ローカルTop-K方式でロック競合が消えた。",
    "会議が多く実装時間が取れなかった。細切れ時間の使い方を再考する必要がある。",
    "mmapベースのローダを書いた。ページフォルトのタイミングを計測して先読みの効果を確認。",
    "ドキュメント整備に集中。設計判断の理由を残すことの重要性を再認識した。",
    "プロファイラでホットスポットを特定。キャッシュミスがボトルネックだと判明。",
]
_LEARNINGS = [
    "AoSoA形式はSIMDレーンを常に埋められるのが強み。データ設計が性能を決める。",
    "早い段階で計測環境を作ると意思決定が速くなる。推測より計測。",
    "疲れている日は判断の質が落ちる。重要な設計判断は午前中に行うべき。",
    "小さく作って動かすサイクルが結局一番速い。",
    "人に説明できない設計は自分でも理解できていない設計だ。",
]
_HEALTHS = [
    "朝に30分のランニング。集中力が午後まで持続した。",
    "睡眠6時間。やや寝不足で午後に集中が切れた。",
    "散歩のみ。デスクワーク続きで肩こりが気になる。",
    "筋トレ実施。運動した日は入眠が早い傾向がある。",
    "休養日。意識的に画面から離れる時間を作った。",
]


def generate_dummy_diary(path: Path, days: int = 30) -> None:
    """自己分析に役立つ構造(気分/仕事/学び/健康)のダミー日記を生成する。"""
    rng = np.random.default_rng(42)
    start = date.today() - timedelta(days=days)
    lines = ["# 開発日誌(自動生成ダミー)", ""]
    for i in range(days):
        d = start + timedelta(days=i)
        lines += [
            f"## {d.isoformat()}",
            f"- 気分: {_MOODS[int(rng.integers(len(_MOODS)))]}",
            f"- 仕事: {_WORKS[int(rng.integers(len(_WORKS)))]}",
            f"- 学び: {_LEARNINGS[int(rng.integers(len(_LEARNINGS)))]}",
            f"- 健康: {_HEALTHS[int(rng.integers(len(_HEALTHS)))]}",
            "",
        ]
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines), encoding="utf-8")
    print(f"[pipeline] ダミー日記を生成: {path} ({days} entries)")


# ---------------------------------------------------------------- チャンク構築
def load_chunks() -> list[dict]:
    """日記とLINE履歴を日付 (YYYY-MM-DD) で結合した DailyContext チャンクを返す。

    1チャンク = 1日分 (日記本文 + その日のLINE会話全テキスト)。
    検索時に日記とLINEの文脈が統合されたチャンクが直接ヒットする。
    """
    from .data_merger import load_daily_contexts  # 遅延import (循環回避)
    chunks = load_daily_contexts()
    both = sum(1 for c in chunks if c["has_diary"] and c["has_line"])
    print(f"[pipeline] DailyContextチャンク数: {len(chunks)} (日記+LINE両方: {both}日)")
    return chunks


# ---------------------------------------------------------------- 埋め込み
class HashedNgramEmbedder:
    """SentenceTransformers が使えない環境向けの決定論的フォールバック。

    文字 tri-gram を 384 バケットへ符号付きハッシュし L2 正規化する。
    意味的品質は劣るが、バイナリ生成〜C++検索のパイプライン検証には十分。
    """

    name = "hashed-ngram-384 (fallback)"

    def encode(self, texts: list[str], **_) -> np.ndarray:
        out = np.zeros((len(texts), DIM), dtype=np.float32)
        for row, t in enumerate(texts):
            s = f"^{t}$"
            for i in range(len(s) - 2):
                h = hashlib.blake2b(s[i : i + 3].encode("utf-8"), digest_size=8).digest()
                v = int.from_bytes(h, "little")
                out[row, v % DIM] += 1.0 if (v >> 63) & 1 else -1.0
        return out


def build_embedder():
    """本番: SentenceTransformers / 不可時: フォールバックを返す。"""
    try:
        from sentence_transformers import SentenceTransformer

        model_name = "sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2"
        model = SentenceTransformer(model_name)
        test = model.encode(["dim check"])
        if test.shape[-1] != DIM:
            raise RuntimeError(f"次元不一致: {test.shape[-1]} != {DIM}")
        print(f"[pipeline] 埋め込みモデル: {model_name} (384-dim)")
        model.name = model_name
        return model
    except Exception as e:  # モジュール未導入・モデル未取得(完全オフライン)など
        print(f"[pipeline] SentenceTransformers 不可 -> フォールバック使用: {type(e).__name__}: {e}")
        return HashedNgramEmbedder()


def l2_normalize(v: np.ndarray) -> np.ndarray:
    norms = np.linalg.norm(v, axis=-1, keepdims=True)
    norms[norms == 0.0] = 1.0
    return (v / norms).astype(np.float32)


# ---------------------------------------------------------------- AoSoA 変換 & バイナリ出力
def to_aosoa(vectors: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """(N, 384) -> (num_blocks, 384, 4) の AoSoA と (num_blocks, 4) の chunk_ids。"""
    n = vectors.shape[0]
    num_blocks = (n + LANES - 1) // LANES
    padded = np.zeros((num_blocks * LANES, DIM), dtype=np.float32)
    padded[:n] = vectors
    ids = np.full(num_blocks * LANES, -1, dtype=np.int32)
    ids[:n] = np.arange(n, dtype=np.int32)

    # (blocks, lanes, dim) -> 転置で (blocks, dim, lanes): C++ の data[384][4] と一致
    aosoa = np.ascontiguousarray(
        padded.reshape(num_blocks, LANES, DIM).transpose(0, 2, 1), dtype=np.float32
    )
    return aosoa, ids.reshape(num_blocks, LANES)


def write_binary(aosoa: np.ndarray, ids: np.ndarray, num_vectors: int,
                 bin_path: Path = VECTORS_BIN) -> None:
    num_blocks = aosoa.shape[0]
    header = struct.pack(
        "<8sIIIIII", MAGIC, DIM, LANES, num_vectors, num_blocks, BLOCK_BYTES, 0
    )
    bin_path.parent.mkdir(parents=True, exist_ok=True)
    with open(bin_path, "wb") as f:
        f.write(header)
        for b in range(num_blocks):
            f.write(aosoa[b].tobytes(order="C"))   # float data[384][4]
            f.write(ids[b].tobytes(order="C"))     # int32 chunk_ids[4]

    expected = len(header) + num_blocks * BLOCK_BYTES
    actual = bin_path.stat().st_size
    assert actual == expected, f"サイズ不一致: {actual} != {expected}"
    print(f"[pipeline] バイナリ出力: {bin_path} ({actual:,} bytes, {num_blocks} blocks)")


def build_index(chunks: list[dict], bin_path: Path, meta_path: Path,
                embedder=None, source: str = ""):
    """任意のチャンク群からAoSoAインデックス(bin+metadata)を構築する汎用API。

    日記本体だけでなく外部知識(data/knowledge/)のインデックス化にも使う。
    embedder を返すので呼び出し側でモデルロードを再利用できる。
    """
    if not chunks:
        raise ValueError("チャンクが空です")
    if embedder is None:
        embedder = build_embedder()
    for i, c in enumerate(chunks):
        c["id"] = i
    texts = [f"{c['title']}\n{c['text']}" for c in chunks]
    vectors = l2_normalize(np.asarray(embedder.encode(texts), dtype=np.float32))
    aosoa, ids = to_aosoa(vectors)
    write_binary(aosoa, ids, len(chunks), bin_path)

    meta = {
        "format": MAGIC.decode(),
        "dim": DIM,
        "lanes": LANES,
        "num_vectors": len(chunks),
        "num_blocks": int(aosoa.shape[0]),
        "block_bytes": BLOCK_BYTES,
        "normalized": True,
        "similarity": "dot_product (== cosine)",
        "embedder": getattr(embedder, "name", type(embedder).__name__),
        "source": source,
        "chunks": chunks,
    }
    meta_path.write_text(json.dumps(meta, ensure_ascii=False, indent=2),
                         encoding="utf-8")
    print(f"[pipeline] メタデータ出力: {meta_path}")
    return embedder


# ---------------------------------------------------------------- メイン
def main() -> None:
    t0 = time.perf_counter()
    chunks = load_chunks()

    embedder = build_index(chunks, VECTORS_BIN, METADATA_JSON,
                           source=("diary.md + line_history.txt + calendar.json "
                                   "+ ai_consultations.json + finance.json (DailyContext)"))

    # C++ 側デモ用のサンプルクエリベクトル (float32 × 384)
    qvec = l2_normalize(np.asarray(embedder.encode([SAMPLE_QUERY]), dtype=np.float32))[0]
    QUERY_BIN.write_bytes(qvec.tobytes())
    print(f"[pipeline] クエリ出力: {QUERY_BIN} (query='{SAMPLE_QUERY}')")
    print(f"[pipeline] 完了 ({time.perf_counter() - t0:.2f}s)")


if __name__ == "__main__":
    sys.exit(main())
