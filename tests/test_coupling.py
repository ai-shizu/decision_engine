# -*- coding: utf-8 -*-
"""Target Echo E2: core/coupling.py の決定論的テスト (docs/SPEC_ECHO_GENESIS.md §3.3)。

外部依存なし (stdlib + numpy のみ、LLM 不使用、乱数不使用)。合成データは
np.random を一切使わず、決定論的な式 (sin/cos + 疑似ノイズ式) のみで生成する
(I-17 の精神をテストデータ生成にも適用し、再現性の議論から乱数由来の
不確実性を排除する)。

E2 ゲート (SPEC §5.9 + Rev.3 §5.10.3 の追加要求):
  1. 既知ラグの注入信号 → 検出 (合成データ)
  2. 独立系列 → sig ゼロ (帰無の対照群)
  3. 決定論 (2 回実行のビット同一)
  4. W-8: 共有欠測パターンを持つ独立レーンが sig にならない (合成対照群)
  5. W-11: ラグ符号の恒等式 rho_ij(tau) == rho_ji(-tau)
  6. 系列長不足 (遠ラグ帰無が安全に取れない) は CouplingError
"""
from __future__ import annotations

import sys
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core import coupling as cp  # noqa: E402

N = 800   # NULL_LAG_RANGE (45,365) を安全に取れる最小要件 (>2*365) を満たす長さ


def _wave(n: int, phase_offset: float = 0.0) -> np.ndarray:
    """決定論的な合成信号 (低周波成分 + 疑似ノイズ)。np.random は使わない。"""
    t = np.arange(n, dtype=np.float64)
    base = np.sin(2 * np.pi * t / 37) + 0.5 * np.sin(2 * np.pi * t / 11 + phase_offset)
    pseudo_noise = 0.15 * np.sin((t * 12.9898 + phase_offset * 78.233))
    return base + pseudo_noise


# ============================================================ 1. 既知ラグの注入検出
def test_known_lag_injection_detected() -> None:
    lag_true = 5
    lane_i = _wave(N)
    lane_j = np.roll(lane_i, lag_true)   # lane_j(t) = lane_i(t - lag_true)
    values = np.stack([lane_i, lane_j], axis=1)
    mask = np.ones((N, 2), dtype=bool)

    result = cp.coupling_matrix(values, mask, max_lag=cp.MAX_LAG)
    assert len(result["pairs"]) == 1
    pair = result["pairs"][0]
    assert pair["src"] == 0 and pair["dst"] == 1
    assert pair["lag"] == lag_true, pair
    assert pair["sig"], pair
    assert abs(pair["rho"]) >= cp.RHO_MIN
    print("  known-lag injection detected OK")


# ============================================================ 2. 独立系列 → 帰無対照群
def test_independent_series_no_significant_coupling() -> None:
    lane_i = _wave(N, phase_offset=0.0)
    lane_j = _wave(N, phase_offset=91.7)   # 別の位相・周波数成分 (無関係な系列)
    values = np.stack([lane_i, lane_j], axis=1)
    mask = np.ones((N, 2), dtype=bool)

    result = cp.coupling_matrix(values, mask, max_lag=cp.MAX_LAG)
    sig_pairs = [p for p in result["pairs"] if p["sig"]]
    assert not sig_pairs, f"無関係な系列が有意結合として検出された: {sig_pairs}"
    print("  independent series -> no significant coupling OK")


# ============================================================ 3. 決定論性
def test_deterministic_repeated_calls() -> None:
    lane_i = _wave(N)
    lane_j = np.roll(_wave(N, phase_offset=3.3), 2)
    values = np.stack([lane_i, lane_j], axis=1)
    mask = np.ones((N, 2), dtype=bool)

    r1 = cp.coupling_matrix(values, mask)
    r2 = cp.coupling_matrix(values, mask)
    assert r1 == r2, "同一入力の2回実行で結果が一致しない (I-17 違反)"
    print("  deterministic repeated calls OK")


# ============================================================ 4. W-8: 共有欠測パターンの対照群
def test_shared_missingness_no_false_positive() -> None:
    """相関の無い2レーンが「同じ曜日パターンで欠測」を共有していても、
    n/S_x/S_y/S_xx/S_yy の6配列レシピが正しく効いていれば sig にならない。
    ここを省略すると共有欠測パターンがそのまま正の相関として析出する
    (E1.1 と同じ機序のアーティファクト — SPEC Rev.3 §5.10.3 W-8)。"""
    lane_i = _wave(N, phase_offset=17.0)
    lane_j = _wave(N, phase_offset=203.5)   # lane_i とは無関係

    shared_mask = np.ones(N, dtype=bool)
    shared_mask[::7] = False    # 週次の欠測パターンを両レーンで共有させる
    mask = np.stack([shared_mask, shared_mask], axis=1)
    values = np.stack([lane_i, lane_j], axis=1)

    result = cp.coupling_matrix(values, mask, max_lag=cp.MAX_LAG)
    sig_pairs = [p for p in result["pairs"] if p["sig"]]
    assert not sig_pairs, \
        f"共有欠測パターンが偽の有意結合として検出された (W-8 違反): {sig_pairs}"
    print("  shared-missingness control group (W-8) OK")


# ============================================================ 5. W-11: ラグ符号の恒等式
def test_lag_sign_identity() -> None:
    lag_true = 4
    lane_i = _wave(N, phase_offset=5.5)
    lane_j = np.roll(lane_i, lag_true)
    mask = np.ones((N, 2), dtype=bool)

    values_ij = np.stack([lane_i, lane_j], axis=1)
    result_ij = cp.coupling_matrix(values_ij, mask, max_lag=cp.MAX_LAG)
    pair_ij = result_ij["pairs"][0]

    values_ji = np.stack([lane_j, lane_i], axis=1)   # 列を入れ替え (src/dst 反転)
    result_ji = cp.coupling_matrix(values_ji, mask, max_lag=cp.MAX_LAG)
    pair_ji = result_ji["pairs"][0]

    assert pair_ji["lag"] == -pair_ij["lag"], (pair_ij, pair_ji)
    assert abs(pair_ji["rho"] - pair_ij["rho"]) < 1e-6, (pair_ij, pair_ji)
    print("  lag-sign identity rho_ij(tau) == rho_ji(-tau) (W-11) OK")


# ============================================================ 6. 系列長不足のガード
def test_short_series_raises_coupling_error() -> None:
    short_n = 400   # <= 2 * NULL_LAG_RANGE[1] (730) — 遠ラグ帰無が安全に取れない
    values = np.zeros((short_n, 2))
    mask = np.ones((short_n, 2), dtype=bool)
    try:
        cp.coupling_matrix(values, mask)
        raise AssertionError("系列長不足なのに CouplingError が発生しなかった")
    except cp.CouplingError:
        pass
    print("  short series -> CouplingError OK")


if __name__ == "__main__":
    test_known_lag_injection_detected()
    test_independent_series_no_significant_coupling()
    test_deterministic_repeated_calls()
    test_shared_missingness_no_false_positive()
    test_lag_sign_identity()
    test_short_series_raises_coupling_error()
    print("test_coupling: ALL PASS")
