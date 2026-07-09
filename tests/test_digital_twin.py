# -*- coding: utf-8 -*-
"""Target Echo E3: core/digital_twin.py の決定論的テスト (docs/SPEC_ECHO_GENESIS.md)。

外部依存なし (stdlib + numpy のみ、LLM 不使用、乱数不使用の合成データ)。
TensorStore の実ファイルは書かず、`window()`/`dates`/`content_hash64`/`flags`
のみを duck-type した FakeTensorStore で依存注入する (SearchDaemonClient の
fake spawn / FakeBackend と同じ確立済みパターン)。

E3 ゲート (SPEC §5.9 + Rev.4 §5.10.4 の追加要求):
  1. グリッド探索 + fit_twin の決定論 (2回実行の完全一致)
  2. walk-forward がゲートを落とすケース (失策数不足)
  3. seed 固定 MC の再現性 (round(,6) 比較)
  4. O(P) メモリの構造確認 (出力サイズが horizon のみに依存し paths に依存しない)
  5. W-16: 完全分離データで IRLS が発散せず収束し ||beta|| < 100
  6. W-15: 非有限な TwinParams が simulate() の入口で TwinFitError
  7. W-19: trailing causal baseline が「全履歴からの素朴な閾値」より
     見かけの予測スキル (BSS) を正しく抑制する回帰テスト
"""
from __future__ import annotations

import sys
from datetime import date, timedelta
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core import digital_twin as dt  # noqa: E402
from core import tensor_store as ts  # noqa: E402

LANE = ts.FEATURES
N_LANES = ts.KTEN_FEAT


class FakeTensorStore:
    """TensorStore の duck-type フェイク。window() は常に全域を返す
    (テストは常に store.dates の全域を要求するため十分)。"""

    def __init__(self, values: np.ndarray, mask: np.ndarray, dates: tuple[str, str],
                content_hash64: int = 42, flags: int = 0):
        self._values = values
        self._mask = mask
        self._dates = dates
        self.content_hash64 = content_hash64
        self.flags = flags

    @property
    def dates(self) -> tuple[str, str]:
        return self._dates

    def window(self, start_iso: str, end_iso: str) -> tuple[np.ndarray, np.ndarray]:
        return self._values, self._mask


def _make_synthetic_store(n: int = 600, phase: float = 0.0, flags: int = 0) -> FakeTensorStore:
    """状態方程式入力 (private_hours/switch/out_chars/friction) + D(t) 成分
    (reply_latency/guilt/task) + lapse 条件成分 (spend_hedonic/night_out/
    out_msgs/in_msgs) を決定論式 (sin 合成 + 固定スケジュール) で構成する。
    np.random は使わない (I-17 の精神をテストデータにも適用)。"""
    values = np.zeros((n, N_LANES), dtype=np.float32)
    mask = np.zeros((n, N_LANES), dtype=bool)
    t = np.arange(n, dtype=np.float64)

    def _set(name: str, arr: np.ndarray) -> None:
        values[:, LANE[name]] = arr
        mask[:, LANE[name]] = True

    _set("cal_private_hours", np.clip(2.0 + 1.5 * np.sin(2 * np.pi * t / 14 + phase), 0, 4))
    _set("line_night_out", np.clip(3 + 2 * np.sin(2 * np.pi * t / 9 + 1.0 + phase), 0, None))
    _set("cal_switch_count", np.clip(3 + 2 * np.sin(2 * np.pi * t / 11 + 2.0), 0, None))
    _set("line_out_chars", np.clip(500 + 300 * np.sin(2 * np.pi * t / 17 + 0.5), 0, None))
    _set("friction_events", (np.sin(2 * np.pi * t / 23) > 0.7).astype(np.float64))

    _set("line_reply_med_min", np.clip(20 + 15 * np.sin(2 * np.pi * t / 13 + phase), 1, None))
    _set("guilt_idx", np.clip(1 + np.sin(2 * np.pi * t / 19), 0, None))
    _set("task_declared", np.ones(n))
    _set("task_executed", (t.astype(np.int64) % 3 != 0).astype(np.float64))

    _set("spend_hedonic", np.clip(50 + 40 * np.sin(2 * np.pi * t / 21 + 1.5), 0, None))
    _set("line_out_msgs", np.clip(10 + 8 * np.sin(2 * np.pi * t / 16 + 0.3), 0, None))
    _set("line_in_msgs", np.clip(10 + 8 * np.sin(2 * np.pi * t / 16 + 2.1), 0, None))

    first = "2025-01-01"
    last = (date.fromisoformat(first) + timedelta(days=n - 1)).isoformat()
    return FakeTensorStore(values, mask, (first, last), flags=flags)


