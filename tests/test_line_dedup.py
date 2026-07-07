# -*- coding: utf-8 -*-
"""IMP-1/IMP-2 是正指令 (docs/AI_SKILLS.md §14 / T-20〜T-25) の回帰テスト。

profiler.load_line_messages() のエクスポートブロック単位・多重集合 max
デデュープ (T-20)、コンタクト単位の is_self 3段階決定論 (T-21)、ヘッダ無し
追記のブロック境界検知 (T-23)、ConversationSession の日次スライス添付
(T-22) とトリップワイヤ (T-24)、グループチャットの受動観測ログ化
(T-25 Rev.2) を検証する。実行は一時 PKB_PROJECT_ROOT 上で行い、実データ
(data/raw/line_history.txt) には一切触れない。
"""
from __future__ import annotations

import os
import sys
import tempfile
from datetime import date, timedelta
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

_TMP = tempfile.mkdtemp(prefix="pkb_line_dedup_")
os.environ["PKB_PROJECT_ROOT"] = _TMP
os.environ.setdefault("HF_HUB_OFFLINE", "1")
os.environ.setdefault("TRANSFORMERS_OFFLINE", "1")

from core import facade, profiler  # noqa: E402
from core.data_merger import (  # noqa: E402
    extract_conversation_sessions, extract_group_daily_logs, load_daily_contexts,
)
from core.paths import DIARY_MD, LINE_HISTORY  # noqa: E402
from core.profile_store import save_fixed_attributes  # noqa: E402


def _msg(contact: str, d: str, time: str, is_self: bool, text: str,
        sender: str | None = None) -> dict:
    return {"source": "line", "contact": contact, "date": d, "time": time,
            "sender": sender or ("自分" if is_self else contact),
            "text": text, "is_self": is_self}


def _block(contact: str, date: str, msgs: list[tuple[str, str, str]]) -> str:
    """1つの [LINE] ヘッダブロック (= 1回のエクスポート) を組み立てる。"""
    lines = [f"[LINE] {contact}とのトーク履歴", "保存日時：2026/07/07 12:00", "",
             f"{date}(火)"]
    for time_s, sender, text in msgs:
        lines.append(f"{time_s}\t{sender}\t{text}")
    lines.append("")
    return "\n".join(lines)


def _write(raw: str) -> None:
    LINE_HISTORY.parent.mkdir(parents=True, exist_ok=True)
    LINE_HISTORY.write_text(raw, encoding="utf-8")


def _key_counts(msgs: list[dict]) -> dict[tuple, int]:
    counts: dict[tuple, int] = {}
    for m in msgs:
        k = (m["contact"], m["date"], m["time"], m["sender"], m["text"])
        counts[k] = counts.get(k, 0) + 1
    return counts


# ---------------------------------------------------------------- (a) 同一エクスポート2回取込 → 件数不変
def test_duplicate_export_reimport_count_invariant() -> None:
    msgs = [("21:14", "友人", "今夜どう？"), ("21:20", "自分", "行く！")]
    single = _block("友人", "2026/07/01", msgs)

    _write(single)
    once = profiler.load_line_messages()

    _write(single + single)  # 同一エクスポートを12回相当で2回取込を模擬
    twice = profiler.load_line_messages()

    assert len(once) == len(msgs), once
    assert len(twice) == len(once), \
        f"同一エクスポートの再取込で件数が変化した: {len(once)} -> {len(twice)}"
    assert _key_counts(once) == _key_counts(twice)
    print("  duplicate export reimport -> count invariant OK")


# ---------------------------------------------------------------- (b) 部分重複 (旧 ⊂ 新) → 和集合
def test_partial_overlap_union() -> None:
    old_msgs = [("21:14", "友人", "今夜どう？"), ("21:20", "自分", "行く！")]
    new_msgs = old_msgs + [("21:22", "友人", "了解、店予約しとくわ")]
    old_block = _block("友人", "2026/07/01", old_msgs)
    new_block = _block("友人", "2026/07/01", new_msgs)

    _write(old_block + new_block)  # 旧エクスポート後、新エクスポート (差分1件追加) を取込
    result = profiler.load_line_messages()

    assert len(result) == len(new_msgs), result
    texts = {m["text"] for m in result}
    assert texts == {t for _, _, t in new_msgs}, \
        "部分重複エクスポートの和集合が正しく取れていない"
    print("  partial overlap (old subset of new) -> union OK")


