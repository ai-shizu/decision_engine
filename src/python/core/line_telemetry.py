#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
core/line_telemetry.py — 対人プロトコル・テレメトリ (Target Delta-LINE / DL1)
==================================================================================
LINE 全ログ (本人+他者) を「人間関係の物理的衝突ログ」として解析する第3チャネル。
docs/SPEC_CHARLIE_DELTA.md §3.9 の実装。

不変条件 (AI_SKILLS §10 / SPEC I-14〜I-16 を継承):
  - 決定論のみ。感情推定に LLM を使わない (統治原則1)。
  - 第三者の実名は保存しない。salt 付き一方向 alias のみ (I-15)。
  - 主観コーパス (日記+相談) / 客観コーパス (家計簿+予定+line_self_text) の
    定義は変更しない。ここで読む「全ログ」はこの第3チャネル専用 (I-16)。
  - グループチャットは v1 では全指標から除外する (多者間帰属の曖昧性 = 罠 T-11)。

【実装上の訂正】SPEC §3.9.1 は「パーサ新設不要 (data_merger._session_from_buffer
を流用)」としていたが、既存の extract_conversation_sessions() は「相手が発言 →
本人が返信」の一方向のみを対象にした状態機械であり (本人が会話を開始した
バーストは構造的に捨てられる)、initiation_ratio の計測には使えないことが
実装時に判明した。本モジュールは対称なバースト抽出を独自に行う。
"""

from __future__ import annotations

import hashlib
import json
import os
import re
from datetime import datetime, timedelta

from .data_merger import _parse_dt, normalize_date
from .canonicalization import canonicalize_json, canonicalize_text
from .durable_persistence import durable_atomic_write_text, read_json_file
from .paths import DATA_PROCESSED
from .secure_identity import (
    contact_short_id as _contact_short_id,
    keyed_text_identity,
    validate_contact_identity,
)

# ---------------------------------------------------------------- 時間閾値
BURST_GAP = timedelta(minutes=30)          # 同一発話者の連続メッセージを1バーストにまとめる間隔
CONVERSATION_GAP = timedelta(hours=6)      # これを超える沈黙で「新しい会話」とみなす (initiation 判定用)
THREAD_DEATH = timedelta(hours=72)         # 摩擦シグナル(c): 活動中dyadでこれ以上無通信ならスレッド死
NIGHT_START, NIGHT_END = 23, 8             # 深夜窓 (到着時刻がこの範囲なら latency 計測から除外)
LATENCY_OUTLIER_HOURS = 48                 # これを超える遅延はレイテンシではなくスレッド死として扱う
MIN_EXCHANGES = 20                         # 軸を確定させる最低の応答ペア数 (confidence ゲート)

TELEMETRY_PATH = DATA_PROCESSED / "line_telemetry.json"
TELEMETRY_FORMAT = "line_telemetry.v2"

# ---------------------------------------------------------------- 語彙表
# gap_analysis.py の GUILT_MARKERS/PRODUCTIVITY_MARKERS と同じ配置規約 (平文リスト)。
FRICTION_MARKERS = [
    "むかつく", "イライラ", "うざい", "最悪", "許せない", "冷たい", "無視",
    "ひどい", "怒って", "距離を置き", "もういい", "疲れた", "面倒くさい人",
]
APOLOGY_MARKERS = ["ごめん", "すまん", "申し訳", "悪かった"]
ESCALATION_MARKERS = ["は?", "意味わからん", "冗談じゃ", "知らない", "勝手にすれば"]
REPAIR_MARKERS = ["大丈夫?", "話せる?", "誤解", "ちゃんと話", "仲直り"]
FORMALITY_MARKERS = ["です", "ます", "いただ", "恐れ入り", "よろしくお願い", "でしょうか"]
CASUAL_MARKERS = ["だよ", "じゃん", "www", "笑", "だね", "っしょ", "やばい"]
_STAMP_RE = re.compile(r"\[スタンプ\]|\[スタンプ:.*?\]|^\(スタンプ\)$")

# DL2: 一人称×二人称の衝突 (social_positioning_gap) 用語彙
SELF_ROLE_LISTENER_MARKERS = ["聞き役", "相談役", "頼られる", "話を聞いて", "支える側", "聞いてあげる"]
RELATIONSHIP_EFFORT_MARKERS = ["気を遣う", "気疲れ", "連絡を絶やさない", "誘うのはいつも",
                                "関係を保つ", "疎遠にならない"]
SOCIAL_POSITIONING_THEME = "対人関係・役割認識"


# ---------------------------------------------------------------- 第三者最小化 (I-15)
class AliasCollisionError(RuntimeError):
    """Distinct canonical contacts produced one persistent identity."""


def contact_alias(contact: str) -> str:
    """Return the authoritative 256-bit keyed identity for one contact."""
    digest = keyed_text_identity(
        contact,
        domain=b"decision-engine/line-contact-identity/v2",
    )
    return "C-" + digest.hex()


def contact_short_id(identity: str) -> str:
    return _contact_short_id(identity)


# ---------------------------------------------------------------- バースト抽出
def _bursts_by_contact(line_messages: list[dict]) -> dict[str, list[dict]]:
    """1:1 dyad ごとに時系列バーストへ分割する。

    バースト = 同一発話者 (is_self) の連続メッセージで、間隔が BURST_GAP 以内。
    話者が変わる、または間隔が BURST_GAP を超えると新バーストになる。
    グループチャット (contact 名にグループ判定が付くもの) は呼び出し側で除外済みの前提。
    """
    grouped: dict[str, list[dict]] = {}
    for m in line_messages:
        d = normalize_date(m.get("date") or "")
        contact = canonicalize_text(str(m.get("contact", "")))
        if not d or not m.get("time") or not contact:
            continue
        grouped.setdefault(contact, []).append(
            {**m, "contact": contact, "date": d, "dt": _parse_dt(d, m["time"])})

    out: dict[str, list[dict]] = {}
    for contact, msgs in grouped.items():
        msgs.sort(key=lambda x: x["dt"])
        bursts: list[dict] = []
        cur: dict | None = None
        for msg in msgs:
            if cur is not None and msg["is_self"] == cur["is_self"] and \
                    msg["dt"] - cur["end"] <= BURST_GAP:
                cur["end"] = msg["dt"]
                cur["texts"].append(msg["text"])
            else:
                if cur is not None:
                    bursts.append(cur)
                cur = {"is_self": msg["is_self"], "start": msg["dt"], "end": msg["dt"],
                       "texts": [msg["text"]]}
        if cur is not None:
            bursts.append(cur)
        out[contact] = bursts
    return out


def _is_night_arrival(dt: datetime) -> bool:
    return dt.hour >= NIGHT_START or dt.hour < NIGHT_END


# ---------------------------------------------------------------- 摩擦応答分類
def _classify_friction_response(reply_texts: list[str], reply_latency_ok: bool,
                                thread_died: bool) -> str:
    """多重シグナル (2-of-3) で摩擦成立が確定した後、本人の次アクションを分類する。
    avoid: 無応答/スレッド死。escalate: 否定語彙の増量。repair: 疑問形+修復語彙。
    appease: 謝罪 (非自責文脈)。優先順位は avoid > escalate > repair > appease。"""
    if thread_died or not reply_texts:
        return "avoid"
    blob = "".join(reply_texts)
    if any(k in blob for k in ESCALATION_MARKERS):
        return "escalate"
    if any(k in blob for k in REPAIR_MARKERS):
        return "repair"
    if any(k in blob for k in APOLOGY_MARKERS):
        return "appease"
    return "avoid" if not reply_latency_ok else "repair"


def _detect_friction_events(bursts: list[dict], user_median: float | None) -> list[dict]:
    """2-of-3 多重シグナル判定 (罠 T-11 対策: 単一キーワード判定は絶対にしない)。
      (a) FRICTION_MARKERS が相手のバーストに出現 (本人の発話は「反応」の対象外)
      (b) 本人の応答レイテンシが「本人自身の通常の返信速度 (user_median)」の3倍超
          — 他者の返信速度と比較するのではなく、本人の自己ベースラインからの
          逸脱を見る (「いつもより返信が遅い」を検出するのが目的)
      (c) スレッド死 (直後 72h 無通信) または直後バーストに謝罪マーカー

    「直後バースト」は必ず bursts[i+1] (話者を問わず、間に他のバーストを
    挟まない) を使う。何バーストも先まで自分の発話を探しに行くと、この
    摩擦点とは無関係な後日の応答を誤って紐付けてしまう (T-11 系の誤帰属)。
    """
    events = []
    for i, b in enumerate(bursts):
        if b["is_self"]:
            continue   # 本人の発話は「反応」ではないため対象外
        if not any(k in "".join(b["texts"]) for k in FRICTION_MARKERS):
            continue
        nxt = bursts[i + 1] if i + 1 < len(bursts) else None
        thread_died = nxt is None or (nxt["start"] - b["end"] >= THREAD_DEATH)
        sig_b = False
        if nxt is not None and nxt["is_self"] and user_median:
            gap_min = (nxt["start"] - b["end"]).total_seconds() / 60
            sig_b = gap_min > user_median * 3
        sig_c = thread_died or (nxt is not None and any(
            k in "".join(nxt["texts"]) for k in APOLOGY_MARKERS))
        if sum([sig_b, sig_c]) >= 1:   # (a) は成立済みなので残り2条件から1つで2-of-3達成
            response = _classify_friction_response(
                nxt["texts"] if (nxt is not None and nxt["is_self"]) else [],
                not sig_b, thread_died)
            events.append({"at": b["end"].isoformat(), "response": response})
    return events


# ---------------------------------------------------------------- dyad 集計
def _formality_index(texts: list[str]) -> float:
    if not texts:
        return 0.5
    blob = "".join(texts)
    formal = sum(blob.count(k) for k in FORMALITY_MARKERS)
    casual = sum(blob.count(k) for k in CASUAL_MARKERS)
    total = formal + casual
    return formal / total if total else 0.5


def _median(values: list[float]) -> float | None:
    if not values:
        return None
    s = sorted(values)
    n = len(s)
    mid = n // 2
    return s[mid] if n % 2 else (s[mid - 1] + s[mid]) / 2.0


def compute_dyad_stats(line_messages: list[dict], *, group_contacts: set[str] | None = None
                       ) -> list[dict]:
    """dyad (1:1 トークルーム) ごとの決定論的集計を返す。グループチャットは除外 (v1)。"""
    group_contacts = {
        canonicalize_text(str(contact)) for contact in (group_contacts or set())
    }
    by_contact = {
        contact: bursts
        for contact, bursts in _bursts_by_contact(line_messages).items()
        if contact not in group_contacts
    }

    results: list[dict] = []
    identity_owners: dict[str, str] = {}
    for contact, bursts in by_contact.items():
        if len(bursts) < 2:
            continue
        user_latencies: list[float] = []    # 相手バースト終端 -> 本人バースト開始 (分)
        peer_latencies: list[float] = []    # 本人バースト終端 -> 相手バースト開始 (分)
        conversations_total = 0
        conversations_self_started = 0
        prev_end: datetime | None = None
        for i, b in enumerate(bursts):
            if prev_end is None or b["start"] - prev_end > CONVERSATION_GAP:
                conversations_total += 1
                if b["is_self"]:
                    conversations_self_started += 1
            if i > 0:
                prev = bursts[i - 1]
                if prev["is_self"] != b["is_self"]:
                    gap_min = (b["start"] - prev["end"]).total_seconds() / 60
                    is_outlier = gap_min > LATENCY_OUTLIER_HOURS * 60
                    # 着信 (prev の終端) が深夜窓なら除外する。返信時刻ではなく
                    # 「相手が気づけなかった時間帯にメッセージが来たか」が交絡因子。
                    is_night = _is_night_arrival(prev["end"])
                    if not is_outlier and not is_night:
                        if b["is_self"]:
                            user_latencies.append(gap_min)
                        else:
                            peer_latencies.append(gap_min)
            prev_end = b["end"]

        exchanges = len(user_latencies) + len(peer_latencies)
        user_texts = [t for b in bursts if b["is_self"] for t in b["texts"]]
        peer_texts = [t for b in bursts if not b["is_self"] for t in b["texts"]]
        user_chars = sum(len(t) for t in user_texts)
        peer_chars = sum(len(t) for t in peer_texts)
        user_median = _median(user_latencies)
        peer_median = _median(peer_latencies)

        friction_events = _detect_friction_events(bursts, user_median)
        responses: dict[str, int] = {}
        for ev in friction_events:
            responses[ev["response"]] = responses.get(ev["response"], 0) + 1

        identity = contact_alias(contact)
        previous_owner = identity_owners.get(identity)
        if previous_owner is not None and previous_owner != contact:
            raise AliasCollisionError("persistent contact identity collision")
        identity_owners[identity] = contact
        results.append({
            "contact_alias": identity,
            "contact_short_id": contact_short_id(identity),
            "exchanges": exchanges,
            "user_reply_median_min": _median(user_latencies),
            "peer_reply_median_min": peer_median,
            "initiation_ratio": (conversations_self_started / conversations_total
                                 if conversations_total else None),
            "user_msg_share": (user_chars / (user_chars + peer_chars)
                               if (user_chars + peer_chars) else None),
            "formality_index": _formality_index(user_texts),
            "friction_events": len(friction_events),
            "friction_responses": responses,
        })
    return results


# ---------------------------------------------------------------- 3軸の決定論的計算
def _sigmoid(x: float) -> float:
    import math
    return 1.0 / (1.0 + math.exp(-x))


def compute_interpersonal_axes(dyads: list[dict]) -> dict:
    """対人プロトコル3軸 (F/L/P) を dyads から算出する。
    標本数不足 (exchanges < MIN_EXCHANGES) の dyad は各軸から除外し、
    有効 dyad が無ければ score=None (confidence=0) を返す (gap_analysis の
    data_sufficiency と同じ「断定を避ける」規約)。"""
    eligible = [d for d in dyads if d["exchanges"] >= MIN_EXCHANGES]

    # F: friction_response (avoid+0.5*appease の比率の全dyad平均)
    friction_scores = []
    for d in eligible:
        r = d["friction_responses"]
        total = sum(r.values())
        if total:
            friction_scores.append((r.get("avoid", 0) + 0.5 * r.get("appease", 0)) / total)
    friction_axis = {
        "score": (sum(friction_scores) / len(friction_scores)) if friction_scores else None,
        "confidence": min(1.0, len(friction_scores) / 3.0),
        "n_dyads": len(friction_scores),
    }

    # L: latency_asymmetry (log(peer/user) の dyad中央値 -> sigmoid)
    log_ratios = []
    for d in eligible:
        u, p = d["user_reply_median_min"], d["peer_reply_median_min"]
        if u and p and u > 0 and p > 0:
            import math
            log_ratios.append(math.log(p / u))
    latency_axis = {
        "score": _sigmoid(_median(log_ratios)) if log_ratios else None,
        "confidence": min(1.0, len(log_ratios) / 3.0),
        "n_dyads": len(log_ratios),
    }

    # P: protocol_plasticity (formality_index の dyad間分散を正規化)
    formality_vals = [d["formality_index"] for d in eligible]
    plasticity_axis = {"score": None, "confidence": 0.0, "n_dyads": len(formality_vals)}
    if len(formality_vals) >= 3:
        mean = sum(formality_vals) / len(formality_vals)
        var = sum((v - mean) ** 2 for v in formality_vals) / len(formality_vals)
        plasticity_axis["score"] = min(1.0, var / 0.25)   # 分散0.25(理論最大)で正規化
        plasticity_axis["confidence"] = min(1.0, len(formality_vals) / 5.0)

    return {"friction_response": friction_axis, "latency_asymmetry": latency_axis,
            "protocol_plasticity": plasticity_axis}


# ---------------------------------------------------------------- DL2: 一人称×二人称の衝突
def _quote(text: str, kw: str) -> dict | None:
    """gap_analysis.py の引用抽出 (start-15:end+25) と同じ切り出し規約に揃える。"""
    m = re.search(re.escape(kw), text)
    if not m:
        return None
    s = max(0, m.start() - 15)
    snippet = text[s:m.end() + 25].replace("\n", " ").strip()
    return {"quote": f"…{snippet}…"}


def analyze_social_positioning(daily: list[dict], dyads: list[dict], *,
                               min_dyads: int = 3) -> list[dict]:
    """自己認識 (日記・相談での役割自認) と LINE 実測の対人挙動を突合し、
    gap_analysis と同型の gap dict (type=intention_gap/blind_spot) を返す。

    既存の intention_gap/blind_spot 型名をそのまま使う (SPEC の当初案は新規
    type "social_positioning_gap" だったが、format_gap_table のラベル表を
    拡張せずに済み、_gap_section() の呼び出し隔離もそのまま効くため、
    既存2型への合流を選んだ — Architect's Override)。

    data_sufficiency ゲート: 有効 dyad (exchanges>=MIN_EXCHANGES) が
    min_dyads 未満なら空リストを返す (標本不足で断定しない)。
    """
    from .gap_analysis import GENUINE_DOC_MIN_WEIGHT, build_subjective_corpus

    corpus = build_subjective_corpus(daily)
    listener_hits = 0.0
    listener_quotes: list[dict] = []
    effort_hits = 0
    for doc in corpus:
        weight = float(doc.get("weight", 1.0))
        for kw in SELF_ROLE_LISTENER_MARKERS:
            if kw in doc["text"]:
                listener_hits += weight
                if weight >= GENUINE_DOC_MIN_WEIGHT and len(listener_quotes) < 3:
                    q = _quote(doc["text"], kw)
                    if q:
                        listener_quotes.append({"date": doc["date"], **q})
        for kw in RELATIONSHIP_EFFORT_MARKERS:
            if kw in doc["text"]:
                effort_hits += 1

    eligible = [d for d in dyads if d["exchanges"] >= MIN_EXCHANGES]
    gaps: list[dict] = []
    if len(eligible) < min_dyads:
        return gaps

    user_shares = [d["user_msg_share"] for d in eligible if d["user_msg_share"] is not None]
    initiation_ratios = [d["initiation_ratio"] for d in eligible
                         if d["initiation_ratio"] is not None]

    # 例1 (intention_gap): 「聞き役」自認 なのに会話占有率シェアが高い
    if listener_hits >= 2 and user_shares:
        med_share = _median(user_shares)
        if med_share is not None and med_share > 0.6:
            gaps.append({
                "theme": SOCIAL_POSITIONING_THEME, "type": "intention_gap",
                "gap": round(med_share - 0.3, 3),
                "subjective": {"quotes": listener_quotes, "hits": listener_hits},
                "objective": {"median_user_msg_share": round(med_share, 3),
                             "n_dyads": len(user_shares)},
                "insight": (f"日記・相談で「聞き役」を自認する記述があるが、"
                           f"実際の会話量シェアは{len(user_shares)}件のdyad中央値で"
                           f"{med_share:.0%}と、自分の方が多く話している。"),
            })

    # 例2 (blind_spot): 会話開始が本人に偏るのに関係維持コストへの言及がゼロ
    if initiation_ratios:
        med_init = _median(initiation_ratios)
        if med_init is not None and med_init > 0.75 and effort_hits == 0:
            gaps.append({
                "theme": SOCIAL_POSITIONING_THEME, "type": "blind_spot",
                "gap": round(med_init - 0.5, 3),
                "subjective": {"quotes": [], "hits": 0},
                "objective": {"median_initiation_ratio": round(med_init, 3),
                             "n_dyads": len(initiation_ratios)},
                "insight": (f"{len(initiation_ratios)}件のdyadで会話開始の中央値"
                           f"{med_init:.0%}を本人が占めているが、関係維持の労力に"
                           "ついて日記・相談での言及が一度もない。"),
            })
    return gaps


# ---------------------------------------------------------------- Bounty (D3 の資源循環用)
BOUNTY_PATH = DATA_PROCESSED / "bounty_store.json"
BOUNTY_TENSION_THRESHOLD = 0.3   # この値以上の |gap| を持つギャップだけを Bounty 化する


def _bounty_id(theme: str, gtype: str, insight: str) -> str:
    canonical = canonicalize_json({
        "insight": insight,
        "theme": theme,
        "type": gtype,
    }).encode("utf-8")
    h = hashlib.blake2b(
        b"decision-engine/bounty-id/v2\0" + canonical,
        digest_size=6,
    )
    return "bt-" + h.hexdigest()


def _save_bounties(bounties: list[dict]) -> None:
    BOUNTY_PATH.parent.mkdir(parents=True, exist_ok=True)
    tmp = BOUNTY_PATH.with_suffix(".json.tmp")
    tmp.write_text(json.dumps(bounties, ensure_ascii=False, indent=2), encoding="utf-8")
    os.replace(tmp, BOUNTY_PATH)


def register_bounties(gaps: list[dict]) -> list[dict]:
    """tension (|gap|) が閾値以上のギャップを Bounty として永続化する。

    Bounty 自体はここでは「存在するだけ」— 実際の質問キュー化・面接官への
    無菌化注入は D3 (Puppeteer) の責務。ここで生成する Bounty の中身
    (theme/insight 等) が面接官コンテキストへ渡ることは絶対にない
    (I-14: 講評フェーズ以外で Bounty の存在自体を面接官に見せない)。
    """
    existing = load_bounties()
    by_id = {b["id"]: b for b in existing}
    for g in gaps:
        tension = abs(g.get("gap", 0.0))
        if tension < BOUNTY_TENSION_THRESHOLD:
            continue
        bid = _bounty_id(g["theme"], g["type"], g.get("insight", ""))
        if bid in by_id:
            continue   # 同一内容の Bounty は重複登録しない (安定 ID)
        by_id[bid] = {
            "id": bid, "theme": g["theme"], "type": g["type"],
            "tension": round(tension, 3), "status": "open",
            "bank_question_id": None,
        }
    result = list(by_id.values())
    _save_bounties(result)
    return result


def mark_bounty_status(bounty_id: str, status: str, *,
                       bank_question_id: str | None = None) -> None:
    """Bounty の状態遷移 (open -> queued -> resolved) を記録する。

    D3 (Puppeteer) が質問を選んだ時に "queued" + bank_question_id を、
    面接講評が終わった時に "resolved" を書き込む。ここで書き込む内容は
    ID・状態・バンク質問IDのみ — insight/theme等の生テキストは扱わない。
    """
    bounties = load_bounties()
    for b in bounties:
        if b["id"] == bounty_id:
            b["status"] = status
            if bank_question_id is not None:
                b["bank_question_id"] = bank_question_id
            break
    else:
        return
    _save_bounties(bounties)


def load_bounties() -> list[dict]:
    if BOUNTY_PATH.exists():
        return json.loads(BOUNTY_PATH.read_text(encoding="utf-8"))
    return []


# ---------------------------------------------------------------- 永続化
def sync_line_telemetry(*, group_contacts: set[str] | None = None) -> dict:
    """line_history.txt から dyad 統計 + 対人3軸を計算し永続化する。
    再計算トリガは import.line のみ (AI_SKILLS §1 の profiler 自動実行規約に相乗り —
    RECORD 保存・consult では呼ぶな)。"""
    from . import profiler
    messages = profiler.load_line_messages()
    dyads = compute_dyad_stats(messages, group_contacts=group_contacts)
    axes = compute_interpersonal_axes(dyads)
    payload = {"format": TELEMETRY_FORMAT, "dyads": dyads, "interpersonal": axes}
    durable_atomic_write_text(TELEMETRY_PATH, canonicalize_json(payload))
    return payload


def load_line_telemetry() -> dict:
    try:
        payload = read_json_file(TELEMETRY_PATH)
    except FileNotFoundError:
        return {"format": TELEMETRY_FORMAT, "dyads": [], "interpersonal": {}}
    if type(payload) is not dict or payload.get("format") != TELEMETRY_FORMAT:
        raise ValueError("legacy or invalid LINE telemetry must be rebuilt")
    dyads = payload.get("dyads")
    if type(dyads) is not list:
        raise ValueError("LINE telemetry dyads must be list")
    for dyad in dyads:
        if type(dyad) is not dict:
            raise ValueError("LINE telemetry dyad must be object")
        identity = validate_contact_identity(dyad.get("contact_alias"))
        if dyad.get("contact_short_id") != contact_short_id(identity):
            raise ValueError("LINE telemetry short ID mismatch")
    return payload
