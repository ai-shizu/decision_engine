#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
core/coupling.py — 時系列直交結合マトリクス: FFT 相互相関 + 遠ラグ帰無 (Target Echo / E2)
==================================================================================
docs/SPEC_ECHO_GENESIS.md §3.3 / §5.3 / §5.10.3 (W-7〜W-14 最終警告)。

数理の核心 (§2 Note 3): 教科書的な t 検定/p 値は自己相関のある系列に対して嘘を
つく (実効標本数の過大評価)。FFT で全ラグの相互相関を一括計算すると、
信号 (|τ| <= MAX_LAG) と帰無分布 (遠ラグ |τ'| ∈ NULL_LAG_RANGE) が
「同じ 1 回の計算から」手に入る。乱数も外部ライブラリも不要な完全決定論の
有意性判定である。

【W-7〜W-14 (SPEC Rev.3 §5.10.3) を全項目遵守】
  W-7  ゼロパディング不足禁止 (M = 2^ceil(log2(2N)) 未満への「効率化」禁止)
  W-8  n, S_xy, S_x, S_y, S_xx, S_yy の 6 配列レシピは 1 本も省略不可
  W-9  n(τ) は np.rint 後に N_MIN 比較 (irfft は 99.99999997 を返す)
  W-10 分散項は max(·, 0.0) でクランプし、両方 > 1e-9 を有意判定の前提に
  W-11 ρ_ij(τ) = corr(y_i(t), y_j(t+τ))。ラグ符号規約は恒等式でテスト固定
  W-12 帰無ラグにも同じ N_MIN ゲート。有効 null ラグ < 50 は unratable
  W-13 デトレンド/季節調整の追加禁止 (遠ラグ帰無が自動吸収する)
  W-14 タイ破りに乱数 jitter を使わない (平均ランクのみ。I-17 違反防止)
"""

from __future__ import annotations

import numpy as np

MAX_LAG = 14
RHO_MIN = 0.15
N_MIN = 100
NULL_LAG_RANGE = (45, 365)
MIN_NULL_SAMPLES = 50          # W-12: これ未満は帰無分布が信頼できず unratable
MAX_PAIRS_REPORTED = 64


class CouplingError(RuntimeError):
    """coupling_matrix() の入力前提違反 (系列長不足など)。"""


# ---------------------------------------------------------------- §3.2 ランク変換
def _rank_transform(x: np.ndarray, mask: np.ndarray) -> np.ndarray:
    """有効値のみを対象に平均ランク (タイは平均) を付与し、[-1,1] へ線形写像する。
    欠測は 0 (I-18)。単発の外れ値 (突発的な大支出等) が Pearson を支配するのを
    防ぐための Spearman 化。argsort(kind="stable") のみを用い、乱数によるタイ
    破り (W-14) は行わない — 決定論性 (I-17) の要求。"""
    out = np.zeros(x.shape[0], dtype=np.float64)
    valid_idx = np.flatnonzero(mask)
    n = valid_idx.size
    if n < 2:
        return out   # 有効値 1 点以下ではランクが定義できない (分散 0 として扱われる)

    vals = x[valid_idx]
    order = np.argsort(vals, kind="stable")
    ranks = np.empty(n, dtype=np.float64)
    ranks[order] = np.arange(n, dtype=np.float64)

    sorted_vals = vals[order]
    i = 0
    while i < n:
        j = i
        while j + 1 < n and sorted_vals[j + 1] == sorted_vals[i]:
            j += 1
        if j > i:
            avg = ranks[order[i:j + 1]].mean()
            ranks[order[i:j + 1]] = avg
        i = j + 1

    mapped = (ranks / (n - 1)) * 2.0 - 1.0
    out[valid_idx] = mapped
    return out


# ---------------------------------------------------------------- FFT 相互相関
def _circular_xcorr(A: np.ndarray, B: np.ndarray, m: int) -> np.ndarray:
    """(a*b)[tau] = sum_t a[t]*b[t+tau] (circular)。
    = irfft(conj(rfft(a)) * rfft(b))。A,B は事前計算済みの rfft 結果。
    tau は [0, M) の実数配列として返る。負ラグは呼び出し側が負インデックスで
    参照する (numpy の負インデックスがそのまま M+tau を指すため一致する —
    W-11 のラグ符号規約はこの一点に懸かっている)。"""
    return np.fft.irfft(np.conj(A) * B, n=m)


def coupling_matrix(values: np.ndarray, mask: np.ndarray,
                    max_lag: int = MAX_LAG) -> dict:
    """時系列直交結合マトリクス C[i,j,tau] を計算する (SPEC §3.3)。

    values: (N, K) f32/f64。mask: (N, K) bool。K レーンの全ペア (i<j のみ —
    対称性 rho_ij(tau)==rho_ji(-tau) により半分で足りる。W-11) について、
    信号域 |tau|<=max_lag の中から採択条件を満たす最良のラグを報告する。
    採択されない (n_eff 不足で全ラグ unratable な) ペアは省略する。
    """
    values = np.asarray(values, dtype=np.float64)
    mask = np.asarray(mask, dtype=bool)
    if values.shape != mask.shape or values.ndim != 2:
        raise CouplingError(f"values/mask の shape 不一致または非2次元: "
                           f"{values.shape} vs {mask.shape}")
    n_rows, k_lanes = values.shape
    null_lo, null_hi = NULL_LAG_RANGE
    # W-7: ゼロパディングが遠ラグ帰無レンジより短いと、遠ラグ自体が信号域との
    # circular wrap-around を起こし「去年の年末と今年の年始の偽結合」を生む。
    if n_rows <= 2 * null_hi:
        raise CouplingError(
            f"系列長 n_rows={n_rows} は遠ラグ帰無 (|tau|<={null_hi}) を安全に "
            f"取るには短すぎる (最低 {2 * null_hi + 1} 行必要)")

    m = 1 << int(np.ceil(np.log2(2 * n_rows)))   # W-7: 2N 未満への切り詰め禁止

    # 各レーンにつき 3 本の rfft を前計算 (§3.3)
    Y = np.zeros((k_lanes, m // 2 + 1), dtype=np.complex128)
    Mh = np.zeros((k_lanes, m // 2 + 1), dtype=np.complex128)
    Q = np.zeros((k_lanes, m // 2 + 1), dtype=np.complex128)
    for k in range(k_lanes):
        y = _rank_transform(values[:, k], mask[:, k])
        mm = mask[:, k].astype(np.float64)
        Y[k] = np.fft.rfft(y, n=m)
        Mh[k] = np.fft.rfft(mm, n=m)
        Q[k] = np.fft.rfft(y * y, n=m)

    all_lags = np.arange(-max_lag, max_lag + 1)
    null_lags = np.concatenate([
        np.arange(-null_hi, -null_lo + 1),
        np.arange(null_lo, null_hi + 1),
    ])

    def rho_at(S_xy, S_x, S_y, S_xx, S_yy, n_arr, tau: int):
        # W-9: irfft の n(tau) は数学的には整数だが浮動小数の丸め誤差
        # (99.99999997 等) を持つ。N_MIN 比較の前に必ず np.rint する。
        n_round = float(np.rint(n_arr[tau]))
        if n_round < N_MIN:
            return None
        # W-8: この 4 項 (S_x, S_y, S_xx, S_yy) は「グローバル中心化したから
        # 不要」という簡略化を絶対に許さない。省略すると共有欠測パターンが
        # そのまま正の相関として析出する (E1.1 と同じ機序のアーティファクト)。
        var_x = max(n_round * S_xx[tau] - S_x[tau] ** 2, 0.0)   # W-10: 微小負のクランプ
        var_y = max(n_round * S_yy[tau] - S_y[tau] ** 2, 0.0)
        if var_x <= 1e-9 or var_y <= 1e-9:
            # W-10: 0 除算の回避策ではなく「この pair はどちらかの分散が
            # 実質ゼロで評価不能」という情報量の判定そのもの。
            return None
        num = n_round * S_xy[tau] - S_x[tau] * S_y[tau]
        rho = float(np.clip(num / np.sqrt(var_x * var_y), -1.0, 1.0))
        return rho, int(n_round)

    pairs: list[dict] = []
    for i in range(k_lanes):
        for j in range(i + 1, k_lanes):   # 対称性により i<j のみ (W-11)
            S_xy = _circular_xcorr(Y[i], Y[j], m)
            S_x = _circular_xcorr(Y[i], Mh[j], m)
            S_y = _circular_xcorr(Mh[i], Y[j], m)
            S_xx = _circular_xcorr(Q[i], Mh[j], m)
            S_yy = _circular_xcorr(Mh[i], Q[j], m)
            n_arr = _circular_xcorr(Mh[i], Mh[j], m)

            candidates = []
            for tau in all_lags:
                res = rho_at(S_xy, S_x, S_y, S_xx, S_yy, n_arr, int(tau))
                if res is not None:
                    candidates.append((int(tau), res[0], res[1]))
            if not candidates:
                continue   # 全ラグで n_eff 不足 — このペアは記録しない

            # W-12: 帰無ラグにも同一ゲート (N_MIN) を適用する。マスクにより
            # 遠ラグほど n が痩せるため、小標本の高分散 |rho| を帰無に混ぜると
            # q99 が膨張し検出力が死ぬ。
            null_abs = []
            for tau in null_lags:
                res = rho_at(S_xy, S_x, S_y, S_xx, S_yy, n_arr, int(tau))
                if res is not None:
                    null_abs.append(abs(res[0]))
            null_insufficient = len(null_abs) < MIN_NULL_SAMPLES
            null_q99 = (float(np.quantile(null_abs, 0.99, method="linear"))
                       if null_abs else None)

            # 信号域の中から |rho| 最大の候補を選ぶ。all_lags は固定順序の
            # 配列なのでタイの場合も max() は決定論的に先頭要素を返す (I-17)。
            best_tau, best_rho, best_n = max(candidates, key=lambda c: abs(c[1]))

            sig = bool(
                not null_insufficient and null_q99 is not None
                and abs(best_rho) > null_q99
                and abs(best_rho) >= RHO_MIN
                and best_n >= N_MIN
            )
            pairs.append({
                "src": i, "dst": j, "lag": best_tau,
                "rho": round(best_rho, 6), "n_eff": best_n,
                "null_q99": round(null_q99, 6) if null_q99 is not None else None,
                "sig": sig,
                "reason": "null_insufficient" if null_insufficient else None,
            })

    pairs.sort(key=lambda p: -abs(p["rho"]))
    return {"pairs": pairs[:MAX_PAIRS_REPORTED], "n_rows": n_rows,
           "max_lag": max_lag, "n_lanes": k_lanes}
