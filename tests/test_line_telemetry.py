# -*- coding: utf-8 -*-
"""対人プロトコル・テレメトリ (Target Delta-LINE / DL1) の決定論的テスト。

外部依存なし (stdlib のみ、LLM 不使用)。実行は一時 PKB_PROJECT_ROOT 上で行い、
実データには一切触れない。

対象: docs/SPEC_CHARLIE_DELTA.md §3.9 (Delta-LINE)。
  - 第三者最小化 (I-15): alias の不可逆性・安定性
  - initiation_ratio / レイテンシ非対称の基礎集計
  - 摩擦検出の 2-of-3 多重シグナル (単一キーワード判定の禁止 = 罠 T-11 の中核対策)
  - T-11 対照群: 親密 dyad (負の語彙が多いが即レス・継続) はフラグしない
  - T-12 対照群: 深夜到着はレイテンシ計測から除外される
  - 3軸の confidence ゲート (標本不足では score を確定させない)
"""
from __future__ import annotations

import os
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

# SPEC_FOXTROT_UI.md §10.1 (F-15): pytest 経由では tests/conftest.py がテスト
# 収集より前に PKB_PROJECT_ROOT を Sandbox へ設定済み。setdefault により
# それを尊重しつつ、本ファイルを単独実行 (`python tests/test_line_telemetry.py`)
# した場合の後方互換 (自前の一時ルート) も両立する (W-50: 上書きしない)。
_TMP = tempfile.mkdtemp(prefix="pkb_line_telemetry_")
os.environ.setdefault("PKB_PROJECT_ROOT", _TMP)
os.environ.setdefault("HF_HUB_OFFLINE", "1")
os.environ.setdefault("TRANSFORMERS_OFFLINE", "1")

from core import line_telemetry as lt  # noqa: E402
from core.paths import DATA_RAW, LINE_HISTORY  # noqa: E402


def _day(date: str, diary: str = "", queries: list[str] | None = None) -> dict:
    return {"date": date, "diary_text": diary,
            "consultations": [{"query": q} for q in (queries or [])]}


def _dyad(exchanges: int = 25, user_msg_share: float | None = 0.5,
         initiation_ratio: float | None = 0.5) -> dict:
    return {"contact_alias": "C-test", "exchanges": exchanges,
            "user_reply_median_min": 5.0, "peer_reply_median_min": 5.0,
            "initiation_ratio": initiation_ratio, "user_msg_share": user_msg_share,
            "formality_index": 0.5, "friction_events": 0, "friction_responses": {}}


def _msg(contact: str, date: str, time: str, is_self: bool, text: str) -> dict:
    return {"source": "line", "contact": contact, "date": date, "time": time,
            "sender": "自分" if is_self else contact, "text": text, "is_self": is_self}


# ---------------------------------------------------------------- 第三者最小化
def test_alias_irreversible_and_stable() -> None:
    a1 = lt.contact_alias("山田太郎")
    a2 = lt.contact_alias("山田太郎")
    assert a1 == a2, "OS-protected root key produces a stable identity"
    assert "山田" not in a1 and "太郎" not in a1
    assert len(a1) == 66 and a1.startswith("C-")
    short = lt.contact_short_id(a1)
    assert short.startswith("C~") and len(short) == 14
    assert short != a1
    print("  alias irreversible + stable OK")


# ---------------------------------------------------------------- バースト/集計基礎
def test_burst_extraction_merges_same_speaker() -> None:
    msgs = [
        _msg("友人A", "2026-07-01", "10:00", False, "おはよう"),
        _msg("友人A", "2026-07-01", "10:05", False, "元気?"),   # 5分後 = 同一バースト
        _msg("友人A", "2026-07-01", "10:10", True, "元気だよ"),
        _msg("友人A", "2026-07-01", "12:00", True, "そういえば"),  # 110分後・同一話者 = 新バースト
    ]
    bursts = lt._bursts_by_contact(msgs)["友人A"]
    assert len(bursts) == 3
    assert bursts[0]["is_self"] is False and len(bursts[0]["texts"]) == 2
    assert bursts[1]["is_self"] is True and len(bursts[1]["texts"]) == 1
    assert bursts[2]["is_self"] is True
    print("  burst extraction merges same-speaker OK")


