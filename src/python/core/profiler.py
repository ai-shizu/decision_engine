#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
マルチソース・プロファイリング・エンジン
==========================================
data/raw/diary.md (日記) と data/raw/line_history.txt (LINEトーク履歴) を
読み込みタグ付けし、4つの分析フレームワークで自己エミュレーション用の
深層プロファイル data/processed/deep_profile.json を生成・更新する。

  1. 認知的バイアス     : 語彙パターンから思考の癖を検出し、証拠付きで定量化
  2. 価値観の階層構造   : 行動を駆動する根源的欲求を頻度加重で抽出・ランク付け
  3. 感情的反応パターン : LINEデータから対人反応と感情の揺らぎ(分散)を定量化
  4. 意思決定アルゴリズム: 「状況A→行動/帰結B」の条件分岐ルールを共起マイニング

完全オフライン・決定論的(ルールベース)。外部APIは一切使用しない。
"""

from __future__ import annotations

import json
import re
import statistics
import sys
from collections import Counter
from datetime import datetime, date, timedelta
from .paths import DEEP_PROFILE, DIARY_MD, LINE_HISTORY, PROJECT_ROOT as ROOT, USER_PROFILE

DIARY = DIARY_MD
PROFILE_JSON = DEEP_PROFILE
USER_PROFILE_JSON = USER_PROFILE

# ============================================================ 語彙辞書
SENTIMENT_LEXICON = {
    # word: (valence -1.0〜+1.0, label)
    "嬉しい": (0.8, "joy"), "楽しい": (0.8, "joy"), "楽しみ": (0.7, "joy"),
    "最高": (0.9, "joy"), "面白い": (0.6, "joy"), "いいね": (0.5, "joy"),
    "ありがとう": (0.6, "gratitude"), "助かる": (0.5, "gratitude"), "感謝": (0.6, "gratitude"),
    "了解": (0.1, "neutral"), "なるほど": (0.2, "neutral"),
    "ごめん": (-0.3, "apology"), "すまん": (-0.3, "apology"), "申し訳": (-0.4, "apology"),
    "疲れた": (-0.5, "fatigue"), "しんどい": (-0.6, "fatigue"), "眠い": (-0.3, "fatigue"),
    "無理": (-0.5, "refusal"), "難しい": (-0.3, "difficulty"),
    "不安": (-0.6, "anxiety"), "心配": (-0.5, "anxiety"), "焦": (-0.5, "anxiety"),
    "イライラ": (-0.7, "anger"), "ムカ": (-0.7, "anger"),
    "辛い": (-0.7, "sadness"), "モヤモヤ": (-0.4, "sadness"),
}

BIAS_PATTERNS = {
    "サンクコスト効果": {
        "patterns": [r"せっかく", r"ここまでやった", r"今更", r"もったいない", r"引き返せ"],
        "description": "既に投じた時間・労力を理由に、撤退が合理的な局面でも継続を選ぶ傾向",
    },
    "確証バイアス": {
        "patterns": [r"やっぱり", r"案の定", r"思った通り", r"はずだ", r"間違いない"],
        "description": "既存の仮説を支持する情報を優先的に採用し、反証を軽視する傾向",
    },
    "損失回避": {
        "patterns": [r"失敗したくない", r"リスク", r"怖い", r"避けたい", r"守り"],
        "description": "同等の利得より損失を過大評価し、挑戦を先送りする傾向",
    },
    "現状維持バイアス": {
        "patterns": [r"いつも通り", r"とりあえず(続け|様子見)", r"変えな(い|くて)", r"今のまま"],
        "description": "変更コストを過大視し、デフォルトの選択肢に留まる傾向",
    },
    "過度の一般化": {
        "patterns": [r"いつも(?!通り)", r"絶対", r"全然", r"結局.*(だめ|ダメ|一番)"],
        "description": "少数の経験から普遍的な法則を導いてしまう傾向",
    },
}

VALUE_MAP = {
    "成長・熟達": {
        "keywords": ["学び", "学習", "挑戦", "新しい", "極め", "理解", "設計判断", "再認識"],
        "root_need": "有能感 (Competence)",
    },
    "効率・最適化": {
        "keywords": ["速く", "最適化", "効率", "性能", "高速", "計測", "ボトルネック", "サイクル"],
        "root_need": "有能感 (Competence) / 統制感",
    },
    "健康・持続可能性": {
        "keywords": ["運動", "睡眠", "体調", "ランニング", "筋トレ", "休養", "散歩"],
        "root_need": "生存基盤・長期パフォーマンスの維持",
    },
    "自律・裁量": {
        "keywords": ["自分で", "裁量", "自由", "意思決定", "判断", "選ぶ"],
        "root_need": "自己決定 (Autonomy)",
    },
    "つながり・信頼": {
        "keywords": ["家族", "妻", "友人", "飲み会", "感謝", "ありがとう", "一緒"],
        "root_need": "関係性 (Relatedness)",
    },
    "誠実さ・説明責任": {
        "keywords": ["丁寧", "説明", "ドキュメント", "理由を残す", "正直"],
        "root_need": "自己一致・信頼の獲得",
    },
}

# 状況→帰結 の共起マイニング用タグ
SITUATION_TAGS = {
    "睡眠不足": ["寝不足", "睡眠6時間", "睡眠不足"],
    "朝の運動実施": ["ランニング", "筋トレ", "運動した"],
    "会議過多": ["会議が多く"],
    "疲労蓄積": ["疲労", "疲れ", "肩こり"],
    "他者からの誘い": ["飲み会", "来れる", "誘い", "行かない?", "行かない？"],
    "実装に没頭中": ["実装が乗って", "集中したい", "没頭"],
}
OUTCOME_TAGS = {
    "午後も集中が持続": ["集中力が午後まで持続", "集中が持続"],
    "集中力の低下": ["集中が切れた", "集中できず", "判断の質が落ちる"],
    "実装時間の喪失": ["実装時間が取れなかった", "細切れ"],
    "入眠の改善": ["入眠が早い"],
    "婉曲的な辞退": ["パスする", "また今度", "今回はやめ", "ごめん"],
    "承諾": ["行くよ", "参加する", "いいよ"],
}

# 既知の (状況, 帰結) → 推奨行動 の変換テーブル
RULE_RECOMMENDATIONS = {
    ("睡眠不足", "集中力の低下"): "睡眠6時間以下の日は、重要な設計判断・意思決定を午前中に前倒しするか翌日に延期する",
    ("朝の運動実施", "午後も集中が持続"): "高負荷なタスクがある日は朝に30分の運動を先に済ませる",
    ("朝の運動実施", "入眠の改善"): "睡眠の質が落ちている週は運動頻度を上げて調整する",
    ("会議過多", "実装時間の喪失"): "会議が3件を超える日は深い実装を計画せず、細切れで可能なタスクに切り替える",
    ("他者からの誘い", "婉曲的な辞退"): "実装に没頭している期間の誘いは、謝罪+代替日提案のテンプレートで早めに返信する",
    ("疲労蓄積", "集中力の低下"): "疲労シグナル(肩こり・眠気)を検知したら意図的に休養日を差し込む",
}


# ============================================================ データ読み込み
def load_diary_entries() -> list[dict]:
    """diary.md を日付エントリにパースし source タグを付ける。"""
    if not DIARY.exists():
        # pipeline.py と同一のダミー生成ロジックを再利用
        from .pipeline import generate_dummy_diary
        generate_dummy_diary(DIARY)

    entries: list[dict] = []
    current: dict | None = None
    for line in DIARY.read_text(encoding="utf-8").splitlines():
        m = re.match(r"^##\s+(\d{4}-\d{2}-\d{2})", line)
        if m:
            if current:
                entries.append(current)
            current = {"source": "diary", "date": m.group(1), "fields": {}, "text": ""}
        elif current is not None:
            fm = re.match(r"^-\s*(気分|仕事|学び|健康):\s*(.*)", line)
            if fm:
                current["fields"][fm.group(1)] = fm.group(2)
            current["text"] += line + "\n"
    if current:
        entries.append(current)
    return entries


_DUMMY_LINE_TALKS = {
    "同僚・田中": [
        ("21:14", "田中", "今夜さくっと飲み会どう？久々に行かない？"),
        ("21:20", "自分", "ごめん、今NEON最適化の実装が乗ってるからパスするわ。また今度誘って"),
        ("21:21", "田中", "了解w 根詰めすぎんなよ"),
        ("12:03", "田中", "昨日のレビューの件、やっぱり例の設計で正解だったっぽいよ"),
        ("12:10", "自分", "案の定か。計測データも揃ってたし間違いないと思ってた"),
        ("19:40", "田中", "金曜の勉強会、LT枠空いてるけどやる？"),
        ("19:52", "自分", "面白そうだけど今週は無理そう…準備の時間が取れない。失敗したくないし次回にする"),
    ],
    "妻": [
        ("18:05", "妻", "今日の夕飯、外で食べない？"),
        ("18:06", "自分", "いいよ！ちょうど区切りついたところ。ありがとう、楽しみ"),
        ("22:30", "妻", "最近夜遅くまで作業してるけど大丈夫？"),
        ("22:41", "自分", "ちょっと寝不足気味かも。心配かけてごめん。今週末はちゃんと休む"),
        ("08:15", "妻", "朝ラン行ってきたの？えらい"),
        ("08:20", "自分", "行ってきた！走った日は午後まで頭が冴えるんだよね。最高"),
    ],
    "友人・佐藤": [
        ("23:10", "佐藤", "転職エージェントと話したんだけど、AI系すごい売り手市場らしいよ"),
        ("23:25", "自分", "気になるけど、せっかくここまで低レイヤ極めてきたし今更方向転換もな…とは思う"),
        ("23:26", "佐藤", "その積み上げがあるから強いんじゃない？"),
        ("23:40", "自分", "確かに。ただ失敗したくない気持ちが強くて動けてない。モヤモヤする"),
        ("20:05", "佐藤", "この前話してたエッジAIの記事送るわ"),
        ("20:15", "自分", "ありがとう、助かる！週末に読む"),
    ],
}


def generate_dummy_line_history(path: Path) -> None:
    """LINE公式エクスポート形式に準じたダミートーク履歴を生成する。"""
    base = date.today() - timedelta(days=14)
    weekday_jp = "月火水木金土日"
    lines: list[str] = []
    for t_idx, (contact, msgs) in enumerate(_DUMMY_LINE_TALKS.items()):
        lines += [f"[LINE] {contact}とのトーク履歴",
                  f"保存日時：{datetime.now().strftime('%Y/%m/%d %H:%M')}", ""]
        # 2メッセージごとに日付を進めて数日に分散させる
        for i in range(0, len(msgs), 2):
            d = base + timedelta(days=t_idx * 4 + i // 2 * 2)
            lines.append(f"{d.strftime('%Y/%m/%d')}({weekday_jp[d.weekday()]})")
            for time_s, sender, text in msgs[i : i + 2]:
                lines.append(f"{time_s}\t{sender}\t{text}")
            lines.append("")
        lines.append("")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines), encoding="utf-8")
    print(f"[profiler] ダミーLINE履歴を生成: {path}")


def _declared_self_name() -> str:
    """user_profile.json に明示設定された本人の LINE 表示名 (T-21 tier1)。"""
    from .profile_store import load_user_profile

    name = load_user_profile().get("fixed_attributes", {}).get("line_self_name", "")
    return name.strip()


def _resolve_self_by_contact(blocks: list[list[tuple]]) -> dict[str, str]:
    """T-21 (IMP-2): is_self をコンタクト単位の3段階決定論で解決する。

    感情推定・ハードコードなし、集合演算のみ:
      1. user_profile 明示設定 (line_self_name) がそのコンタクトの sender
         集合に含まれていれば採用。
      2. "自分"/"self" (ダミーデータ互換の既定リテラル) が含まれていれば採用。
      3. フォールバック: 「真の1対1 (sender がちょうど2種の) コンタクト」
         全体に共通する sender の積集合が単一要素ならそれを採用。実 LINE
         エクスポートは自分の表示名が全コンタクトで一貫するため、複数
         コンタクトを横断する唯一の共通 sender = 本人となる。sender が
         1種類しかない自己メモ的コンタクトや、3人以上のグループチャット
         (T-25) は積集合を汚染するため除外する — グループは自分がほぼ
         発言しないことも多く、sender 集合に自分の表示名が含まれない
         場合に積集合を空集合へ潰してしまう (docs/AI_SKILLS.md §14)。
    戻り値: {contact: 本人 sender 名}。解決できないコンタクトは含めない
    (安全側フォールバック — is_self=False のまま、データは失わない)。
    """
    per_contact: dict[str, set[str]] = {}
    for block in blocks:
        for contact, _date, _time, sender, _text in block:
            per_contact.setdefault(contact, set()).add(sender)

    declared = _declared_self_name()
    dyad_sets = [senders for senders in per_contact.values() if len(senders) == 2]
    common_fallback: str | None = None
    if len(dyad_sets) >= 2:
        common = set.intersection(*dyad_sets)
        if len(common) == 1:
            common_fallback = next(iter(common))

    resolved: dict[str, str] = {}
    for contact, senders in per_contact.items():
        if declared and declared in senders:
            resolved[contact] = declared
        elif "自分" in senders:
            resolved[contact] = "自分"
        elif "self" in senders:
            resolved[contact] = "self"
        elif common_fallback is not None and common_fallback in senders:
            resolved[contact] = common_fallback
    return resolved


def load_line_messages() -> list[dict]:
    """line_history.txt をメッセージ単位にパースし source タグを付ける。

    T-20/T-23 (IMP-1/IMP-2): 追記式取込 (import.line) は冪等ではなく、同一
    エクスポートの再取込や `[LINE]` ヘッダの無い追記でブロック境界が失われ
    うる。ブロックは `[LINE]` ヘッダに加え、日付の後退 (通常あり得ない逆行 —
    ヘッダ無し追記の兆候) でも区切る。ブロック単位の多重集合として扱い、
    キー (contact, date, time, sender, text) の出現数はブロック間で **max**
    を採る (sum ではない)。1 ブロック内の多重度は「同一分内の本物の連投」
    としてそのまま保存される (docs/AI_SKILLS.md §14)。
    T-21 (IMP-2): is_self は _resolve_self_by_contact() の3段階決定論で解決
    する (実 LINE エクスポートは本人も実名で記録されるため "自分" 固定判定
    は実データで全滅する)。
    """
    if not LINE_HISTORY.exists():
        generate_dummy_line_history(LINE_HISTORY)

    blocks: list[list[tuple]] = []
    contact = "不明"
    cur_date: str | None = None
    prev_date: str | None = None
    cur_block: list[tuple] = []

    def _flush_block() -> None:
        if cur_block:
            blocks.append(cur_block)

    for line in LINE_HISTORY.read_text(encoding="utf-8").splitlines():
        m = re.match(r"^\[LINE\]\s*(.+?)とのトーク履歴", line)
        if m:
            _flush_block()
            cur_block = []
            contact = m.group(1).strip()
            cur_date = None
            prev_date = None
            continue
        m = re.match(r"^(\d{4}/\d{2}/\d{2})", line)
        if m:
            new_date = m.group(1)
            if prev_date is not None and new_date < prev_date:
                _flush_block()
                cur_block = []
            cur_date = new_date
            prev_date = new_date
            continue
        m = re.match(r"^(\d{1,2}:\d{2})\t([^\t]+)\t(.*)", line)
        if m:
            cur_block.append((contact, cur_date, m.group(1), m.group(2), m.group(3)))
    _flush_block()

    self_by_contact = _resolve_self_by_contact(blocks)

    merged_max: dict[tuple, int] = {}
    for block in blocks:
        counts = Counter(block)
        for key, cnt in counts.items():
            if cnt > merged_max.get(key, 0):
                merged_max[key] = cnt

    messages: list[dict] = []
    for key, cnt in merged_max.items():
        contact_k, date_k, time_k, sender_k, text_k = key
        msg = {
            "source": "line", "contact": contact_k, "date": date_k,
            "time": time_k, "sender": sender_k, "text": text_k,
            "is_self": sender_k == self_by_contact.get(contact_k),
        }
        messages.extend([msg] * cnt)
    return messages


# ============================================================ 分析ユーティリティ
def sentiment_of(text: str) -> tuple[float, list[str]]:
    """語彙ベースの感情スコア(-1〜+1)と検出ラベル。"""
    score, labels = 0.0, []
    for word, (valence, label) in SENTIMENT_LEXICON.items():
        if word in text:
            score += valence
            labels.append(label)
    return max(-1.0, min(1.0, score)), labels


def _iter_documents(daily: list[dict]):
    """DailyContext を (source_label, date, self_text) で走査する。

    self_text = 日記 + 自分のLINE発話 + AI相談の User Query。
    相談内容はユーザーの課題・価値観の証拠として分析対象に含める。
    """
    for dc in daily:
        if dc["self_text"].strip():
            yield f"daily:{'+'.join(dc['sources'])}", dc["date"], dc["self_text"]


# ============================================================ 1. 認知的バイアス
def analyze_biases(daily: list[dict]) -> list[dict]:
    results = []
    for bias, spec in BIAS_PATTERNS.items():
        hits = []
        for src, d, text in _iter_documents(daily):
            for pat in spec["patterns"]:
                for m in re.finditer(pat, text):
                    s = max(0, m.start() - 18)
                    snippet = text[s : m.end() + 22].replace("\n", " ").strip()
                    hits.append({"source": src, "date": d, "quote": f"…{snippet}…"})
        n_docs = sum(1 for _ in _iter_documents(daily))
        results.append({
            "bias": bias,
            "description": spec["description"],
            "hit_count": len(hits),
            "intensity": round(min(1.0, len(hits) / max(1, n_docs) * 8), 3),
            "evidence": hits[:5],
        })
    results.sort(key=lambda r: -r["hit_count"])
    return results


# ============================================================ 2. 価値観の階層構造
def analyze_values(daily: list[dict]) -> list[dict]:
    scores: dict[str, dict] = {v: {"count": 0, "evidence": []} for v in VALUE_MAP}
    for src, d, text in _iter_documents(daily):
        for value, spec in VALUE_MAP.items():
            for kw in spec["keywords"]:
                if kw in text:
                    scores[value]["count"] += 1
                    if len(scores[value]["evidence"]) < 3:
                        scores[value]["evidence"].append(
                            {"source": src, "date": d, "keyword": kw})
    total = sum(s["count"] for s in scores.values()) or 1
    ranked = [{
        "rank": 0,
        "value": v,
        "root_need": VALUE_MAP[v]["root_need"],
        "weight": round(s["count"] / total, 3),
        "signal_count": s["count"],
        "evidence": s["evidence"],
    } for v, s in scores.items()]
    ranked.sort(key=lambda r: -r["signal_count"])
    for i, r in enumerate(ranked):
        r["rank"] = i + 1
    return ranked


# ============================================================ 3. 感情的反応パターン
def analyze_emotions(msgs: list[dict]) -> dict:
    contacts: dict[str, dict] = {}
    for m in msgs:
        c = contacts.setdefault(m["contact"], {"self_scores": [], "self_labels": [],
                                               "n_self": 0, "n_other": 0})
        if m["is_self"]:
            score, labels = sentiment_of(m["text"])
            c["self_scores"].append(score)
            c["self_labels"] += labels
            c["n_self"] += 1
        else:
            c["n_other"] += 1

    per_contact = []
    for name, c in contacts.items():
        scores = c["self_scores"] or [0.0]
        labels = c["self_labels"]
        apology = labels.count("apology")
        gratitude = labels.count("gratitude")
        anxiety = labels.count("anxiety") + labels.count("sadness")
        # 典型反応のヒューリスティック分類
        if apology >= 2 and any(l == "refusal" for l in labels):
            typical = "謝罪を伴う婉曲的な辞退(関係維持を優先した断り方)"
        elif gratitude >= 2:
            typical = "感謝表明が多く、受容的・肯定的な応答"
        elif anxiety >= 2:
            typical = "不安・迷いの自己開示(本音を出せる相手)"
        else:
            typical = "中立的・実務的な応答"
        per_contact.append({
            "contact": name,
            "messages_sent": c["n_self"],
            "messages_received": c["n_other"],
            "avg_sentiment": round(statistics.mean(scores), 3),
            "volatility_stdev": round(statistics.pstdev(scores), 3),
            "apology_count": apology,
            "gratitude_count": gratitude,
            "typical_reaction": typical,
        })

    all_scores = [s for c in contacts.values() for s in c["self_scores"]] or [0.0]
    return {
        "overall_avg_sentiment": round(statistics.mean(all_scores), 3),
        "overall_volatility_stdev": round(statistics.pstdev(all_scores), 3),
        "interpretation": (
            "感情の揺らぎ(標準偏差)が大きい相手ほど本音の自己開示が多い。"
            "揺らぎが小さい相手には社会的テンプレート応答が優位。"
        ),
        "per_contact": per_contact,
    }


# ============================================================ 4. 意思決定アルゴリズム
def analyze_decision_rules(daily: list[dict], diary: list[dict]) -> list[dict]:
    """DailyContext (日記+LINEが日付で結合済み) 単位で状況→帰結の共起を数える。
    日記とLINEが同じ文書に入るため、日をまたいだソース間の共起も自然に捕捉される。"""
    situation_support = {s: 0 for s in SITUATION_TAGS}
    cooccur: dict[tuple[str, str], list] = {}
    for dc in daily:
        text = dc["text"]
        sits = [s for s, kws in SITUATION_TAGS.items() if any(k in text for k in kws)]
        outs = [o for o, kws in OUTCOME_TAGS.items() if any(k in text for k in kws)]
        for s in sits:
            situation_support[s] += 1
            for o in outs:
                cooccur.setdefault((s, o), []).append(
                    {"date": dc["date"], "sources": dc["sources"]})

    rules = []
    for (s, o), ev in sorted(cooccur.items(), key=lambda kv: -len(kv[1])):
        support = situation_support[s]
        conf = len(ev) / support if support else 0.0
        if len(ev) < 1 or conf < 0.2:
            continue
        rules.append({
            "situation": s,
            "observed_consequence": o,
            "support": support,
            "co_occurrences": len(ev),
            "confidence": round(conf, 3),
            "recommended_action": RULE_RECOMMENDATIONS.get(
                (s, o), f"「{s}」の際は「{o}」が起きやすい前提で計画する"),
            "evidence": ev[:4],
        })

    # 日記の「学び」欄は本人が言語化済みの内在ルールとしてそのまま採用
    for e in diary:
        learning = e["fields"].get("学び", "")
        if re.search(r"(べき|べし|が一番|方が|決める)", learning):
            rules.append({
                "situation": "内省による自己ルール(明示的)",
                "observed_consequence": "-",
                "support": 1, "co_occurrences": 1, "confidence": 1.0,
                "recommended_action": learning,
                "evidence": [{"source": "diary", "date": e["date"]}],
            })
    return rules


# ============================================================ 5. セッション単位の対話分析 (状態保持型)
STIMULUS_TYPES = {
    "誘い": ["飲み", "行かない", "来れる", "どう？", "どう?", "食べない"],
    "依頼・打診": ["やる？", "やる?", "お願い", "枠", "頼", "打診"],
    "気遣い・心配": ["大丈夫", "心配", "無理しないで", "根詰め"],
    "情報共有": ["らしいよ", "記事", "送るわ", "話したんだけど", "っぽいよ"],
}
RESPONSE_TYPES = {
    "承諾": ["いいよ", "行くよ", "参加する", "楽しみ", "ちょうど"],
    "辞退": ["パスする", "行けない", "今回はやめ", "今週は無理", "無理そう"],
    "謝罪": ["ごめん", "すまん", "申し訳"],
    "保留・先送り": ["また今度", "次回", "週末に"],
    "感謝": ["ありがとう", "助かる"],
    "自己開示(不安・迷い)": ["モヤモヤ", "不安", "動けてない", "とは思う", "自信"],
}
BUSY_DIARY_KW = ["会議", "実装", "忙", "没頭", "会議が多"]
FATIGUE_DIARY_KW = ["疲", "寝不足", "睡眠6時間", "しんどい"]


def _classify(text: str, table: dict[str, list[str]]) -> list[str]:
    types = [t for t, kws in table.items() if any(k in text for k in kws)]
    if "謝罪" in types and "辞退" in types:
        types = [t for t in types if t not in ("謝罪", "辞退")] + ["謝罪付き辞退"]
    return types or ["その他"]


def _latency_bucket(minutes: int | None) -> str | None:
    if minutes is None:
        return None
    if minutes < 30:
        return "即レス(<30m)"
    if minutes < 360:
        return "通常遅延(30m-6h)"
    return "長時間遅延(>6h)"


def analyze_interaction_sessions(daily: list[dict]) -> dict:
    """ConversationSession 全体の発言群から刺激×反応の共起とレイテンシ傾向を抽出。
    分析対象はユーザーの Response のみ。Stimulus は状況分類に限定。"""
    from .data_merger import collect_conversation_sessions
    sessions = collect_conversation_sessions(daily)
    daily_by_date = {dc["date"]: dc for dc in daily}

    rule_counts: dict[tuple[str, str], list] = {}
    stim_support: dict[str, int] = {}
    examples = []
    latency_patterns: list[dict] = []

    for s in sessions:
        stims = _classify(s["stimulus"], STIMULUS_TYPES)
        resps = _classify(s["response"], RESPONSE_TYPES)
        senti, _ = sentiment_of(s["response"])
        bucket = _latency_bucket(s.get("latency_minutes"))

        diary_blob = " ".join(
            daily_by_date[d]["diary_text"]
            for d in s["dates"] if d in daily_by_date and daily_by_date[d]["diary_text"]
        )
        busy = any(k in diary_blob for k in BUSY_DIARY_KW)
        fatigued = any(k in diary_blob for k in FATIGUE_DIARY_KW)

        for st in stims:
            stim_support[st] = stim_support.get(st, 0) + 1
            for r in resps:
                rule_counts.setdefault((st, r), []).append(
                    {"date": s["start_date"], "contact": s["contact"],
                     "latency": s["response_latency"]})

        if bucket and s.get("latency_minutes") is not None:
            if bucket == "長時間遅延(>6h)" and not busy:
                latency_patterns.append({
                    "type": "心理的抵抗",
                    "insight": (f"日記上は暇/非多忙なのに {s['contact']} への返信が "
                                f"{s['response_latency']} — 回避行動の可能性"),
                    "contact": s["contact"], "date": s["start_date"],
                    "latency": s["response_latency"],
                })
            elif bucket == "即レス(<30m)" and busy:
                latency_patterns.append({
                    "type": "優先度高",
                    "insight": (f"多忙日({s['start_date']})にも {s['contact']} へ "
                                f"即レス({s['response_latency']}) — 関係性の優先度が高い"),
                    "contact": s["contact"], "date": s["start_date"],
                    "latency": s["response_latency"],
                })
            elif bucket == "即レス(<30m)" and fatigued:
                latency_patterns.append({
                    "type": "疲労下の即応",
                    "insight": (f"疲労シグナル日に {s['contact']} へ即レス — "
                                "エネルギー残量より関係維持を優先"),
                    "contact": s["contact"], "date": s["start_date"],
                })

        if len(examples) < 5:
            examples.append({
                "header": s["header"],
                "contact": s["contact"],
                "response_sentiment": round(senti, 2),
                "latency": s["response_latency"],
            })

    rules = []
    for (st, r), ev in sorted(rule_counts.items(), key=lambda kv: -len(kv[1])):
        if st == "その他" and r == "その他":
            continue
        rules.append({
            "stimulus_type": st,
            "response_type": r,
            "occurrences": len(ev),
            "support": stim_support[st],
            "confidence": round(len(ev) / stim_support[st], 3),
            "evidence": ev[:4],
            "insight": f"セッション内で相手の「{st}」に対し「{r}」で応じる傾向",
        })

    return {
        "sessions_analyzed": len(sessions),
        "stimulus_response_rules": rules,
        "latency_patterns": latency_patterns[:8],
        "examples": examples,
    }


def llm_interaction_analysis(daily: list[dict]) -> dict | None:
    """7BクラスLLMで ConversationSession + 日記から行動心理学的深層分析。"""
    from .data_merger import collect_conversation_sessions
    from .llm_backend import LlamaStdioBackend
    from .llm_config import find_gguf
    from .paths import LLAMA_CLI_EXE

    model = find_gguf()
    if not (model and LLAMA_CLI_EXE.exists()):
        print("[profiler] ローカルLLM未検出 → LLM因果分析をスキップ")
        return None

    sessions = collect_conversation_sessions(daily)
    finance_days = [
        dc for dc in daily
        if dc.get("has_finance") and dc.get("finance_text", "").strip()
        and not dc["finance_text"].startswith("(この日")
    ]
    if not sessions and not finance_days:
        return None

    daily_by_date = {dc["date"]: dc for dc in daily}
    blocks = []
    for i, s in enumerate(sessions[:12]):
        diary_parts = [
            f"[日記 {d}]\n{daily_by_date[d]['diary_text'].strip()}"
            for d in s["dates"]
            if d in daily_by_date and daily_by_date[d]["diary_text"].strip()
        ]
        diary_ctx = "\n".join(diary_parts) if diary_parts else "(該当日記なし)"
        blocks.append(
            f"### セッション {i+1}\n{s['text']}\n\n**同日の日記:**\n{diary_ctx}")

    session_block = "\n\n".join(blocks) if blocks else "(対話セッションなし)"

    consult_blocks = [
        f"### {dc['date']}\n{dc['consultation_text']}"
        for dc in daily
        if dc.get("has_consultation") and dc.get("consultation_text", "").strip()
        and not dc["consultation_text"].startswith("(この日")
    ]
    consult_section = ""
    if consult_blocks:
        consult_section = (
            "\n\n# AI相談履歴 (ユーザーの課題と意思決定プロセス)\n"
            + "\n\n".join(consult_blocks[:8])
        )

    finance_blocks = []
    for dc in finance_days[:14]:
        cal_hint = ""
        if dc.get("has_calendar") and dc.get("calendar_events"):
            cal_hint = f"\n**同日の予定数:** {len(dc['calendar_events'])}件"
        diary_hint = ""
        if dc.get("diary_text", "").strip():
            diary_hint = f"\n**同日の日記:**\n{dc['diary_text'].strip()[:400]}"
        finance_blocks.append(
            f"### {dc['date']}\n{dc['finance_text']}{cal_hint}{diary_hint}")
    finance_section = ""
    if finance_blocks:
        finance_section = (
            "\n\n# 家計簿ハードデータ (消費・収入の客観的事実)\n"
            + "\n\n".join(finance_blocks)
        )

    system = (
        "あなたは行動心理学と認知科学に精通したプロファイリング専門家である。"
        "対話セッション・日記・家計簿から、ユーザー本人の無意識の心理的抵抗、"
        "意思決定プロセス、消費行動パターンを具体的かつ論理的に分析する。"
    )
    prompt = f"""以下のデータから、ユーザーの「無意識の心理的抵抗」「意思決定プロセス」
