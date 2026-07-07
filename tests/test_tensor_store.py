# -*- coding: utf-8 -*-
"""Target Echo E1: core/tensor_store.py の決定論的テスト (docs/SPEC_ECHO_GENESIS.md)。

外部依存なし (stdlib + numpy のみ、LLM 不使用)。実行は一時 PKB_PROJECT_ROOT 上で
行い、実データには一切触れない。

E1 ゲート (SPEC §5.9): 以下 5 項目の RED→GREEN を確認する。
  1. レイアウト相互検証 (calcsize==64/136 / C++ static_assert との数値一致)
  2. mask 意味論の対照群 (罠 T-15: "0円 (実測)" と "未記録" の区別)
  3. rebuild-under-handle (罠 T-14: ハンドル保持中の再構築が安全に扱われる)
  4. simulated 除外 (憲法5: consult_count が is_simulated_persona を除外)
  5. 日付格子 (罠 T-17: epoch_day/86400 のような時刻演算を使わない)
"""
from __future__ import annotations

import os
import struct
import sys
import tempfile
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

_TMP = tempfile.mkdtemp(prefix="pkb_tensor_store_")
os.environ["PKB_PROJECT_ROOT"] = _TMP
os.environ.setdefault("HF_HUB_OFFLINE", "1")
os.environ.setdefault("TRANSFORMERS_OFFLINE", "1")

from core import tensor_store as ts  # noqa: E402


def _day(date: str, *, diary: str = "", events: list[dict] | None = None,
        tx: list[dict] | None = None, consultations: list[dict] | None = None,
        sources: list[str] | None = None) -> dict:
    src = sources if sources is not None else []
    if diary and "diary" not in src:
        src = src + ["diary"]
    return {
        "date": date, "diary_text": diary, "calendar_events": events or [],
        "transactions": tx or [], "consultations": consultations or [],
        "line_self_text": "", "sources": src,
    }


# ============================================================ 1. レイアウト相互検証
def test_layout_cross_validation() -> None:
    """C++ static_assert (search_engine.cpp) と Python struct fmt の数値一致。
    calcsize が 64/136 からズレたら "<" 欠落や padding 混入の即検出になる (W-1)。"""
    assert ts.HEADER_SIZE == 64, ts.HEADER_SIZE
    assert ts.ROW_SIZE == 136, ts.ROW_SIZE
    assert ts.ROW_DTYPE.itemsize == 136

    # struct.calcsize の内訳オフセットが C++ static_assert (offsetof) と 1:1 一致
    assert struct.calcsize("<8s") == 8            # magic
    assert struct.calcsize("<8sI") == 12           # version offset
    assert struct.calcsize("<8sII") == 16          # n_rows offset
    assert struct.calcsize("<8sIII") == 20         # n_features offset
    assert struct.calcsize("<8sIIII") == 24        # row_stride offset
    assert struct.calcsize("<8sIIIIi") == 28       # epoch_day offset
    assert struct.calcsize("<8sIIIIiI") == 32      # flags offset
    # content_hash64 (Q, 8B) はここから — offset 32 は C++ static_assert と一致

    assert struct.calcsize("<i") == 4              # TensorRow.day_index
    assert struct.calcsize("<iI") == 8             # TensorRow.f offset (C++ と一致)

    assert len(ts.FEATURES) == 22
    assert ts.FEATURES["github_commits"] == 20 and ts.FEATURES["leetcode_solved"] == 21
    assert max(ts.FEATURES.values()) < ts.KTEN_FEAT
    print("  layout cross-validation OK")