def test_initiation_ratio_symmetric() -> None:
    """自分が起点の会話と相手が起点の会話、両方を正しく数えられるか。
    (extract_conversation_sessions は相手起点しか捉えられない旧実装だった —
    Delta-LINE 独自のバースト抽出がこれを解決している回帰確認)"""
    msgs = [
        # 会話1: 自分が起点 (CONVERSATION_GAP=6h 空けて開始)
        _msg("友人B", "2026-07-01", "09:00", True, "ちょっと相談あるんだけど"),
        _msg("友人B", "2026-07-01", "09:10", False, "なになに"),
        # 会話2: 相手が起点 (7時間後)
        _msg("友人B", "2026-07-01", "16:30", False, "今日空いてる?"),
        _msg("友人B", "2026-07-01", "16:40", True, "空いてるよ"),
    ]
    dyads = lt.compute_dyad_stats(msgs)
    d = next(d for d in dyads if d["exchanges"] >= 1)
    assert d["initiation_ratio"] == 0.5, d
    print("  initiation_ratio symmetric OK")


def test_night_arrival_excluded_from_latency() -> None:
    """T-12: 深夜到着 (23:00-08:00) への返信はレイテンシ計測から除外する
    (生活リズムをレイテンシ非対称と誤認しない)。"""
    msgs = [
        _msg("友人C", "2026-07-01", "23:30", False, "寝る前にひとこと"),
        _msg("友人C", "2026-07-02", "08:30", True, "おはよう返信"),  # 深夜到着への返信 = 除外対象
        _msg("友人C", "2026-07-02", "12:00", False, "お昼だよ"),
        _msg("友人C", "2026-07-02", "12:05", True, "了解"),          # 通常時間帯 = 採用
    ]
    dyads = lt.compute_dyad_stats(msgs)
    d = dyads[0]
    # 深夜到着ペアが除外され、通常ペアの5分だけが user_reply に採用される
    assert d["user_reply_median_min"] == 5.0, d
    print("  night arrival excluded from latency OK")


# ---------------------------------------------------------------- 摩擦検出 (核心)
def test_friction_requires_multi_signal_not_keyword_alone() -> None:
    """罠 T-11 の中核: 摩擦語彙が出ても、即レス・スレッド継続・謝罪なしなら
    フラグしない (単一シグナルでは検出しない)。"""
    msgs = [
        _msg("友人D", "2026-07-01", "10:00", False, "今日は最悪だった"),  # 摩擦語彙 (a)
        _msg("友人D", "2026-07-01", "10:02", True, "そうなんだ、大変だったね"),  # 即レス (b不成立)
        _msg("友人D", "2026-07-01", "10:10", False, "うん、聞いてくれてありがとう"),  # 継続 (c不成立)
        _msg("友人D", "2026-07-01", "10:12", True, "いつでも聞くよ"),
    ]
    dyads = lt.compute_dyad_stats(msgs)
    d = dyads[0]
    assert d["friction_events"] == 0, "単一シグナルなのにフラグされている"
    print("  friction requires multi-signal (T-11 core) OK")


def test_friction_detected_with_slow_reply_and_thread_death() -> None:
    """摩擦語彙 + (自己ベースラインより大幅に遅い返信) + スレッド死 の複合で検出。"""
    msgs = [
        # 通常時の user_median を確立 (5分の速い返信を複数回)
        _msg("友人E", "2026-06-01", "10:00", False, "調子どう"),
        _msg("友人E", "2026-06-01", "10:05", True, "元気だよ"),
        _msg("友人E", "2026-06-02", "10:00", False, "今日暇?"),
        _msg("友人E", "2026-06-02", "10:05", True, "暇だよ"),
        _msg("友人E", "2026-06-03", "10:00", False, "ランチ行く?"),
        _msg("友人E", "2026-06-03", "10:05", True, "行くよ"),
        # 摩擦バースト: 相手が強い否定語彙 → 本人が72h以上沈黙 (スレッド死)
        _msg("友人E", "2026-07-01", "20:00", False, "もういい、うざい"),
    ]
    dyads = lt.compute_dyad_stats(msgs)
    d = dyads[0]
    assert d["friction_events"] == 1, d
    assert d["friction_responses"].get("avoid") == 1, d["friction_responses"]
    print("  friction detected with thread death OK")


