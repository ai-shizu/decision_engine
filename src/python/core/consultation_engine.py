#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
相談ロジック最適化エンジン
============================
チャットで相談が送信された時 *のみ* 以下のパイプラインを起動する:

  a. クエリをベクトル化し、C++検索エンジン (NEON/OpenMP) で
     [過去日記インデックス] と [外部知識インデックス] の双方から Top-K 抽出
  b. [ユーザー属性 (user_profile.json)] + [深層プロファイル] +
     [抽出コンテキスト] + [相談内容] を結合してプロンプトを構築
  c. ローカル llama.cpp で「現状分析・価値観整合性・スキルギャップ・次の一手」を生成

日記保存は sync_diary_index() のみを裏で行い、分析(profiler)は走らせない。
インデックスはソースファイルの mtime と比較して古い時だけ再構築する。
全処理は完全オフライン。
"""

from __future__ import annotations

import json
import os
import re
import struct
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path

os.environ.setdefault("HF_HUB_OFFLINE", "1")
os.environ.setdefault("TRANSFORMERS_OFFLINE", "1")

from .paths import (
    AI_CONSULTATIONS_JSON,
    CALENDAR_JSON,
    DEEP_PROFILE,
    DIARY_BIN,
    DIARY_MD,
    DIARY_META,
    FINANCE_JSON,
    KNOWLEDGE_BIN,
    KNOWLEDGE_DIR,
    KNOWLEDGE_META,
    LINE_HISTORY,
    LLAMA_DIR,
    MODELS_DIR,
    PROCESSED,
    PROJECT_ROOT as ROOT,
    SEARCH_EXE,
    USER_PROFILE,
)
from .profile_store import (
    FIXED_ATTRIBUTE_FIELDS,
    FIXED_ATTRIBUTES,
    FIXED_ATTRIBUTE_LABELS,
    format_user_profile_summary,
    load_user_profile,
    save_fixed_attributes,
)
from . import pipeline  # noqa: E402
from .llm_config import find_gguf, llama_server_cmd, SERVER_PORT  # noqa: E402

SYSTEM_PROMPT = (
    "あなたはユーザーの思考・価値観を完全に理解する分身AIである。"
    "ユーザーの意思決定を支援せよ。"
    "与えられたユーザー属性・深層プロファイル・過去の日記・外部知識のみを根拠として、"
    "本人に寄り添いながら誠実かつ論理的に助言すること。"
)

OUTPUT_FRAMEWORK = """回答は必ず以下の4セクション構成のMarkdownで出力すること:

## 1. 現状分析
(日記コンテキストと属性から、いま何が起きているかを客観的に記述)

## 2. 価値観との整合性
(価値観の階層構造と照らし、選択肢が根源的欲求と整合するか。認知バイアスへの警告も含める)

## 3. 必要なスキルギャップ
(外部知識を根拠に、目標達成のため埋めるべき具体的スキル・経験を列挙)

## 4. 次の一手
(意思決定ルールに従い、今週から実行可能な具体的アクションを2〜3個。可逆な小さい一歩を優先)