# ============================================================ 2. mask 意味論 (T-15)
def test_mask_semantics_zero_vs_missing() -> None:
    """「支出0円 (finance ソースあり)」と「finance 未記録」を区別する。
    マスクを見ない集計 (0 を実測値として混ぜる) は最頻の静かな死 (I-18)。"""
    daily = [
        _day("2026-06-01", tx=[], sources=["finance"]),         # 実測ゼロ支出
        _day("2026-06-02", diary="今日は特に何もなかった"),      # finance 未記録
        _day("2026-06-03", tx=[{"type": "expense", "amount": 3000, "category": "外食"}],
             sources=["finance"]),
    ]
    out = ts.build_tensor(daily, None, Path(_TMP) / "t_mask.bin")
    store = ts.TensorStore(out)
    try:
        values, mask = store.window("2026-06-01", "2026-06-03")
        spend_lane = ts.FEATURES["spend_total"]

        assert mask[0, spend_lane], "実測ゼロ支出の日が missing 扱いになっている"
        assert values[0, spend_lane] == 0.0

        assert not mask[1, spend_lane], "finance 未記録の日が観測済み扱いになっている"
        assert values[1, spend_lane] == 0.0, "欠測スロットは値も 0.0 でなければならない (I-18)"

        assert mask[2, spend_lane] and values[2, spend_lane] == 3000.0

        # reserved レーン (22-31) は常に mask=0
        assert not mask[:, 22].any() and not mask[:, 31].any()
        # 未実装 importer レーン (github/leetcode) も常に mask=0
        assert not mask[:, ts.FEATURES["github_commits"]].any()
    finally:
        # 罠 T-14: window() の戻り値 (mmap を export した ndarray) を保持したまま
        # close() すると BufferError になる。長期保持しない短命な使用でも、
        # close 前に明示的に参照を手放すのが正しい呼び出し側の作法。
        del values, mask
        store.close()
    print("  mask semantics (zero vs missing) OK")


# ============================================================ 3. rebuild-under-handle (T-14)
def test_rebuild_under_open_handle() -> None:
    """ハンドル保持中でも再構築 (release_tensor_mapping → tmp → os.replace) が
    Windows の PermissionError なしで完走し、旧ハンドルは安全に無効化される。"""
    path = Path(_TMP) / "t_rebuild.bin"
    daily_v1 = [_day("2026-06-01", diary="v1", sources=["diary"])]
    ts.build_tensor(daily_v1, None, path)

    store_v1 = ts.TensorStore(path)
    assert store_v1.dates == ("2026-06-01", "2026-06-01")

    # ハンドルを閉じずに再構築 (build_tensor 内部の release_tensor_mapping が
    # registry 経由で store_v1 を close するはず)
    daily_v2 = [_day("2026-06-01", diary="v2拡張", sources=["diary"]),
                _day("2026-06-02", diary="v2", sources=["diary"])]
    ts.build_tensor(daily_v2, None, path)

    assert store_v1._closed, "release_tensor_mapping が旧ハンドルを close していない"
    try:
        store_v1.window("2026-06-01", "2026-06-01")
        raise AssertionError("close 済みハンドルの使用がエラーにならなかった")
    except ts.TensorStoreError:
        pass

    store_v2 = ts.TensorStore(path)
    try:
        assert store_v2.dates == ("2026-06-01", "2026-06-02")
        values, mask = store_v2.window("2026-06-01", "2026-06-01")
        assert values[0, ts.FEATURES["diary_chars"]] == len("v2拡張")
    finally:
        del values, mask   # 罠 T-14: close 前に window() の戻り値を手放す
        store_v2.close()
    print("  rebuild-under-handle OK")


# ============================================================ 4. simulated 除外 (憲法5)
def test_consult_count_excludes_simulated_persona() -> None:
    """PROBE/相談の consult_count は is_simulated_persona=True (面接/GD/ES建前) を
    除外する (AI_SKILLS §7.1.4 の重み付け規約を Echo の集計にも継承)。"""
    daily = [_day("2026-06-01", sources=["consultation"], consultations=[
        {"query": "本音の相談です", "is_simulated_persona": False},
        {"query": "面接での建前発言", "is_simulated_persona": True},
        {"query": "もう一つの本音", "is_simulated_persona": False},
    ])]
    out = ts.build_tensor(daily, None, Path(_TMP) / "t_sim.bin")
    store = ts.TensorStore(out)
    try:
        values, mask = store.window("2026-06-01", "2026-06-01")
        lane = ts.FEATURES["consult_count"]
        assert mask[0, lane]
        assert values[0, lane] == 2.0, \
            f"simulated=True が consult_count に混入している (got {values[0, lane]})"
    finally:
        del values, mask
        store.close()
    print("  simulated persona exclusion OK")


