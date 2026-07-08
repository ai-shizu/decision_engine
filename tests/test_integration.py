# -*- coding: utf-8 -*-
"""フェーズ2 拡張の統合テスト (決定論・オフライン)。

対象:
  1. ライフバランス・スタビライザー評価 (gap_analysis.analyze_life_balance)
  2. CONSULT interview_sim モード (状態機械 + gap_insights 接続講評)
  3. 外部知識オンデマンド・インジェクション (knowledge_fetcher)
     — オフライン既定・キュー永続化・モック取得での取り込みまで

モックペルソナ: トップティア SWE を目指して焦る情報理工系大学3年生。
躰道部の新歓マネジメントとパートナーとの時間に意義を見出し、
C++ の低レイヤ開発に没頭している。

実行は一時 PKB_PROJECT_ROOT 上で行い、実データには一切触れない。
"""
import json
import os
import shutil
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

# core 各モジュールはインポート時に PKB_PROJECT_ROOT を解決するため、先に設定する
_TMP = tempfile.mkdtemp(prefix="pkb_integration_")
os.environ["PKB_PROJECT_ROOT"] = _TMP
os.environ.setdefault("HF_HUB_OFFLINE", "1")
os.environ.setdefault("TRANSFORMERS_OFFLINE", "1")
os.environ.pop("PKB_ALLOW_ONLINE_FETCH", None)  # オフライン既定を保証

from core import es_manager  # noqa: E402
from core import knowledge_fetcher as kf  # noqa: E402
from core.gap_analysis import analyze_gaps  # noqa: E402
from core.consultation_engine import (  # noqa: E402
    ConsultationEngine,
    GD_SYSTEM_PROMPT,
    INTERVIEW_CASE_BANK,
    INTERVIEWER_SYSTEM_PROMPT,
    build_gd_system_prompt,
)
from core.paths import (  # noqa: E402
    AI_CONSULTATIONS_JSON,
    DEEP_PROFILE,
    ES_DIR,
    INTERVIEW_RECORDS_DIR,
    KNOWLEDGE_DIR,
)

# 日常プロファイル (gap_insights) にのみ存在し、ES には現れない語。
# 議論フェーズのプロンプトにこれらが混入したら情報漏洩 (隔離違反) である。
# "既読スルー" = Target Delta-LINE DL2 (social_positioning_gap 合流) の
# 隔離ガード専用マーカー (AI_SKILLS §11 / SPEC I-14)。
# "oracle_payload"/"twin_forecast"/"coupling"/"OII"/"認知リソース" = Target Echo
# E0 の隔離ガード専用マーカー (docs/SPEC_ECHO_GENESIS.md §5.5-1 / I-22)。
GAP_LEAK_MARKERS = ("躰道部", "パートナー", "既読スルー",
                    "oracle_payload", "twin_forecast", "coupling", "OII", "認知リソース")


def _day(date: str, diary: str = "", events: list[dict] | None = None,
         tx: list[dict] | None = None, line_self: str = "") -> dict:
    return {
        "date": date,
        "diary_text": diary,
        "consultations": [],
        "transactions": tx or [],
        "calendar_events": events or [],
        "line_self_text": line_self,
    }


class FakeBackend:
    """generate 呼び出しを記録する決定論バックエンド (LLM 不使用)。"""

    name = "fake-backend"

    def __init__(self, canned: str = ""):
        self.calls: list[tuple[str, str]] = []
        self.canned = canned

    def generate(self, system: str, user: str, max_tokens=None, on_token=None):
        self.calls.append((system, user))
        answer = self.canned or f"FAKE応答{len(self.calls)}"
        if on_token:
            on_token(answer)
        return answer

    def stop(self):
        pass


class ScriptedBackend:
    """呼び出しごとに指定した応答を順番に返す決定論バックエンド (F4b テスト専用)。
    script を使い切ったら最後の要素を繰り返す (リトライループの検証用)。"""

    name = "scripted-backend"

    def __init__(self, script: list[str]):
        self.calls: list[tuple[str, str]] = []
        self.script = script

    def generate(self, system: str, user: str, max_tokens=None, on_token=None):
        self.calls.append((system, user))
        idx = min(len(self.calls) - 1, len(self.script) - 1)
        answer = self.script[idx]
        if on_token:
            on_token(answer)
        return answer

    def stop(self):
        pass


# ============================================================ 1. スタビライザー
def test_life_balance_stabilizer() -> None:
    daily = [
        # 事前2日: 生産性シグナルなし (低調)
        _day("2026-06-08", diary="今日も面接対策が手につかない。"),
        _day("2026-06-09", diary="なんとなく一日が終わった。"),
        # 私的時間 + 罪悪感 (Productivity_Guilt_Trap)
        _day("2026-06-10",
             diary="今日はデートで一日使った。LeetCodeが進まない気がして罪悪感がある。",
             events=[{"time": "12:00", "title": "パートナーと記念日ディナー"}]),
        # 事後2日: 別テーマ (低レイヤ開発) の効率が明確に向上
        _day("2026-06-11",
             diary="C++の低レイヤ最適化の実装が進んだ。頭が冴えて一気に進んだ。"),
        _day("2026-06-12", diary="今日も集中できた。SIMDカーネルが解けた。"),
        # 対照群: 私的時間 + 罪悪感だが、その後の効率向上なし → 肯定しない
        _day("2026-06-20",
             diary="また遊んでしまった。勉強できなかった。",
             events=[{"time": "19:00", "title": "友人と飲み会"}]),
        _day("2026-06-21", diary="疲れていて何もできず。"),
        _day("2026-06-22", diary="ぼんやりして終わった。"),
    ]
    result = analyze_gaps(daily)
    lb = result["life_balance"]
    assert len(lb["episodes"]) == 2, lb["episodes"]

    ep = {e["date"]: e for e in lb["episodes"]}
    good = ep["2026-06-10"]
    assert good["stabilizer_confirmed"], good
    assert good["productivity_after"] > good["productivity_before"]
    assert good["guilt"]["quote"], "罪悪感の引用が空"

    bad = ep["2026-06-20"]
    assert not bad["stabilizer_confirmed"], "効率向上のない罪悪感を誤って肯定"

    flags = [g for g in result["gaps"] if g["type"] == "stabilizer_effect"]
    assert len(flags) == 1, flags
    assert "パートナー" in flags[0]["theme"]
    assert "充電" in flags[0]["insight"] and "推奨" in flags[0]["insight"]
    print("  life-balance stabilizer OK")


