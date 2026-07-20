#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
core/oracle.py — Hyper-Personalized Oracle: 無菌化ペイロード + 決定論的介入選択
(Target Echo / E4)
==================================================================================
docs/SPEC_ECHO_GENESIS.md §3.7 / §5.3 / §5.6 / §5.7 / §5.10.5。

【この段の存在理由】
coupling.py (結合行列) と digital_twin.py (認知リソース状態・walk-forward
スキルゲート) の出力を、無菌化された oracle_payload.v1 (§5.6) へ集約し、
LLM には「言語化のみ」を許す。介入は Puppeteer (question_bank.py) と同じ
「生成ではなく選択」— INTERVENTION_BANK からの決定論的選択のみであり、
LLM によるリライトは禁止 (I-11 と同一原理)。

【聖域 (I-22) の継承】
本モジュールが生成する oracle_payload の合法出口は 3 つだけ:
  (1) consult の静的プレフィックス (gap_analysis と同じ更新頻度 — profiler
      再実行時のみ変化するため、動的サフィックスではなく静的側に置く方が
      KV キャッシュ効率が高い。SPEC_ECHO_GENESIS.md §4 の初期記述は
      「動的サフィックス」としていたが、実装時に gap_analysis の既存配置
      (静的プレフィックス) を確認し、これに合わせるのが正しいと判断した —
      本モジュールの as-built が正)
  (2) interview_sim / gd_sim の講評フェーズ (議論フェーズには絶対に出さない)
  (3) PROFILE UI (Foxtrot タブ。E4 では未実装)