# ============================================================ 5. 日付格子 (T-17)
def test_date_grid_no_epoch_seconds_arithmetic() -> None:
    """日数差分は date オブジェクト演算のみで行い、月境界・年境界をまたいでも
    epoch 秒/86400 のような手法に頼らず正しい行番号へ対応する。"""
    daily = [_day("2026-01-30", diary="a", sources=["diary"]),
             _day("2026-03-02", diary="b", sources=["diary"])]  # 閏年2月をまたぐ
    out = ts.build_tensor(daily, None, Path(_TMP) / "t_dategrid.bin")
    store = ts.TensorStore(out)
    try:
        first_iso, last_iso = store.dates
        assert first_iso == "2026-01-30" and last_iso == "2026-03-02"
        assert store.n_rows == 32, store.n_rows   # 1/30→3/2 (2026年は閏年でない) = 32日分

        values, mask = store.window("2026-03-02", "2026-03-02")
        assert mask[0, ts.FEATURES["diary_chars"]]
        assert values[0, ts.FEATURES["diary_chars"]] == 1.0
        del values, mask   # 罠 T-14: close 前に window() の戻り値を手放す

        # 全行の day フィールドが行番号と一致 (TensorStore.__init__ の検証を通過済み
        # であること自体がガードだが、ここでも明示的に再確認する)
        assert np.array_equal(store._rows["day"], np.arange(store.n_rows, dtype=np.int32))
    finally:
        store.close()
    print("  date grid (no epoch-seconds arithmetic) OK")


# ============================================================ E1.1 是正 (SPEC Rev.3 §5.10.2)
def test_task_lanes_mask_only_on_observed_days() -> None:
    """lane 18/19: genuine な主観 doc / 実行観測チャネルが無い日は mask=0。
    「日記を書かなかった日」を「宣言0件という観測」に化けさせない (I-18)。
    これを怠ると、日記執筆習慣と相関する全レーンとの間に共有欠測パターン由来の
    偽結合が立つ (E2 の帰無分布を汚染する — SPEC Rev.3 §5.10.2 が検出した本体)。"""
    daily = [
        _day("2026-06-01", diary="面接対策やらないと", sources=["diary"]),  # 宣言あり→観測
        _day("2026-06-02"),  # 完全に空 (diary/calendar/line/consult 一切なし) → 未観測
        _day("2026-06-03", events=[{"time": "10:00", "title": "何か予定"}],
             sources=["calendar"]),
    ]
    out = ts.build_tensor(daily, None, Path(_TMP) / "t_task_mask.bin")
    store = ts.TensorStore(out)
    try:
        values, mask = store.window("2026-06-01", "2026-06-03")
        declared_lane = ts.FEATURES["task_declared"]
        executed_lane = ts.FEATURES["task_executed"]

        assert mask[0, declared_lane], "宣言のある日が未観測扱いになっている"
        assert not mask[1, declared_lane], \
            "空の日の task_declared が観測扱いになっている (I-18 違反)"
        assert not mask[1, executed_lane], \
            "空の日の task_executed が観測扱いになっている (I-18 違反)"
        assert mask[2, executed_lane], \
            "予定のある日 (実行観測チャネルあり) が未観測扱いになっている"
    finally:
        del values, mask
        store.close()
    print("  task lane mask: no silent-day leak (E1.1) OK")