# ============================================================ 1. fit_twin 決定論
def test_fit_twin_deterministic_repeated_calls() -> None:
    store = _make_synthetic_store()
    p1 = dt.fit_twin(store)
    p2 = dt.fit_twin(store)
    assert p1.to_dict() == p2.to_dict(), (p1.to_dict(), p2.to_dict())
    print("  fit_twin deterministic repeated calls OK")


# ============================================================ 2. walk-forward ゲート落ち
def test_walk_forward_gate_fails_with_insufficient_lapses() -> None:
    n = 600
    R = np.linspace(0.8, 0.3, n)
    lapse = np.zeros(n, dtype=bool)
    lapse[300] = lapse[301] = lapse[302] = True   # 3件のみ (< MIN_TEST_LAPSES_GATE=10)
    lapse_valid = np.ones(n, dtype=bool)
    bss, n_test, gate = dt._walk_forward(R, lapse, lapse_valid)
    assert not gate, (bss, n_test, gate)
    print("  walk-forward gate fails on insufficient lapses OK")


def test_fit_twin_gate_fails_on_degenerate_data() -> None:
    """lapse が一度も観測されない合成データでは gate_passed=False で正直に報告する
    (エラーを投げない — data_sufficiency と同じ「確度の自己申告」思想)。
    3条件 (a)(b)(c) を個別に無効化する: task_executed を常に 1 (a を消す)、
    friction_events を常に 0 (b を消す)、in_msgs を out_msgs より常に大きくする
    (c の in_msgs<p50 が成立しなくする)。"""
    store = _make_synthetic_store()
    n = store._values.shape[0]
    store._values[:, LANE["task_executed"]] = 1.0
    store._values[:, LANE["friction_events"]] = 0.0
    store._values[:, LANE["line_in_msgs"]] = store._values[:, LANE["line_out_msgs"]] + 1000.0

    lapse, lapse_valid = dt._build_lapse(store._values, store._mask)
    assert lapse.sum() == 0, "3条件を無効化したのに lapse が残っている (テスト前提の誤り)"

    params = dt.fit_twin(store)
    assert not params.gate_passed
    assert params.n_lapse_test == 0
    print("  fit_twin gate fails on degenerate (lapse-free) data OK")


# ============================================================ 3. seed 固定 MC の再現性
def test_mc_seed_reproducible() -> None:
    store = _make_synthetic_store()
    params = dt.fit_twin(store)
    scenario = {"horizon_days": 10, "calendar": []}
    r1 = dt.simulate(params, store, scenario, paths=512)
    r2 = dt.simulate(params, store, scenario, paths=512)
    assert [round(v, 6) for v in r1["r_q50"]] == [round(v, 6) for v in r2["r_q50"]]
    assert [round(v, 6) for v in r1["r_q10"]] == [round(v, 6) for v in r2["r_q10"]]
    assert r1["critical_days"] == r2["critical_days"]
    print("  MC seed reproducible OK")


# ============================================================ 4. O(P) メモリの構造確認
def test_mc_output_size_independent_of_paths() -> None:
    """出力 (r_q10/q50/q90/p_lapse) の長さは horizon のみに依存し、paths には
    依存しない — 軌跡の全保存を構造的に否定する (§3.5 O(P) メモリ制約)。"""
    store = _make_synthetic_store()
    params = dt.fit_twin(store)
    scenario = {"horizon_days": 12, "calendar": []}
    r_small = dt.simulate(params, store, scenario, paths=256)
    r_large = dt.simulate(params, store, scenario, paths=4096)
    assert len(r_small["r_q50"]) == 12 == len(r_large["r_q50"])
    assert len(r_small["r_q10"]) == 12 and len(r_large["r_q90"]) == 12
    print("  MC output size O(horizon) regardless of paths (structural) OK")


# ============================================================ 5. W-16: 分離対照群
def test_irls_separation_control_group() -> None:
    """完全分離データ (下位20%だけ lapse=1) で IRLS が発散せず収束し、
    ||beta|| < 100 に収まる (Hessian のみのリッジでは満たせない — SPEC Rev.4)。"""
    n = 200
    x_centered = np.linspace(-1, 1, n)
    y = (x_centered < -0.6).astype(np.float64)   # 完全分離
    beta, converged, grad_norm = dt._irls_fit(x_centered, y)
    assert converged, (beta, grad_norm)
    assert np.linalg.norm(beta) < 100, beta
    print("  IRLS separation control group (W-16) OK")