def test_friction_appease_response_classification() -> None:
    msgs = [
        _msg("友人F", "2026-06-01", "10:00", False, "元気?"),
        _msg("友人F", "2026-06-01", "10:05", True, "元気だよ"),
        _msg("友人F", "2026-07-01", "20:00", False, "冷たいこと言うね、傷ついた"),
        _msg("友人F", "2026-07-01", "20:03", True, "ごめん、そんなつもりじゃなかった"),
        _msg("友人F", "2026-07-01", "20:10", False, "うん、大丈夫"),
    ]
    dyads = lt.compute_dyad_stats(msgs)
    d = dyads[0]
    assert d["friction_events"] == 1
    assert d["friction_responses"].get("appease") == 1, d["friction_responses"]
    print("  friction appease classification OK")


# ---------------------------------------------------------------- 3軸の confidence ゲート
def test_interpersonal_axes_confidence_gate() -> None:
    """exchanges < MIN_EXCHANGES の dyad は3軸の計算対象から除外される。"""
    sparse_dyads = [{"exchanges": 3, "user_reply_median_min": 5.0,
                     "peer_reply_median_min": 5.0, "initiation_ratio": 0.5,
                     "user_msg_share": 0.5, "formality_index": 0.5,
                     "friction_events": 0, "friction_responses": {}}]
    axes = lt.compute_interpersonal_axes(sparse_dyads)
    assert axes["friction_response"]["score"] is None
    assert axes["friction_response"]["confidence"] == 0.0
    assert axes["latency_asymmetry"]["score"] is None
    assert axes["protocol_plasticity"]["score"] is None
    print("  interpersonal axes confidence gate OK")


def test_interpersonal_axes_computed_with_enough_dyads() -> None:
    def _dyad(user_med, peer_med, formality, friction=None):
        return {"exchanges": lt.MIN_EXCHANGES, "user_reply_median_min": user_med,
                "peer_reply_median_min": peer_med, "initiation_ratio": 0.5,
                "user_msg_share": 0.5, "formality_index": formality,
                "friction_events": 1 if friction else 0,
                "friction_responses": ({friction: 1} if friction else {})}

    dyads = [_dyad(5.0, 5.0, 0.9, "avoid"), _dyad(10.0, 40.0, 0.1, "repair"),
             _dyad(5.0, 5.0, 0.5, None)]
    axes = lt.compute_interpersonal_axes(dyads)
    assert axes["friction_response"]["score"] is not None
    assert 0.0 <= axes["friction_response"]["score"] <= 1.0
    assert axes["latency_asymmetry"]["score"] is not None
    assert axes["protocol_plasticity"]["score"] is not None
    assert axes["protocol_plasticity"]["score"] > 0   # formality にばらつきがある
    print("  interpersonal axes computed with enough dyads OK")


# ---------------------------------------------------------------- 永続化 + グループ除外
def _write_line_history(sessions: list[tuple[str, str, list[tuple[str, str, str]]]]) -> None:
    """[(contact, date, [(time, sender, text), ...]), ...] を LINE_HISTORY 形式で書く。"""
    lines = []
    for contact, date, msgs in sessions:
        lines.append(f"[LINE] {contact}とのトーク履歴")
        lines.append(date.replace("-", "/"))
        for time_, sender, text in msgs:
            lines.append(f"{time_}\t{sender}\t{text}")
    DATA_RAW.mkdir(parents=True, exist_ok=True)
    LINE_HISTORY.write_text("\n".join(lines), encoding="utf-8")


def test_sync_persists_and_excludes_group_contacts() -> None:
    _write_line_history([
        ("友人G", "2026-07-01", [("10:00", "友人G", "やあ"), ("10:05", "自分", "どうも")]),
        ("チーム雑談", "2026-07-01", [("11:00", "田中", "会議です"), ("11:05", "自分", "了解")]),
    ])
    payload = lt.sync_line_telemetry(group_contacts={"チーム雑談"})
    assert lt.TELEMETRY_PATH.exists()
    aliases = {d["contact_alias"] for d in payload["dyads"]}
    # グループチャットは除外され、1:1 の「友人G」だけが dyad として残る
    assert len(payload["dyads"]) == 1
    loaded = lt.load_line_telemetry()
    assert loaded["dyads"][0]["contact_alias"] in aliases
    print("  sync persists + excludes group contacts OK")


