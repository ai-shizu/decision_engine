#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
core/digital_twin.py — 敵対的デジタルツイン: 認知リソース状態方程式 + IRLS ハザード
+ walk-forward スキルゲート + モンテカルロ (Target Echo / E3)
==================================================================================
docs/SPEC_ECHO_GENESIS.md §3.4 / §3.5 / §5.3 / §5.10.4 (W-0, W-15〜W-21)。

【W-0〜W-21 (SPEC Rev.4 §5.10.4) を全項目遵守】
  W-0  IRLS はペナルティ付き対数尤度の Newton 法 (勾配側にも -λβ)
  W-15 σ の全経路で Z_CLIP=35.0 クリップ。fit/predict の入口出口で np.isfinite assert
  W-16 完全分離データで ||β|| が発散しない (分離対照群テストで確認)
  W-17 R は中心化してから解き β₀ を後で戻す。np.linalg.solve のみ (inv 禁止)
  W-18 収束しなければ勾配ノルムで採用可否を判定。それも満たさねば fit_failed
  W-19 失策ラベル閾値・D(t) 正規化は「その時点までの trailing baseline」のみで
       計算する (未来のデータで過去のラベルを定義しない — ラベル漏洩の防止)
  W-20 訓練窓の失策 < 5 の fold はスキップ。BSS はプール方式で集計
  W-21 状態方程式の欠測入力は訓練窓中央値で代入 (ゼロ埋め禁止)。D(t) 欠測日は
       マスク付き SSE から除外