# ============================================================ 2. interview_sim
def test_interview_sim_flow() -> None:
    # gap_insights (日常行動ギャップ) を deep_profile として配置
    DEEP_PROFILE.parent.mkdir(parents=True, exist_ok=True)
    DEEP_PROFILE.write_text(json.dumps({
        "schema": "deep_profile.v6",
        "gap_analysis": {
            "schema": "gap_analysis.v3",
            "data_sufficiency": 1.0,
            "gaps": [
                {"theme": "就活タスク: 面接対策", "type": "task_avoidance",
                 "gap": 0.8,
                 "insight": "躰道部の新歓マネジメントを口実に面接対策を先延ばしにしている",
                 "subjective": {"quotes": [
                     {"date": "2026-06-01", "quote": "…明日こそ面接対策をやる…"}]}},
                {"theme": "技術開発・ものづくり", "type": "true_gakuchika",
                 "gap": 0.6,
                 "insight": "実熱量は C++ 低レイヤ最適化に集中している",
                 "subjective": {"quotes": []}},
            ],
        },
    }, ensure_ascii=False), encoding="utf-8")

    fake = FakeBackend()
    eng = ConsultationEngine()
    eng._backend = fake

    # (1) 出題: ケースバンク先頭のテーマが面接官プロンプトに載る
    a1 = eng.consult("開始", mode="interview_sim")
    assert a1 == "FAKE応答1"
    sys1, user1 = fake.calls[0]
    assert sys1 == INTERVIEWER_SYSTEM_PROMPT
    assert INTERVIEW_CASE_BANK[0]["theme"] in user1
    assert eng._interview_state is not None

    # (2) 議論継続: 候補者発言がトランスクリプトへ入り、深掘り指示が出る
    eng.consult("市場を法人と個人にMECEに分割して推定します", mode="interview_sim")
    _, user2 = fake.calls[1]
    assert "市場を法人と個人にMECEに分割して推定します" in user2
    assert "これまでの議論" in user2

    # (3) 講評: MECE 講評 + gap_insights 接続 + 日常改善アクション
    eng.consult("講評", mode="interview_sim")
    _, user3 = fake.calls[2]
    assert "MECE" in user3
    assert "日常行動のギャップ分析" in user3
    assert "躰道部の新歓マネジメント" in user3, "gap_insights が講評プロンプトに未注入"
    assert "改善アクション" in user3
    assert eng._interview_state is None, "講評後にセッションが未リセット"

    # 講評は相談履歴に [interview_sim] として保存される
    log = json.loads(AI_CONSULTATIONS_JSON.read_text(encoding="utf-8"))
    entries = [e for day in log.values() for e in day]
    assert any("[interview_sim]" in e["query"] for e in entries)

    # (4) 再開始でケースバンクが決定論的に巡回する
    # (F4b: 直前の講評で成績表生成の追加呼び出しが挟まるため、直後の呼び出し
    # である fake.calls[-1] で判定する — 固定インデックスへの依存をやめる)
    eng.consult("開始", mode="interview_sim")
    _, user4 = fake.calls[-1]
    assert INTERVIEW_CASE_BANK[1]["theme"] in user4
    print("  interview_sim flow OK")


# ============================================================ 3. knowledge_fetcher
def test_fetch_tag_hook_and_queue() -> None:
    text = ("## 1. 現状分析\n本文。\n"
            "<fetch_query>ロックフリーキュー 設計 面接</fetch_query>\n"
            "<fetch_query>HFT レイテンシ 最新動向</fetch_query>")
    clean, queries = kf.extract_fetch_queries(text)
    assert "<fetch_query>" not in clean
    assert queries == ["ロックフリーキュー 設計 面接", "HFT レイテンシ 最新動向"]

    assert kf.queue_fetch_queries(queries) == 2
    assert kf.queue_fetch_queries(queries) == 0, "重複クエリが再追加された"
    queue = kf.load_queue()
    assert len(queue) == 2 and all(e["status"] == "pending" for e in queue)
    print("  fetch tag hook + queue OK")


def test_offline_default_never_fetches() -> None:
    """PKB_ALLOW_ONLINE_FETCH 未設定では、いかなる経路でも通信しない。"""
    assert not kf.online_fetch_allowed()
    summary = kf.process_pending()  # fetcher=None + 未許可 → 完全スキップ
    assert summary["processed"] == 0 and summary["skipped_offline"] == 2
    assert all(e["status"] == "pending" for e in kf.load_queue())

    try:
        kf.default_online_fetcher("test")
        raise AssertionError("未許可なのに default_online_fetcher が通った")
    except RuntimeError as exc:
        assert "PKB_ALLOW_ONLINE_FETCH" in str(exc)
    print("  offline-default safety OK")