# ---------------------------------------------------------------- (c) 本物の連投 (同一分内の同文複数回) は失われない
def test_genuine_burst_within_block_preserved() -> None:
    # 同一分内に同一文面を2回送信 (本物の連投)
    msgs = [("21:14", "自分", "了解了解"), ("21:14", "自分", "了解了解")]
    single = _block("友人", "2026/07/01", msgs)

    _write(single)
    once = profiler.load_line_messages()
    assert len(once) == 2, \
        f"1ブロック内の本物の連投(多重度2)が保存されていない: {len(once)}"

    _write(single + single)  # このブロックごと12回相当の重複取込を模擬
    reimported = profiler.load_line_messages()
    assert len(reimported) == 2, \
        (f"連投(多重度2)とブロック重複(2回取込)が sum(=4) されてしまった: "
         f"{len(reimported)} (期待値: max(2,2)=2)")
    print("  genuine burst within one block survives reimport (max, not sum) OK")


# ---------------------------------------------------------------- (T-21) 実名記録の is_self 解決 (tier3 フォールバック)
def test_is_self_resolved_via_common_sender_across_contacts() -> None:
    # 実 LINE エクスポートは本人も実名で記録されるため "自分" リテラル判定は
    # 全滅する。複数コンタクトに共通する唯一の sender を本人とみなす。
    block1 = _block("友人A", "2026/07/01", [
        ("10:00", "友人A", "おはよう"), ("10:05", "田中太郎", "おはよう!"),
    ])
    block2 = _block("友人B", "2026/07/01", [
        ("11:00", "友人B", "元気?"), ("11:05", "田中太郎", "元気だよ"),
    ])
    _write(block1 + block2)
    msgs = profiler.load_line_messages()
    self_msgs = [m for m in msgs if m["is_self"]]
    assert len(self_msgs) == 2, self_msgs
    assert all(m["sender"] == "田中太郎" for m in self_msgs)
    print("  is_self resolved via common sender across contacts (T-21 tier3) OK")


# ---------------------------------------------------------------- (T-21) user_profile 明示設定 (tier1) が優先される
def test_is_self_declared_name_takes_priority() -> None:
    save_fixed_attributes({"line_self_name": "鈴木花子"})
    try:
        block = _block("友人C", "2026/07/01", [
            ("09:00", "友人C", "やあ"), ("09:05", "鈴木花子", "どうも"),
        ])
        _write(block)
        msgs = profiler.load_line_messages()
        self_msgs = [m for m in msgs if m["is_self"]]
        assert len(self_msgs) == 1 and self_msgs[0]["sender"] == "鈴木花子"
        print("  is_self declared line_self_name (T-21 tier1) OK")
    finally:
        save_fixed_attributes({"line_self_name": ""})  # 後続テストへの汚染防止


# ---------------------------------------------------------------- (T-23) ヘッダ無し追記への自前ヘッダ付与
def test_format_line_import_injects_header_when_missing() -> None:
    text = "2026/07/01(水)\n10:00\t友人\tやあ"
    formatted = facade.format_line_import(text, "line_export.txt")
    assert formatted.startswith("[LINE]"), formatted
    already_headed = "[LINE] 友人とのトーク履歴\n" + text
    assert facade.format_line_import(already_headed, "x.txt") == already_headed.strip()
    print("  format_line_import injects header only when missing (T-23) OK")


# ---------------------------------------------------------------- (T-23) ヘッダ無し重複はブロック境界 (日付後退) で吸収される
def test_date_regression_without_header_splits_block() -> None:
    block = _block("友人", "2026/07/01", [("21:14", "友人", "了解了解")])
    block += "2026/07/03(金)\n21:20\t友人\t進捗どう？\n"  # 同ブロック内で日付前進
    # ヘッダ無しで過去日付 (07/01) の内容が再掲された (画面キャプチャ再送等を模倣)。
    # 同一の再掲を連続で2回貼ると、後続の再掲同士は日付が後退しない (同日付)
    # ため同一ブロックへ吸収され「本物の連投」と区別不能になる — これは
    # 意図された挙動であり、本テストは1回の再掲 (=1個の新ブロック) を検証する。
    headerless_repeat = "2026/07/01(水)\n21:14\t友人\t了解了解\n"
    _write(block + headerless_repeat)
    result = profiler.load_line_messages()
    texts = [m["text"] for m in result]
    assert texts.count("了解了解") == 1, \
        f"日付後退のブロック境界検知が効かず sum に退化した: count={texts.count('了解了解')}"
    assert "進捗どう？" in texts
    print("  date-regression without header splits new block (T-23) OK")


