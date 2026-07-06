# -*- coding: utf-8 -*-
"""gap_analysis (主観×客観 差分分析) の決定論的テスト。

外部依存なし (stdlib のみ)。LLM は使わない — 決定論コアの検出精度と
主観/客観コーパスの分離原則を検証する。
"""
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core.gap_analysis import (  # noqa: E402
    analyze_gaps,
    build_objective_signals,
    build_subjective_corpus,
    format_gap_table,
    hyperbolic_discount,
)


def _day(date: str, diary: str = "", queries: list[str] | None = None,
         tx: list[dict] | None = None, events: list[dict] | None = None,
         line_self: str = "") -> dict:
    return {
        "date": date,
        "diary_text": diary,
        "consultations": [{"query": q} for q in (queries or [])],
        "transactions": tx or [],
        "calendar_events": events or [],
        "line_self_text": line_self,
    }


def test_intention_gap_and_blind_spot() -> None:
    """日記はキャリアの焦り一色、行動は娯楽消費一色 → 双方向のギャップを検出。"""
    daily = [
        _day("2026-07-01",
             diary="キャリアに焦りがある。転職すべきか、このままでいいのか不安だ。",
             tx=[{"type": "expense", "category": "ゲーム課金", "amount": 12000}]),
        _day("2026-07-02",
             diary="市場価値が上がっていない気がして焦る。スキル不足を感じる。",
             queries=["キャリアの方向転換をすべきでしょうか"],
             tx=[{"type": "expense", "category": "娯楽", "amount": 8000}],
             line_self="新作に課金した。ポチったやつ届いた"),
        _day("2026-07-03",
             diary="将来のことを考えると眠れない。何者かになりたい。",
             tx=[{"type": "expense", "category": "ゲーム", "amount": 15000}]),
    ]
    result = analyze_gaps(daily)
    by_theme = {(g["theme"], g["type"]): g for g in result["gaps"]}

    career = by_theme.get(("キャリア・仕事の将来", "intention_gap"))
    assert career is not None, f"キャリアの意図過剰ギャップ未検出: {result['gaps']}"
    assert career["gap"] > 0
    assert career["objective"]["money_spent"] == 0
    assert career["subjective"]["quotes"], "主観の証拠引用が空"

    leisure = by_theme.get(("娯楽・消費", "blind_spot"))
    assert leisure is not None, f"娯楽の盲点ギャップ未検出: {result['gaps']}"
    assert leisure["gap"] < 0
    assert leisure["objective"]["money_spent"] == 35000

    # ギャップ表の整形が両タイプを含むこと
    table = format_gap_table(result)
    assert "意図過剰" in table and "盲点" in table
    print("  intention_gap + blind_spot OK")


def test_aligned_behavior_no_career_gap() -> None:
    """内省と行動が一致している場合、そのテーマの意図過剰ギャップは出ない。"""
    daily = [
        _day("2026-07-01",
             diary="キャリアに焦りがある。転職を目指したい。",
             tx=[{"type": "expense", "category": "書籍", "amount": 3000}],
             events=[{"time": "19:00", "title": "勉強会"}],
             line_self="金曜の勉強会、参加するよ。面接の練習もしたい"),
    ]
    result = analyze_gaps(daily)
    career_gaps = [
        g for g in result["gaps"]
        if g["theme"] == "キャリア・仕事の将来" and g["type"] == "intention_gap"
    ]
    assert not career_gaps, f"行動が伴っているのに意図過剰と誤検出: {career_gaps}"
    print("  aligned no-gap OK")


def test_subjective_objective_separation() -> None:
    """LINE 発話は客観軸のみ。主観コーパスに混入してはならない (設計原則)。"""
    daily = [
        _day("2026-07-01",
             line_self="転職エージェントと話した。キャリアの相談をしてきた"),
    ]
    subj = build_subjective_corpus(daily)
    assert subj == [], f"LINE発話が主観コーパスに混入: {subj}"
    signals = build_objective_signals(daily)
    assert len(signals["line_docs"]) == 1, "LINE発話が客観シグナルに入っていない"

    # 収入 (income) は支出集計に含めない
    daily2 = [_day("2026-07-02",
                   tx=[{"type": "income", "category": "給与", "amount": 300000}])]
    signals2 = build_objective_signals(daily2)
    assert signals2["spend_by_category"] == {}, "収入が支出として集計された"
    print("  subjective/objective separation OK")


def test_empty_and_sparse_data_safe() -> None:
    """空データ・疎データでも例外なく動作し、少データは充足度が下がる。"""
    result = analyze_gaps([])
    assert result["gaps"] == []
    assert result["data_sufficiency"] == 0.0
    assert "未検出" in format_gap_table(result)

    sparse = analyze_gaps([_day("2026-07-01", diary="キャリアが不安だ。")])
    assert sparse["data_sufficiency"] < 0.5
    print("  empty/sparse safety OK")