def test_mock_fetch_ingestion_pipeline() -> None:
    """モック取得 → Markdown 永続化 → 既存ナレッジローダーでの取り込み。"""
    def mock_fetcher(query: str) -> list[dict]:
        return [{
            "title": f"{query} の解説",
            "url": "https://example.com/article",
            "text": f"{query} に関する専門知識のダミー本文。" * 5,
        }]

    summary = kf.process_pending(fetcher=mock_fetcher)
    assert summary["processed"] == 2 and summary["failed"] == 0, summary
    assert all(e["status"] == "done" for e in kf.load_queue())

    files = sorted(KNOWLEDGE_DIR.glob("fetched_*.md"))
    assert len(files) == 2, files
    body = files[0].read_text(encoding="utf-8")
    assert "出典: https://example.com/article" in body
    assert "専門知識のダミー本文" in body

    # 既存の知識チャンクローダー (consult の検索対象) が自動で拾うこと
    from core.consultation_engine import load_knowledge_chunks
    chunks = load_knowledge_chunks()
    assert any("専門知識のダミー本文" in c["text"] for c in chunks), \
        "取得知識が knowledge チャンクに統合されていない"

    # 失敗する fetcher は該当クエリだけを failed にし、全体を止めない
    kf.queue_fetch_queries(["失敗するクエリ"])

    def failing_fetcher(query: str) -> list[dict]:
        raise OSError("network unreachable")

    summary2 = kf.process_pending(fetcher=failing_fetcher)
    assert summary2["failed"] == 1 and summary2["processed"] == 0
    failed = [e for e in kf.load_queue() if e["status"] == "failed"]
    assert failed and "OSError" in failed[0]["error"]
    print("  mock fetch ingestion OK")


# ============================================================ フェーズ3: ES 駆動シミュレーター
ES_BODY = """# エントリーシート (2027卒)
志望職種: リアルタイムグラフィックスエンジニア

## 学生時代に力を入れたこと
C++ で OpenGL を用いた可視化エンジンを個人開発した。描画パイプラインの
ボトルネックを計測し、バッチングとカリングの導入でフレームレートを3倍に改善した。
設計から実装・検証まで、すべて一人で完遂したことが強みである。
"""


def _write_phase3_assets() -> None:
    """モックシナリオ: ES では OpenGL 可視化エンジンを強みと主張するが、
    日常ログでは躰道部の新歓・合宿マネジメントの摩擦から逃避し、
    パートナーとの時間と一人で完結するコーディングに閉じこもるユーザー。"""
    ES_DIR.mkdir(parents=True, exist_ok=True)
    es_path = ES_DIR / "opengl_engine.md"
    es_path.write_text(ES_BODY, encoding="utf-8")

    # 明示フィールドなしのクリエイティブ領域 ES (ドメイン非依存の証明用)
    film = ES_DIR / "film_planning.md"
    film.write_text(
        "# 企画書\n映像制作の企画。映像制作のワークフローを再設計し、"
        "コンポジットとカラーグレーディングを内製化する。映像制作の"
        "コンポジット工程を自動化した実績を持つ。\n",
        encoding="utf-8",
    )
    older = es_path.stat().st_mtime - 100
    os.utime(film, (older, older))  # select_es(None) は opengl を最新として選ぶ

    DEEP_PROFILE.parent.mkdir(parents=True, exist_ok=True)
    DEEP_PROFILE.write_text(json.dumps({
        "schema": "deep_profile.v6",
        "gap_analysis": {
            "schema": "gap_analysis.v3",
            "data_sufficiency": 1.0,
            "gaps": [
                {"theme": "課外活動・組織運営", "type": "task_avoidance",
                 "gap": 0.85,
                 "insight": "躰道部の新歓・合宿マネジメントの対人調整 (摩擦) から逃避し、"
                            "連絡・調整タスクを先延ばしにしている",
                 "subjective": {"quotes": [
                     {"date": "2026-06-01", "quote": "…合宿の調整は明日やる…"}]}},
                {"theme": "充電効果: パートナーとの時間", "type": "stabilizer_effect",
                 "gap": 0.5,
                 "insight": "パートナーとの時間の後に生産性が向上しており、充電として機能",
                 "subjective": {"quotes": []}},
                # Target Delta-LINE DL2: line_telemetry.analyze_social_positioning が
                # 合流させる想定の対人ギャップ (社会的立ち位置)。講評フェーズ以外の
                # プロンプトに絶対に出てはならない (GAP_LEAK_MARKERS の "既読スルー" 参照)。
                {"theme": "対人関係・役割認識", "type": "blind_spot",
                 "gap": 0.4,
                 "insight": "友人からの相談には既読スルーしがちな傾向があり、"
                            "関係維持コストへの言及が主観コーパスに一度もない",
                 "subjective": {"quotes": []}},
                # Target Echo E0: oracle.build_oracle_payload が合流させる想定の
                # 認知リソース枯渇の兆候 (docs/SPEC_ECHO_GENESIS.md §4)。講評フェーズ
                # 以外のプロンプトに絶対に出てはならない (GAP_LEAK_MARKERS 参照)。
                {"theme": "認知リソース枯渇の兆候 (Target Echo)", "type": "blind_spot",
                 "gap": 0.4,
                 "insight": "認知リソースの枯渇兆候が oracle_payload の coupling / "
                            "twin_forecast (OII 含む) から検出されているが、"
                            "本人はこれを自覚していない",
                 "subjective": {"quotes": []}},
                # format_gap_table の既定 max_gaps=4 切り詰めの外側に置く
                # (このエントリの内容は他テストで参照されない — 切り詰められてよい)。
                {"theme": "技術開発・ものづくり", "type": "true_gakuchika",
                 "gap": 0.6,
                 "insight": "一人で完結するコーディングに閉じこもる傾向。"
                            "実熱量は低レイヤ開発に集中している",
                 "subjective": {"quotes": []}},
            ],
            "interpersonal": {
                "friction_response": {"score": 0.3, "confidence": 0.8, "n_dyads": 5},
            },
        },
    }, ensure_ascii=False), encoding="utf-8")


def _assert_no_gap_leak(*prompt_parts: str) -> None:
    blob = "\n".join(prompt_parts)
    for marker in GAP_LEAK_MARKERS:
        assert marker not in blob, f"議論フェーズに日常プロファイルが漏洩: {marker}"