# ---------------------------------------------------------------- (T-25) グループチャットの自動除外
def test_group_chat_excluded_from_session_extraction() -> None:
    # sender が3人以上 = グループチャット。awaiting_user 状態機械の前提
    # (1対1 dyad) が根本的に成立しないため、session抽出から除外する。
    group_msgs = [
        _msg("グループX", "2026-07-01", "10:00", False, "やあ", sender="Aさん"),
        _msg("グループX", "2026-07-02", "10:00", False, "こんにちは", sender="Bさん"),
        _msg("グループX", "2026-07-03", "10:00", False, "おつかれ", sender="Cさん"),
    ]
    dyad_msgs = [
        _msg("友人", "2026-07-01", "09:00", False, "元気?"),
        _msg("友人", "2026-07-01", "09:05", True, "元気だよ"),
    ]
    sessions = extract_conversation_sessions(group_msgs + dyad_msgs)
    contacts = {s["contact"] for s in sessions}
    assert "グループX" not in contacts, \
        "3人以上のグループチャットが session 抽出から除外されていない"
    assert "友人" in contacts, "正常な1対1 dyad まで除外されてしまった"
    print("  group chat (3+ senders) excluded from session extraction (T-25) OK")


def test_group_chat_does_not_poison_is_self_tier3() -> None:
    # グループチャットに自分の表示名が (ほぼ) 出てこない場合でも、
    # tier3 の積集合計算からグループが除外され、他の1対1コンタクトから
    # is_self が正しく解決されること (T-25 が profiler 側でも効くこと)。
    dyad1 = _block("友人E", "2026/07/01", [
        ("10:00", "友人E", "やあ"), ("10:05", "山田次郎", "どうも"),
    ])
    dyad2 = _block("友人F", "2026/07/01", [
        ("11:00", "友人F", "元気?"), ("11:05", "山田次郎", "元気だよ"),
    ])
    group = _block("グループY", "2026/07/01", [
        ("12:00", "Aさん", "やっほー"), ("12:01", "Bさん", "はーい"),
        ("12:02", "Cさん", "こんちは"),
    ])
    _write(dyad1 + dyad2 + group)
    msgs = profiler.load_line_messages()
    self_msgs = [m for m in msgs if m["is_self"]]
    assert len(self_msgs) == 2 and all(m["sender"] == "山田次郎" for m in self_msgs), \
        "グループチャットの sender 集合が tier3 積集合を汚染した"
    print("  group chat does not poison is_self tier3 intersection (T-25) OK")


# ---------------------------------------------------------------- (T-22) セッションの日次スライス添付 (span複製の防止)
def test_session_day_slicing_no_duplication_across_span() -> None:
    # ルールb (返信までクローズしない) により、返信が来るまで awaiting_user=True
    # のままアイドル判定を受けずに継続する。この間に複数日分の相手発言が
    # 溜まっても、日付別スライスは各日1回だけに正しく分解されねばならない。
    msgs = [
        _msg("友人", "2026-07-01", "10:00", False, "元気?"),
        _msg("友人", "2026-07-10", "10:00", False, "久しぶり"),
        _msg("友人", "2026-07-10", "10:05", True, "元気だよ、久しぶり"),
    ]
    sessions = extract_conversation_sessions(msgs)
    assert len(sessions) == 1, sessions
    s = sessions[0]
    assert set(s["dates"]) == {"2026-07-01", "2026-07-10"}
    assert s["turns_by_date"]["2026-07-01"] != s["turns_by_date"]["2026-07-10"]
    total_via_dates = sum(len(v) for v in s["turns_by_date"].values())
    assert total_via_dates == len(s["turns"]) == 3, \
        "日付別ターンの合計がセッション全体のターン数と一致しない (情報ロス or 重複)"
    print("  session day-slicing: no duplication across span (T-22) OK")


# ---------------------------------------------------------------- (T-24) セッション・トリップワイヤ
def test_session_tripwire_fires_on_pathological_span() -> None:
    base = date(2026, 1, 1)
    msgs = [_msg("鯨", (base + timedelta(days=i)).isoformat(), "09:00", False, f"day{i}")
           for i in range(40)]
    msgs.append(_msg("鯨", (base + timedelta(days=40)).isoformat(), "09:00", True, "了解"))
    try:
        extract_conversation_sessions(msgs)
        raise AssertionError("span超過なのにトリップワイヤが発火しなかった")
    except RuntimeError as e:
        assert "異常肥大" in str(e), str(e)
    print("  session tripwire fires on pathological span (T-24) OK")