「消費行動バイアス」をディープに分析せよ。

【分析視点】
- 交渉と意思決定: 相手の連続した働きかけに対し、ユーザーがどう妥協・反発・受容したか。
- レイテンシ分析: 返信にかかった時間 (Response Latency) を行動シグナルとして評価せよ。
- 矛盾と回避行動: その日の日記（忙しさ・タスク量）と照らし合わせ、
  「暇なはずなのに返信が遅い（心理的抵抗）」
  「忙しいのに特定の相手には即レスしている（優先度）」等の不一致を暴き出し、言語化せよ。
- 【消費行動と感情の相関】: ユーザーの『日記の感情・ストレス状態・予定の過密さ』と
  『その日の消費金額・カテゴリ（家計簿）』のハードデータを突合させよ。
  例えば、『ストレスが高い日に浪費が増える』『重要なタスクの後に自己投資を行う』
  といった無意識の消費バイアスやモチベーションの源泉を事実ベースで抽出し、言語化せよ。

【絶対制約】
- 抽出する価値観・バイアス・意思決定傾向は **ユーザー本人のもの** に限定すること。
- 相手の性格・価値観・感情をユーザーのプロファイルに混入させてはならない。
- Stimulus(相手の発言)は文脈情報としてのみ用い、分析根拠は Response(ユーザーの反応) に置くこと。
- 家計簿分析は推測ではなく、提示された金額・カテゴリ・日次サマリと日記/予定の共起に基づくこと。