def test_es_manager_dynamic_domain() -> None:
    _write_phase3_assets()
    docs = es_manager.load_es_documents()
    assert len(docs) == 2 and docs[0]["name"] == "opengl_engine", \
        [d["name"] for d in docs]

    es = es_manager.select_es("opengl")
    assert es["target_domain"] == "リアルタイムグラフィックスエンジニア"
    assert es["explicit_domain"] is True
    assert "OpenGL" in es["keywords"], es["keywords"]

    # 明示フィールドなし → 本文語彙からドメイン導出 (業界ハードコードなしの証明)
    film = es_manager.select_es("film_planning")
    assert film["explicit_domain"] is False
    assert "映像制作" in film["target_domain"], film["target_domain"]

    persona = es_manager.build_interviewer_persona(es)
    assert "リアルタイムグラフィックスエンジニア" in persona
    assert "攻撃" in persona and "Adversarial" in persona
    print("  es_manager dynamic domain OK")


def test_es_review_isolation() -> None:
    fake = FakeBackend()
    eng = ConsultationEngine()
    eng._backend = fake
    eng.consult("opengl_engine", mode="es_review")

    system, user = fake.calls[0]
    assert "リアルタイムグラフィックスエンジニア" in system
    assert "採用責任者" in system
    assert "可視化エンジン" in user and "論理破綻" in user
    _assert_no_gap_leak(system, user)  # es_review は gap を絶対に注入しない

    log = json.loads(AI_CONSULTATIONS_JSON.read_text(encoding="utf-8"))
    entries = [e for day in log.values() for e in day]
    assert any("[es_review] opengl_engine" in e["query"] for e in entries)
    print("  es_review isolation OK")


def test_adversarial_interview_with_es() -> None:
    fake = FakeBackend()
    eng = ConsultationEngine()
    eng._backend = fake

    # (1) 出題: ES 駆動 + 敵対的ペルソナ。gap は隔離される
    eng.consult("開始", mode="interview_sim")
    sys1, user1 = fake.calls[0]
    assert "リアルタイムグラフィックスエンジニア" in sys1
    assert "攻撃" in sys1
    assert "可視化エンジン" in user1 and "圧迫質問" in user1
    _assert_no_gap_leak(sys1, user1)

    # (2) 議論継続: ES + トランスクリプトのみ。gap 漏洩なし
    eng.consult("バッチング導入は計測データに基づく自分の判断です",
                mode="interview_sim")
    sys2, user2 = fake.calls[1]
    assert "バッチング導入は計測データに基づく自分の判断です" in user2
    assert "可視化エンジン" in user2  # ES は文脈として与える
    _assert_no_gap_leak(sys2, user2)

    # (3) 講評: ES + 会話録 + gap_insights をすべて統合
    eng.consult("講評", mode="interview_sim")
    _, user3 = fake.calls[2]
    assert "可視化エンジン" in user3, "講評に ES が未統合"
    assert "躰道部" in user3, "講評に gap_insights が未統合"
    assert "既読スルー" in user3, "講評に Delta-LINE 対人ギャップが未統合 (DL2)"
    assert "認知リソース" in user3, "講評に Target Echo oracle_payload が未統合 (E0 ガード)"
    assert "防御" in user3 and "改善アクション" in user3
    assert eng._interview_state is None

    log = json.loads(AI_CONSULTATIONS_JSON.read_text(encoding="utf-8"))
    entries = [e for day in log.values() for e in day]
    assert any("[interview_sim] ES面接: opengl_engine" in e["query"]
               for e in entries)
    print("  adversarial interview (ES-driven) OK")


# ============================================================ Target Echo E4: oracle 実配線
def test_oracle_payload_isolated_to_review_phase() -> None:
    """E0 の隔離ガード (GAP_LEAK_MARKERS) は _write_phase3_assets() のフィクスチャ
    経由で検証済みだが、これは実配線 (_oracle_section()) を経由していない。
    ここでは実在の oracle_payload (gate_passed=True・具体的な介入ラベル付き) を
    deep_profile.json に書き、_oracle_section() の実配線そのものが (1) 議論
    フェーズへ絶対に漏れず、(2) 講評フェーズにのみ現れることを検証する。"""
    _write_phase3_assets()
    profile = json.loads(DEEP_PROFILE.read_text(encoding="utf-8"))
    profile["oracle_payload"] = {
        "schema": "oracle_payload.v1",
        "generated": "2026-07-07",
        "scope": {"kind": "global", "alias": None},
        "sufficiency": {"days_observed": 400, "coverage": 0.8, "dead_lanes": [],
                        "twin_bss": 0.12, "n_lapse_test": 15, "gate_passed": True},
        "state": {"r_now": 0.32, "r_trend_7d": -0.05, "oii_ema": None, "oii_streak_days": 0},
        "couplings": [{"src": "cal_private_hours", "dst": "productivity_idx",
                      "lag_days": 1, "rho": 0.44, "n_eff": 210, "null_q99": 0.3,
                      "sig": True}],
        "forecast": {"horizon_days": 14, "r_q10": [0.2], "r_q50": [0.3], "r_q90": [0.4],
                    "p_lapse": [0.1], "critical_days": ["2026-07-10"]},
        "findings": [{"rule_id": "R-GATE-01", "severity": 0.6,
                     "metrics": {"r_now": 0.32, "theta_r": 0.4}}],
        "interventions": [{"bank_id": "iv-003", "trigger_rule": "R-GATE-01",
                          "target_lane": 18, "params": {}}],
    }
    DEEP_PROFILE.write_text(json.dumps(profile, ensure_ascii=False), encoding="utf-8")

    fake = FakeBackend()
    eng = ConsultationEngine()
    eng._backend = fake

    # (1) 議論フェーズ: 介入ラベル・bank_id・R 値のいずれも漏れてはならない
    eng.consult("開始", mode="interview_sim")
    sys1, user1 = fake.calls[0]
    eng.consult("市場を法人と個人にMECEに分割して推定します", mode="interview_sim")
    sys2, user2 = fake.calls[1]
    for blob in (sys1, user1, sys2, user2):
        assert "意思決定モラトリアム" not in blob, "介入ラベルが議論フェーズへ漏洩 (I-22 違反)"
        assert "iv-003" not in blob, "bank_id が議論フェーズへ漏洩 (I-22 違反)"
        assert "0.32" not in blob, "R 実測値が議論フェーズへ漏洩 (I-22 違反)"

    # (2) 講評フェーズ: 実配線 (_oracle_section -> render_oracle_consult) の
    # 出力が実際に統合されていることを確認 (プレースホルダ文言ではない)
    eng.consult("講評", mode="interview_sim")
    _, user3 = fake.calls[2]
    assert "意思決定モラトリアム" in user3, "講評に oracle の介入が未統合 (E4 実配線)"
    assert "R-GATE-01" in user3, "講評に rule_id が未統合"
    print("  oracle payload isolated to review phase (E4 real wiring) OK")


