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

import io
import json
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


# ---------------------------------------------------------------- 相関ID (SPEC_FOXTROT_UI.md §9)
def test_cid_stamped_on_all_events_single_path() -> None:
    """W-48 (SPEC_FOXTROT_UI.md §9.3): cid の刻印は main() の emit_event
    ラッパー1箇所のみ。dispatch 内のコマンドが status/chunk イベントをいくつ
    emit しても、全て req["cid"] を運ぶことを検証する (刻印経路の単一性)。"""
    emitted: list[dict] = []
    original_emit = engine_stdio._emit
    engine_stdio._emit = lambda payload: emitted.append(payload)

    def fake_dispatch(cmd, params, emit=None):
        assert emit is not None
        emit({"event": "status", "message": "考え中"})
        emit({"event": "chunk", "text": "a"})
        emit({"event": "chunk", "text": "b"})
        return {"answer": "done"}

    original_dispatch = engine_stdio.dispatch
    engine_stdio.dispatch = fake_dispatch

    request_line = json.dumps({"id": 1, "cid": 42, "cmd": "consult", "params": {"query": "x"}})
    original_stdin = sys.stdin
    sys.stdin = io.StringIO(request_line + "\n")
    try:
        engine_stdio.main()
    finally:
        sys.stdin = original_stdin
        engine_stdio.dispatch = original_dispatch
        engine_stdio._emit = original_emit

    # ready (main 冒頭・特定リクエスト非紐付け) + status + chunk×2 + 最終応答 = 5件
    # 最終応答 ({"ok": ...}) は「イベント」ではなく同期 RPC の戻り値そのもの
    # (engine.rs::invoke_sync は event キーを持たない行を forward_event せず
    # 直接 Ok(result) として返す) — cid の刻印対象は emit_event 経由の中間
    # イベント行のみで、最終応答には不要 (SPEC_FOXTROT_UI.md §9.1 の図参照)。
    assert len(emitted) == 5, f"想定と異なるイベント数: {emitted}"
    ready = emitted[0]
    assert ready.get("event") == "ready"
    assert ready.get("cid") is None, "ready はどのリクエストにも紐づかない (cid なし)"
    for payload in emitted[1:4]:
        assert payload.get("cid") == 42, f"cid が全イベントへ刻印されていない: {payload}"
    final = emitted[-1]
    assert final.get("id") == 1
    assert final.get("ok") is True
    assert "event" not in final, "最終応答はイベント行ではない (forward_event 対象外)"
    print("  cid stamped on all events via single emit_event path (W-48) OK")


def test_cid_distinguishes_sequential_requests() -> None:
    """W-45/W-49 の前提となるバックエンド側の性質: 異なる cid を持つ2つの
    リクエストが直列実行 (§9.2 の境界線 — プロセスロックは据え置き) された
    場合、各リクエストのイベントは自分の cid のみを運ぶ。これがフロントの
    accepts() で cid_A ≠ cid_B の旧イベントを弾ける土台になる。

    (フロント側 useCorrelationId.accepts() の純ロジックはこのリポジトリに
    フロント用テストランナーが存在しないため直接のユニットテストを持たず、
    lib/useCorrelationId.ts のコメントで不変条件を明記する形で申告する —
    as-built 記録参照。)"""
    emitted: list[dict] = []
    original_emit = engine_stdio._emit
    engine_stdio._emit = lambda payload: emitted.append(payload)

    def fake_dispatch(cmd, params, emit=None):
        emit({"event": "status", "message": f"{cmd}-status"})
        return {"answer": cmd}

    original_dispatch = engine_stdio.dispatch
    engine_stdio.dispatch = fake_dispatch

    lines = [
        json.dumps({"id": 1, "cid": 10, "cmd": "consult", "params": {}}),
        json.dumps({"id": 2, "cid": 20, "cmd": "consult", "params": {}}),
    ]
    original_stdin = sys.stdin
    sys.stdin = io.StringIO("\n".join(lines) + "\n")
    try:
        engine_stdio.main()
    finally:
        sys.stdin = original_stdin
        engine_stdio.dispatch = original_dispatch
        engine_stdio._emit = original_emit

    status_events = [p for p in emitted if p.get("event") == "status"]
    assert len(status_events) == 2
    assert status_events[0]["cid"] == 10
    assert status_events[1]["cid"] == 20
    assert status_events[0]["cid"] != status_events[1]["cid"], \
        "超過リクエストの cid が混線している (W-45 の前提が崩れる)"
    print("  sequential requests carry distinct cid, no cross-contamination (W-45 basis) OK")


def test_cid_absent_request_emits_null_cid() -> None:
    """cid 未指定のリクエスト (例: health) は Rust 側で null としてシリアライズ
    される (serde_json の Option<u64>::None)。Python 側は None として扱い、
    イベントを出すコマンドでも cid: null が刻印される (無害 — W-48 と矛盾しない)。"""
    emitted: list[dict] = []
    original_emit = engine_stdio._emit
    engine_stdio._emit = lambda payload: emitted.append(payload)

    def fake_dispatch(cmd, params, emit=None):
        emit({"event": "status", "message": "no cid"})
        return {"answer": "ok"}

    original_dispatch = engine_stdio.dispatch
    engine_stdio.dispatch = fake_dispatch

    request_line = json.dumps({"id": 1, "cid": None, "cmd": "consult", "params": {}})
    original_stdin = sys.stdin
    sys.stdin = io.StringIO(request_line + "\n")
    try:
        engine_stdio.main()
    finally:
        sys.stdin = original_stdin
        engine_stdio.dispatch = original_dispatch
        engine_stdio._emit = original_emit

    status_events = [p for p in emitted if p.get("event") == "status"]
    assert len(status_events) == 1
    assert status_events[0].get("cid") is None
    print("  cid-less request emits null cid without crashing OK")


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
    test_cid_stamped_on_all_events_single_path()
    test_cid_distinguishes_sequential_requests()
    test_cid_absent_request_emits_null_cid()
    print("test_oracle: ALL PASS")