# ---------------------------------------------------------------- DL2: 一人称×二人称の衝突
def test_social_positioning_intention_gap() -> None:
    """「聞き役」自認 なのに実際の会話占有率が高い → intention_gap。"""
    daily = [
        _day("2026-07-01", diary="私はいつも友達の聞き役だと思う。"),
        _day("2026-07-02", queries=["自分は相談役に向いてるでしょうか"]),
    ]
    dyads = [_dyad(user_msg_share=0.7), _dyad(user_msg_share=0.75),
             _dyad(user_msg_share=0.65)]
    gaps = lt.analyze_social_positioning(daily, dyads)
    hit = next((g for g in gaps if g["type"] == "intention_gap"), None)
    assert hit is not None, gaps
    assert hit["theme"] == lt.SOCIAL_POSITIONING_THEME
    assert hit["gap"] > 0
    assert hit["subjective"]["quotes"], "主観の証拠引用が空"
    print("  social positioning intention_gap OK")


def test_social_positioning_blind_spot() -> None:
    """会話開始が本人に極端に偏るのに、関係維持コストへの言及が皆無 → blind_spot。"""
    daily = [_day("2026-07-01", diary="今日は普通の一日だった。特に何も。")]
    dyads = [_dyad(initiation_ratio=0.9), _dyad(initiation_ratio=0.85),
             _dyad(initiation_ratio=0.8)]
    gaps = lt.analyze_social_positioning(daily, dyads)
    hit = next((g for g in gaps if g["type"] == "blind_spot"), None)
    assert hit is not None, gaps
    assert hit["objective"]["median_initiation_ratio"] > 0.75
    print("  social positioning blind_spot OK")


def test_social_positioning_control_group_no_false_positive() -> None:
    """対照群: 自認と実測が一致していれば (「聞き役」自認なし・会話量も対等、
    かつ関係維持コストへの言及がある) フラグしない (退化防止の回帰ガード)。"""
    daily = [_day("2026-07-01", diary="友達との連絡、気を遣うけど大事にしたい。")]
    dyads = [_dyad(user_msg_share=0.5, initiation_ratio=0.5) for _ in range(3)]
    gaps = lt.analyze_social_positioning(daily, dyads)
    assert gaps == [], gaps
    print("  social positioning control group (no false positive) OK")


def test_social_positioning_insufficient_dyads() -> None:
    """有効 dyad が min_dyads 未満なら断定を避けて空を返す (data_sufficiency)。"""
    daily = [_day("2026-07-01", diary="私はいつも聞き役だと思う。")]
    dyads = [_dyad(user_msg_share=0.9), _dyad(user_msg_share=0.85)]  # 2件 < デフォルト3
    gaps = lt.analyze_social_positioning(daily, dyads)
    assert gaps == []
    print("  social positioning insufficient dyads OK")


# ---------------------------------------------------------------- Bounty
def test_register_bounties_threshold_and_stability() -> None:
    gaps = [
        {"theme": "テーマA", "type": "intention_gap", "gap": 0.5, "insight": "強い乖離"},
        {"theme": "テーマB", "type": "blind_spot", "gap": 0.1, "insight": "弱い乖離"},
    ]
    result = lt.register_bounties(gaps)
    assert len(result) == 1, result   # 閾値未満 (0.1) は Bounty 化されない
    bounty = result[0]
    assert bounty["theme"] == "テーマA" and bounty["status"] == "open"
    assert set(bounty) == {"id", "theme", "type", "tension", "status",
                           "bank_question_id"}, \
        "Bounty に insight/quotes 等の生テキストを含めるな (I-14 の面接官非開示と整合)"

    # 同一内容を再登録しても重複しない (安定 ID)
    again = lt.register_bounties(gaps)
    assert len(again) == 1
    assert again[0]["id"] == bounty["id"]

    loaded = lt.load_bounties()
    assert loaded == again
    print("  register_bounties threshold + stability OK")