def test_gd_sim_chaos() -> None:
    fake = FakeBackend()
    eng = ConsultationEngine()
    eng._backend = fake

    # (1) 開始: 3ペルソナ (クラッシャー/フリーライダー/クラウザー) の多重人格指示
    eng.consult("開始", mode="gd_sim")
    sys1, user1 = fake.calls[0]
    assert sys1 == GD_SYSTEM_PROMPT
    assert "クラッシャー" in sys1 and "フリーライダー" in sys1 and "クラウザー" in sys1
    assert "[学生A]" in sys1
    assert "リアルタイムグラフィックスエンジニア" in user1  # ES ドメインに追従
    _assert_no_gap_leak(sys1, user1)

    # (2) 議論継続: gap 漏洩なし
    eng.consult("学生Bさんはどう思いますか？と発言を振ります", mode="gd_sim")
    sys2, user2 = fake.calls[1]
    assert "学生Bさんはどう思いますか" in user2
    _assert_no_gap_leak(sys2, user2)

    # (3) 講評: gap_insights + 摩擦 (Friction) 回避構造への接続
    eng.consult("講評", mode="gd_sim")
    _, user3 = fake.calls[2]
    assert "躰道部" in user3, "GD 講評に gap_insights が未統合"
    assert "既読スルー" in user3, "GD 講評に Delta-LINE 対人ギャップが未統合 (DL2)"
    assert "認知リソース" in user3, "GD 講評に Target Echo oracle_payload が未統合 (E0 ガード)"
    assert "摩擦" in user3 and "Friction" in user3
    assert "フリーライダー" in user3 and "クラッシャー" in user3
    assert eng._gd_state is None

    log = json.loads(AI_CONSULTATIONS_JSON.read_text(encoding="utf-8"))
    entries = [e for day in log.values() for e in day]
    assert any("[gd_sim]" in e["query"] for e in entries)
    print("  chaos GD sim OK")


# ============================================================ Target Delta D3: Puppeteer
def test_puppeteer_injects_whitelisted_text_only() -> None:
    """Puppeteer (黒幕) が議論フェーズへ注入するのは QUESTION_BANK の実在
    テキストのみであり、Bounty の theme (生テキスト) は絶対に漏れない
    ことを検証する (不変条件 I-11 / I-14 のガード)。

    このテストは Puppeteer 本体の配線より前に書く (SPEC §5.4 の
    「ガードが先、機能が後」規律)。"""
    from core import line_telemetry, question_bank

    line_telemetry.BOUNTY_PATH.unlink(missing_ok=True)   # 他テストの残骸を排除
    line_telemetry.register_bounties([
        {"theme": "課外活動・組織運営", "type": "task_avoidance", "gap": 0.85,
         "insight": "躰道部の新歓・合宿マネジメントの対人調整から逃避している"},
    ])
    _write_phase3_assets()

    fake = FakeBackend()
    eng = ConsultationEngine()
    eng._backend = fake
    eng.consult("開始", mode="interview_sim")
    eng.consult("バッチング導入は計測データに基づく自分の判断です",
               mode="interview_sim")
    sys2, user2 = fake.calls[1]

    bank_texts = [q["text"] for q in question_bank.QUESTION_BANK.values()
                 if q["type"] == "task_avoidance"]
    assert any(t in user2 for t in bank_texts), \
        "Puppeteer が議論ターンへ質問を注入していない"
    # I-14: Bounty の中身 (theme) は絶対に面接官へ渡らない
    assert "課外活動・組織運営" not in user2
    _assert_no_gap_leak(sys2, user2)

    eng.consult("講評", mode="interview_sim")
    line_telemetry.BOUNTY_PATH.unlink(missing_ok=True)   # 後続テストを汚さない
    print("  puppeteer injects whitelisted bank text only OK")