面接官/GD 議論/es_review のコンテキストへは一切出さない。
"""

from __future__ import annotations

import re
from datetime import date as _date

from . import coupling, digital_twin, tensor_store

# ---------------------------------------------------------------- 無菌検査 (§5.5-2)
# feature id (レーン名) / rule id (R-XXX-nn) / bank id (iv-nnn) / ISO 日付 /
# dyad alias (C-xxxxxxxx) / schema 名 / 固定キーワードのいずれかにのみ一致を許す。
_STERILE_WHITELIST = re.compile(
    r"^("
    r"global|dyad|"                                   # scope.kind
    r"oracle_payload\.v1|"                             # schema
    r"iv-\d{3}|"                                        # bank id
    r"R-[A-Z]+-\d{2,3}|"                                # rule id
    r"C-[0-9a-f]{64}|"                                  # persistent dyad identity
    r"C~[0-9a-f]{12}|"                                  # display-only short id
    r"\d{4}-\d{2}-\d{2}|"                               # ISO date
    r"null_insufficient|"                               # reason 定数
    + "|".join(re.escape(k) for k in tensor_store.FEATURES) +   # feature id
    r")$"
)


def _assert_sterile(payload: object, _path: str = "$") -> None:
    """payload を再帰走査し、全文字列がホワイトリストのいずれかに一致することを
    assert する。自由テキスト混入 (第三者の言及・生成された自然文等) は
    AssertionError で検出する。本番コードに置く実行時ガード
    (Puppeteer のホワイトリスト assert と同格 — テスト専用にしない)。"""
    if isinstance(payload, dict):
        for k, v in payload.items():
            _assert_sterile(v, f"{_path}.{k}")
    elif isinstance(payload, list):
        for i, v in enumerate(payload):
            _assert_sterile(v, f"{_path}[{i}]")
    elif isinstance(payload, str):
        if not _STERILE_WHITELIST.match(payload):
            # 例外メッセージに混入テキスト自体を反響させない (罠 T-13 の Echo 適用)。
            raise AssertionError(
                f"oracle_payload の無菌検査に違反する自由テキストが {_path} にある "
                f"(長さ {len(payload)} 文字)")
    # int/float/bool/None はそのまま許可 (数値は無菌性の対象外)


# ---------------------------------------------------------------- §5.7 INTERVENTION_BANK
INTERVENTION_BANK: dict[str, dict] = {
    "iv-001": {"label": "送信クールダウン", "target_lane": tensor_store.FEATURES["line_out_msgs"],
              "template": "R < θ_R の間、当該 dyad への非緊急送信を保留し R 回復後に見直す"},
    "iv-002": {"label": "支出クールオフ", "target_lane": tensor_store.FEATURES["spend_hedonic"],
              "template": "失策ハザード高位日の裁量支出に 24h の遅延を課す"},
    "iv-003": {"label": "意思決定モラトリアム", "target_lane": tensor_store.FEATURES["task_declared"],
              "template": "R < θ_R の日に不可逆コミットメント (応募/購入/約束) をしない"},
    "iv-004": {"label": "回復ブロック予約", "target_lane": tensor_store.FEATURES["cal_private_hours"],
              "template": "結合行列が示す回復→生産性ラグに合わせ私的時間を予定に置く"},
    "iv-005": {"label": "面接前テーパリング", "target_lane": tensor_store.FEATURES["cal_switch_count"],
              "template": "面接前 48h のコンテキストスイッチ数を上限管理する"},
    "iv-006": {"label": "深夜送信ゲート", "target_lane": tensor_store.FEATURES["line_night_out"],
              "template": "23:00-08:00 の下書きを朝の R 回復後レビューまで保留する"},
}

# I-19: 「相手」を標的にするレーンはレジストリに存在しないため、これで構成的に閉じる。
_VALID_LANES = set(tensor_store.FEATURES.values())
for _bank_id, _entry in INTERVENTION_BANK.items():
    assert "target_lane" in _entry, f"{_bank_id} に target_lane がない"
    assert _entry["target_lane"] in _VALID_LANES, \
        f"{_bank_id}.target_lane={_entry['target_lane']} は FEATURES に存在しない"


class OracleError(RuntimeError):
    """payload 構築・無菌検査・scope 前提違反の失敗。"""


# ---------------------------------------------------------------- ORACLE_RULES
def _dominant_load_axis(params: digital_twin.TwinParams) -> str:
    """状態方程式の3負荷係数のうち最大のものを返す (決定論。タイは辞書順で
    beta1 > beta2 > gamma の優先順位)。"""
    candidates = [("beta1", params.beta1), ("beta2", params.beta2), ("gamma", params.gamma)]
    return max(candidates, key=lambda c: c[1])[0]


def _rule_low_resource_now(params, coup, forecast) -> tuple[bool, float, dict]:
    if params is None or not params.gate_passed or params.theta_r is None:
        return False, 0.0, {}
    if not forecast.get("r_q50"):
        return False, 0.0, {}
    r0 = float(forecast["r_q50"][0])
    triggered = r0 < params.theta_r
    severity = max(0.0, min(1.0, (params.theta_r - r0) / max(params.theta_r, 1e-6)))
    return triggered, severity, {"r_now": round(r0, 4), "theta_r": round(params.theta_r, 4)}


def _rule_switch_load_dominant(params, coup, forecast) -> tuple[bool, float, dict]:
    if params is None:
        return False, 0.0, {}
    total = params.beta1 + params.beta2 + params.gamma
    if total <= 1e-9:
        return False, 0.0, {}
    triggered = _dominant_load_axis(params) == "beta1"
    severity = params.beta1 / total
    return triggered, severity, {"beta1": round(params.beta1, 4), "total": round(total, 4)}


def _rule_comm_volume_dominant(params, coup, forecast) -> tuple[bool, float, dict]:
    if params is None:
        return False, 0.0, {}
    total = params.beta1 + params.beta2 + params.gamma
    if total <= 1e-9:
        return False, 0.0, {}
    triggered = _dominant_load_axis(params) == "beta2"
    severity = params.beta2 / total
    return triggered, severity, {"beta2": round(params.beta2, 4), "total": round(total, 4)}


def _rule_night_out_coupling(params, coup, forecast) -> tuple[bool, float, dict]:
    for pair in coup.get("pairs", []):
        if pair.get("sig") and "line_night_out" in (pair["src"], pair["dst"]):
            return True, abs(pair["rho"]), {"lag_days": pair["lag"], "rho": round(pair["rho"], 4)}
    return False, 0.0, {}


def _rule_recovery_coupling_confirmed(params, coup, forecast) -> tuple[bool, float, dict]:
    for pair in coup.get("pairs", []):
        if (pair.get("sig") and pair["rho"] > 0
                and "cal_private_hours" in (pair["src"], pair["dst"])):
            return True, abs(pair["rho"]), {"lag_days": pair["lag"], "rho": round(pair["rho"], 4)}
    return False, 0.0, {}


def _rule_hedonic_spend_coupling(params, coup, forecast) -> tuple[bool, float, dict]:
    for pair in coup.get("pairs", []):
        if pair.get("sig") and "spend_hedonic" in (pair["src"], pair["dst"]):
            return True, abs(pair["rho"]), {"lag_days": pair["lag"], "rho": round(pair["rho"], 4)}
    return False, 0.0, {}


# (rule_id, 述語関数, 許容 bank_id タプル) の静的表。述語は決定論のみ
# (params: TwinParams|None, coup: coupling_matrix() の戻り値, forecast: dict) を受け
# (triggered, severity, metrics) を返す。
ORACLE_RULES: tuple = (
    ("R-GATE-01", _rule_low_resource_now, ("iv-003",)),
    ("R-SWITCH-01", _rule_switch_load_dominant, ("iv-005",)),
    ("R-VOL-01", _rule_comm_volume_dominant, ("iv-001",)),
    ("R-NIGHT-01", _rule_night_out_coupling, ("iv-006",)),
    ("R-RECOVERY-01", _rule_recovery_coupling_confirmed, ("iv-004",)),
    ("R-SPEND-01", _rule_hedonic_spend_coupling, ("iv-002",)),
)
_RULE_LOOKUP = {rid: bank_ids for rid, _pred, bank_ids in ORACLE_RULES}


def _evaluate_rules(params, coup: dict, forecast: dict) -> list[dict]:
    findings = []
    for rule_id, predicate, _bank_ids in ORACLE_RULES:
        triggered, severity, metrics = predicate(params, coup, forecast)
        if triggered:
            findings.append({"rule_id": rule_id, "severity": round(float(severity), 4),
                             "metrics": metrics})
    return findings


def _select_interventions(findings: list[dict]) -> list[dict]:
    """Puppeteer と同一原理: 「生成ではなく選択」。bank_id は rule ごとに固定
    (複数候補があっても先頭を決定論的に選ぶ — 乱数不使用 I-17)。"""
    interventions = []
    for f in findings:
        bank_ids = _RULE_LOOKUP[f["rule_id"]]
        bank_id = bank_ids[0]
        entry = INTERVENTION_BANK[bank_id]
        interventions.append({
            "bank_id": bank_id, "trigger_rule": f["rule_id"],
            "target_lane": entry["target_lane"], "params": {},
        })
    return interventions


# ---------------------------------------------------------------- payload 構築
def _empty_payload(scope: str, alias: str | None, reason: str = "") -> dict:
    payload = {
        "schema": "oracle_payload.v1",
        "generated": _date.today().isoformat(),
        "scope": {"kind": scope, "alias": alias},
        "sufficiency": {
            "days_observed": 0, "coverage": 0.0, "dead_lanes": [],
            "twin_bss": 0.0, "n_lapse_test": 0, "gate_passed": False,
        },
        "state": {"r_now": None, "r_trend_7d": None, "oii_ema": None, "oii_streak_days": 0},
        "couplings": [], "forecast": {
            "horizon_days": 0, "r_q10": [], "r_q50": [], "r_q90": [],
            "p_lapse": [], "critical_days": [],
        },
        "findings": [], "interventions": [],
    }
    return payload


def _lane_name(idx: int) -> str:
    for name, i in tensor_store.FEATURES.items():
        if i == idx:
            return name
    return str(idx)


def build_oracle_payload(scope: str, alias: str | None = None) -> dict:
    """oracle_payload.v1 (§5.6) を構築する。無菌検査 (`_assert_sterile`) を
    通してから返す。TensorStore は本関数の実行内で open → 計算 → close を
    完結させる (常駐エンジンにキャッシュしない — SPEC §5.10.5 の E4 実装ノート、
    罠 T-14 の構造的回避)。"""
    if scope == "dyad":
        # focus dyad を指定する UI/config が現時点で存在しないため、
        # グローバルデータでの代用 (偽装) をせず正直に unratable を返す
        # (SPEC §5.10.5 の E4 実装ノート — dyad スコープは E4 では未配線)。
        return _empty_payload("dyad", alias, reason="dyad focus 未配線")
    if scope != "global":
        raise OracleError(f"不明な scope: {scope!r}")

    path = tensor_store.TENSOR_GLOBAL_BIN
    if not path.exists():
        return _empty_payload("global", None, reason="tensor 未構築")

    store = tensor_store.TensorStore(path)
    try:
        values, mask = store.window(*store.dates)
        n_rows = values.shape[0]
        lane_observed = mask.any(axis=0)
        dead_lanes = [name for name, idx in tensor_store.FEATURES.items()
                     if not lane_observed[idx]]
        coverage = float(mask.mean())

        try:
            coup = coupling.coupling_matrix(values, mask)
        except coupling.CouplingError:
            coup = {"pairs": []}
        # 罠 T-14: window() の戻り値 (mmap を export した ndarray) を保持したまま
        # store.close() すると BufferError になる。以降 values/mask は不要なので
        # ここで明示的に手放す (digital_twin 側は store 経由で自前の window() を
        # 呼ぶため、ここでの解放とは独立)。
        del values, mask

        try:
            params = digital_twin.fit_twin(store)
        except digital_twin.TwinFitError:
            params = None

        if params is not None and params.gate_passed:
            scenario = {"horizon_days": digital_twin.MC_HORIZON_DEFAULT, "calendar": []}
            sim = digital_twin.simulate(params, store, scenario)
            forecast = {
                "horizon_days": sim["horizon_days"], "r_q10": sim["r_q10"],
                "r_q50": sim["r_q50"], "r_q90": sim["r_q90"],
                "p_lapse": [p if p is not None else 0.0 for p in sim["p_lapse"]],
                "critical_days": sim["critical_days"],
            }
            r_now = float(sim["r_q50"][0]) if sim["r_q50"] else None
            r_trend_7d = (float(sim["r_q50"][6] - sim["r_q50"][0])
                         if len(sim["r_q50"]) > 6 else None)
        else:
            forecast = {"horizon_days": 0, "r_q10": [], "r_q50": [], "r_q90": [],
                       "p_lapse": [], "critical_days": []}
            r_now, r_trend_7d = None, None

        findings = _evaluate_rules(params, coup, forecast) if (
            params is not None and params.gate_passed) else []
        # I-20: gate_passed=false のとき forecast/interventions は空配列。
        interventions = _select_interventions(findings) if findings else []

        couplings_out = [{
            "src": _lane_name(p["src"]), "dst": _lane_name(p["dst"]),
            "lag_days": p["lag"], "rho": p["rho"], "n_eff": p["n_eff"],
            "null_q99": p["null_q99"] if p["null_q99"] is not None else 0.0,
            "sig": p["sig"],
        } for p in coup.get("pairs", []) if p.get("sig")]

        payload = {
            "schema": "oracle_payload.v1",
            "generated": _date.today().isoformat(),
            "scope": {"kind": "global", "alias": None},
            "sufficiency": {
                "days_observed": n_rows, "coverage": round(coverage, 4),
                "dead_lanes": dead_lanes,
                "twin_bss": round(params.bss, 4) if params else 0.0,
                "n_lapse_test": params.n_lapse_test if params else 0,
                "gate_passed": bool(params.gate_passed) if params else False,
            },
            "state": {
                "r_now": round(r_now, 4) if r_now is not None else None,
                "r_trend_7d": round(r_trend_7d, 4) if r_trend_7d is not None else None,
                "oii_ema": None, "oii_streak_days": 0,   # global スコープでは OII 非適用
            },
            "couplings": couplings_out,
            "forecast": forecast,
            "findings": findings,
            "interventions": interventions,
        }
        _assert_sterile(payload)
        return payload
    finally:
        store.close()


# ---------------------------------------------------------------- 言語化 (LLM への言い回し)
def render_oracle_consult(payload: dict) -> str:
    """oracle_payload を CONSULT プロンプトへ埋め込む text block を組み立てる
    (LLM 呼び出しは行わない — 言語化の「材料」を返すのみ)。gap_analysis の
    format_gap_table と同格の役割。「LINE 上の観測範囲では」の限定句を
    含める箇所は呼び出し側の SYSTEM_PROMPT/eval_prompt が担う (罠 T-12)。"""
    if not payload.get("sufficiency", {}).get("gate_passed"):
        return ("(Echo: データ不足または R-失策関係が未確認のため予測・介入は"
               "非表示。観測を継続すること)")
    lines = []
    state = payload["state"]
    if state.get("r_now") is not None:
        lines.append(f"- 現在の認知リソース推定 R={state['r_now']:.2f}"
                     f" (walk-forward BSS={payload['sufficiency']['twin_bss']:.3f})")
    for c in payload["couplings"][:4]:
        lines.append(f"- 結合: {c['src']} → {c['dst']} (ラグ{c['lag_days']:+d}日, "
                     f"ρ={c['rho']:+.2f}, n={c['n_eff']})")
    for iv in payload["interventions"][:3]:
        entry = INTERVENTION_BANK[iv["bank_id"]]
        lines.append(f"- 介入候補 [{iv['bank_id']}] {entry['label']}: {entry['template']}"
                     f" (根拠: {iv['trigger_rule']})")
    if payload["forecast"].get("critical_days"):
        lines.append(f"- 臨界日 (R が閾値を割る確率>0.5): "
                     f"{', '.join(payload['forecast']['critical_days'][:5])}")
    if not lines:
        return "(Echo: 有意な結合・介入トリガーは現時点で検出されていない)"
    return "\n".join(lines)