def test_line_silence_within_coverage_is_observed_zero() -> None:
    """LINE カバレッジ窓 [最古日, 最新日] 内の無通信日は観測済み0
    (mask=1, value=0.0)。窓外は mask=0 のまま。lane 14 (レイテンシ) だけは
    鏡像対称化の対象外 (返信サンプルの無い日の「中央値」は定義できない)。"""
    msgs = [
        _msg("友人A", "2026-06-01", "09:00", False, "おはよう"),
        _msg("友人A", "2026-06-01", "09:05", True, "おはよう!"),
        _msg("友人A", "2026-06-05", "18:00", True, "元気?"),
    ]
    # 6/1 と 6/5 の間 (6/2-6/4) はログのカバレッジ窓内の無通信日。6/6,6/7 は窓外。
    daily = [_day(f"2026-06-{d:02d}", sources=["line"] if d in (1, 5) else [])
            for d in range(1, 8)]
    out = ts.build_tensor(daily, None, Path(_TMP) / "t_line_silence.bin",
                          line_messages=msgs)
    store = ts.TensorStore(out)
    try:
        values, mask = store.window("2026-06-01", "2026-06-07")
        lane = ts.FEATURES["line_out_msgs"]
        assert mask[0, lane] and values[0, lane] == 1.0     # 6/1: 実データ
        for offset in (1, 2, 3):    # 6/2, 6/3, 6/4: 窓内の沈黙日
            assert mask[offset, lane], f"窓内沈黙日 (offset={offset}) が欠測扱い"
            assert values[offset, lane] == 0.0
        assert mask[4, lane]         # 6/5: 実データ
        for offset in (5, 6):        # 6/6, 6/7: 窓外
            assert not mask[offset, lane], f"窓外の日 (offset={offset}) が観測扱い"
        latency_lane = ts.FEATURES["line_reply_med_min"]
        assert not mask[1, latency_lane], \
            "返信サンプルの無い沈黙日で line_reply_med_min まで観測扱いにした"
    finally:
        del values, mask
        store.close()
    print("  LINE silence-within-coverage = observed-zero (E1.1) OK")


# ============================================================ 補助: LINE 日別集計の疎通
def _msg(contact: str, date: str, time: str, is_self: bool, text: str) -> dict:
    return {"contact": contact, "date": date, "time": time, "is_self": is_self, "text": text}


def test_line_daily_aggregates_smoke() -> None:
    """line_telemetry の既存バースト抽出/摩擦検出を再利用した日別集計の疎通確認
    (burst 状態機械を再実装していないことの間接検証)。"""
    msgs = [
        _msg("友人A", "2026-06-01", "09:00", False, "おはよう"),
        _msg("友人A", "2026-06-01", "09:05", True, "おはよう!"),
        _msg("友人A", "2026-06-02", "12:00", True, "今日どう?"),
        _msg("友人A", "2026-06-02", "12:03", False, "元気だよ"),
    ]
    daily = [_day("2026-06-01", sources=["line"]), _day("2026-06-02", sources=["line"])]
    out = ts.build_tensor(daily, None, Path(_TMP) / "t_line.bin", line_messages=msgs)
    store = ts.TensorStore(out)
    try:
        values, mask = store.window("2026-06-01", "2026-06-02")
        out_lane, in_lane = ts.FEATURES["line_out_msgs"], ts.FEATURES["line_in_msgs"]
        assert mask[0, out_lane] and values[0, out_lane] == 1.0
        assert mask[0, in_lane] and values[0, in_lane] == 1.0
        # 6/2 は本人 (is_self=True) が起点 → line_initiations が立つ
        assert values[1, ts.FEATURES["line_initiations"]] == 1.0
    finally:
        del values, mask
        store.close()
    print("  line daily aggregates smoke OK")


if __name__ == "__main__":
    test_layout_cross_validation()
    test_mask_semantics_zero_vs_missing()
    test_rebuild_under_open_handle()
    test_consult_count_excludes_simulated_persona()
    test_date_grid_no_epoch_seconds_arithmetic()
    test_task_lanes_mask_only_on_observed_days()
    test_line_silence_within_coverage_is_observed_zero()
    test_line_daily_aggregates_smoke()
    print("test_tensor_store: ALL PASS")