# ============================================================ Target Alpha: KV プレフィックス・ピニング
def test_kv_prefix_cache() -> None:
    from core import kv_cache
    from core.consultation_engine import SYSTEM_PROMPT
    from core.paths import KV_SLOTS_DIR

    # ---- (1) プロンプト分割の不変条件 ----
    eng = ConsultationEngine()
    static = eng.build_static_prefix()
    hits = [{"title": "2026-06-01", "score": 0.9, "text": "検索ヒット本文",
             "has_diary": True}]
    dynamic = eng.build_dynamic_suffix("これはテスト相談です", hits, [])
    full = eng.build_prompt("これはテスト相談です", hits, [])
    assert full == static + dynamic, "分割の連結が build_prompt と不一致"
    assert full.startswith(static), "静的プレフィックスが先頭にない"
    assert "躰道部" in static, "プロファイル (gap) が静的側にない"
    assert "これはテスト相談です" in dynamic and "検索ヒット本文" in dynamic
    assert "これはテスト相談です" not in static and "検索ヒット本文" not in static
    assert "Future Context" in dynamic and "Future Context" not in static

    # ---- (2) 決定論的ハッシュとプロファイル更新による失効 ----
    h1 = kv_cache.prefix_hash(SYSTEM_PROMPT, static)
    assert h1 == kv_cache.prefix_hash(SYSTEM_PROMPT, static), "ハッシュが非決定的"
    profile = json.loads(DEEP_PROFILE.read_text(encoding="utf-8"))
    profile["gap_analysis"]["gaps"][0]["insight"] += " (プロファイル更新)"
    DEEP_PROFILE.write_text(json.dumps(profile, ensure_ascii=False),
                            encoding="utf-8")
    static2 = eng.build_static_prefix()
    h2 = kv_cache.prefix_hash(SYSTEM_PROMPT, static2)
    assert h2 != h1, "プロファイル更新でハッシュが変わらない (パージ不能)"

    # ---- (3) スロット状態機械 (fake HTTP transport / 通信なし) ----
    calls: list[tuple[str, dict]] = []

    def ok_post(url: str, payload: dict):
        calls.append((url, payload))
        return 200, {"success": True}

    c1 = kv_cache.SlotCacheClient(port=19999, http_post=ok_post)
    assert c1.ensure_prefix(h1) == "cold" and not calls, "cold で通信が発生"
    assert c1.commit_prefix(h1) is True
    assert "action=save" in calls[-1][0]
    assert calls[-1][1]["filename"] == kv_cache.slot_filename(h1)
    assert kv_cache.load_state()["hash"] == h1
    # サーバーが書くはずのスロットファイルを擬似作成
    (KV_SLOTS_DIR / kv_cache.slot_filename(h1)).write_bytes(b"kv")

    assert c1.ensure_prefix(h1) == "hit", "同一プロセス内の再利用が hit にならない"
    n_calls = len(calls)
    assert c1.commit_prefix(h1) is True and len(calls) == n_calls, \
        "永続化済みハッシュを再保存した (ファイルチャーン)"

    c2 = kv_cache.SlotCacheClient(port=19999, http_post=ok_post)
    assert c2.ensure_prefix(h1) == "restored", "サーバー再起動相当で復元されない"
    assert "action=restore" in calls[-1][0]

    # ハッシュ変化 → 新規保存 + 旧ファイルの決定論的パージ
    assert c2.ensure_prefix(h2) == "cold"
    assert c2.commit_prefix(h2) is True
    assert kv_cache.load_state()["hash"] == h2
    assert not (KV_SLOTS_DIR / kv_cache.slot_filename(h1)).exists(), \
        "旧ハッシュのスロットファイルが残留"

    # 失敗時の優雅な劣化: 500 は無効化しない / 404 は以後無効化
    def err500(url, payload):
        return 500, {}

    (KV_SLOTS_DIR / kv_cache.slot_filename(h2)).write_bytes(b"kv")
    c3 = kv_cache.SlotCacheClient(port=19999, http_post=err500)
    assert c3.ensure_prefix(h2) == "cold" and not c3._disabled

    def err404(url, payload):
        return 404, {}

    c4 = kv_cache.SlotCacheClient(port=19999, http_post=err404)
    assert c4.ensure_prefix(h2) == "cold" and c4._disabled
    assert c4.ensure_prefix(h2) == "disabled"
    assert c4.commit_prefix(h2) is False
    print("  KV prefix cache OK")


# ============================================================ フェーズ4: レイテンシ / 動的ペルソナ / 建前隔離
def test_latency_evaluation() -> None:
    """UI 計測の response_time_sec がターンと講評の両方に反映される。"""
    fake = FakeBackend()
    eng = ConsultationEngine()
    eng._backend = fake

    eng.consult("開始", mode="interview_sim")
    eng.consult("推定の前提は3つあります", mode="interview_sim",
                response_time_sec=42.0)
    _, user2 = fake.calls[1]
    assert "42.0 秒" in user2, "ターンプロンプトに応答時間が未注入"

    eng.consult("講評", mode="interview_sim")
    _, user3 = fake.calls[2]
    assert "応答時間 (Response Latency)" in user3
    assert "42.0 秒" in user3
    assert "思考速度" in user3, "講評指示に思考速度の評価観点がない"
    print("  latency evaluation OK")


def test_dynamic_gd_personas() -> None:
    """フロントエンドから渡した N 人のペルソナ配列が動的展開される。"""
    personas = [
        {"name": "田中", "trait": "クラッシャー"},
        {"name": "鈴木", "trait": "協調型"},
        {"name": "佐々木", "trait": "フリーライダー"},
        {"name": "李", "trait": "常に費用対効果の数字だけで反論する京都弁の投資家"},
    ]
    fake = FakeBackend()
    eng = ConsultationEngine()
    eng._backend = fake

    eng.consult("開始", mode="gd_sim", personas=personas)
    sys1, user1 = fake.calls[0]
    assert "4 人" in sys1
    assert "[田中] (クラッシャー)" in sys1
    assert "橋渡し" in sys1  # 協調型プリセットの挙動指示が展開されている
    assert "京都弁の投資家" in sys1  # 自由記述 trait はそのまま挙動指示になる
    assert "[田中] の最初の発言" in user1
    _assert_no_gap_leak(sys1, user1)

    eng.consult("鈴木さん、論点を整理してもらえますか", mode="gd_sim",
                response_time_sec=7.5)
    sys2, user2 = fake.calls[1]
    assert sys2 == sys1, "セッション中にペルソナ system が変わった"
    assert "7.5 秒" in user2

    eng.consult("講評", mode="gd_sim")
    _, user3 = fake.calls[2]
    assert "応答時間 (Response Latency)" in user3
    assert "躰道部" in user3  # 講評では gap 統合
    assert eng._gd_state is None

    # 9 人上限 (10 人渡しても 9 人に切り詰め)
    many = [{"name": f"P{i}", "trait": "協調型"} for i in range(10)]
    sys_many = build_gd_system_prompt(many)
    assert "9 人" in sys_many and "[P8]" in sys_many and "[P9]" not in sys_many
    # 未指定は既定3人構成 (後方互換)
    assert build_gd_system_prompt(None) == GD_SYSTEM_PROMPT
    print("  dynamic GD personas OK")