【Future Context 評価ルール】
向こう1ヶ月間のスケジュール (Future Context) が提供されている場合、
過密さ・重要イベント・移動/準備時間を考慮し、提案する次の一手が
物理的に実行可能か、キャパシティオーバーにならないかを厳密に評価せよ。
実行困難な提案は縮小・延期・代替案を提示し、セクション4の重み付けを
Future Context の制約に合わせて調整すること。"""

DEFAULT_ATTRIBUTES = {
    "age": "",
    "gender": "",
    "location": "",
    "family": "",
    "occupation": "",
    "education": "",
    "skills": "",
    "hobbies": "",
    "health": "",
    "work_style": "",
    "short_term_goal": "",
    "career_goal": "",
    "long_term_vision": "",
    "constraints": "",
    "values": "",
    "free_notes": "",
}

ATTRIBUTE_LABELS = {
    "age": "年齢",
    "gender": "性別",
    "location": "居住地",
    "family": "家族構成",
    "occupation": "職業",
    "education": "学歴・専門分野",
    "skills": "スキル・強み",
    "hobbies": "趣味・関心",
    "health": "健康・体調",
    "work_style": "働き方の理想",
    "short_term_goal": "短期目標 (3ヶ月)",
    "career_goal": "キャリア目標",
    "long_term_vision": "長期ビジョン",
    "constraints": "制約・事情",
    "values": "大切にしていること",
    "free_notes": "自由記述",
}

# 旧UI互換 (編集不可 — profiler が自動生成)
ATTRIBUTE_FIELDS = list(ATTRIBUTE_LABELS.items())


# ============================================================ 知識チャンク分割
def load_knowledge_chunks() -> list[dict]:
    chunks: list[dict] = []
    KNOWLEDGE_DIR.mkdir(parents=True, exist_ok=True)
    for f in sorted(KNOWLEDGE_DIR.iterdir()):
        if f.suffix.lower() not in (".md", ".txt"):
            continue
        text = f.read_text(encoding="utf-8")
        title, body = f.stem, []
        for line in text.splitlines():
            m = re.match(r"^##\s+(.*)", line)
            if m:
                if body and "".join(body).strip():
                    chunks.append({"title": title, "text": "\n".join(body).strip(),
                                   "file": f.name})
                title, body = f"{f.stem} § {m.group(1).strip()}", []
            else:
                body.append(line)
        if body and "".join(body).strip():
            chunks.append({"title": title, "text": "\n".join(body).strip(),
                           "file": f.name})
    return chunks


# ============================================================ LLMバックエンド
class LlamaServerBackend:
    """llama-server (127.0.0.1) 経由の推論。プロセス・通信ともに完全ローカル。

    ライフサイクル管理:
      - 自分が spawn したサーバープロセスのみを終了対象とする
        (既存サーバーを再利用した場合は他所有プロセスを殺さない)
      - stop() は Terminate → 5秒待機 → Kill の段階的終了
      - インスタンス生成時に atexit へ登録し、TUI/CLI がどのような経路で
        終了してもゾンビプロセスを残さない
    """

    name = "llama-server (127.0.0.1, ARM64 native)"

    def __init__(self, exe: Path, model: Path, port: int):
        self.exe, self.model, self.port = exe, model, port
        self.proc: subprocess.Popen | None = None
        import atexit
        atexit.register(self.stop)

    def _port_open(self) -> bool:
        import socket
        with socket.socket() as s:
            s.settimeout(0.3)
            return s.connect_ex(("127.0.0.1", self.port)) == 0

    def start(self, timeout_s: int | None = None) -> None:
        import urllib.request
        from llm_config import model_startup_timeout
        if timeout_s is None:
            timeout_s = model_startup_timeout(self.model)
        if self.proc is not None and self.proc.poll() is None:
            return  # 自前サーバーが稼働中
        if self._port_open():
            return  # 既存サーバーを再利用 (所有権なし → stop対象外)
        self.proc = subprocess.Popen(
            llama_server_cmd(self.exe, self.model, self.port),
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        deadline = time.time() + timeout_s
        while time.time() < deadline:
            try:
                with urllib.request.urlopen(
                        f"http://127.0.0.1:{self.port}/health", timeout=2) as r:
                    if json.load(r).get("status") == "ok":
                        return
            except OSError:
                time.sleep(1.0)
        self.stop()
        raise RuntimeError("llama-server の起動がタイムアウトしました")

    def generate(self, system: str, user: str, max_tokens: int = 900) -> str:
        import urllib.request
        self.start()
        payload = json.dumps({
            "messages": [{"role": "system", "content": system},
                         {"role": "user", "content": user}],
            "max_tokens": max_tokens, "temperature": 0.6,
        }).encode("utf-8")
        req = urllib.request.Request(
            f"http://127.0.0.1:{self.port}/v1/chat/completions",
            data=payload, headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=600) as r:
            return json.load(r)["choices"][0]["message"]["content"].strip()

    def stop(self) -> None:
        """自分が起動したサーバーを確実に終了させる (Terminate → Kill)。"""
        proc, self.proc = self.proc, None
        if proc is None or proc.poll() is not None:
            return
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                pass


class RuleBasedBackend:
    """LLM環境が無い場合の決定論的フォールバック。"""

    name = "rule-based reasoner (LLMなしフォールバック)"

    def generate(self, system: str, user: str, max_tokens: int = 0) -> str:
        return ("## 1. 現状分析\n(ローカルLLM未検出のため簡易応答)\n\n"
                "## 2. 価値観との整合性\ndeep_profile.json の value_hierarchy を参照。\n\n"
                "## 3. 必要なスキルギャップ\ndata/knowledge/ を参照。\n\n"
                "## 4. 次の一手\nmodels/ にGGUFを配置するとLLM推論が有効化される。")

    def stop(self) -> None:
        pass


# find_gguf は llm_config から import 済み


# ============================================================ エンジン本体
class ConsultationEngine:
    """遅延初期化: 埋め込みモデル・LLMサーバーは初回相談まで起動しない。"""

    def __init__(self):
        self._embedder = None
        self._backend = None

    # ---- 埋め込み (pipeline.py と同一空間) --------------------------------
    @property
    def embedder(self):
        if self._embedder is None:
            self._embedder = pipeline.build_embedder()
        return self._embedder

    def embed(self, text: str):
        import numpy as np
        return pipeline.l2_normalize(
            np.asarray(self.embedder.encode([text]), dtype=np.float32))[0]

    # ---- インデックス同期 (RECORDタブ保存時に裏で呼ぶ) ---------------------
    @staticmethod
    def _stale(bin_path: Path, *sources: Path) -> bool:
        if not bin_path.exists():
            return True
        t = bin_path.stat().st_mtime
        return any(s.exists() and s.stat().st_mtime > t for s in sources)

    def sync_diary_index(self, force: bool = False) -> bool:
        """diary.md / line_history.txt / calendar.json / ai_consultations.json /
        finance.json が更新されていれば DailyContext インデックスを再構築する。"""
        sources = [DIARY_MD, LINE_HISTORY]
        if CALENDAR_JSON.exists():
            sources.append(CALENDAR_JSON)
        if AI_CONSULTATIONS_JSON.exists():
            sources.append(AI_CONSULTATIONS_JSON)
        if FINANCE_JSON.exists():
            sources.append(FINANCE_JSON)
        if not force and not self._stale(DIARY_BIN, *sources):
            return False
        chunks = pipeline.load_chunks()  # 日付結合済み DailyContext
        pipeline.build_index(chunks, DIARY_BIN, DIARY_META,
                             embedder=self.embedder,
                             source=("diary.md + line_history.txt + calendar.json "
                                     "+ ai_consultations.json + finance.json (DailyContext)"))
        return True

    def sync_knowledge_index(self, force: bool = False) -> bool:
        sources = [f for f in KNOWLEDGE_DIR.glob("*") if f.is_file()]
        if not sources:
            return False
        if not force and not self._stale(KNOWLEDGE_BIN, *sources):
            return False
        chunks = load_knowledge_chunks()
        pipeline.build_index(chunks, KNOWLEDGE_BIN, KNOWLEDGE_META,
                             embedder=self.embedder, source="data/knowledge/")
        return True

    # ---- a. C++検索エンジン呼び出し ---------------------------------------
    def search_index(self, bin_path: Path, meta_path: Path, qvec,
                     top_k: int = 3) -> list[dict]:
        if not bin_path.exists():
            return []
        meta = json.loads(meta_path.read_text(encoding="utf-8"))
        chunks = {c["id"]: c for c in meta["chunks"]}

        hits: list[tuple[int, float]] = []
        if SEARCH_EXE.exists():
            tmp = PROCESSED / f"_ce_query_{os.getpid()}.bin"
            tmp.write_bytes(qvec.tobytes())
            try:
                out = subprocess.run(
                    [str(SEARCH_EXE), str(bin_path), str(tmp), str(top_k)],
                    capture_output=True, text=True, timeout=60,
                    encoding="utf-8", errors="replace")
                for m in re.finditer(r"chunk_id=(\d+)\s+score=([\d.\-]+)", out.stdout):
                    hits.append((int(m.group(1)), float(m.group(2))))
            finally:
                tmp.unlink(missing_ok=True)
        if not hits:  # exe不在時のNumPyフォールバック
            import numpy as np
            raw = bin_path.read_bytes()
            _, dim, lanes, nvec, nblk, blk_b, _ = struct.unpack("<8sIIIIII", raw[:32])
            blocks = np.frombuffer(raw[32:], dtype=np.uint8).reshape(nblk, blk_b)
            data = blocks[:, :dim * lanes * 4].copy().view(np.float32).reshape(nblk, dim, lanes)
            ids = blocks[:, dim * lanes * 4:].copy().view(np.int32).reshape(-1)
            vecs = data.transpose(0, 2, 1).reshape(-1, dim)
            scores = vecs @ qvec
            order = [i for i in np.argsort(-scores) if ids[i] >= 0][:top_k]
            hits = [(int(ids[i]), float(scores[i])) for i in order]
        return [{"score": sc, **chunks[cid]} for cid, sc in hits if cid in chunks]

    # 日記とLINEの両方が存在する日 = 「その日の行動ログ」として最も情報量が
    # 多いため、リランキングで優遇する
    FULL_DAY_LOG_BOOST = 1.15

    def search_daily(self, qvec, top_k: int = 3) -> list[dict]:
        """DailyContextインデックスを検索し、日記+LINEが揃った日を重み付けして
        リランキングする。候補は top_k の2倍取得してから絞り込む。"""
        hits = self.search_index(DIARY_BIN, DIARY_META, qvec, top_k * 2)
        for h in hits:
            full = h.get("has_diary") and h.get("has_line")
            h["is_full_day_log"] = bool(full)
            h["ranking_score"] = h["score"] * (self.FULL_DAY_LOG_BOOST if full else 1.0)
        hits.sort(key=lambda h: -h["ranking_score"])
        return hits[:top_k]

    # ---- b. プロンプト構築 -------------------------------------------------
    @staticmethod
    def _fixed_attributes_section() -> str:
        attrs = load_user_profile().get("fixed_attributes", {})
        lines = [
            f"- {FIXED_ATTRIBUTE_LABELS.get(k, k)}: {v}"
            for k, v in attrs.items() if str(v).strip()
        ]
        return "\n".join(lines) if lines else "(未入力 — SETTINGSで基本情報を保存)"

    @staticmethod
    def _inferred_profile_section() -> str:
        p = load_user_profile()
        inferred = p.get("inferred_profile", {})
        lines: list[str] = []
        if inferred.get("meta_narrative"):
            lines.append(f"自己モデル: {inferred['meta_narrative']}")
        factual = inferred.get("factual_signals", {})
        for cat, labels in factual.get("categories", {}).items():
            lines.append(f"{cat}: {', '.join(labels)}")
        for g in factual.get("stated_goals", [])[:2]:
            lines.append(f"言及目標: {g}")
        abstract = inferred.get("abstract_identity", {})
        for d in abstract.get("core_drives", [])[:3]:
            lines.append(f"コアドライブ: {d.get('drive')} — {d.get('meaning', '')}")
        style = abstract.get("cognitive_style", {})
        if style.get("label"):
            lines.append(f"認知スタイル: {style['label']}")
        for t in abstract.get("internal_tensions", [])[:2]:
            lines.append(f"内的葛藤: {t.get('tension', t)}")
        energy = abstract.get("energy_regulation", {})
        if energy.get("label"):
            lines.append(f"エネルギー管理: {energy['label']}")
        rel = abstract.get("relational_stance", {})
        if rel.get("stance") and rel["stance"] != "データ不足":
            lines.append(f"対人スタンス: {rel['stance']}")
        auto = p.get("auto_extracted", {})
        for h in auto.get("abstract_heuristics", [])[:3]:
            lines.append(f"ヒューリスティック: {h}")
        return "\n".join(f"- {l}" for l in lines) if lines else "(未生成: profiler.py を実行)"

    @staticmethod
    def _profile_section() -> str:
        if not DEEP_PROFILE.exists():
            return "(未生成: profiler.py 未実行)"
        p = json.loads(DEEP_PROFILE.read_text(encoding="utf-8"))
        lines = ["価値観(重み順): " + " > ".join(
            f"{v['value']}" for v in p["value_hierarchy"][:4])]
        biases = [b for b in p["cognitive_biases"] if b["hit_count"] > 0][:3]
        if biases:
            lines.append("注意すべきバイアス: " + ", ".join(b["bias"] for b in biases))
        seen = []
        for r in p["decision_rules"]:
            if r["recommended_action"] not in seen:
                seen.append(r["recommended_action"])
            if len(seen) >= 5:
                break
        lines += [f"自己ルール: {a}" for a in seen]
        for r in p.get("cross_source_patterns", {}).get("diary_to_line_rules", [])[:2]:
            lines.append(f"日跨ぎ傾向: {r['insight']} (確信度{r['confidence']})")
        for r in p.get("interaction_patterns", {}).get("stimulus_response_rules", []):
            if r["stimulus_type"] != "その他" and r["response_type"] != "その他":
                lines.append(f"対人反応傾向: {r['insight']} (確信度{r['confidence']})")
                if sum(1 for l in lines if l.startswith("対人反応傾向")) >= 2:
                    break
        for lp in p.get("interaction_patterns", {}).get("latency_patterns", [])[:2]:
            lines.append(f"レイテンシ傾向: {lp['insight']}")
        if p.get("meta_narrative"):
            lines.append(f"抽象自己記述: {p['meta_narrative']}")
        abstract = p.get("abstract_identity", {})
        for h in abstract.get("decision_heuristics", [])[:3]:
            lines.append(f"抽象ルール: {h.get('abstract_rule', '')}")
        for t in abstract.get("internal_tensions", [])[:2]:
            lines.append(f"内的葛藤: {t.get('tension', t)}")
        llm = p.get("llm_interaction_insights", {})
        if llm.get("analysis"):
            snippet = llm["analysis"].strip()
            if len(snippet) > 400:
                snippet = snippet[:400] + "…"
            lines.append(f"LLM深層分析: {snippet}")
        return "\n".join(f"- {l}" for l in lines)

    @staticmethod
    def _future_context_section(days_ahead: int = 30) -> str:
        from .calendar_manager import format_future_context, load_future_events
        events = load_future_events(days_ahead=days_ahead)
        return format_future_context(events)

    def build_prompt(self, query: str, diary_hits: list[dict],
                     knowledge_hits: list[dict]) -> str:
        def _tag(c: dict) -> str:
            return ("その日の行動ログ(日記+LINE)" if c.get("is_full_day_log")
                    else "日記" if c.get("has_diary") else "LINE")
        diary_ctx = "\n\n".join(
            f"[{_tag(c)} {c['title']} / 類似度{c['score']:.3f}]\n{c['text'].strip()[:600]}"
            for c in diary_hits) or "(該当なし)"
        knowledge_ctx = "\n\n".join(
            f"[知識 {c['title']} / 類似度{c['score']:.3f}]\n{c['text'].strip()[:500]}"
            for c in knowledge_hits) or "(該当なし)"
        future_ctx = self._future_context_section(days_ahead=30)
        return f"""# 基本情報 (本人入力・固定)
{self._fixed_attributes_section()}

