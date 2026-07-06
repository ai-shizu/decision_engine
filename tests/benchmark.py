#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
tests/benchmark.py
C++ ベクトル検索エンジン (search_engine.exe) のスループット (QPS) と
レイテンシ (μs/query) を計測する。

データ生成:
  pipeline.py と同一の PKBVEC01 / AoSoA レイアウトでランダム正規化ベクトルを書き出す。
  (埋め込みモデル不要 — 検索カーネル性能の計測が目的)

計測方法:
  search_engine.exe は内部で 100 回検索を回し avg μs/query を stdout に出す。
  本スクリプトはそれをパースして QPS = 1e6 / latency_us を表示する。
"""
from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core.pipeline import (  # noqa: E402
    BLOCK_BYTES,
    DIM,
    LANES,
    l2_normalize,
    to_aosoa,
    write_binary,
)

BINARY_DIR = ROOT / "data" / "processed" / "benchmark"
ENGINE_PATH = ROOT / "build" / "search_engine.exe"
QUERY_BIN = BINARY_DIR / "query.bin"

DEFAULT_CHUNK_SIZES = (10_000, 100_000, 500_000)
LATENCY_RE = re.compile(r"latency:\s+([\d.]+)\s+us/query", re.IGNORECASE)


def generate_dummy_data(n_chunks: int, bin_path: Path, query_path: Path, seed: int = 42) -> None:
    """n_chunks 本の L2 正規化ランダムベクトル → AoSoA .bin + クエリ .bin。"""
    print(f"  Generating {n_chunks:,} vectors -> {bin_path.name} ...")
    rng = np.random.default_rng(seed)
    vectors = l2_normalize(rng.standard_normal((n_chunks, DIM)).astype(np.float32))
    aosoa, ids = to_aosoa(vectors)
    write_binary(aosoa, ids, n_chunks, bin_path)

    qvec = l2_normalize(rng.standard_normal((1, DIM)).astype(np.float32))[0]
    query_path.parent.mkdir(parents=True, exist_ok=True)
    query_path.write_bytes(qvec.tobytes())
    expected = n_chunks * BLOCK_BYTES + 32  # header + blocks (approx check)
    size = bin_path.stat().st_size
    blocks = (n_chunks + LANES - 1) // LANES
    expected_exact = 32 + blocks * BLOCK_BYTES
    assert size == expected_exact, f"size mismatch: {size} != {expected_exact}"


def run_engine(bin_path: Path, query_path: Path, top_k: int = 5) -> tuple[float, str]:
    """search_engine.exe を実行し (latency_us, stdout) を返す。"""
    if not ENGINE_PATH.is_file():
        raise FileNotFoundError(
            f"search_engine not found: {ENGINE_PATH}\n"
            f"Build with: .\\build.ps1  or see README.md"
        )
    proc = subprocess.run(
        [str(ENGINE_PATH), str(bin_path), str(query_path), str(top_k)],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"search_engine failed (exit {proc.returncode}):\n"
            f"{proc.stderr or proc.stdout}"
        )
    m = LATENCY_RE.search(proc.stdout)
    if not m:
        raise RuntimeError(f"Could not parse latency from output:\n{proc.stdout}")
    return float(m.group(1)), proc.stdout


def benchmark_size(
    n_chunks: int,
    *,
    top_k: int,
    repeats: int,
    reuse: bool,
    seed: int,
) -> dict:
    bin_path = BINARY_DIR / f"vectors_{n_chunks}.bin"
    if not reuse or not bin_path.is_file():
        generate_dummy_data(n_chunks, bin_path, QUERY_BIN, seed=seed)
    elif not QUERY_BIN.is_file():
        generate_dummy_data(n_chunks, bin_path, QUERY_BIN, seed=seed)

    # プロセス起動 + mmap のウォームアップ (計測値は C++ 内の 100 iter avg)
    run_engine(bin_path, QUERY_BIN, top_k)

    latencies = []
    last_out = ""
    for _ in range(repeats):
        lat, last_out = run_engine(bin_path, QUERY_BIN, top_k)
        latencies.append(lat)

    avg_lat = float(np.mean(latencies))
    std_lat = float(np.std(latencies)) if len(latencies) > 1 else 0.0
    qps = 1_000_000.0 / avg_lat
    return {
        "chunks": n_chunks,
        "latency_us": avg_lat,
        "latency_std_us": std_lat,
        "qps": qps,
        "repeats": repeats,
        "stdout_sample": last_out,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="PKB C++ vector search benchmark")
    parser.add_argument(
        "--sizes", type=int, nargs="+", default=list(DEFAULT_CHUNK_SIZES),
        help="number of vectors per dataset (default: 10000 100000 500000)",
    )
    parser.add_argument("--top-k", type=int, default=5, help="Top-K passed to search_engine")
    parser.add_argument(
        "--repeats", type=int, default=3,
        help="how many subprocess runs per size (C++ already averages 100 queries each)",
    )
    parser.add_argument("--reuse", action="store_true", help="skip regeneration if .bin exists")
    parser.add_argument("--quick", action="store_true", help="small sizes: 1000 5000 10000")
    parser.add_argument("--seed", type=int, default=42)
    args = parser.parse_args()

    sizes = [1000, 5000, 10_000] if args.quick else args.sizes
    BINARY_DIR.mkdir(parents=True, exist_ok=True)

    print(f"Engine: {ENGINE_PATH}")
    print(f"Output: {BINARY_DIR}")
    print(f"Dim={DIM}, lanes={LANES}, top_k={args.top_k}, repeats={args.repeats}")
    print()
    print(f"{'Chunks':>10} | {'Latency (μs/q)':>16} | {'±σ':>8} | {'QPS':>12}")
    print("-" * 56)

    results = []
    for n in sizes:
        try:
            r = benchmark_size(
                n, top_k=args.top_k, repeats=args.repeats,
                reuse=args.reuse, seed=args.seed,
            )
            results.append(r)
            print(
                f"{r['chunks']:>10,} | {r['latency_us']:>16.2f} | "
                f"{r['latency_std_us']:>8.2f} | {r['qps']:>12,.0f}"
            )
        except Exception as e:
            print(f"{n:>10,} | ERROR: {e}")
            return 1

    if results and args.repeats == 1:
        print()
        print("--- last run detail ---")
        print(results[-1]["stdout_sample"].rstrip())

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