# ============================================================ F4a/F4b: コンフィギュレータ・成績表
def test_interview_configurator_no_es() -> None:
    """F4a: ES 不在時、config (industry/genre/difficulty) がケースバンクの
    巡回に代わって出題を決める。未知フィールドは無視する境界防衛も検証する。"""
    fake = FakeBackend()
    eng = ConsultationEngine()
    eng._backend = fake

    config = {
        "industry": "foreign_finance", "genre": "algorithm", "difficulty": "extreme",
        "unknown_field": "無視されるべき",
    }
    eng.consult("開始", mode="interview_sim", config=config)
    sys1, user1 = fake.calls[0]
    assert sys1 == INTERVIEWER_SYSTEM_PROMPT
    assert "外資系金融 (HFT/クオンツ)" in user1
    assert "アルゴリズム・データ構造の技術面接" in user1
    assert "最難関" in user1
    assert eng._interview_state["config"] == config, "config が state に保持されていない"

    # プリセット外の自由記述 industry/genre はそのまま表示ラベルとして使われる
    eng2 = ConsultationEngine()
    fake2 = FakeBackend()
    eng2._backend = fake2
    eng2.consult("開始", mode="interview_sim",
                config={"industry": "国内メガベンチャー", "genre": "", "difficulty": "hard"})
    _, user2 = fake2.calls[0]
    assert "国内メガベンチャー" in user2
    print("  F4a configurator (no ES, whitelist bypass) OK")


def test_interview_report_schema_and_latency() -> None:
    """F4b: interview_report.v1 のスキーマ・軸ホワイトリスト・evidence必須・
    latency のコード合成 (LLMには一切書かせない) を検証する。"""
    valid_json = json.dumps({"metrics": [
        {"axis": "論理性", "score": 82.6, "evidence": "MECEな分解を提示した"},
        {"axis": "技術力", "score": 55, "evidence": "実装の詳細に踏み込めなかった"},
        {"axis": "構成力", "score": 90, "evidence": "結論から述べる構成だった"},
        {"axis": "具体性", "score": 40, "evidence": "定量的根拠が薄かった"},
    ]}, ensure_ascii=False)
    backend = ScriptedBackend(["出題", "面接官応答1", "面接官応答2", "講評本文です", valid_json])
    eng = ConsultationEngine()
    eng._backend = backend

    eng.consult("開始", mode="interview_sim",
                config={"industry": "foreign_it", "genre": "fermi", "difficulty": "hard"})
    eng.consult("推定の前提は3つあります", mode="interview_sim", response_time_sec=12.0)
    eng.consult("もう少し粘って考えます", mode="interview_sim", response_time_sec=30.0)
    eng.consult("講評", mode="interview_sim")

    report = eng._last_interview_report
    assert report is not None, "interview_report.v1 が生成されていない"
    assert report["schema"] == "interview_report.v1"
    assert report["simulated"] is True
    assert report["config"]["genre"] == "fermi"
    axes = {m["axis"] for m in report["metrics"]}
    assert axes == {"論理性", "技術力", "構成力", "具体性"}, axes
    for m in report["metrics"]:
        assert 0 <= m["score"] <= 100
        assert m["evidence"]
    scores = {m["axis"]: m["score"] for m in report["metrics"]}
    assert scores["論理性"] == 83, "82.6 が四捨五入で int へ clamp されていない"
    # latency はコードが state["latencies"] から合成する (LLM の JSON には無い)
    assert report["latency"] == {"median_sec": 21.0, "max_sec": 30.0, "n": 2}
    assert report["summary"] == "講評本文です"

    # 永続化: data/records/interviews/ に書かれ、genre がファイル名に反映される
    files = list(INTERVIEW_RECORDS_DIR.glob("interview_*_fermi.json"))
    assert files, "成績表が data/records/interviews/ へ永続化されていない"
    on_disk = json.loads(files[0].read_text(encoding="utf-8"))
    assert on_disk["schema"] == "interview_report.v1"
    print("  F4b interview report schema + latency synthesis OK")


def test_interview_report_axis_rejection() -> None:
    """F4b: ホワイトリスト外の軸・evidence欠如の採点はパース時に削られ、
    スコアは0-100へclampされる。軸を1つも復元できなくても summary/latency
    は必ず返る (退化フォールバック)。"""
    invalid_json = json.dumps({"metrics": [
        {"axis": "コミュニケーション力", "score": 90, "evidence": "笑顔が良かった"},
        {"axis": "論理性", "score": 200, "evidence": ""},
        {"axis": "技術力", "score": -10, "evidence": "妥当な深掘りができた"},
    ]}, ensure_ascii=False)
    backend = ScriptedBackend(["出題", "講評本文", invalid_json, invalid_json, invalid_json])
    eng = ConsultationEngine()
    eng._backend = backend

    eng.consult("開始", mode="interview_sim")
    eng.consult("講評", mode="interview_sim")

    report = eng._last_interview_report
    assert report is not None
    axes = [m["axis"] for m in report["metrics"]]
    assert "コミュニケーション力" not in axes, "ホワイトリスト外の軸が混入した"
    assert "論理性" not in axes, "evidence欠如の採点が混入した"
    assert axes == ["技術力"], axes
    assert report["metrics"][0]["score"] == 0, "負のスコアが0へclampされていない"
    assert report["summary"] == "講評本文"
    assert report["latency"] == {"median_sec": 0.0, "max_sec": 0.0, "n": 0}
    print("  F4b interview report axis whitelist rejection OK")


