# -*- coding: utf-8 -*-
"""Target Echo E0: 憲法ガード (docs/SPEC_ECHO_GENESIS.md §5.5)。

oracle.py / tensor_store.py 本体 (E1〜E4) 着手前に、以下 2 つのガードを先に書く
(「ガードが先、機能が後」)。E0 時点で ImportError の RED を確認済み。

SPEC Rev.2 §5.5-4 (Architect's Note) に基づき、未実装モジュールの ImportError
【のみ】をゲート付き SKIP とする (I-5 の「exe 必須テストはゲート付き SKIP」
パターンに合流。常設 RED は DoD の全 PASS 原則と矛盾するため)。
ImportError 以外の例外は全て FAIL である。E4 完了時のゲート条件は
「本ファイルが SKIP なしで GREEN」— SKIP 分岐を残したまま E4 を完了と言うな。

対象:
  - I-19: INTERVENTION_BANK 全エントリの target_lane が FEATURES に実在すること
    (第三者の反応を標的にする介入をレジストリ不在で構成的に排除する)
  - oracle._assert_sterile: oracle_payload の自由テキスト混入を検出する無菌検査
    (本番コードに置く実行時ガード。テスト専用にしない — SPEC §5.5-2)
"""
from __future__ import annotations

import os
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

_TMP = tempfile.mkdtemp(prefix="pkb_oracle_")
os.environ["PKB_PROJECT_ROOT"] = _TMP

# engine_stdio は import 時に sys.stdout を stderr へ退避する (プロトコル純度
# 規約 — engine_stdio.py 冒頭参照)。テスト内で遅延 import すると、それより前の
# print() がリバインド前の sys.stdout に書かれたまま明示 flush されず消える
# ことがあるため、モジュール先頭で import して以後の全 print を一貫させる。
import engine_stdio  # noqa: E402
os.environ.setdefault("HF_HUB_OFFLINE", "1")
os.environ.setdefault("TRANSFORMERS_OFFLINE", "1")


# ---------------------------------------------------------------- I-19
def test_intervention_bank_target_lane_whitelist() -> None:
    """SPEC I-19 / §5.5-3: INTERVENTION_BANK の target_lane は
    tensor_store.FEATURES に実在するレーン番号のみを許す。第三者の反応・感情を
    標的にする介入・数式・プロンプトは、レジストリ不在という型シグネチャの
    水準で構成的に禁止される (憲法7の系)。"""
    from core import oracle, tensor_store

    valid_lanes = set(tensor_store.FEATURES.values())
    assert valid_lanes, "tensor_store.FEATURES が空 — レジストリ未定義"
    assert oracle.INTERVENTION_BANK, "INTERVENTION_BANK が空"
    for bank_id, entry in oracle.INTERVENTION_BANK.items():
        assert "target_lane" in entry, f"{bank_id} に target_lane がない"
        assert entry["target_lane"] in valid_lanes, \
            f"{bank_id}.target_lane={entry['target_lane']} は FEATURES に存在しない"
    print("  intervention bank target_lane whitelist OK")


# ---------------------------------------------------------------- 無菌検査
def test_assert_sterile_rejects_free_text() -> None:
    """SPEC §5.5-2: oracle_payload.v1 の全文字列は、feature id / rule id /
    iv-\\d{3} / ISO 日付 / C-alias / schema 名のいずれかのホワイトリストに
    一致しなければならない。自由テキストの混入 (第三者の言及等) は
    AssertionError で構成的に検出する。"""
    from core import oracle

    clean_payload = {
        "schema": "oracle_payload.v1",
        "generated": "2026-07-07",
        "scope": {"kind": "global", "alias": None},
        "couplings": [{"src": "cal_private_hours", "dst": "productivity_idx",
                       "lag_days": 1, "rho": 0.44}],
        "interventions": [{"bank_id": "iv-001", "trigger_rule": "R-OII-01",
                           "target_lane": 11}],
    }
    oracle._assert_sterile(clean_payload)  # 例外なし = 合格

    dirty_payload = {
        "schema": "oracle_payload.v1",
        "findings": [{"rule_id": "R-OII-01",
                      "note": "彼女への返信をもっと遅らせるべきだ"}],  # 自由テキスト混入
    }
    try:
        oracle._assert_sterile(dirty_payload)
        raise AssertionError("自由テキスト混入が無菌検査を通過してしまった")
    except AssertionError as exc:
        assert "彼女" not in str(exc), \
            "例外メッセージに混入テキストを反響させるな (罠 T-13 の Echo 適用)"
    print("  oracle payload sterility check OK")