# ============================================================ 6. W-15: 非有限値の入口拒否
def test_simulate_rejects_non_finite_params() -> None:
    store = _make_synthetic_store()
    bad_params = dt.TwinParams(
        rho=0.1, beta1=0.1, beta2=0.1, gamma=0.1, kappa=float("nan"),
        theta_r=1.0, bss=0.1, n_lapse_test=20, gate_passed=True,
        fitted_window="2025-01-01..2025-01-10")
    try:
        dt.simulate(bad_params, store, {"horizon_days": 5})
        raise AssertionError("非有限な TwinParams が simulate() を素通りした")
    except dt.TwinFitError:
        pass
    print("  simulate rejects non-finite params (W-15) OK")


def test_sse_grid_search_rejects_non_finite_inputs() -> None:
    """NaN は clip() を経ても NaN のまま伝播する (inf と異なり境界にクランプ
    されない) ため、状態方程式入力の非有限混入を検出する経路の実効性を
    NaN 注入で検証する (TensorStore 自体は書込み時に NaN を拒否するが — I-18 —
    digital_twin 単体の防御層をここでは独立に確認する)。"""
    store = _make_synthetic_store()
    store._values[100, LANE["cal_private_hours"]] = np.nan
    try:
        dt.fit_twin(store)
        raise AssertionError("非有限な状態方程式入力が検出されなかった")
    except dt.TwinFitError:
        pass
    print("  fit_twin rejects non-finite state-equation inputs (W-15) OK")


# ============================================================ 7. W-19: ラベル漏洩回帰テスト
def test_causal_baseline_suppresses_naive_global_leakage() -> None:
    """全履歴 (未来を含む) から分位点を1回だけ計算する「素朴な」ラベル付けは、
    時間トレンドを共有するだけの無関係な2系列 (緩やかに減衰する R と、
    無関係な上昇トレンドを持つ spend_hedonic) の間に見かけの予測スキルを
    作り出す。trailing causal baseline (dt._rolling_quantile_causal) は
    これを正しく抑制し、より低い (誠実な) BSS を報告する (SPEC Rev.4 W-19)。"""
    n = 600
    t = np.arange(n, dtype=np.float64)
    R = 0.8 - 0.5 * (t / n) + 0.02 * np.sin(2 * np.pi * t / 11)          # 無関係な緩やかな減衰
    spend = 20 + 160 * (t / n) + 15 * np.sin(2 * np.pi * t / 13)         # 無関係な上昇トレンド
    spend_mask = np.ones(n, dtype=bool)
    task_declared = np.ones(n)
    task_executed = (t.astype(np.int64) % 3 != 0).astype(np.float64)    # 時間と無関係な固定周期

    # 素朴 (漏洩あり): 全履歴 (未来を含む) から p90 を1回だけ計算して全日へ適用
    naive_p90 = np.quantile(spend, 0.90)
    lapse_naive = (spend > naive_p90) & (task_declared > 0) & (task_executed == 0)
    lapse_valid_naive = np.ones(n, dtype=bool)

    # 正しい: trailing causal baseline (時刻 t は [t-180, t) のみ参照)
    p90_causal, ok_p90 = dt._rolling_quantile_causal(spend, spend_mask, dt.BASELINE_DAYS, 0.90)
    lapse_causal = ok_p90 & (spend > p90_causal) & (task_declared > 0) & (task_executed == 0)
    lapse_valid_causal = ok_p90

    bss_naive, n_test_naive, _ = dt._walk_forward(R, lapse_naive, lapse_valid_naive)
    bss_causal, n_test_causal, _ = dt._walk_forward(R, lapse_causal, lapse_valid_causal)

    assert bss_causal <= bss_naive + 1e-9, (
        f"causal BSS ({bss_causal}) が naive-global BSS ({bss_naive}) を"
        f"上回った — trailing baseline がラベル漏洩を抑制できていない")
    print(f"  W-19 leakage regression: causal BSS={bss_causal:.4f} "
         f"<= naive-global BSS={bss_naive:.4f} OK")


# ============================================================ 補助: compute_oii の疎通
def test_compute_oii_dyad_scope_only() -> None:
    dyad_store = _make_synthetic_store(flags=ts.FLAG_DYAD_SCOPE)
    result = dt.compute_oii(dyad_store)
    assert "oii_ema_last" in result and "streak_days" in result

    global_store = _make_synthetic_store(flags=0)
    try:
        dt.compute_oii(global_store)
        raise AssertionError("global スコープの store で compute_oii が拒否されなかった")
    except dt.TwinFitError:
        pass
    print("  compute_oii dyad-scope-only guard OK")