def test_interview_records_isolated_from_profiler() -> None:
    """W-38 (壁A — `_assert_no_gap_leak` の鏡像): 面接成績表ディレクトリ
    (data/records/interviews/) を読み書きするコードは core/interview_report.py
    のみであることを静的に保証する。profiler/gap_analysis/tensor_store/oracle
    がこのパスへ結合したら壁A違反として検出する。"""
    import core.digital_twin as digital_twin
    import core.gap_analysis as gap_analysis
    import core.oracle as oracle
    import core.profiler as profiler
    import core.tensor_store as tensor_store

    guarded = (gap_analysis, profiler, tensor_store, oracle, digital_twin)
    for mod in guarded:
        src = Path(mod.__file__).read_text(encoding="utf-8")
        assert "INTERVIEW_RECORDS_DIR" not in src, \
            f"{mod.__name__} が面接成績表ディレクトリを参照 (壁A違反)"
        assert "records/interviews" not in src.replace("\\", "/"), \
            f"{mod.__name__} が面接成績表パスを直接参照 (壁A違反)"
    print("  interview report sanctuary isolation (W-38) OK")


def test_simulated_persona_isolation() -> None:
    """建前人格フラグの永続化と、Gap 主観スコアの 0.1 ウェイト化。"""
    from core.consultation_log import (
        append_consultation, extract_user_queries, load_consultations,
    )
    from core.gap_analysis import (
        SIMULATED_PERSONA_WEIGHT, build_subjective_corpus,
    )

    # (1) シミュレーター由来ログには is_simulated_persona=True が付与済み
    log = load_consultations()
    entries = [e for day in log.values() for e in day]
    sim_entries = [e for e in entries if e["query"].startswith(
        ("[interview_sim]", "[gd_sim]", "[es_review]"))]
    assert sim_entries, "シミュレーターのログが見つからない"
    assert all(e["is_simulated_persona"] for e in sim_entries), \
        "シミュレーターログに建前フラグが未付与"

    # (2) 通常相談は False で保存され、ローダーがフラグを保持する
    append_consultation("普通の相談です", "回答")
    reloaded = [e for day in load_consultations().values() for e in day]
    normal = [e for e in reloaded if e["query"] == "普通の相談です"]
    assert normal and normal[0]["is_simulated_persona"] is False

    # (3) profiler チャネル (self_text) から建前は除外される
    mixed = [
        {"query": "本音の相談", "is_simulated_persona": False},
        {"query": "面接用の建前発言", "is_simulated_persona": True},
    ]
    genuine_text = extract_user_queries(mixed)
    assert "本音の相談" in genuine_text and "建前発言" not in genuine_text

    # (4) gap_analysis: 同一テキストでも建前は主観スコア寄与が 0.1 倍
    career_text = "キャリアの軸は明確です。就活の選考も面接も順調に対策しています。"
    day_genuine = [_day("2026-07-01")]
    day_genuine[0]["consultations"] = [
        {"query": career_text, "is_simulated_persona": False}]
    day_simulated = [_day("2026-07-01")]
    day_simulated[0]["consultations"] = [
        {"query": career_text, "is_simulated_persona": True}]

    docs_g = build_subjective_corpus(day_genuine)
    docs_s = build_subjective_corpus(day_simulated)
    assert docs_g[0]["weight"] == 1.0
    assert docs_s[0]["weight"] == SIMULATED_PERSONA_WEIGHT

    hits_g = analyze_gaps(day_genuine)["subjective_scores"][
        "キャリア・仕事の将来"]["weighted_hits"]
    hits_s = analyze_gaps(day_simulated)["subjective_scores"][
        "キャリア・仕事の将来"]["weighted_hits"]
    assert hits_g > 0
    assert abs(hits_s - hits_g * SIMULATED_PERSONA_WEIGHT) < 0.11, \
        f"建前の重みが 0.1 になっていない: genuine={hits_g} simulated={hits_s}"

    # (5) 建前 doc からは先延ばしの「宣言」も採らない
    day_decl = [_day("2026-07-02")]
    day_decl[0]["consultations"] = [
        {"query": "明日こそ面接対策をやると面接で宣言した",
         "is_simulated_persona": True}]
    result = analyze_gaps(day_decl)
    assert result["procrastination"]["declarations"] == 0, \
        "建前人格の発言から宣言を誤検出"
    print("  simulated-persona isolation OK")


if __name__ == "__main__":
    try:
        test_life_balance_stabilizer()
        test_interview_sim_flow()  # ES 不在時のケースバンク・フォールバック
        # ---- F4a/F4b (ES 不在前提。ES駆動時の優先順位は既存テストで別途保証) ----
        test_interview_configurator_no_es()
        test_interview_report_schema_and_latency()
        test_interview_report_axis_rejection()
        test_interview_records_isolated_from_profiler()
        test_fetch_tag_hook_and_queue()
        test_offline_default_never_fetches()
        test_mock_fetch_ingestion_pipeline()
        # ---- フェーズ3 (ES 駆動) — 以降は data/es/ が存在する状態 ----
        test_es_manager_dynamic_domain()
        test_es_review_isolation()
        test_adversarial_interview_with_es()
        test_oracle_payload_isolated_to_review_phase()
        test_gd_sim_chaos()
        # ---- Target Delta D3 (Puppeteer) ----
        test_puppeteer_injects_whitelisted_text_only()
        # ---- フェーズ4 (レイテンシ / 動的ペルソナ / 建前隔離) ----
        test_latency_evaluation()
        test_dynamic_gd_personas()
        test_simulated_persona_isolation()
        # ---- Target Alpha (KV プレフィックス・ピニング) ----
        test_kv_prefix_cache()
        print("test_integration: ALL PASS")
    finally:
        shutil.rmtree(_TMP, ignore_errors=True)