# ---------------------------------------------------------------- E4: build_oracle_payload E2E
def test_build_oracle_payload_end_to_end_real_tensor() -> None:
    """build_oracle_payload("global") を実際に一度も通していなかったせいで、
    (a) TensorStore.close() の T-14 (window() 戻り値を手放さず close して
    BufferError)、(b) tensor_store.TENSOR_GLOBAL_BIN の再エクスポート漏れ、
    の2つの実バグが単体テストをすり抜けていた (N=3650 ベンチマークで発覚)。
    ここで実際にテンソルを構築して呼ぶ E2E を回帰ガードとして固定する。"""
    from core import tensor_store as ts

    daily = [{"date": f"2026-01-{d:02d}", "diary_text": "テスト日記です" if d % 2 == 0 else "",
             "calendar_events": [], "transactions": [], "consultations": [],
             "line_self_text": "", "sources": ["diary"] if d % 2 == 0 else []}
            for d in range(1, 29)]
    ts.build_tensor(daily, None, ts.TENSOR_GLOBAL_BIN)

    from core import oracle
    payload = oracle.build_oracle_payload("global")   # 内部で _assert_sterile 済み
    assert payload["schema"] == "oracle_payload.v1"
    assert payload["scope"] == {"kind": "global", "alias": None}
    assert payload["sufficiency"]["days_observed"] == 28

    dyad_payload = oracle.build_oracle_payload("dyad", alias="C-abcd1234")
    assert dyad_payload["scope"]["kind"] == "dyad"
    assert not dyad_payload["sufficiency"]["gate_passed"]
    print("  build_oracle_payload end-to-end with real tensor (E4) OK")


# ---------------------------------------------------------------- 境界防衛 (SPEC §5.10.5)
def test_stdio_dispatch_ignores_unknown_params() -> None:
    """consult 系ディスパッチャは params の未知キーを黙って無視する (facade.consult
    は既知の kwargs のみを受け取る構造)。UI からの誤送信 (余計な state) が
    面接官プロンプトの構築経路へ到達しないことの構造的保証 — echo-back 漏洩の
    第一防壁 (境界の反対側はバックエンドの構造的隔離そのもの)。"""
    captured: dict = {}

    def fake_consult(query, status=None, on_token=None, mode="consult",
                     personas=None, response_time_sec=None, config=None):
        captured["kwargs"] = {"query": query, "mode": mode, "personas": personas,
                              "response_time_sec": response_time_sec, "config": config}
        return "FAKE ANSWER"

    class _FakeFacade:
        consult = staticmethod(fake_consult)
        last_interview_report = staticmethod(lambda: None)

    original_import = engine_stdio._import_facade
    engine_stdio._import_facade = lambda: _FakeFacade
    try:
        result = engine_stdio.dispatch("consult", {
            "query": "テスト相談です", "mode": "consult",
            "unexpected_field": "should be discarded",
            "gap_insights": "injection attempt via unknown param",
        }, emit=None)
    finally:
        engine_stdio._import_facade = original_import

    assert result["answer"] == "FAKE ANSWER"
    assert captured["kwargs"]["query"] == "テスト相談です"
    assert "unexpected_field" not in captured["kwargs"]
    assert "gap_insights" not in captured["kwargs"]
    print("  stdio dispatch ignores unknown params (boundary defense) OK")


if __name__ == "__main__":
    # ゲート付き SKIP (SPEC Rev.2 §5.5-4): 未実装モジュールの ImportError のみを
    # SKIP 扱いにする。テスト本体の AssertionError はこの except に到達しない
    # (各テストは import 成功後に assert するため、隔離ガードの検出力は落ちない)。
    try:
        from core import oracle, tensor_store  # noqa: F401
    except ImportError as exc:
        print(f"test_oracle: SKIP (E1/E4 未実装: {exc})")
        sys.exit(0)
    test_intervention_bank_target_lane_whitelist()
    test_assert_sterile_rejects_free_text()
    test_build_oracle_payload_end_to_end_real_tensor()
    test_stdio_dispatch_ignores_unknown_params()
    print("test_oracle: ALL PASS")
