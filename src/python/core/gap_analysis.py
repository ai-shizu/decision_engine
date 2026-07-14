#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
主観×客観 差分分析 (Gap Analysis)
====================================
ユーザーデータを 2 軸に分離して突合し、認知的不協和と盲点を検出する。

  主観 (What you think):
    - 日記 (diary_text) — 私的な内省・感情・願望
    - AI相談の Query (consultations[].query) — 言語化された課題意識
  客観/行動 (What you do):
    - 家計簿 (transactions) — 金の投じ先というハードデータ
    - カレンダー (calendar_events) — 時間の投じ先
    - LINE の自分発話 (line_self_text) — 他者に対して表明した行動・約束

検出する差分 (v1 ベースライン: 線形差分・閾値 0.25):
  - intention_gap (意図過剰): 主観で頻出するのに金・時間・対外行動が伴わない
    → 「焦り・願望が行動に変換されていない」認知的不協和
  - blind_spot   (盲点)    : 金・時間を大きく投じているのに内省が皆無
    → 本人が言語化していない無自覚の行動パターン

非線形評価モデル (v2 拡張 — 就活・自己分析特化):
  - task_avoidance (双曲割引): 宣言タスクの実行遅延 D 日を V=1/(1+kD) で減衰評価
    → 先延ばし (面接対策・ES・コーディングテスト等) のタスク逃避を定量化
  - true_gakuchika (乖離ペア): 主観首位テーマ (建前/サンクコスト) と
    客観首位テーマ (真の熱量) のラベリング → ガクチカ候補の自動発掘
  - intellectualization_gap (防衛機制): 就活アクション 0 の週に日記の
    抽象語彙が急上昇 → 不安を難解な内省で覆う「知性化」の検知

設計原則:
  - 決定論的コア (このモジュール) は完全オフライン・LLM 不要で動作する
  - LLM 深化 (llm_gap_analysis) は任意。ギャップ表 + 生証拠を入力とし、
    「主観バイアス」と「行動データが示す事実」の言語化のみを担う
  - 各ギャップには必ず定量証拠 (金額・件数・引用) を添付し反証可能にする

【重み付けの絶対原則 — 削除・変更禁止】
  LINE 等の他者コミュニケーションの自分発話は、私的な日記より
  「行動コミットメント」に近い (他者に表明した瞬間に社会的拘束が生まれる)。
  そのため客観軸では LINE 発話を予定と同格の行動証拠として扱い、
  主観軸には決して混入させない。この分離が崩れると
  「日記にもLINEにも書いた=行動した」という自己申告の二重計上が起き、
  ギャップ検出の意味が消滅する。