# ---------------------------------------------------------------- (T-25 Rev.2) 増幅ゼロの証明
def test_group_message_attached_exactly_once_per_date() -> None:
    DIARY_MD.write_text("", encoding="utf-8")
    group = _block("グループZ", "2026/07/01", [
        ("10:00", "Aさん", "やっほー"), ("10:01", "Bさん", "はーい"),
        ("10:02", "Cさん", "こんちは"),
    ])
    _write(group)
    contexts = load_daily_contexts()
    by_date = {c["date"]: c for c in contexts}
    d = "2026-07-01"
    assert d in by_date, by_date
    dc = by_date[d]
    assert dc["has_line_group"] is True
    assert dc["group_line_text"].count("やっほー") == 1
    assert dc["group_line_text"].count("はーい") == 1
    assert dc["group_line_text"].count("こんちは") == 1
    other_days = [c for c in contexts if c["date"] != d and "やっほー" in c["group_line_text"]]
    assert not other_days, "グループ発言が他日へ複製された (増幅再発)"
    print("  group message attached exactly once per date, zero amplification (T-25 Rev.2) OK")


# ---------------------------------------------------------------- (T-25 Rev.2) 鯨の発生阻止
def test_group_long_silence_no_whale_session() -> None:
    DIARY_MD.write_text("", encoding="utf-8")
    base = date(2026, 1, 1)
    body: list[str] = []
    for i in range(40):  # T-24 の span 閾値 (30日) を超える長期間
        d = base + timedelta(days=i)
        sender = ["Aさん", "Bさん", "Cさん"][i % 3]  # 3人以上 -> グループ判定
        body.append(f"{d.strftime('%Y/%m/%d')}(月)")
        body.append(f"09:00\t{sender}\tメッセージ{i}")
    raw = f"[LINE] グループWとのトーク履歴\n保存日時：2026/07/07 12:00\n\n" + "\n".join(body) + "\n"
    _write(raw)
    msgs = profiler.load_line_messages()
    sessions = extract_conversation_sessions(msgs)  # 例外を投げず、セッションも作らない
    assert sessions == [], sessions
    logs = extract_group_daily_logs(msgs)
    assert len(logs) == 40, len(logs)
    print("  long-silent group produces no whale session, tripwire silent (T-25 Rev.2) OK")


# ---------------------------------------------------------------- (T-25 Rev.2) 隔離ガード
def test_group_speech_isolated_from_self_channels() -> None:
    DIARY_MD.write_text("日記本文サンプル", encoding="utf-8")
    dyad = _block("友人", "2026/07/01", [
        ("09:00", "友人", "元気?"), ("09:05", "自分", "元気だよ"),
    ])
    group = _block("グループV", "2026/07/01", [
        ("10:00", "Aさん", "極秘の予定"), ("10:01", "Bさん", "了解"),
        ("10:02", "Cさん", "OK"),
    ])
    _write(dyad + group)
    contexts = load_daily_contexts()
    dc = next(c for c in contexts if c["date"] == "2026-07-01")
    assert "極秘の予定" not in dc["line_self_text"], "グループ発言が line_self_text に混入した"
    assert "極秘の予定" not in dc["self_text"], "グループ発言が self_text に混入した"
    assert "極秘の予定" in dc["group_line_text"]
    assert dc["has_line"] is True  # dyad は正常検出 (グループ判定に巻き込まれない)
    print("  group speech isolated from line_self_text/self_text (T-25 Rev.2) OK")


# ---------------------------------------------------------------- (T-25 Rev.2) dyad 不変
def test_dyad_behavior_unchanged_by_group_logic() -> None:
    DIARY_MD.write_text("", encoding="utf-8")
    dyad = _block("友人", "2026/07/01", [
        ("09:00", "友人", "元気?"), ("09:05", "自分", "元気だよ"),
    ])
    _write(dyad)
    msgs = profiler.load_line_messages()
    sessions = extract_conversation_sessions(msgs)
    assert len(sessions) == 1, sessions
    assert sessions[0]["contact"] == "友人"
    assert sessions[0]["turn_count"] == 2
    print("  dyad behavior unchanged by group-chat logic (T-25 Rev.2) OK")


if __name__ == "__main__":
    test_duplicate_export_reimport_count_invariant()
    test_partial_overlap_union()
    test_genuine_burst_within_block_preserved()
    test_is_self_resolved_via_common_sender_across_contacts()
    test_is_self_declared_name_takes_priority()
    test_format_line_import_injects_header_when_missing()
    test_date_regression_without_header_splits_block()
    test_group_chat_excluded_from_session_extraction()
    test_group_chat_does_not_poison_is_self_tier3()
    test_session_day_slicing_no_duplication_across_span()
    test_session_tripwire_fires_on_pathological_span()
    test_group_message_attached_exactly_once_per_date()
    test_group_long_silence_no_whale_session()
    test_group_speech_isolated_from_self_channels()
    test_dyad_behavior_unchanged_by_group_logic()
    print("test_line_dedup: ALL PASS")