def test_hyperbolic_procrastination() -> None:
    """双曲割引: 宣言→実行の遅延日数に応じて行動価値が非線形に減衰する。"""
    # 減衰関数そのものの性質: 即日=1.0、単調減少
    assert hyperbolic_discount(0) == 1.0
    assert hyperbolic_discount(1) > hyperbolic_discount(7) > hyperbolic_discount(30)

    daily = [
        # 面接対策: 6/1 に宣言 → 6/8 のカレンダー予定で実行 (遅延7日)
        _day("2026-06-01", diary="明日こそ面接対策をやる。逃げない。"),
        _day("2026-06-08", events=[{"time": "10:00", "title": "面接対策 模擬面接"}]),
        # LeetCode: 6/2 に宣言 → 実行ログなし (完全逃避)
        _day("2026-06-02", diary="今週末にLeetCodeを解く。絶対にやる。"),
        # SPI: 6/3 に宣言 → 翌日実行 (遅延1日 = 健全な対照群)
        _day("2026-06-03", diary="明日はSPIの対策するぞ。"),
        _day("2026-06-04", events=[{"time": "09:00", "title": "SPI模試"}]),
    ]
    result = analyze_gaps(daily)
    tasks = {t["task"]: t for t in result["procrastination"]["tasks"]}

    mensetsu = tasks["面接対策"]
    rec = mensetsu["records"][0]
    assert rec["delay_days"] == 7 and rec["executed"] == "2026-06-08", rec
    assert rec["discounted_value"] == round(hyperbolic_discount(7), 3)
    assert mensetsu["flagged"], "遅延7日の先延ばしが未フラグ"

    leet = tasks["コーディングテスト対策"]
    assert leet["records"][0]["executed"] is None
    assert leet["records"][0]["discounted_value"] == 0.0
    assert leet["avoidance_index"] == 1.0 and leet["flagged"]

    spi = tasks["Webテスト対策"]
    assert spi["records"][0]["delay_days"] == 1
    assert not spi["flagged"], "翌日実行の健全タスクを誤フラグ"

    avoid_types = [g["theme"] for g in result["gaps"] if g["type"] == "task_avoidance"]
    assert "就活タスク: 面接対策" in avoid_types
    assert "就活タスク: コーディングテスト対策" in avoid_types
    assert "就活タスク: Webテスト対策" not in avoid_types
    print("  hyperbolic procrastination OK")


def test_true_gakuchika_discovery() -> None:
    """金融志望を焦って語るが、実熱量は部活マネジメントにある就活生シナリオ。"""
    daily = [
        _day("2026-06-01",
             diary="金融業界を目指して就活しなきゃと焦る。選考が不安だ。",
             tx=[{"type": "expense", "category": "部費", "amount": 60000}],
             events=[{"time": "18:00", "title": "部活 練習"}],
             line_self="今日の練習出るよ。合宿の集合時間まとめて送る"),
        _day("2026-06-02",
             diary="内定が出る気がしない。ガクチカも書けていない。",
             tx=[{"type": "expense", "category": "遠征", "amount": 15000}],
             events=[{"time": "18:00", "title": "部活 ミーティング"}],
             line_self="後輩のシフト調整終わった。大会のエントリーは任せて"),
        _day("2026-06-03",
             queries=["金融業界の就活の軸が定まりません"],
             events=[{"time": "19:00", "title": "合宿 準備"}]),
    ]
    result = analyze_gaps(daily)
    gk = result["gakuchika"]
    assert gk["detected"], f"ガクチカ乖離が未検出: {gk}"
    assert gk["declared_focus"]["theme"] == "キャリア・仕事の将来"
    assert gk["declared_focus"]["label"] == "建前/サンクコスト"
    assert gk["true_passion"]["theme"] == "課外活動・組織運営"
    assert "ガクチカ" in gk["true_passion"]["label"]
    assert gk["true_passion"]["money_spent"] == 75000
    assert gk["declared_focus"]["quotes"], "建前側の引用証拠が空"

    flags = [g for g in result["gaps"] if g["type"] == "true_gakuchika"]
    assert flags and flags[0]["theme"] == "課外活動・組織運営"
    assert "課外活動・組織運営" in flags[0]["insight"]
    assert "キャリア・仕事の将来" in flags[0]["insight"]
    print("  true gakuchika discovery OK")


def test_intellectualization_detection() -> None:
    """就活アクション 0 の週の抽象語急増のみを知性化としてフラグする。"""
    abstract_diary = "本質。本質。本質。哲学。哲学。パラダイム。パラダイム。方法論。俯瞰。"
    daily = [
        # W1: アクションあり・抽象語 0 (健全な活動週)
        _day("2026-06-01", diary="説明会に行ってきた。",
             events=[{"time": "10:00", "title": "企業説明会"}]),
        # W2: アクション 0 + 抽象語 9 回 → 知性化フラグ
        _day("2026-06-08", diary=abstract_diary),
        # W3: 同じ抽象語 9 回だがアクションあり → フラグしない (対照群)
        _day("2026-06-15", diary=abstract_diary,
             events=[{"time": "14:00", "title": "一次面接"}]),
    ]
    result = analyze_gaps(daily)
    intel = result["intellectualization"]
    flagged = [w for w in intel["weeks"] if w["flagged"]]
    assert len(flagged) == 1, f"知性化フラグ数が想定外: {flagged}"
    assert "2026-06-08" in flagged[0]["dates"]
    assert flagged[0]["abstract_hits"] == 9
    assert flagged[0]["quotes"], "知性化の引用証拠が空"

    gaps = [g for g in result["gaps"] if g["type"] == "intellectualization_gap"]
    assert len(gaps) == 1
    assert gaps[0]["objective"]["action_count"] == 0
    # アクションのある W3 は同じ抽象度でもフラグされない
    w3 = next(w for w in intel["weeks"] if "2026-06-15" in w["dates"])
    assert not w3["flagged"] and w3["action_count"] >= 1
    print("  intellectualization detection OK")


if __name__ == "__main__":
    test_intention_gap_and_blind_spot()
    test_aligned_behavior_no_career_gap()
    test_subjective_objective_separation()
    test_empty_and_sparse_data_safe()
    test_hyperbolic_procrastination()
    test_true_gakuchika_discovery()
    test_intellectualization_detection()
    print("test_gap_analysis: ALL PASS")