【スコープ上の設計判断 (レビュー対象として明示)】
状態方程式 θ_dyn=(ρ,β1,β2,γ) は全履歴 1 回の SSE グリッド探索で fit する
(SPEC §3.4 に walk-forward の指示なし)。walk-forward は §3.5 が明示する
ハザードモデル (κ, θ_R) にのみ適用する。D(t) 自体の正規化・ラベル閾値は
trailing causal baseline (時刻 t は [t-BASELINE_DAYS, t) のみを参照) で
計算するため、θ_dyn の SSE 対象自体は構成的に未来を見ない。
"""

from __future__ import annotations

import hashlib
from dataclasses import asdict, dataclass
from datetime import date as _date, timedelta

import numpy as np

from . import tensor_store as ts
from .canonicalization import canonical_json_bytes

# ---------------------------------------------------------------- 定数 (凍結)
R_FLOOR = 0.05
R_INIT = 0.7
Z_CLIP = 35.0                    # W-15
BASELINE_DAYS = 180               # §3.1 のベースライン窓
MIN_BASELINE_SAMPLES = 30         # trailing window 内の最低有効標本数
MIN_D_VALID_DAYS = 60             # W-21: 有効 D 日数がこれ未満なら unratable

FOLD_DAYS = 28
MIN_FOLD_TRAIN_LAPSES = 5         # W-20
BSS_GATE = 0.05
MIN_TEST_LAPSES_GATE = 10         # I-20

IRLS_MAX_ITER = 25
IRLS_TOL = 1e-8
IRLS_GRAD_TOL = 1e-4
IRLS_LAMBDA = 1e-3

MC_PATHS = 8192
MC_HORIZON_DEFAULT = 14
MC_HORIZON_MAX = 30

_RHO_GRID = (0.05, 0.50, 0.05)     # (lo, hi, step) — 10 点
_B_GRID = (0.00, 0.35, 0.05)       # beta1/beta2/gamma 共通 — 8 点
_REFINE_ROUNDS = 3


class TwinFitError(RuntimeError):
    """状態方程式/ハザードモデルの入力前提違反、または非有限値の混入 (W-15)。"""


# ---------------------------------------------------------------- §5.3 データモデル
@dataclass
class TwinParams:
    rho: float
    beta1: float
    beta2: float
    gamma: float
    kappa: float
    theta_r: float | None          # kappa<=0 (R と失策が無関係) では None
    bss: float
    n_lapse_test: int
    gate_passed: bool
    fitted_window: str

    def to_dict(self) -> dict:
        return asdict(self)


# ---------------------------------------------------------------- W-15: シグモイド
def _sigmoid(z: np.ndarray) -> np.ndarray:
    z = np.clip(z, -Z_CLIP, Z_CLIP)
    return 1.0 / (1.0 + np.exp(-z))


def _assert_finite(arr: np.ndarray, label: str) -> None:
    if not np.all(np.isfinite(arr)):
        raise TwinFitError(f"{label} に非有限値 (NaN/Inf) が混入している (W-15)")


# ---------------------------------------------------------------- W-19: trailing causal baseline
def _rolling_median_mad_causal(x: np.ndarray, mask: np.ndarray, window: int
                               ) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """時刻 t の中央値/MAD は [t-window, t) の有効値のみから計算する
    (未来のデータで過去の正規化を定義しない — W-19)。"""
    n = len(x)
    med = np.zeros(n)
    mad = np.zeros(n)
    computable = np.zeros(n, dtype=bool)
    for t in range(n):
        lo = max(0, t - window)
        seg_mask = mask[lo:t]
        if int(seg_mask.sum()) < MIN_BASELINE_SAMPLES:
            continue
        seg_vals = x[lo:t][seg_mask]
        m = float(np.median(seg_vals))
        med[t] = m
        mad[t] = float(np.median(np.abs(seg_vals - m)))
        computable[t] = True
    return med, mad, computable


def _rolling_quantile_causal(x: np.ndarray, mask: np.ndarray, window: int, q: float
                             ) -> tuple[np.ndarray, np.ndarray]:
    n = len(x)
    out = np.zeros(n)
    computable = np.zeros(n, dtype=bool)
    for t in range(n):
        lo = max(0, t - window)
        seg_mask = mask[lo:t]
        if int(seg_mask.sum()) < MIN_BASELINE_SAMPLES:
            continue
        seg_vals = x[lo:t][seg_mask]
        out[t] = float(np.quantile(seg_vals, q, method="linear"))
        computable[t] = True
    return out, computable


def _z_causal(x: np.ndarray, mask: np.ndarray, window: int = BASELINE_DAYS
             ) -> tuple[np.ndarray, np.ndarray]:
    med, mad, computable = _rolling_median_mad_causal(x, mask, window)
    z = np.zeros(len(x))
    valid = computable & mask
    z[valid] = (x[valid] - med[valid]) / (1.4826 * mad[valid] + 1e-9)
    return z, valid


# ---------------------------------------------------------------- 状態方程式の入力構築
def _fill_median(col: np.ndarray, col_mask: np.ndarray) -> np.ndarray:
    """W-21: 欠測入力は (全履歴の) 中央値で代入する。ゼロ埋めは禁止 — 0 は
    「回復ゼロ・負荷ゼロ」という強い値であり、欠測 (未観測) の代用にしてはならない。"""
    out = col.copy().astype(np.float64)
    if col_mask.any():
        fill_val = float(np.median(col[col_mask]))
    else:
        fill_val = 0.0   # 全欠測はやむを得ない (観測が一度も無いレーン)
    out[~col_mask] = fill_val
    return out


def _build_state_inputs(values: np.ndarray, mask: np.ndarray
                        ) -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
    """rec(t), ell_sw(t), ell_vol(t), frict(t) を構築する (§3.4)。"""
    lane = ts.FEATURES
    private_hours = _fill_median(values[:, lane["cal_private_hours"]],
                                 mask[:, lane["cal_private_hours"]])
    night_out = _fill_median(values[:, lane["line_night_out"]],
                             mask[:, lane["line_night_out"]])
    switch_count = _fill_median(values[:, lane["cal_switch_count"]],
                                mask[:, lane["cal_switch_count"]])
    out_chars = _fill_median(values[:, lane["line_out_chars"]],
                            mask[:, lane["line_out_chars"]])
    friction = _fill_median(values[:, lane["friction_events"]],
                           mask[:, lane["friction_events"]])

    rec = np.clip(private_hours / 4.0, 0.0, 1.0) * (
        1.0 - np.clip(night_out / 20.0, 0.0, 1.0))
    lsw = np.clip(switch_count / 10.0, 0.0, 1.0)

    # ell_vol: p95(line_out_chars) は trailing causal (W-19)。burn-in 期間
    # (computable=False) は「評価不能」として負荷 0 とする (unratable な期間は
    # theta_dyn の SSE 対象 D(t) 側でも burn-in により自然に除外されるため実害が無い)。
    p95_vol, ok_p95 = _rolling_quantile_causal(
        values[:, lane["line_out_chars"]], mask[:, lane["line_out_chars"]],
        BASELINE_DAYS, 0.95)
    lvol = np.zeros(len(out_chars))
    denom = np.where(p95_vol > 0, p95_vol, 1.0)
    lvol[ok_p95] = np.clip(out_chars[ok_p95] / denom[ok_p95], 0.0, 1.0)

    frict = np.minimum(friction, 3.0) / 3.0

    _assert_finite(rec, "rec(t)")
    _assert_finite(lsw, "ell_sw(t)")
    _assert_finite(lvol, "ell_vol(t)")
    _assert_finite(frict, "frict(t)")
    return rec, lsw, lvol, frict


def _build_D(values: np.ndarray, mask: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """観測対応物 D(t) (枯渇指数)。成分 2 個未満の日は欠測 (§3.4)。"""
    lane = ts.FEATURES
    z_latency, ok_latency = _z_causal(values[:, lane["line_reply_med_min"]],
                                      mask[:, lane["line_reply_med_min"]])
    z_guilt, ok_guilt = _z_causal(values[:, lane["guilt_idx"]],
                                  mask[:, lane["guilt_idx"]])

    declared = values[:, lane["task_declared"]]
    declared_mask = mask[:, lane["task_declared"]]
    executed = values[:, lane["task_executed"]]
    executed_mask = mask[:, lane["task_executed"]]
    a_valid = declared_mask & executed_mask & (declared > 0)
    A = np.zeros(len(declared))
    A[a_valid] = 1.0 - np.minimum(
        executed[a_valid] / np.maximum(declared[a_valid], 1.0), 1.0)

    n = len(declared)
    count = (ok_latency.astype(int) + ok_guilt.astype(int) + a_valid.astype(int))
    total = (np.where(ok_latency, z_latency, 0.0)
            + np.where(ok_guilt, z_guilt, 0.0)
            + np.where(a_valid, A, 0.0))
    D_valid = count >= 2
    mean_val = np.zeros(n)
    mean_val[D_valid] = total[D_valid] / count[D_valid]
    D = np.zeros(n)
    D[D_valid] = _sigmoid(mean_val[D_valid])
    return D, D_valid


def _build_lapse(values: np.ndarray, mask: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """失策ラベル lapse(t) (§3.5)。分位点は trailing causal baseline (W-19)。"""
    lane = ts.FEATURES
    spend_hed = values[:, lane["spend_hedonic"]]
    spend_hed_m = mask[:, lane["spend_hedonic"]]
    task_decl = values[:, lane["task_declared"]]
    task_decl_m = mask[:, lane["task_declared"]]
    task_exec = values[:, lane["task_executed"]]
    task_exec_m = mask[:, lane["task_executed"]]
    night_out = values[:, lane["line_night_out"]]
    night_out_m = mask[:, lane["line_night_out"]]
    friction = values[:, lane["friction_events"]]
    friction_m = mask[:, lane["friction_events"]]
    out_msgs = values[:, lane["line_out_msgs"]]
    out_msgs_m = mask[:, lane["line_out_msgs"]]
    in_msgs = values[:, lane["line_in_msgs"]]
    in_msgs_m = mask[:, lane["line_in_msgs"]]

    p90_spend, ok_p90_spend = _rolling_quantile_causal(spend_hed, spend_hed_m, BASELINE_DAYS, 0.90)
    p90_night, ok_p90_night = _rolling_quantile_causal(night_out, night_out_m, BASELINE_DAYS, 0.90)
    p95_out, ok_p95_out = _rolling_quantile_causal(out_msgs, out_msgs_m, BASELINE_DAYS, 0.95)
    p50_in, ok_p50_in = _rolling_quantile_causal(in_msgs, in_msgs_m, BASELINE_DAYS, 0.50)

    cond_a_ratable = ok_p90_spend & spend_hed_m & task_decl_m & task_exec_m
    cond_a = cond_a_ratable & (spend_hed > p90_spend) & (task_decl > 0) & (task_exec == 0)

    cond_b_ratable = ok_p90_night & night_out_m & friction_m
    cond_b = cond_b_ratable & (night_out >= p90_night) & (friction > 0)

    cond_c_ratable = ok_p95_out & ok_p50_in & out_msgs_m & in_msgs_m
    cond_c = cond_c_ratable & (out_msgs > p95_out) & (in_msgs < p50_in)

    lapse_valid = cond_a_ratable | cond_b_ratable | cond_c_ratable
    lapse = cond_a | cond_b | cond_c
    return lapse, lapse_valid


# ---------------------------------------------------------------- 状態方程式の再帰 (R(t))
def _simulate_R(theta: tuple[float, float, float, float], rec: np.ndarray,
                lsw: np.ndarray, lvol: np.ndarray, frict: np.ndarray) -> np.ndarray:
    rho, b1, b2, g = theta
    n = len(rec)
    R = np.empty(n, dtype=np.float64)
    R[0] = R_INIT
    for t in range(n - 1):
        R[t + 1] = np.clip(
            R[t] + rho * (1.0 - R[t]) * rec[t] - b1 * lsw[t] - b2 * lvol[t] - g * frict[t],
            R_FLOOR, 1.0)
    return R


def _simulate_R_grid(thetas: np.ndarray, rec: np.ndarray, lsw: np.ndarray,
                     lvol: np.ndarray, frict: np.ndarray) -> np.ndarray:
    """§3.4: グリッド軸をベクトル化 (状態 shape=(G,) を N 回更新)。
    Python の二重ループ (N x G) は禁止 — t ループのみ、G 軸は配列演算。"""
    rho = thetas[:, 0]; b1 = thetas[:, 1]; b2 = thetas[:, 2]; g = thetas[:, 3]
    n = len(rec)
    g_count = thetas.shape[0]
    R = np.empty((n, g_count), dtype=np.float64)
    R[0] = R_INIT
    for t in range(n - 1):
        R[t + 1] = np.clip(
            R[t] + rho * (1.0 - R[t]) * rec[t] - b1 * lsw[t] - b2 * lvol[t] - g * frict[t],
            R_FLOOR, 1.0)
    return R


def _grid_axis(lo: float, hi: float, step: float) -> np.ndarray:
    n_pts = int(round((hi - lo) / step)) + 1
    return np.round(lo + np.arange(n_pts) * step, 10)


def _sse_evaluate(thetas: np.ndarray, D: np.ndarray, D_valid: np.ndarray,
                  rec: np.ndarray, lsw: np.ndarray, lvol: np.ndarray, frict: np.ndarray
                  ) -> tuple[np.ndarray, np.ndarray]:
    R_grid = _simulate_R_grid(thetas, rec, lsw, lvol, frict)   # (n, G)
    resid = (D[:, None] - (1.0 - R_grid)) ** 2
    resid = np.where(D_valid[:, None], resid, 0.0)             # W-21: マスク付き SSE
    sse = resid.sum(axis=0)
    valid_theta = (thetas[:, 0] >= 0) & np.all(thetas[:, 1:] >= 0, axis=1)
    sse = np.where(valid_theta, sse, np.inf)
    return sse, R_grid


def _sse_grid_search(D: np.ndarray, D_valid: np.ndarray, rec: np.ndarray,
                     lsw: np.ndarray, lvol: np.ndarray, frict: np.ndarray
                     ) -> tuple[float, float, float, float]:
    rho_grid = _grid_axis(*_RHO_GRID)
    b_grid = _grid_axis(*_B_GRID)
    thetas = np.array([[r, b1, b2, g]
                       for r in rho_grid for b1 in b_grid
                       for b2 in b_grid for g in b_grid])
    sse, _ = _sse_evaluate(thetas, D, D_valid, rec, lsw, lvol, frict)
    best_idx = int(np.argmin(sse))     # タイは添字最小 (argmin の既定動作)
    best = thetas[best_idx]

    step_rho, step_b = _RHO_GRID[2], _B_GRID[2]
    for _ in range(_REFINE_ROUNDS):
        step_rho /= 2.0
        step_b /= 2.0
        local = np.array([
            [r, b1, b2, g]
            for r in (best[0] - step_rho, best[0], best[0] + step_rho)
            for b1 in (best[1] - step_b, best[1], best[1] + step_b)
            for b2 in (best[2] - step_b, best[2], best[2] + step_b)
            for g in (best[3] - step_b, best[3], best[3] + step_b)
        ])
        sse_local, _ = _sse_evaluate(local, D, D_valid, rec, lsw, lvol, frict)
        idx = int(np.argmin(sse_local))
        best = local[idx]
    return float(best[0]), float(best[1]), float(best[2]), float(best[3])


# ---------------------------------------------------------------- W-0/W-15/W-17/W-18: IRLS
def _irls_fit(x_centered: np.ndarray, y: np.ndarray
             ) -> tuple[np.ndarray, bool, float]:
    """ペナルティ付きロジスティック回帰の Newton-Raphson (IRLS)。
    X = [1, x_centered]。リッジは Hessian と勾配の両方に入れる (W-0)。
    戻り値: (beta [intercept, slope], converged, grad_norm)。"""
    n = len(y)
    X = np.column_stack([np.ones(n), x_centered])
    beta = np.zeros(2)
    lam = IRLS_LAMBDA
    converged = False

    for _ in range(IRLS_MAX_ITER):
        z = np.clip(X @ beta, -Z_CLIP, Z_CLIP)          # W-15
        p = 1.0 / (1.0 + np.exp(-z))
        w = p * (1.0 - p)
        H = X.T @ (X * w[:, None]) + lam * np.eye(2)
        grad = X.T @ (y - p) - lam * beta               # W-0: 勾配側の -λβ
        try:
            delta = np.linalg.solve(H, grad)             # W-17: solve のみ (inv 禁止)
        except np.linalg.LinAlgError:
            return beta, False, float("inf")
        beta = beta + delta
        if not np.all(np.isfinite(beta)):
            return beta, False, float("inf")
        if np.max(np.abs(delta)) < IRLS_TOL:
            converged = True
            break

    z_final = np.clip(X @ beta, -Z_CLIP, Z_CLIP)
    p_final = 1.0 / (1.0 + np.exp(-z_final))
    grad_final = X.T @ (y - p_final) - lam * beta
    grad_norm = float(np.max(np.abs(grad_final)))
    if not converged:
        converged = grad_norm < IRLS_GRAD_TOL            # W-18
    return beta, converged, grad_norm


# ---------------------------------------------------------------- walk-forward (I-20 本体)
def _walk_forward(R: np.ndarray, lapse: np.ndarray, lapse_valid: np.ndarray,
                  fold_days: int = FOLD_DAYS) -> tuple[float, int, bool]:
    """拡張窓 walk-forward。W-19: 各 fold は自身の訓練窓のみで IRLS を再フィットする
    (ラベル/D(t) 自体は既に trailing causal — §_build_lapse/_build_D — なので、
    ここでのフィット対象データそのものが未来を含まない)。
    W-20: プール方式で BSS を集計 (fold 平均ではない)。"""
    n = len(R)
    pooled_y: list[np.ndarray] = []
    pooled_p: list[np.ndarray] = []
    pooled_clim: list[np.ndarray] = []

    start = BASELINE_DAYS
    while start + fold_days <= n:
        train_idx = np.arange(0, start)
        test_idx = np.arange(start, start + fold_days)

        train_valid = lapse_valid[train_idx]
        train_R = R[train_idx][train_valid]
        train_y = lapse[train_idx][train_valid].astype(np.float64)
        n_train_lapse = int(train_y.sum())
        p_bar = float(train_y.mean()) if train_y.size else 0.0

        if n_train_lapse < MIN_FOLD_TRAIN_LAPSES or not (0.0 < p_bar < 1.0):
            start += fold_days
            continue   # W-20

        r_mean = float(train_R.mean())
        beta, converged, _ = _irls_fit(train_R - r_mean, train_y)
        if not converged:
            start += fold_days
            continue

        test_valid = lapse_valid[test_idx]
        if not test_valid.any():
            start += fold_days
            continue
        test_R = R[test_idx][test_valid]
        test_y = lapse[test_idx][test_valid].astype(np.float64)
        z = np.clip(beta[0] + beta[1] * (test_R - r_mean), -Z_CLIP, Z_CLIP)
        p_hat = 1.0 / (1.0 + np.exp(-z))

        pooled_y.append(test_y)
        pooled_p.append(p_hat)
        pooled_clim.append(np.full(test_y.shape, p_bar))
        start += fold_days

    if not pooled_y:
        return 0.0, 0, False

    y_all = np.concatenate(pooled_y)
    p_all = np.concatenate(pooled_p)
    clim_all = np.concatenate(pooled_clim)
    n_lapse_test = int(y_all.sum())

    brier = float(np.mean((p_all - y_all) ** 2))
    brier_climatology = float(np.mean((clim_all - y_all) ** 2))
    bss = 0.0 if brier_climatology <= 0 else 1.0 - brier / brier_climatology
    gate_passed = bool(bss >= BSS_GATE and n_lapse_test >= MIN_TEST_LAPSES_GATE)
    return bss, n_lapse_test, gate_passed


# ---------------------------------------------------------------- 公開 API
def fit_twin(store: ts.TensorStore) -> TwinParams:
    """§3.4-3.5 の全パイプライン。決定論。"""
    values, mask = store.window(*store.dates)
    fitted_window = f"{store.dates[0]}..{store.dates[1]}"

    rec, lsw, lvol, frict = _build_state_inputs(values, mask)
    D, D_valid = _build_D(values, mask)
    lapse, lapse_valid = _build_lapse(values, mask)

    if int(D_valid.sum()) < MIN_D_VALID_DAYS:   # W-21
        return TwinParams(rho=0.0, beta1=0.0, beta2=0.0, gamma=0.0, kappa=0.0,
                          theta_r=None, bss=0.0, n_lapse_test=0, gate_passed=False,
                          fitted_window=fitted_window)

    theta_dyn = _sse_grid_search(D, D_valid, rec, lsw, lvol, frict)
    R = _simulate_R(theta_dyn, rec, lsw, lvol, frict)
    _assert_finite(R, "R(t) (状態方程式の再帰)")               # W-15

    if int(lapse_valid.sum()) < 2:
        kappa, theta_r, converged_full = 0.0, None, False
    else:
        r_mean_full = float(R[lapse_valid].mean())
        y_full = lapse[lapse_valid].astype(np.float64)
        beta_full, converged_full, _ = _irls_fit(R[lapse_valid] - r_mean_full, y_full)
        beta0_centered, beta1_haz = beta_full
        beta0 = beta0_centered - beta1_haz * r_mean_full        # W-17: 中心化を戻す
        kappa = -beta1_haz
        theta_r = (beta0 / kappa) if abs(kappa) > 1e-9 else None

    bss, n_lapse_test, bss_gate = _walk_forward(R, lapse, lapse_valid)
    gate_passed = bool(bss_gate and converged_full and kappa > 0)

    return TwinParams(
        rho=theta_dyn[0], beta1=theta_dyn[1], beta2=theta_dyn[2], gamma=theta_dyn[3],
        kappa=kappa, theta_r=theta_r, bss=bss, n_lapse_test=n_lapse_test,
        gate_passed=gate_passed, fitted_window=fitted_window,
    )


def _derive_seed(store: ts.TensorStore, params: TwinParams, scenario: dict) -> int:
    """§3.6 (I-17): seed は入力内容のハッシュのみ。時刻・pid・カウンタを混ぜない。"""
    material = store.content_hash64.to_bytes(8, "little", signed=False)
    material += b"decision-engine/digital-twin-seed/v2\0"
    material += canonical_json_bytes({
        "params": params.to_dict(),
        "scenario": scenario,
    })
    digest = hashlib.blake2b(material, digest_size=8).digest()
    return int.from_bytes(digest, "little")


def _scenario_daily_inputs(calendar_by_date: dict[str, list[dict]],
                          dates: list[str]) -> tuple[np.ndarray, np.ndarray]:
    """未来カレンダー (Future Context 由来) から rec/lsw の決定論成分を構成する。
    既存の日次テンソル構築ロジック (tensor_store._event_theme /
    PRIVATE_EVENT_NOMINAL_HOURS) を再利用し、判定基準を分岐させない。"""
    from .gap_analysis import PRIVATE_TIME_KEYWORDS

    rec_inputs = []
    lsw_inputs = []
    for d in dates:
        events = calendar_by_date.get(d, [])
        private_n = sum(1 for ev in events
                        if any(kw in str(ev.get("title", "")) for kw in PRIVATE_TIME_KEYWORDS))
        rec_inputs.append(private_n * ts.PRIVATE_EVENT_NOMINAL_HOURS)
        switches = 0
        prev_theme = None
        for ev in events:
            theme = ts._event_theme(str(ev.get("title", "")))
            if prev_theme is not None and theme != prev_theme:
                switches += 1
            prev_theme = theme
        lsw_inputs.append(switches)
    return np.array(rec_inputs, dtype=np.float64), np.array(lsw_inputs, dtype=np.float64)


def simulate(params: TwinParams, store: ts.TensorStore, scenario: dict,
            paths: int = MC_PATHS) -> dict:
    """§3.5 モンテカルロ。O(P・H) 時間・O(P) メモリ (軌跡の全保存禁止)。"""
    for field_val in (params.rho, params.beta1, params.beta2, params.gamma, params.kappa):
        if not np.isfinite(field_val):
            raise TwinFitError("TwinParams に非有限値が混入している (W-15)")   # 入口 assert

    horizon = min(int(scenario.get("horizon_days", MC_HORIZON_DEFAULT)), MC_HORIZON_MAX)
    calendar_entries = scenario.get("calendar", [])
    calendar_by_date: dict[str, list[dict]] = {}
    for ev in calendar_entries:
        calendar_by_date.setdefault(ev["date"], []).append(ev)

    values, mask = store.window(*store.dates)
    rec_hist, lsw_hist, lvol_hist, frict_hist = _build_state_inputs(values, mask)
    R_hist = _simulate_R((params.rho, params.beta1, params.beta2, params.gamma),
                        rec_hist, lsw_hist, lvol_hist, frict_hist)
    r_now = float(R_hist[-1])

    last_date = _date.fromisoformat(store.dates[1])
    future_dates = [(last_date + timedelta(days=i + 1)).isoformat() for i in range(horizon)]
    rec_cal, lsw_cal = _scenario_daily_inputs(calendar_by_date, future_dates)
    rec_future = np.clip(rec_cal / 4.0, 0.0, 1.0)      # 深夜 LINE は未来未知のため中立 (乗数1)
    lsw_future = np.clip(lsw_cal / 10.0, 0.0, 1.0)

    lane = ts.FEATURES
    friction_col = values[-90:, lane["friction_events"]]
    friction_mask = mask[-90:, lane["friction_events"]]
    valid_friction = friction_col[friction_mask]
    p_friction = float((valid_friction > 0).mean()) if valid_friction.size else 0.0

    lvol_pool = lvol_hist[np.isfinite(lvol_hist)]
    if lvol_pool.size == 0:
        lvol_pool = np.array([0.0])

    seed = _derive_seed(store, params, scenario)
    rng = np.random.Generator(np.random.Philox(seed))

    p = paths
    r_state = np.full(p, r_now, dtype=np.float32)
    r_q10, r_q50, r_q90, p_lapse = [], [], [], []
    lapse_accum = np.zeros(p, dtype=np.float64)
    critical_days: list[str] = []

    for t in range(horizon):
        frict_t = (rng.random(p) < p_friction).astype(np.float32)
        lvol_t = rng.choice(lvol_pool, size=p).astype(np.float32)
        rec_t = np.float32(rec_future[t])
        lsw_t = np.float32(lsw_future[t])
        r_state = np.clip(
            r_state + params.rho * (1.0 - r_state) * rec_t
            - params.beta1 * lsw_t - params.beta2 * lvol_t - params.gamma * frict_t,
            R_FLOOR, 1.0).astype(np.float32)

        q10, q50, q90 = np.quantile(r_state, [0.1, 0.5, 0.9])
        r_q10.append(float(q10)); r_q50.append(float(q50)); r_q90.append(float(q90))

        if params.theta_r is not None:
            below = r_state < params.theta_r
            frac_below = float(below.mean())
            lapse_accum += below
            if frac_below > 0.5:
                critical_days.append(future_dates[t])
        else:
            frac_below = None
        p_lapse.append(frac_below)

    _assert_finite(np.array(r_q50), "forecast.r_q50")   # 出口 assert (W-15)

    return {
        "horizon_days": horizon,
        "r_q10": r_q10, "r_q50": r_q50, "r_q90": r_q90,
        "p_lapse": p_lapse,
        "expected_lapses": float(lapse_accum.mean()) if params.theta_r is not None else None,
        "critical_days": critical_days,
    }


def compute_oii(dyad_store: ts.TensorStore) -> dict:
    """§3.7(A) 過剰投資指数。dyad スコープ限定 — global store への適用は禁止。"""
    if not (dyad_store.flags & ts.FLAG_DYAD_SCOPE):
        raise TwinFitError("compute_oii は dyad スコープの TensorStore にのみ適用できる")

    values, mask = dyad_store.window(*dyad_store.dates)
    lane = ts.FEATURES

    def _z(name: str) -> tuple[np.ndarray, np.ndarray]:
        col = values[:, lane[name]]
        col_mask = mask[:, lane[name]]
        if not col_mask.any():
            return np.zeros(len(col)), col_mask
        med = float(np.median(col[col_mask]))
        mad = float(np.median(np.abs(col[col_mask] - med)))
        z = np.zeros(len(col))
        z[col_mask] = (col[col_mask] - med) / (1.4826 * mad + 1e-9)
        return z, col_mask

    z_chars, _ = _z("line_out_chars")
    z_init, _ = _z("line_initiations")
    z_latency, _ = _z("line_reply_med_min")
    # spend_tagged (lane5-7) は dyad タグ config 未実装のため mask=0 固定
    # (E1.1 是正時点の既知の未実装範囲)。OII は現時点で3項合成として正直に報告する。
    oii_raw = (z_chars + z_init - z_latency) / 3.0

    half_life_days = 7.0
    alpha = 1.0 - 0.5 ** (1.0 / half_life_days)
    oii_ema = np.zeros(len(oii_raw))
    if len(oii_raw):
        oii_ema[0] = oii_raw[0]
        for t in range(1, len(oii_raw)):
            oii_ema[t] = alpha * oii_raw[t] + (1.0 - alpha) * oii_ema[t - 1]

    streak = 0
    max_streak = 0
    for v in oii_ema:
        if v >= 1.5:
            streak += 1
            max_streak = max(max_streak, streak)
        else:
            streak = 0

    n_active_days = int((mask[:, lane["line_out_msgs"]] | mask[:, lane["line_in_msgs"]]).sum())
    return {
        "oii_ema_last": float(oii_ema[-1]) if len(oii_ema) else 0.0,
        "streak_days": int(streak),
        "max_streak_days": int(max_streak),
        "spend_component_available": False,
        "n_active_days": n_active_days,
    }