# 推定プロフィール (日記・LINE・相談から自動抽出)
{self._inferred_profile_section()}

# 深層プロファイル (profiler.py 多層分析)
{self._profile_section()}

# コンテキスト1: 関連する過去の日記 (NEONベクトル検索)
{diary_ctx}

# コンテキスト2: 関連する外部知識 (NEONベクトル検索)
{knowledge_ctx}

# Future Context: 向こう1ヶ月の予定 (calendar.json から構造化抽出)
{future_ctx}

# ユーザーの相談
{query}

{OUTPUT_FRAMEWORK}"""

    # ---- c. 推論 -----------------------------------------------------------
    @property
    def backend(self):
        if self._backend is None:
            model = find_gguf()
            server = LLAMA_DIR / "llama-server.exe"
            if model and server.exists():
                self._backend = LlamaServerBackend(server, model, SERVER_PORT)
            else:
                self._backend = RuleBasedBackend()
        return self._backend

    def consult(self, query: str, top_k: int = 3, status=None) -> str:
        """相談1件を処理して4セクションMarkdownを返す。status は進捗コールバック。"""
        say = status or (lambda msg: None)

        say("クエリをベクトル化中…")
        qvec = self.embed(query)

        say("インデックス同期を確認中…")
        self.sync_diary_index()
        self.sync_knowledge_index()

        say("NEON検索エンジンで行動ログ・外部知識を検索中…")
        diary_hits = self.search_daily(qvec, top_k)
        knowledge_hits = self.search_index(KNOWLEDGE_BIN, KNOWLEDGE_META, qvec, top_k)

        prompt = self.build_prompt(query, diary_hits, knowledge_hits)

        say(f"ローカルLLMで推論中… ({self.backend.name})")
        t0 = time.perf_counter()
        answer = self.backend.generate(SYSTEM_PROMPT, prompt)
        say(f"生成完了 ({time.perf_counter() - t0:.1f}s)")

        log = PROCESSED / "last_consultation.md"
        log.write_text(f"# 相談 ({datetime.now().isoformat(timespec='seconds')})\n"
                       f"{query}\n\n# 回答 ({self.backend.name})\n\n{answer}\n",
                       encoding="utf-8")

        from .consultation_log import append_consultation
        append_consultation(query, answer)
        say("相談履歴を保存し DailyContext を再結晶化中…")
        self.sync_diary_index(force=True)

        return answer

    def shutdown(self) -> None:
        if self._backend:
            self._backend.stop()


# ============================================================ CLI (検証用)
if __name__ == "__main__":
    q = sys.argv[1] if len(sys.argv) > 1 else "今週の優先事項をどう決めるべき?"
    eng = ConsultationEngine()
    try:
        print(eng.consult(q, status=lambda m: print(f"[engine] {m}")))
    finally:
        eng.shutdown()  # CLI終了時にサーバーを残さない (atexitは保険)