【出力形式 — 見出し付き箇条書き (日本語)】
1. 無意識の心理的抵抗
2. 意思決定プロセス (交渉・妥協・反発のパターン)
3. レイテンシが示す優先度と回避
4. 日記との矛盾点 (該当があれば)
5. ユーザー固有の価値観・認知バイアス (根拠付き)
6. 消費行動と感情の相関 (家計簿ハードデータに基づく)
7. 抽象化された自己モデル (具体的エピソードを超え、コアドライブ・内的葛藤・
   認知スタイル・対人スタンスをメタレベルで統合。心理学用語で簡潔に)

# 対話セッション
{session_block}{consult_section}{finance_section}"""

    backend = LlamaStdioBackend(LLAMA_CLI_EXE, model)
    try:
        print(f"[profiler] 7B LLMで行動心理学分析を実行中… ({model.name})")
        analysis = backend.generate(system, prompt, max_tokens=1200)
    finally:
        backend.stop()
    return {
        "model": model.name,
        "sessions_used": min(len(sessions), 12),
        "finance_days_used": min(len(finance_days), 14),
        "analysis": analysis,
    }


# ============================================================ 6. クロスソース共起
def analyze_cross_source_patterns(daily: list[dict]) -> dict:
    """「日記に書いた状態」と「同日のLINEでの自分の反応」の日跨ぎ共起を
    明示的にマイニングする。状況は diary セクション、反応は LINE の
    自分発話のみから検出することでソース間の因果方向を保証する。"""
    both_days = [dc for dc in daily if dc["has_diary"] and dc["has_line"]]

    # (日記の状況) x (LINEでの反応) の共起
    rules = []
    support = {s: 0 for s in SITUATION_TAGS}
    cooccur: dict[tuple[str, str], list] = {}
    for dc in both_days:
        sits = [s for s, kws in SITUATION_TAGS.items()
                if any(k in dc["diary_text"] for k in kws)]
        outs = [o for o, kws in OUTCOME_TAGS.items()
                if any(k in dc["line_self_text"] for k in kws)]
        for s in sits:
            support[s] += 1
            for o in outs:
                cooccur.setdefault((s, o), []).append(dc["date"])
    for (s, o), dates in sorted(cooccur.items(), key=lambda kv: -len(kv[1])):
        rules.append({
            "diary_situation": s,
            "line_reaction": o,
            "support_days": support[s],
            "co_occurrence_days": len(dates),
            "confidence": round(len(dates) / support[s], 3) if support[s] else 0.0,
            "evidence_dates": dates[:5],
            "insight": f"日記に「{s}」と書いた日は、LINEで「{o}」の反応をしやすい",
        })

    # 疲労日の対人感情: 日記に疲労シグナルがある日の自分のLINE発話感情 vs 全日平均
    fatigue_kw = ["疲", "寝不足", "睡眠6時間", "しんどい"]
    fatigue_scores, all_scores = [], []
    for dc in both_days:
        msgs = [t for t in dc["line_self_text"].splitlines() if t.strip()]
        day_scores = [sentiment_of(t)[0] for t in msgs]
        all_scores += day_scores
        if any(k in dc["diary_text"] for k in fatigue_kw):
            fatigue_scores += day_scores

    emotional_response = {
        "fatigue_days_with_line": sum(
            1 for dc in both_days if any(k in dc["diary_text"] for k in fatigue_kw)),
        "avg_line_sentiment_on_fatigue_days":
            round(statistics.mean(fatigue_scores), 3) if fatigue_scores else None,
        "avg_line_sentiment_overall":
            round(statistics.mean(all_scores), 3) if all_scores else None,
    }
    if fatigue_scores and all_scores:
        delta = emotional_response["avg_line_sentiment_on_fatigue_days"] - \
                emotional_response["avg_line_sentiment_overall"]
        emotional_response["interpretation"] = (
            f"疲労日はLINE上の感情が平均比 {delta:+.2f}。"
            + ("疲労がネガティブな対人反応として漏出している"
               if delta < -0.05 else "疲労時も対人トーンは維持できている")
        )

    return {
        "days_with_both_sources": len(both_days),
        "diary_to_line_rules": rules,
        "emotional_response_on_fatigue_days": emotional_response,
    }


# ============================================================ 7. 事実シグナル推定 (ユーザー入力なし)
FACT_PATTERNS = {
    "occupation": [
        (r"エンジニア|開発|実装|組み込み|ソフトウェア|SE\b|プログラム", "技術系エンジニア"),
        (r"マネージ|リード|PM\b|プロジェクト", "技術リード/マネジメント寄り"),
        (r"研究|論文|大学|院生", "研究・学術"),
    ],
    "life_stage": [
        (r"妻|夫|配偶|結婚", "配偶者あり"),
        (r"子|育児|保育", "子育て期"),
        (r"転職|キャリア|エージェント", "キャリア転換を検討中"),
    ],
    "work_context": [
        (r"会議|レビュー|デプロイ|リリース", "チーム開発環境"),
        (r"NEON|最適化|低レイヤ|組込", "低レイヤ/性能最適化に関心"),
        (r"AI|LLM|機械学習|推論", "AI/ML領域に関心"),
    ],
    "health_signals": [
        (r"寝不足|睡眠6時間|睡眠不足", "慢性的な睡眠負債のリスク"),
        (r"ランニング|筋トレ|運動", "運動を自己調整手段として使用"),
        (r"疲労|しんどい|肩こり", "デスクワーク由来の疲労蓄積"),
    ],
}


def infer_factual_signals(daily: list[dict]) -> dict:
    """日記・LINE・相談ログから観測可能な事実シグナルを推定 (手入力なし)。"""
    blob = "\n".join(dc["self_text"] for dc in daily if dc["self_text"].strip())
    signals: dict[str, list[str]] = {k: [] for k in FACT_PATTERNS}
    for category, rules in FACT_PATTERNS.items():
        for pat, label in rules:
            if re.search(pat, blob) and label not in signals[category]:
                signals[category].append(label)
    goal_hits = re.findall(
        r"(?:目標|ビジョン|なりたい|目指|ゴール)[はが]?[:：]?\s*([^\n。]{4,40})",
        blob)
    stated_goals = list(dict.fromkeys(g.strip() for g in goal_hits))[:3]
    return {
        "source": "inferred_from_logs",
        "categories": {k: v for k, v in signals.items() if v},
        "stated_goals": stated_goals,
        "data_span_days": len(daily),
        "days_with_self_text": sum(1 for dc in daily if dc["self_text"].strip()),
    }


# ============================================================ 8. 抽象自己モデル (具体→パターン→抽象)
VALUE_TENSION_PAIRS = [
    ("成長・熟達", "健康・持続可能性", "成果追求と回復のトレードオフ"),
    ("効率・最適化", "つながり・信頼", "深い作業と対人応答の時間競合"),
    ("自律・裁量", "誠実さ・説明責任", "自分のペースと他者への説明責任"),
    ("成長・熟達", "つながり・信頼", "スキル深化と社交機会の選択"),
]

COGNITIVE_STYLE_LABELS = {
    "確証バイアス": "仮説を早めに固定し、支持証拠で自己強化する傾向",
    "損失回避": "不確実な挑戦より既存投資の保護を優先する傾向",
    "サンクコスト効果": "過去の投資が現在の選択を縛る傾向",
    "現状維持バイアス": "変更コストを過大評価し、デフォルトに留まる傾向",
    "過度の一般化": "限られた経験から普遍的ルールを抽出する傾向",
}


def synthesize_abstract_identity(profile: dict, daily: list[dict]) -> dict:
    """ルールベース分析結果を心理・行動の抽象レイヤーへ統合する。"""
    values = {v["value"]: v for v in profile.get("value_hierarchy", [])}
    top_values = profile.get("value_hierarchy", [])[:4]

    root_agg: dict[str, float] = {}
    for v in top_values:
        need = v["root_need"]
        root_agg[need] = root_agg.get(need, 0.0) + v["weight"]
    core_drives = [
        {"drive": need, "weight": round(w, 3),
         "meaning": _root_need_meaning(need)}
        for need, w in sorted(root_agg.items(), key=lambda x: -x[1])[:3]
    ]

    active_biases = [b for b in profile.get("cognitive_biases", []) if b["hit_count"] > 0][:3]
    cognitive_style = {
        "label": _classify_cognitive_style(active_biases),
        "biases": [
            {"name": b["bias"], "intensity": b["intensity"],
             "abstract_pattern": COGNITIVE_STYLE_LABELS.get(b["bias"], b["description"])}
            for b in active_biases
        ],
    }

    tensions = []
    for va, vb, desc in VALUE_TENSION_PAIRS:
        wa = values.get(va, {}).get("weight", 0)
        wb = values.get(vb, {}).get("weight", 0)
        if wa >= 0.1 and wb >= 0.1:
            tensions.append({
                "poles": [va, vb],
                "tension": desc,
                "balance": round(wa / (wa + wb), 2),
            })

    heuristics = []
    for r in profile.get("decision_rules", [])[:6]:
        action = r.get("recommended_action", "")
        if len(action) < 8:
            continue
        heuristics.append({
            "trigger": r.get("situation", r.get("stimulus_type", "状況")),
            "abstract_rule": _abstract_rule(action),
            "confidence": r.get("confidence", 1.0),
        })

    ep = profile.get("emotional_patterns", {})
    ip = profile.get("interaction_patterns", {})
    sr_rules = ip.get("stimulus_response_rules", [])
    invite_rules = [r for r in sr_rules if r.get("stimulus_type") == "誘い"]
    dominant_response = invite_rules[0]["response_type"] if invite_rules else None
    relational = {
        "overall_tone": ep.get("overall_avg_sentiment"),
        "volatility": ep.get("overall_volatility_stdev"),
        "stance": _relational_stance(ep, invite_rules),
        "priority_contacts": [
            lp["contact"] for lp in ip.get("latency_patterns", [])
            if lp.get("type") == "優先度高"
        ][:3],
        "dominant_social_response": dominant_response,
    }

    energy_pattern = _infer_energy_pattern(daily)
    return {
        "core_drives": core_drives,
        "cognitive_style": cognitive_style,
        "internal_tensions": tensions[:4],
        "decision_heuristics": heuristics[:5],
        "relational_stance": relational,
        "energy_regulation": energy_pattern,
    }


def _root_need_meaning(need: str) -> str:
    if "統制感" in need:
        return "環境を理解・予測しコントロールする欲求"
    if "Competence" in need or "有能感" in need:
        return "能力発揮と習得による自己効力感の獲得"
    if "Autonomy" in need or "自己決定" in need:
        return "自分の判断軸で動く裁量と主体性"
    if "Relatedness" in need or "関係性" in need:
        return "信頼関係の維持と所属感"
    if "自己一致" in need or ("信頼" in need and "関係" not in need):
        return "言行一致と説明責任による自己整合性"
    if "生存" in need or "パフォーマンス" in need:
        return "長期的な出力を支える身体・リズムの維持"
    return need


def _classify_cognitive_style(biases: list[dict]) -> str:
    names = {b["bias"] for b in biases}
    if "損失回避" in names and "サンクコスト効果" in names:
        return "慎重型 — 既存投資と失敗リスクへの過敏さ"
    if "確証バイアス" in names and "過度の一般化" in names:
        return "パターン固定型 — 早期仮説化と一般化"
    if "現状維持バイアス" in names:
        return "安定志向型 — 変更より現状維持を選びやすい"
    if biases:
        return f"{biases[0]['bias']}優位型"
    return "データ不足 — 傾向未確定"


def _abstract_rule(action: str) -> str:
    """具体ルールを1段抽象化した if-then 形式へ。"""
    if "睡眠" in action or "午前" in action:
        return "認知リソースが低い状態では重要判断を延期または前倒しする"
    if "運動" in action or "ランニング" in action:
        return "身体活性化を認知パフォーマンスの前提条件として組み込む"
    if "会議" in action:
        return "外部割込みが多い日は深い作業モードに入らない"
    if "休養" in action or "疲労" in action:
        return "疲労シグナルを無視せず、回復を計画に組み込む"
    if "辞退" in action or "パス" in action:
        return "関係維持を保ちつつ、優先度の低い社交を構造的に断る"
    return action[:60] + ("…" if len(action) > 60 else "")


def _relational_stance(ep: dict, invite_rules: list[dict]) -> str:
    if not invite_rules:
        return "データ不足"
    responses = [r["response_type"] for r in invite_rules]
    if responses.count("謝罪付き辞退") >= 2:
        return "関係維持型拒否 — 断りながらも信頼を損なわない配慮"
    if responses.count("保留・先送り") >= 2:
        return "緩衝型 — 即断せず時間で調整する"
    if responses.count("承諾") >= 2:
        return "受容型 — 社交機会を積極的に取り込む"
    return "状況依存型 — 相手・文脈で応答が分岐"


def _infer_energy_pattern(daily: list[dict]) -> dict:
    fatigue_kw = ["疲", "寝不足", "睡眠6時間", "しんどい"]
    focus_kw = ["集中", "没頭", "実装", "乗って"]
    fatigue_days = sum(
        1 for dc in daily if any(k in dc.get("diary_text", "") for k in fatigue_kw))
    focus_days = sum(
        1 for dc in daily if any(k in dc.get("diary_text", "") for k in focus_kw))
    n = len(daily) or 1
    label = "回復と出力のバランス型"
    if fatigue_days / n > 0.35:
        label = "出力過多型 — 回復より生産を優先しがち"
    elif focus_days / n > 0.4:
        label = "深い没入型 — フロー状態を好む"
    return {
        "label": label,
        "fatigue_day_ratio": round(fatigue_days / n, 2),
        "deep_focus_day_ratio": round(focus_days / n, 2),
    }


def build_meta_narrative(factual: dict, abstract: dict) -> str:
    """抽象レイヤーを2〜3文の自己記述へ統合。"""
    drives = abstract.get("core_drives", [])
    meanings = list(dict.fromkeys(d["meaning"] for d in drives[:3]))
    drive_text = "・".join(meanings[:2]) if meanings else "動機はデータ不足"
    style = abstract.get("cognitive_style", {}).get("label", "")
    stance = abstract.get("relational_stance", {}).get("stance", "")
    energy = abstract.get("energy_regulation", {}).get("label", "")
    occ = factual.get("categories", {}).get("occupation", ["不明"])
    parts = [
        f"根底には「{drive_text}」が動機として働く。",
        f"認知スタイルは{style}。",
    ]
    if stance and stance != "データ不足":
        parts.append(f"対人場面では{stance}。")
    parts.append(f"エネルギー管理は{energy}。")
    if occ and occ[0] != "不明":
        parts.append(f"文脈上は{occ[0]}としての活動が中心。")
    return " ".join(parts)


# ============================================================ メイン
def build_profile(use_llm: bool = True) -> dict:
    from .data_merger import load_daily_contexts  # 遅延import (循環回避)

    diary = load_diary_entries()
    msgs = load_line_messages()
    daily = load_daily_contexts()
    both = sum(1 for dc in daily if dc["has_diary"] and dc["has_line"])
    consult_days = sum(1 for dc in daily if dc.get("has_consultation"))
    finance_days = sum(1 for dc in daily if dc.get("has_finance"))
    print(f"[profiler] 日記エントリ: {len(diary)} / LINEメッセージ: {len(msgs)}"
          f" (自分の発話: {sum(1 for m in msgs if m['is_self'])})")
    print(f"[profiler] DailyContext: {len(daily)} 日分 (日記+LINE両方: {both}日,"
          f" AI相談: {consult_days}日, 家計簿: {finance_days}日)")

    profile = {
        "schema": "deep_profile.v6",
        "sources": {
            "diary": {"path": str(DIARY.relative_to(ROOT)), "entries": len(diary)},
            "line": {"path": str(LINE_HISTORY.relative_to(ROOT)), "messages": len(msgs)},
            "daily_contexts": {
                "days": len(daily),
                "days_with_both": both,
                "days_with_consultation": consult_days,
                "days_with_finance": finance_days,
            },
        },
        "cognitive_biases": analyze_biases(daily),
        "value_hierarchy": analyze_values(daily),
        "emotional_patterns": analyze_emotions(msgs),
        "decision_rules": analyze_decision_rules(daily, diary),
        "cross_source_patterns": analyze_cross_source_patterns(daily),
        "interaction_patterns": analyze_interaction_sessions(daily),
    }
    factual = infer_factual_signals(daily)
    abstract = synthesize_abstract_identity(profile, daily)
    profile["factual_signals"] = factual
    profile["abstract_identity"] = abstract
    profile["meta_narrative"] = build_meta_narrative(factual, abstract)

    # 主観 (日記・相談) × 客観 (家計簿・予定・LINE発話) の差分分析
    from .gap_analysis import analyze_gaps, llm_gap_analysis
    gap_result = analyze_gaps(daily)

    # 一人称×二人称の衝突 (Target Delta-LINE DL2): LINE テレメトリを再計算し、
    # 対人プロトコル3軸 + social_positioning_gap を既存 gaps へ合流させる。
    # 【隔離原則】ここで gap_result["gaps"] へ追加した内容が触れる出口は
    # _gap_section() (講評フェーズのみ呼ばれる) だけであり、interview_sim/
    # gd_sim の議論フェーズ・es_review には一切渡らない (AI_SKILLS §7.1/§11,
    # SPEC I-14)。新しい直接呼び出しをそれらのメソッドに追加しないこと。
    from .line_telemetry import (
        analyze_social_positioning, register_bounties, sync_line_telemetry,
    )
    telemetry = sync_line_telemetry()
    social_gaps = analyze_social_positioning(daily, telemetry["dyads"])
    gap_result["gaps"].extend(social_gaps)
    gap_result["interpersonal"] = telemetry["interpersonal"]
    register_bounties(gap_result["gaps"])

    profile["gap_analysis"] = gap_result
    print(f"[profiler] Gap分析: {len(gap_result['gaps'])} 件のギャップ検出 "
          f"(データ充足度 {gap_result['data_sufficiency']:.0%}、"
          f"対人ギャップ {len(social_gaps)} 件)")

    # Target Echo (E1-E4): 日次テンソル (PKBTEN01) の全再構築 + oracle_payload。
    # 再計算トリガは profiler 実行 / import.line のみ (I-3)。失敗しても既存の
    # 分析結果 (gap_analysis 等) は維持する (LLM 分析と同じ耐性パターン)。
    try:
        from . import oracle as _oracle
        from . import tensor_store as _tensor_store
        _tensor_store.build_tensor(
            daily, None, _tensor_store.TENSOR_GLOBAL_BIN, line_messages=msgs)
        profile["oracle_payload"] = _oracle.build_oracle_payload("global")
        print(f"[profiler] Echo: テンソル同期 + oracle_payload 更新 "
              f"(gate_passed={profile['oracle_payload']['sufficiency']['gate_passed']})")
    except Exception as e:
        print(f"[profiler] Echo 分析失敗 (既存分析は維持): {type(e).__name__}: {e}")

    if use_llm:
        try:
            llm = llm_interaction_analysis(daily)
            if llm:
                profile["llm_interaction_insights"] = llm
        except Exception as e:
            print(f"[profiler] LLM分析失敗 (ルールベースのみ完走): {type(e).__name__}: {e}")
        try:
            llm_gap = llm_gap_analysis(gap_result, daily)
            if llm_gap:
                profile["gap_analysis"]["llm_synthesis"] = llm_gap
        except Exception as e:
            print(f"[profiler] Gap LLM深化失敗 (決定論結果は維持): {type(e).__name__}: {e}")
    return profile


def update_user_profile(profile: dict) -> None:
    """深層プロファイルを user_profile.json へ反映 (fixed_attributes は保持)。"""
    from .consultation_engine import FIXED_ATTRIBUTES

    fixed = dict(FIXED_ATTRIBUTES)
    if USER_PROFILE_JSON.exists():
        try:
            prev = json.loads(USER_PROFILE_JSON.read_text(encoding="utf-8"))
            fixed.update(prev.get("fixed_attributes", {}))
            for key in FIXED_ATTRIBUTES:
                legacy = prev.get("attributes", {}).get(key, "")
                if legacy and not fixed.get(key):
                    fixed[key] = legacy
        except (json.JSONDecodeError, OSError):
            pass

    llm_abstract = ""
    if "llm_interaction_insights" in profile:
        llm_abstract = profile["llm_interaction_insights"].get("analysis", "")

    user = {
        "schema": "user_profile.v2",
        "fixed_attributes": fixed,
        "inferred_profile": {
            "updated_at": datetime.now().isoformat(timespec="seconds"),
            "meta_narrative": profile.get("meta_narrative", ""),
            "factual_signals": profile.get("factual_signals", {}),
            "abstract_identity": profile.get("abstract_identity", {}),
            "llm_deep_synthesis": llm_abstract,
        },
        "auto_extracted": {
            "updated_at": datetime.now().isoformat(timespec="seconds"),
            "value_hierarchy": [
                {"rank": v["rank"], "value": v["value"],
                 "root_need": v["root_need"], "weight": v["weight"]}
                for v in profile["value_hierarchy"][:5]
            ],
            "dominant_biases": [
                {"bias": b["bias"], "intensity": b["intensity"]}
                for b in profile["cognitive_biases"] if b["hit_count"] > 0
            ][:3],
            "key_decision_rules": list(dict.fromkeys(
                r["recommended_action"] for r in profile["decision_rules"]))[:5],
            "abstract_heuristics": [
                h["abstract_rule"]
                for h in profile.get("abstract_identity", {}).get("decision_heuristics", [])
            ][:5],
            "internal_tensions": [
                t["tension"]
                for t in profile.get("abstract_identity", {}).get("internal_tensions", [])
            ][:3],
            "cross_source_insights": [
                r["insight"] for r in
                profile.get("cross_source_patterns", {}).get("diary_to_line_rules", [])
            ][:3],
            "interaction_tendencies": [
                r["insight"] for r in
                profile.get("interaction_patterns", {}).get("stimulus_response_rules", [])
                if r["stimulus_type"] != "その他" and r["response_type"] != "その他"
            ][:4],
            "latency_insights": [
                lp["insight"] for lp in
                profile.get("interaction_patterns", {}).get("latency_patterns", [])
            ][:4],
            "gap_insights": [
                {"theme": g["theme"], "type": g["type"], "gap": g["gap"],
                 "insight": g["insight"]}
                for g in profile.get("gap_analysis", {}).get("gaps", [])
            ][:4],
            "session_count": profile.get("interaction_patterns", {}).get("sessions_analyzed", 0),
        },
    }
    USER_PROFILE_JSON.parent.mkdir(parents=True, exist_ok=True)
    USER_PROFILE_JSON.write_text(json.dumps(user, ensure_ascii=False, indent=2),
                                 encoding="utf-8")
    print(f"[profiler] ユーザープロファイル更新: {USER_PROFILE_JSON}")


def main() -> None:
    use_llm = "--no-llm" not in sys.argv
    profile = build_profile(use_llm=use_llm)

    # 生成・更新: 既存プロファイルがあれば履歴メタデータを引き継いで更新
    now = datetime.now().isoformat(timespec="seconds")
    if PROFILE_JSON.exists():
        try:
            prev = json.loads(PROFILE_JSON.read_text(encoding="utf-8"))
            profile["meta"] = {
                "first_generated_at": prev.get("meta", {}).get("first_generated_at", now),
                "updated_at": now,
                "revision": prev.get("meta", {}).get("revision", 0) + 1,
            }
        except (json.JSONDecodeError, OSError):
            profile["meta"] = {"first_generated_at": now, "updated_at": now, "revision": 1}
    else:
        profile["meta"] = {"first_generated_at": now, "updated_at": now, "revision": 1}

    PROFILE_JSON.parent.mkdir(parents=True, exist_ok=True)
    PROFILE_JSON.write_text(json.dumps(profile, ensure_ascii=False, indent=2),
                            encoding="utf-8")
    print(f"[profiler] 深層プロファイル出力: {PROFILE_JSON} (rev {profile['meta']['revision']})")
    update_user_profile(profile)
    top_bias = profile["cognitive_biases"][0]
    top_value = profile["value_hierarchy"][0]
    print(f"[profiler]   最頻バイアス: {top_bias['bias']} (hits={top_bias['hit_count']})")
    print(f"[profiler]   最上位価値観: {top_value['value']} -> {top_value['root_need']}")
    print(f"[profiler]   意思決定ルール: {len(profile['decision_rules'])} 件")
    xs = profile["cross_source_patterns"]
    print(f"[profiler]   クロスソース(日記→LINE)ルール: {len(xs['diary_to_line_rules'])} 件 "
          f"(両ソース日: {xs['days_with_both_sources']}日)")
    ip = profile["interaction_patterns"]
    print(f"[profiler]   ConversationSession: {ip['sessions_analyzed']} 件 / "
          f"ルール {len(ip['stimulus_response_rules'])} 件 / "
          f"レイテンシパターン {len(ip['latency_patterns'])} 件")
    if "llm_interaction_insights" in profile:
        print(f"[profiler]   LLM因果分析: 完了 ({profile['llm_interaction_insights']['model']})")


if __name__ == "__main__":
    main()