"""

from __future__ import annotations

import re
import statistics
from datetime import date as _date

# ============================================================ テーマ分類体系
# 各テーマ = 主観マーカー (内省語彙) と客観マーカー (支出/予定/対外行動語彙) の対。
# 主観と客観で語彙を分けているのは意図的 — 「考える言葉」と「行動の痕跡」は別物。
THEME_TAXONOMY: dict[str, dict] = {
    "キャリア・仕事の将来": {
        "subjective": ["キャリア", "転職", "将来", "焦", "昇進", "市場価値",
                       "このままで", "方向転換", "スキル不足", "評価され"],
        "spend_categories": ["書籍", "教材", "セミナー", "講座", "資格",
                             "スクール", "勉強", "研修"],
        "calendar_keywords": ["勉強会", "面談", "面接", "セミナー", "講座",
                              "資格", "LT", "カンファレンス"],
        "line_keywords": ["転職", "エージェント", "勉強会", "面接", "応募"],
    },
    "学習・自己投資": {
        "subjective": ["学び", "学習", "勉強", "理解", "習得", "読みたい",
                       "身につけ", "極め"],
        "spend_categories": ["書籍", "教材", "講座", "サブスク(学習)", "勉強"],
        "calendar_keywords": ["勉強", "読書", "学習", "講座", "写経"],
        "line_keywords": ["読んだ", "勉強して", "写経", "学んだ", "記事"],
    },
    "健康・身体": {
        "subjective": ["健康", "運動", "睡眠", "疲", "体調", "痩せ", "体重",
                       "休息", "休養", "寝不足"],
        "spend_categories": ["ジム", "医療", "薬", "サプリ", "スポーツ", "病院"],
        "calendar_keywords": ["ジム", "ランニング", "筋トレ", "病院", "健診",
                              "ヨガ", "散歩"],
        "line_keywords": ["走った", "ジム", "筋トレ", "朝ラン", "病院"],
    },
    "人間関係・家族": {
        "subjective": ["家族", "妻", "夫", "友人", "孤独", "会いたい",
                       "人間関係", "感謝", "一緒に"],
        "spend_categories": ["交際費", "プレゼント", "外食", "飲み会", "旅行"],
        "calendar_keywords": ["飲み会", "食事", "デート", "帰省", "旅行", "会う"],
        "line_keywords": ["行くよ", "参加する", "会おう", "楽しみ", "ありがとう"],
    },
    "娯楽・消費": {
        "subjective": ["ゲーム", "動画", "趣味", "欲しい", "買いたい", "浪費",
                       "無駄遣い"],
        "spend_categories": ["娯楽", "ゲーム", "趣味", "サブスク", "課金",
                             "ガチャ", "買い物"],
        "calendar_keywords": ["ゲーム", "映画", "ライブ", "観戦"],
        "line_keywords": ["買った", "課金", "ポチった", "届いた"],
    },
    "金銭・経済的安定": {
        "subjective": ["貯金", "お金", "節約", "投資", "資産", "収入", "家計",
                       "金銭", "経済的"],
        "spend_categories": ["投資", "貯蓄", "積立", "保険", "NISA"],
        "calendar_keywords": ["銀行", "証券", "FP", "確定申告"],
        "line_keywords": ["積立", "投資", "節約", "NISA"],
    },
    "課外活動・組織運営": {
        "subjective": ["部活", "サークル", "マネジメント", "後輩", "主将",
                       "幹部", "部員", "組織", "運営", "チームを"],
        "spend_categories": ["部費", "合宿", "遠征", "サークル", "大会",
                             "ユニフォーム"],
        "calendar_keywords": ["部活", "練習", "合宿", "大会", "サークル",
                              "ミーティング", "新歓"],
        "line_keywords": ["練習", "合宿", "部員", "後輩", "大会", "シフト",
                          "集合"],
    },
    "技術開発・ものづくり": {
        "subjective": ["開発", "実装", "コード", "プログラミング", "低レイヤ",
                       "最適化", "OSS", "アルゴリズム", "作りたい"],
        "spend_categories": ["サーバー", "ドメイン", "開発", "PC", "キーボード",
                             "部品", "基板"],
        "calendar_keywords": ["開発", "ハッカソン", "競プロ", "コンテスト",
                              "もくもく", "リリース"],
        "line_keywords": ["実装した", "コミット", "リリース", "デバッグ",
                          "競プロ", "動いた"],
    },
}

# 就活文脈の語彙をキャリアテーマへ拡張 (学生ユーザー: 転職語彙と併存させる)
THEME_TAXONOMY["キャリア・仕事の将来"]["subjective"] += [
    "就活", "ES", "エントリーシート", "面接", "選考", "内定", "ガクチカ",
    "業界", "説明会", "インターン",
]
THEME_TAXONOMY["キャリア・仕事の将来"]["spend_categories"] += [
    "スーツ", "証明写真", "就活", "対策本",
]
THEME_TAXONOMY["キャリア・仕事の将来"]["calendar_keywords"] += [
    "説明会", "選考", "インターン", "ES", "OB訪問", "座談会",
]
THEME_TAXONOMY["キャリア・仕事の将来"]["line_keywords"] += [
    "内定", "選考", "説明会", "ES", "インターン",
]

# 主観スコアの加重: 願望・焦燥マーカーを含む言及は「強い主観」として重み2倍。
# 「〜したい」と書くだけで行動しない状態こそが検出対象のため。
INTENT_MARKERS = ["したい", "しなきゃ", "べき", "焦", "不安", "やらないと",
                  "つもり", "目標", "なりたい", "目指"]

# ギャップ判定閾値: スコアは 0〜1 に正規化されるため、0.25 以上の乖離を有意とみなす
GAP_THRESHOLD = 0.25
# 客観行動がほぼゼロとみなす下限
NEGLIGIBLE = 0.05


# ============================================================ コーパス構築
# 面接・GD・ES添削 (is_simulated_persona) 由来のテキストは「選考用の建前人格」。
# 真の自己分析空間を虚飾で汚染しないよう、主観スコアへの寄与を 0.1 に抑える。
SIMULATED_PERSONA_WEIGHT = 0.1
# 宣言検出など 0/1 判定の文脈で「本物の主観」とみなす重みの下限
GENUINE_DOC_MIN_WEIGHT = 0.5


def build_subjective_corpus(daily: list[dict]) -> list[dict]:
    """主観コーパス: 日記 + 相談 Query のみ。LINE は含めない (行動側)。

    各 doc は weight を持つ: 日記・通常相談 = 1.0、
    建前人格 (is_simulated_persona) = SIMULATED_PERSONA_WEIGHT。"""
    docs = []
    for dc in daily:
        genuine_parts = []
        simulated_parts = []
        if dc.get("diary_text", "").strip():
            genuine_parts.append(dc["diary_text"].strip())
        for c in dc.get("consultations", []):
            q = str(c.get("query", "")).strip()
            if not q:
                continue
            if c.get("is_simulated_persona"):
                simulated_parts.append(q)
            else:
                genuine_parts.append(q)
        if genuine_parts:
            docs.append({"date": dc["date"], "weight": 1.0,
                         "text": "\n".join(genuine_parts)})
        if simulated_parts:
            docs.append({"date": dc["date"],
                         "weight": SIMULATED_PERSONA_WEIGHT,
                         "text": "\n".join(simulated_parts)})
    return docs


def build_objective_signals(daily: list[dict]) -> dict:
    """客観シグナル: 支出 (カテゴリ別金額) / 予定 (タイトル) / LINE 自分発話。"""
    spend: dict[str, int] = {}
    events: list[dict] = []
    line_docs: list[dict] = []
    for dc in daily:
        for tx in dc.get("transactions", []):
            if tx.get("type") == "expense":
                cat = str(tx.get("category", "")).strip() or "(不明)"
                spend[cat] = spend.get(cat, 0) + int(tx.get("amount", 0))
        for ev in dc.get("calendar_events", []):
            events.append({"date": dc["date"], "title": str(ev.get("title", ""))})
        if dc.get("line_self_text", "").strip():
            line_docs.append({"date": dc["date"], "text": dc["line_self_text"]})
    return {"spend_by_category": spend, "events": events, "line_docs": line_docs}


# ============================================================ スコアリング
def _subjective_theme_scores(docs: list[dict]) -> dict[str, dict]:
    """テーマ別の主観言及スコア (0〜1 正規化) と証拠引用。"""
    raw: dict[str, dict] = {
        t: {"weighted_hits": 0.0, "quotes": []} for t in THEME_TAXONOMY
    }
    for doc in docs:
        doc_weight = float(doc.get("weight", 1.0))
        for theme, spec in THEME_TAXONOMY.items():
            for kw in spec["subjective"]:
                for m in re.finditer(re.escape(kw), doc["text"]):
                    s = max(0, m.start() - 15)
                    snippet = doc["text"][s : m.end() + 25].replace("\n", " ").strip()
                    weight = 2.0 if any(im in snippet for im in INTENT_MARKERS) else 1.0
                    raw[theme]["weighted_hits"] += weight * doc_weight
                    # 引用証拠は本物の主観からのみ採る (建前の引用は誤誘導)
                    if doc_weight >= GENUINE_DOC_MIN_WEIGHT \
                            and len(raw[theme]["quotes"]) < 3:
                        raw[theme]["quotes"].append(
                            {"date": doc["date"], "quote": f"…{snippet}…"})
    total = sum(v["weighted_hits"] for v in raw.values()) or 1.0
    return {
        t: {
            "score": round(v["weighted_hits"] / total, 3),
            "weighted_hits": round(v["weighted_hits"], 1),
            "quotes": v["quotes"],
        }
        for t, v in raw.items()
    }


def _objective_theme_scores(signals: dict) -> dict[str, dict]:
    """テーマ別の客観行動スコア。金 (支出シェア)・時間 (予定シェア)・
    対外行動 (LINE 言及シェア) を等重で合成する。

    LINE を予定・支出と同格に置くのは設計原則 (モジュール docstring 参照)。"""
    spend = signals["spend_by_category"]
    events = signals["events"]
    line_docs = signals["line_docs"]
    total_spend = sum(spend.values()) or 1
    total_events = len(events) or 1

    line_hits_by_theme: dict[str, int] = {t: 0 for t in THEME_TAXONOMY}
    line_evidence: dict[str, list] = {t: [] for t in THEME_TAXONOMY}
    total_line_hits = 0
    for doc in line_docs:
        for theme, spec in THEME_TAXONOMY.items():
            for kw in spec["line_keywords"]:
                n = doc["text"].count(kw)
                if n:
                    line_hits_by_theme[theme] += n
                    total_line_hits += n
                    if len(line_evidence[theme]) < 2:
                        line_evidence[theme].append(
                            {"date": doc["date"], "keyword": kw})

    out: dict[str, dict] = {}
    for theme, spec in THEME_TAXONOMY.items():
        theme_spend = sum(
            amt for cat, amt in spend.items()
            if any(kw in cat for kw in spec["spend_categories"])
        )
        theme_events = [
            ev for ev in events
            if any(kw in ev["title"] for kw in spec["calendar_keywords"])
        ]
        money_share = theme_spend / total_spend
        time_share = len(theme_events) / total_events
        line_share = (
            line_hits_by_theme[theme] / total_line_hits if total_line_hits else 0.0
        )
        out[theme] = {
            "score": round((money_share + time_share + line_share) / 3, 3),
            "money_spent": theme_spend,
            "money_share": round(money_share, 3),
            "event_count": len(theme_events),
            "time_share": round(time_share, 3),
            "line_mention_share": round(line_share, 3),
            "evidence": {
                "events": [ev["title"] for ev in theme_events[:3]],
                "line": line_evidence[theme],
            },
        }
    return out


# ============================================================ ギャップ検出
def detect_gaps(
    subjective: dict[str, dict],
    objective: dict[str, dict],
    threshold: float = GAP_THRESHOLD,
) -> list[dict]:
    """主観スコアと客観スコアの乖離から認知的不協和・盲点を列挙する。"""
    gaps: list[dict] = []
    for theme in THEME_TAXONOMY:
        s = subjective[theme]
        o = objective[theme]
        gap = round(s["score"] - o["score"], 3)

        if gap >= threshold and o["score"] <= s["score"]:
            gaps.append({
                "theme": theme,
                "type": "intention_gap",
                "gap": gap,
                "insight": (
                    f"「{theme}」は内省・相談で強く言及される (主観 {s['score']:.0%}) が、"
                    f"支出 {o['money_spent']:,}円 / 関連予定 {o['event_count']}件 / "
                    f"対外的言及シェア {o['line_mention_share']:.0%} と行動が伴っていない。"
                    "考えるだけで資源 (金・時間) を投じていない認知的不協和の候補"
                ),
                "subjective": {"score": s["score"], "quotes": s["quotes"]},
                "objective": {k: o[k] for k in
                              ("score", "money_spent", "event_count",
                               "line_mention_share", "evidence")},
            })
        elif -gap >= threshold and o["score"] > NEGLIGIBLE:
            gaps.append({
                "theme": theme,
                "type": "blind_spot",
                "gap": gap,
                "insight": (
                    f"「{theme}」に支出 {o['money_spent']:,}円 / 予定 {o['event_count']}件と"
                    f"実資源を投じている (客観 {o['score']:.0%}) のに、"
                    f"日記・相談ではほぼ言及がない (主観 {s['score']:.0%})。"
                    "本人が言語化していない無自覚の行動パターン (盲点) の候補"
                ),
                "subjective": {"score": s["score"], "quotes": s["quotes"]},
                "objective": {k: o[k] for k in
                              ("score", "money_spent", "event_count",
                               "line_mention_share", "evidence")},
            })
    gaps.sort(key=lambda g: -abs(g["gap"]))
    return gaps


# ============================================================ 非線形評価1: 双曲割引 (タスク逃避)
# 「宣言した重要タスク」の実行遅延を双曲割引 V = 1/(1+kD) で減衰評価する。
# 人間の先延ばしは指数割引より双曲割引に従う (Ainslie) — 遅延初期の価値低下が
# 急峻なため、数日の先延ばしでも行動価値が大きく毀損したと評価される。
TASK_LEXICON: dict[str, list[str]] = {
    "面接対策": ["面接対策", "面接練習", "模擬面接"],
    "ES執筆": ["ES", "エントリーシート"],
    "コーディングテスト対策": ["LeetCode", "リートコード", "競プロ",
                               "コーディングテスト", "AtCoder", "過去問"],
    "Webテスト対策": ["Webテスト", "SPI", "玉手箱"],
    "企業研究・OB訪問": ["企業研究", "業界研究", "OB訪問"],
}
DECLARE_MARKERS = ["こそ", "やる", "やろう", "やらないと", "やらねば", "しなきゃ",
                   "始める", "取り組む", "進める", "解く", "書く", "出す",
                   "対策する", "受けよう"]
DONE_MARKERS = ["やった", "解いた", "受けた", "終えた", "終わらせた", "提出した",
                "出した", "書き上げた", "行ってきた", "完了", "済ませた"]
K_HYPERBOLIC = 0.3        # 割引率: D=7日 で価値 ≈ 0.32
AVOIDANCE_FLAG_THRESHOLD = 0.5


def hyperbolic_discount(delay_days: int) -> float:
    """遅延 D 日後の実行が持つ行動価値 (双曲割引)。即日=1.0、未実行は呼び出し側で 0。"""
    return 1.0 / (1.0 + K_HYPERBOLIC * max(0, delay_days))


def _parse_date(s: str) -> _date | None:
    try:
        return _date.fromisoformat(s)
    except (TypeError, ValueError):
        return None


def analyze_procrastination(daily: list[dict]) -> dict:
    """宣言 (主観) → 実行 (客観ログ) の遅延から「タスク逃避」を検出する。

    宣言: 日記・相談でタスク語彙 + 意思マーカーが共起 (完了マーカーなし)
    実行: カレンダー予定タイトル / LINE発話+完了マーカー / 日記+完了マーカー"""
    subj_docs = build_subjective_corpus(daily)

    declarations: list[dict] = []
    seen: set[tuple[str, str]] = set()
    for doc in subj_docs:
        # 建前人格 doc (低ウェイト) からは宣言を採らない — 面接で語った
        # 「毎日 LeetCode を解いています」は日常の宣言ではない
        if float(doc.get("weight", 1.0)) < GENUINE_DOC_MIN_WEIGHT:
            continue
        for task, kws in TASK_LEXICON.items():
            for kw in kws:
                for m in re.finditer(re.escape(kw), doc["text"]):
                    s = max(0, m.start() - 25)
                    snippet = doc["text"][s : m.end() + 30].replace("\n", " ")
                    if (any(dm in snippet for dm in DECLARE_MARKERS)
                            and not any(dn in snippet for dn in DONE_MARKERS)
                            and (task, doc["date"]) not in seen):
                        seen.add((task, doc["date"]))
                        declarations.append({
                            "task": task, "date": doc["date"],
                            "quote": f"…{snippet.strip()}…",
                        })

    executions: dict[str, list[tuple[str, str]]] = {t: [] for t in TASK_LEXICON}
    for dc in daily:
        d = dc["date"]
        for task, kws in TASK_LEXICON.items():
            for ev in dc.get("calendar_events", []):
                if any(kw in str(ev.get("title", "")) for kw in kws):
                    executions[task].append((d, f"予定: {ev.get('title', '')}"))
                    break
            line = dc.get("line_self_text", "")
            if line and any(kw in line for kw in kws) \
                    and any(dn in line for dn in DONE_MARKERS):
                executions[task].append((d, "LINE発話 (完了報告)"))
            diary = dc.get("diary_text", "")
            if diary:
                for kw in kws:
                    for m in re.finditer(re.escape(kw), diary):
                        s = max(0, m.start() - 25)
                        snip = diary[s : m.end() + 30]
                        if any(dn in snip for dn in DONE_MARKERS):
                            executions[task].append((d, "日記 (完了記録)"))
                            break
                    else:
                        continue
                    break
    for task in executions:
        executions[task] = sorted(set(executions[task]))

    per_task: dict[str, dict] = {}
    for decl in declarations:
        task = decl["task"]
        decl_d = _parse_date(decl["date"])
        exec_date, exec_kind, delay, value = None, None, None, 0.0
        if decl_d:
            for d_str, kind in executions[task]:
                ex = _parse_date(d_str)
                if ex and ex >= decl_d:
                    exec_date, exec_kind = d_str, kind
                    delay = (ex - decl_d).days
                    value = hyperbolic_discount(delay)
                    break
        entry = per_task.setdefault(task, {"task": task, "records": []})
        entry["records"].append({
            "declared": decl["date"], "executed": exec_date,
            "execution_evidence": exec_kind, "delay_days": delay,
            "discounted_value": round(value, 3), "quote": decl["quote"],
        })

    tasks, flags = [], []
    for task, entry in per_task.items():
        values = [r["discounted_value"] for r in entry["records"]]
        avoidance = round(1.0 - statistics.mean(values), 3)
        entry["avoidance_index"] = avoidance
        entry["flagged"] = avoidance >= AVOIDANCE_FLAG_THRESHOLD
        tasks.append(entry)
        if entry["flagged"]:
            unexec = sum(1 for r in entry["records"] if r["executed"] is None)
            flags.append({
                "theme": f"就活タスク: {task}",
                "type": "task_avoidance",
                "gap": avoidance,
                "insight": (
                    f"「{task}」を {len(entry['records'])} 回宣言したが、"
                    f"未実行 {unexec} 件・実行遅延による双曲割引後の行動価値は "
                    f"{1 - avoidance:.0%}。精神的負荷の高いタスクの先延ばし "
                    "(タスク逃避) パターン"
                ),
                "subjective": {"quotes": [
                    {"date": r["declared"], "quote": r["quote"]}
                    for r in entry["records"][:2]
                ]},
                "objective": {"records": entry["records"][:4]},
            })
    tasks.sort(key=lambda t: -t["avoidance_index"])
    return {"declarations": len(declarations), "tasks": tasks, "flags": flags}


# ============================================================ 非線形評価2: 真のガクチカ発掘
# 主観の最大テーマ (語っている自分) と客観の最大テーマ (資源を投じている自分) の
# 乖離ペアから「建前/サンクコスト」と「真の熱量 (ガクチカ候補)」をラベリングする。
DECLARED_FOCUS_MIN = 0.3   # 建前と認定する最低主観シェア
TRUE_PASSION_MIN = 0.35    # 真の熱量と認定する最低客観シェア


def analyze_gakuchika(subjective: dict[str, dict],
                      objective: dict[str, dict]) -> dict:
    """主観×客観の首位テーマ乖離から自己アピールの武器を抽出する。"""
    declared_theme = max(subjective, key=lambda t: subjective[t]["score"])
    declared = subjective[declared_theme]
    if declared["score"] < DECLARED_FOCUS_MIN:
        return {"detected": False, "reason": "支配的な主観テーマなし", "flags": []}

    candidates = [
        (t, objective[t]["score"]) for t in THEME_TAXONOMY
        if t != declared_theme and objective[t]["score"] >= TRUE_PASSION_MIN
    ]
    if not candidates:
        return {"detected": False, "reason": "客観行動の集中先なし", "flags": []}
    passion_theme, passion_score = max(candidates, key=lambda c: c[1])

    declared_obj = objective[declared_theme]["score"]
    # 語っているテーマに行動が伴い熱量首位でもあるなら、乖離ではなく一致
    if declared_obj >= passion_score * 0.5:
        return {"detected": False, "reason": "主観テーマに行動が伴っている", "flags": []}

    divergence = round(passion_score - declared_obj, 3)
    po = objective[passion_theme]
    result = {
        "detected": True,
        "declared_focus": {
            "theme": declared_theme,
            "label": "建前/サンクコスト",
            "subjective_score": declared["score"],
            "objective_score": declared_obj,
            "quotes": declared["quotes"][:2],
        },
        "true_passion": {
            "theme": passion_theme,
            "label": "真の熱量 (ガクチカ候補)",
            "objective_score": passion_score,
            "money_spent": po["money_spent"],
            "event_count": po["event_count"],
            "evidence": po["evidence"],
        },
        "divergence": divergence,
        "insight": (
            f"言語化された関心は「{declared_theme}」に集中している "
            f"(主観 {declared['score']:.0%}) が、実際の金・時間・対外行動は"
            f"「{passion_theme}」に投下されている (客観 {passion_score:.0%}, "
            f"支出 {po['money_spent']:,}円, 予定 {po['event_count']}件)。"
            f"「{declared_theme}」の語りは建前またはサンクコストの可能性があり、"
            f"具体的エピソードで語れる本物の武器 (ガクチカ) は「{passion_theme}」にある"
        ),
    }
    result["flags"] = [{
        "theme": passion_theme,
        "type": "true_gakuchika",
        "gap": divergence,
        "insight": result["insight"],
        "subjective": {"quotes": declared["quotes"][:2]},
        "objective": {k: po[k] for k in
                      ("score", "money_spent", "event_count", "evidence")},
    }]
    return result


# ============================================================ 非線形評価3: 知性化 (防衛機制) 検知
# 行動 (就活アクション) が止まった週に、日記の抽象語彙が急上昇するパターンを
# 「不安から抽象的思考へ逃避する知性化 (intellectualization)」として検出する。
ABSTRACT_LEXICON = ["本質", "アーキテクチャ", "哲学", "長期的視野", "抽象",
                    "概念", "構造的", "メタ", "俯瞰", "パラダイム", "方法論",
                    "原理原則", "普遍", "体系", "あるべき姿", "自己実現"]
JOBHUNT_ACTION_KEYWORDS = ["面接", "説明会", "選考", "ES", "エントリー",
                           "OB訪問", "インターン", "テスト", "面談", "応募",
                           "座談会"]
ABSTRACT_MIN_HITS = 3          # 急上昇と判定する週あたり最低ヒット数
ABSTRACT_SPIKE_RATIO = 1.5     # 全週平均に対する倍率


def analyze_intellectualization(daily: list[dict]) -> dict:
    """ISO週単位で (抽象語インデックス, 就活アクション数) を突合する。"""
    weeks: dict[tuple[int, int], dict] = {}
    for dc in daily:
        d = _parse_date(dc["date"])
        if d is None:
            continue
        iso = d.isocalendar()
        key = (iso[0], iso[1])
        wk = weeks.setdefault(key, {
            "week": f"{iso[0]}-W{iso[1]:02d}", "abstract_hits": 0,
            "action_count": 0, "quotes": [], "dates": [],
        })
        wk["dates"].append(dc["date"])
        diary = dc.get("diary_text", "")
        for kw in ABSTRACT_LEXICON:
            for m in re.finditer(re.escape(kw), diary):
                wk["abstract_hits"] += 1
                if len(wk["quotes"]) < 2:
                    s = max(0, m.start() - 12)
                    wk["quotes"].append({
                        "date": dc["date"],
                        "quote": f"…{diary[s : m.end() + 20].strip()}…",
                    })
        for ev in dc.get("calendar_events", []):
            if any(kw in str(ev.get("title", "")) for kw in JOBHUNT_ACTION_KEYWORDS):
                wk["action_count"] += 1
        line = dc.get("line_self_text", "")
        if line and any(kw in line for kw in JOBHUNT_ACTION_KEYWORDS):
            wk["action_count"] += 1

    ordered = [weeks[k] for k in sorted(weeks)]
    hit_counts = [w["abstract_hits"] for w in ordered] or [0]
    baseline = statistics.mean(hit_counts)

    flags = []
    for wk in ordered:
        spike = (wk["abstract_hits"] >= ABSTRACT_MIN_HITS
                 and wk["abstract_hits"] >= ABSTRACT_SPIKE_RATIO * max(baseline, 1.0))
        wk["flagged"] = bool(spike and wk["action_count"] == 0)
        if wk["flagged"]:
            flags.append({
                "theme": f"知性化の疑い ({wk['week']})",
                "type": "intellectualization_gap",
                "gap": round(min(1.0, wk["abstract_hits"] / 6.0), 3),
                "insight": (
                    f"{wk['week']} は就活アクション 0 件なのに、日記の抽象語彙が "
                    f"{wk['abstract_hits']} 回 (全期間平均 {baseline:.1f} 回/週) と急上昇。"
                    "行動が止まった不安を抽象的思考で覆い隠す防衛機制 (知性化) の疑い。"
                    "難解な内省が増えた時こそ、小さな具体的行動 (ES1本・過去問1問) に"
                    "立ち返るシグナル"
                ),
                "subjective": {"quotes": wk["quotes"]},
                "objective": {"action_count": 0,
                              "abstract_hits": wk["abstract_hits"],
                              "baseline_avg": round(baseline, 1)},
            })
    return {"weeks": ordered, "baseline_avg_hits": round(baseline, 1),
            "flags": flags}


# ============================================================ 非線形評価4: ライフバランス・スタビライザー
# 「私的時間 (パートナー・友人) への投資日」に生産性の罪悪感を表明していても、
# 直後の別テーマの効率が向上していれば、その時間を「浪費」ではなく
# 「必要な充電 (スタビライザー)」として肯定的に再フレーミングする。
# 罪悪感 (Productivity_Guilt_Trap) の検知だけで終わらせず、行動データで
# 罪悪感の妥当性そのものを検証するのが本モデルの核心。
PRIVATE_TIME_KEYWORDS = ["デート", "パートナー", "彼女", "彼氏", "恋人",
                         "記念日", "旅行", "食事", "飲み会", "会う"]
GUILT_MARKERS = ["罪悪感", "生産性が落ち", "進まなかった", "進んでいない",
                 "サボって", "無駄にした", "遊んでしまった", "勉強できなかった",
                 "進まない", "やるべきだった"]
PRODUCTIVITY_MARKERS = ["捗った", "はかどった", "集中できた", "一気に進んだ",
                        "効率よく", "実装が進んだ", "解けた", "乗ってきた",
                        "スラスラ", "冴えて"]
STABILIZER_WINDOW_DAYS = 2   # 私的時間の前後を比較する日数窓


def _productivity_hits(dc: dict) -> int:
    blob = dc.get("diary_text", "") + "\n" + dc.get("line_self_text", "")
    return sum(blob.count(kw) for kw in PRODUCTIVITY_MARKERS)


def analyze_life_balance(daily: list[dict]) -> dict:
    """私的時間 → 罪悪感 → 事後効率の3点を突合し、充電効果を検証する。"""
    by_date: dict[str, dict] = {}
    dates: list[_date] = []
    for dc in daily:
        d = _parse_date(dc["date"])
        if d is not None:
            by_date[dc["date"]] = dc
            dates.append(d)

    episodes, flags = [], []
    for dc in daily:
        d = _parse_date(dc["date"])
        if d is None:
            continue
        private_evidence = [
            str(ev.get("title", "")) for ev in dc.get("calendar_events", [])
            if any(kw in str(ev.get("title", "")) for kw in PRIVATE_TIME_KEYWORDS)
        ]
        if not private_evidence:
            continue

        # 罪悪感: 当日または翌日の日記
        guilt_quote = None
        for offset in (0, 1):
            nd = (d.__class__.fromordinal(d.toordinal() + offset)).isoformat()
            diary = by_date.get(nd, {}).get("diary_text", "")
            for gm in GUILT_MARKERS:
                m = re.search(re.escape(gm), diary)
                if m:
                    s = max(0, m.start() - 15)
                    guilt_quote = {"date": nd,
                                   "quote": f"…{diary[s : m.end() + 25].strip()}…"}
                    break
            if guilt_quote:
                break
        if guilt_quote is None:
            continue  # 罪悪感の表明がなければトラップではない

        # 事前/事後の生産性シグナルを比較 (前後 STABILIZER_WINDOW_DAYS 日)
        before = after = 0
        for off in range(1, STABILIZER_WINDOW_DAYS + 1):
            b = (d.__class__.fromordinal(d.toordinal() - off)).isoformat()
            a = (d.__class__.fromordinal(d.toordinal() + off)).isoformat()
            if b in by_date:
                before += _productivity_hits(by_date[b])
            if a in by_date:
                after += _productivity_hits(by_date[a])

        episode = {
            "date": dc["date"],
            "private_events": private_evidence[:3],
            "guilt": guilt_quote,
            "productivity_before": before,
            "productivity_after": after,
            "stabilizer_confirmed": after > before,
        }
        episodes.append(episode)

        if episode["stabilizer_confirmed"]:
            delta = after - before
            flags.append({
                "theme": f"充電効果: {private_evidence[0]}",
                "type": "stabilizer_effect",
                "gap": round(min(1.0, delta / 3.0), 3),
                "insight": (
                    f"{dc['date']} の「{private_evidence[0]}」について"
                    f"罪悪感を表明している (Productivity_Guilt_Trap) が、"
                    f"事後 {STABILIZER_WINDOW_DAYS} 日間の生産性シグナルは "
                    f"{before} → {after} と向上している。この時間は浪費ではなく"
                    "「必要な充電 (スタビライザー)」として機能しており、"
                    "罪悪感なく定期的に確保することを推奨する"
                ),
                "subjective": {"quotes": [guilt_quote]},
                "objective": {"private_events": private_evidence[:3],
                              "productivity_before": before,
                              "productivity_after": after},
            })
    return {"episodes": episodes, "flags": flags}


def analyze_gaps(daily: list[dict]) -> dict:
    """Gap Analysis のエントリポイント (決定論・オフライン)。"""
    subj_docs = build_subjective_corpus(daily)
    signals = build_objective_signals(daily)
    subjective = _subjective_theme_scores(subj_docs)
    objective = _objective_theme_scores(signals)
    gaps = detect_gaps(subjective, objective)

    # 非線形評価モデル (線形差分ベースラインに追加合流)
    procrastination = analyze_procrastination(daily)
    gakuchika = analyze_gakuchika(subjective, objective)
    intellectualization = analyze_intellectualization(daily)
    life_balance = analyze_life_balance(daily)
    gaps += procrastination["flags"]
    gaps += gakuchika["flags"]
    gaps += intellectualization["flags"]
    gaps += life_balance["flags"]
    gaps.sort(key=lambda g: -abs(g["gap"]))

    total_spend = sum(signals["spend_by_category"].values())
    coverage = {
        "subjective_docs": len(subj_docs),
        "total_expense_yen": total_spend,
        "calendar_events": len(signals["events"]),
        "line_docs": len(signals["line_docs"]),
    }
    # データ量が乏しい時は信頼度を明示的に下げる (少データでの断定を防ぐ)
    data_sufficiency = min(
        1.0,
        statistics.mean([
            min(1.0, len(subj_docs) / 7),
            min(1.0, (1 if total_spend > 0 else 0)
                + (1 if signals["events"] else 0)
                + (1 if signals["line_docs"] else 0)) ,
        ]),
    )
    return {
        "schema": "gap_analysis.v3",
        "coverage": coverage,
        "data_sufficiency": round(data_sufficiency, 2),
        "subjective_scores": subjective,
        "objective_scores": objective,
        "procrastination": procrastination,
        "gakuchika": gakuchika,
        "intellectualization": intellectualization,
        "life_balance": life_balance,
        "gaps": gaps,
    }


# ============================================================ LLM 深化 (任意)
def format_gap_table(result: dict, max_gaps: int = 4) -> str:
    """ギャップを LLM プロンプト / CONSULT 注入用の簡潔なテキストへ整形。"""
    gaps = result.get("gaps", [])
    if not gaps:
        return "(有意な主観×客観ギャップは未検出)"
    label = {
        "intention_gap": "意図過剰(考えるだけ)",
        "blind_spot": "盲点(無自覚の行動)",
        "task_avoidance": "タスク逃避(先延ばし)",
        "true_gakuchika": "真の熱量(ガクチカ候補)",
        "intellectualization_gap": "知性化(不安の隠蔽)",
        "stabilizer_effect": "充電効果(罪悪感の反証)",
    }
    lines = []
    for g in gaps[:max_gaps]:
        kind = label.get(g["type"], g["type"])
        lines.append(
            f"- [{kind}] {g['theme']} (乖離 {g['gap']:+.2f}): {g['insight']}")
        for q in g.get("subjective", {}).get("quotes", [])[:1]:
            lines.append(f"    主観の証拠: [{q['date']}] {q['quote']}")
    if result.get("data_sufficiency", 1.0) < 0.5:
        lines.append("(注: データ量が少ないため確度は低い — 断定を避けること)")
    return "\n".join(lines)


def llm_gap_analysis(result: dict, daily: list[dict]) -> dict | None:
    """検出済みギャップ表を LLM に渡し、認知的不協和の言語化を深める。

    決定論コアが検出した数値的乖離のみを入力とし、LLM には
    「解釈と言語化」だけを担わせる (ギャップの発見自体を LLM 任せにしない)。"""
    from .llm_backend import LlamaStdioBackend
    from .llm_config import find_gguf
    from .paths import LLAMA_CLI_EXE

    if not result.get("gaps"):
        return None
    model = find_gguf()
    if not (model and LLAMA_CLI_EXE.exists()):
        print("[gap_analysis] ローカルLLM未検出 → LLM深化をスキップ")
        return None

    system = (
        "あなたは認知行動科学の専門家である。ユーザーの主観 (日記・相談) と"
        "客観的行動 (支出・予定・他者への発話) の定量的な乖離データから、"
        "本人が気づいていない認知的不協和を、事実に基づき誠実に言語化する。"
        "行動データ (金額・件数) を根拠の中心に置き、憶測での断定を避ける。"
    )
    prompt = f"""以下は決定論的アルゴリズムが検出した「主観と客観のギャップ」である。

# 検出済みギャップ (数値は 0〜1 正規化スコア)
{format_gap_table(result, max_gaps=6)}

# 分析指示
各ギャップについて以下を 2〜3 文で言語化せよ:
1. 主観的バイアス: 本人はどう思い込んでいるか (日記の引用を根拠に)
2. 行動データが示す事実: 金・時間・対外行動は何を語っているか (数値を根拠に)
3. 自己認識へのインサイト: この乖離を埋める/受け入れるための問い

【制約】
- 検出済みギャップの範囲内で語ること。新たなギャップを創作しない。
- データ量が少ない旨の注記があれば、可能性の提示に留めること。
- 出力は日本語の箇条書き。"""

    backend = LlamaStdioBackend(LLAMA_CLI_EXE, model)
    try:
        print(f"[gap_analysis] LLMでギャップ言語化を実行中… ({model.name})")
        analysis = backend.generate(system, prompt, max_tokens=900)
    finally:
        backend.stop()
    return {"model": model.name, "analysis": analysis}
