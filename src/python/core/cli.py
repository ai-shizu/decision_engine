#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
ハイブリッド意思決定支援推論 (自己エミュレーション・システム)
==============================================================
ユーザーの相談クエリに対し、

  a. C++検索エンジン (build/search_engine.exe, ARM NEON/OpenMP) で
     関連日記チャンク Top-3 を抽出
  b. deep_profile.json (深層プロファイル) + 検索チャンク + data/knowledge/
     (外部知識) をローカルLLM (llama.cpp) のプロンプトへ結合
  c. 「現状分析」「価値観との整合性」「必要なスキルギャップ」「次の一手」の
     4セクションで回答を生成

を行う。全処理は完全オフライン。LLM推論は 127.0.0.1 上の llama.cpp
(llama-server / llama cli) のみを使用し、外部APIは一切叩かない。

使い方:
  python src/python/app.py "相談内容"
  python src/python/app.py "相談内容" --show-prompt --top-k 3
  python src/python/app.py            # 引数なしで対話モード
"""

from __future__ import annotations

import argparse
import json
import os

# 完全オフライン保証: 埋め込みモデルはローカルキャッシュのみ使用し、
# HF Hub への接続を一切行わない (キャッシュ未取得時はフォールバック埋め込みに移行)
os.environ.setdefault("HF_HUB_OFFLINE", "1")
os.environ.setdefault("TRANSFORMERS_OFFLINE", "1")
import re
import subprocess
import sys
import time
from pathlib import Path

from .paths import (
    BUILD_DIR,
    DATA_KNOWLEDGE,
    DATA_PROCESSED,
    METADATA_JSON,
    PROJECT_ROOT as ROOT,
    SEARCH_EXE,
    VECTORS_BIN,
)

VECTORS_BIN = VECTORS_BIN
METADATA_JSON = METADATA_JSON
PROFILE_JSON = DATA_PROCESSED / "deep_profile.json"
KNOWLEDGE_DIR = DATA_KNOWLEDGE
SEARCH_EXE = SEARCH_EXE
TMP_QUERY_BIN = DATA_PROCESSED / "_app_query.bin"
LAST_ANSWER_MD = DATA_PROCESSED / "last_consultation.md"

from .llm_config import (  # noqa: E402
    find_gguf,
    generation_params,
    LLAMA_CTX,
    SERVER_PORT,
)
from .llm_backend import LlamaServerBackend  # noqa: E402
from .pipeline import build_embedder, l2_normalize  # noqa: E402

SYSTEM_PROMPT = (
    "あなたはユーザーの思考・価値観を完全に理解する分身AIである。"
    "ユーザーの意思決定を支援せよ。"
    "与えられた深層プロファイル(認知バイアス・価値観・感情パターン・意思決定ルール)と"
    "過去の日記コンテキスト・外部知識のみを根拠として、本人の口調で誠実に助言すること。"
)

OUTPUT_FRAMEWORK = """回答は必ず以下の4セクション構成のMarkdownで出力すること:

## 1. 現状分析
(検索された日記コンテキストと感情パターンから、いま何が起きているかを客観的に記述)

## 2. 価値観との整合性
(価値観の階層構造と照らし、選択肢が根源的欲求と整合するか。関連する認知バイアスへの警告も含める)

## 3. 必要なスキルギャップ
(外部知識を根拠に、目標達成のため埋めるべき具体的スキル・経験を列挙)

## 4. 次の一手
(意思決定ルールに従い、今週から実行可能な具体的アクションを2〜3個。可逆な小さい一歩を優先)"""


# ============================================================ a. C++検索エンジン呼び出し
def embed_query(text: str):
    """pipeline.py と同一のエンベッダでクエリをベクトル化 (空間の一貫性を保証)。"""
    from .pipeline import build_embedder, l2_normalize  # 遅延import (モデルロードが重い)
    import numpy as np
    emb = build_embedder()
    return l2_normalize(np.asarray(emb.encode([text]), dtype=np.float32))[0]


def search_topk(query_vec, top_k: int = 3) -> list[dict]:
    """NEON検索エンジンを起動し Top-K の (chunk_id, score) を得る。"""
    meta = json.loads(METADATA_JSON.read_text(encoding="utf-8"))
    chunks = {c["id"]: c for c in meta["chunks"]}

    hits: list[tuple[int, float]] = []
    if SEARCH_EXE.exists():
        TMP_QUERY_BIN.write_bytes(query_vec.tobytes())
        out = subprocess.run(
            [str(SEARCH_EXE), str(VECTORS_BIN), str(TMP_QUERY_BIN), str(top_k)],
            capture_output=True, text=True, timeout=60,
            encoding="utf-8", errors="replace",
        )
        for m in re.finditer(r"chunk_id=(\d+)\s+score=([\d.\-]+)", out.stdout):
            hits.append((int(m.group(1)), float(m.group(2))))
        engine = "C++ NEON/OpenMP (search_engine.exe)"
    if not hits:  # exe不在や解析失敗時の純Pythonフォールバック
        import numpy as np
        raw = VECTORS_BIN.read_bytes()
        import struct
        _, dim, lanes, nvec, nblk, _, _ = struct.unpack("<8sIIIIII", raw[:32])
        blocks = np.frombuffer(raw[32:], dtype=np.uint8).reshape(nblk, 6160)
        data = blocks[:, :6144].copy().view(np.float32).reshape(nblk, dim, lanes)
        ids = blocks[:, 6144:].copy().view(np.int32).reshape(-1)
        vecs = data.transpose(0, 2, 1).reshape(-1, dim)
        scores = vecs @ query_vec
        order = [i for i in np.argsort(-scores) if ids[i] >= 0][:top_k]
        hits = [(int(ids[i]), float(scores[i])) for i in order]
        engine = "NumPy fallback"

    print(f"[app] 検索エンジン: {engine} -> Top-{len(hits)}")
    return [{"chunk_id": cid, "score": sc, **chunks[cid]} for cid, sc in hits]


# ============================================================ b. プロンプト合成
def load_profile() -> dict:
    if not PROFILE_JSON.exists():
        print("[app] deep_profile.json が無いため profiler を実行します")
        subprocess.run([sys.executable, str(ROOT / "src" / "python" / "profiler.py")],
                       check=True)
    return json.loads(PROFILE_JSON.read_text(encoding="utf-8"))


def summarize_profile(p: dict) -> str:
    """プロンプト用にプロファイルを要約 (小型モデルの文脈長を節約)。"""
    lines = ["### 認知的バイアス (検出強度順)"]
    for b in p["cognitive_biases"][:3]:
        if b["hit_count"] > 0:
            lines.append(f"- {b['bias']} (強度{b['intensity']}): {b['description']}")
    lines.append("### 価値観の階層構造 (重み順)")
    for v in p["value_hierarchy"][:4]:
        lines.append(f"- 第{v['rank']}位 {v['value']} (重み{v['weight']}) ← 根源的欲求: {v['root_need']}")
    ep = p["emotional_patterns"]
    lines.append("### 感情的反応パターン")
    lines.append(f"- 全体: 平均感情{ep['overall_avg_sentiment']:+.2f} / 揺らぎ(SD){ep['overall_volatility_stdev']:.2f}")
    for c in ep["per_contact"]:
        lines.append(f"- {c['contact']}: {c['typical_reaction']} (感情{c['avg_sentiment']:+.2f}, SD{c['volatility_stdev']:.2f})")
    lines.append("### 意思決定アルゴリズム (IF-THENルール)")
    seen = set()
    for r in p["decision_rules"]:
        act = r["recommended_action"]
        if act in seen:
            continue
        seen.add(act)
        lines.append(f"- IF「{r['situation']}」THEN: {act} (確信度{r['confidence']})")
        if len(seen) >= 6:
            break
    return "\n".join(lines)


# Knowledge excerpt budget (not LLM generation max_tokens).
_DEFAULT_KNOWLEDGE_CHARS = 450 * 2


def load_knowledge(max_chars_per_file: int = _DEFAULT_KNOWLEDGE_CHARS) -> str:
    KNOWLEDGE_DIR.mkdir(parents=True, exist_ok=True)
    parts = []
    for f in sorted(KNOWLEDGE_DIR.glob("*")):
        if f.suffix.lower() in (".md", ".txt"):
            body = f.read_text(encoding="utf-8").strip()[:max_chars_per_file]
            parts.append(f"--- {f.name} ---\n{body}")
    return "\n\n".join(parts) if parts else "(外部知識なし)"


def build_user_prompt(query: str, profile: dict, chunks: list[dict]) -> str:
    ctx = "\n\n".join(
        f"[日記 {c['title']} / 類似度{c['score']:.3f}]\n{c['text'].strip()[:400]}"
        for c in chunks)
    return f"""# 深層プロファイル (自己分析データ)
{summarize_profile(profile)}

# コンテキスト1: 関連する過去の日記 (NEONベクトル検索 Top-{len(chunks)})
{ctx}

# コンテキスト2: 外部知識 (data/knowledge/)
{load_knowledge()}

# ユーザーの相談
{query}

{OUTPUT_FRAMEWORK}"""


# ============================================================ ローカルLLMラッパー
# LlamaServerBackend は core/llm_backend.py が唯一所有 (INC-LLM-CLIENT-01)。


class LlamaCliBackend:
    """llama.exe cli (single-turn) によるフォールバック実行。"""

    name = "llama cli (single-turn)"

    def __init__(self, exe: Path, model: Path):
        self.exe, self.model = exe, model

    def generate(
        self,
        system: str,
        user: str,
        max_tokens: int | None = None,
    ) -> str:
        gen = generation_params()
        effective_max_tokens = (
            max_tokens if max_tokens is not None else gen["max_tokens"]
        )
        temperature = gen["temperature"]
        pf = ROOT / "build" / "_app_prompt.txt"
        pf.write_text(user, encoding="utf-8", newline="\n")
        out = subprocess.run(
            [str(self.exe), "cli", "-m", str(self.model), "-f", str(pf),
             "-sys", system, "-n", str(effective_max_tokens), "-st",
             "--no-display-prompt", "--temp", str(temperature),
             "-c", str(LLAMA_CTX)],
            capture_output=True, timeout=1200)
        text = out.stdout.decode("utf-8", errors="replace")
        # バナー・プロンプトエコー・統計行を除去して本文のみ抽出
        text = re.sub(r"(?s)^.*?available commands:.*?(?=\n> )", "", text)
        text = re.sub(r"(?m)^> .*$", "", text)
        text = re.sub(r"\[ Prompt:.*?\]", "", text)
        text = text.replace("Exiting...", "")
        return text.strip()

    def stop(self) -> None:
        pass


class RuleBasedBackend:
    """LLM実行環境が無い場合でも4セクション回答を返す決定論的フォールバック。"""

    name = "rule-based reasoner (LLMなしフォールバック)"

    def __init__(self, profile: dict, chunks: list[dict]):
        self.profile, self.chunks = profile, chunks

    def generate(self, system: str, user: str, max_tokens: int = 0) -> str:
        p = self.profile
        top_values = p["value_hierarchy"][:2]
        biases = [b for b in p["cognitive_biases"] if b["hit_count"] > 0][:2]
        rules = p["decision_rules"][:3]
        diary = "\n".join(f"- {c['title']}: {c['text'].strip().splitlines()[0][:60]}"
                          for c in self.chunks)
        return f"""## 1. 現状分析
関連する日記エントリは以下の通り:
{diary}
感情ログ全体の平均感情は {p['emotional_patterns']['overall_avg_sentiment']:+.2f}、揺らぎ(SD)は {p['emotional_patterns']['overall_volatility_stdev']:.2f}。

## 2. 価値観との整合性
最上位の価値観は「{top_values[0]['value']}」(根源的欲求: {top_values[0]['root_need']})、次点は「{top_values[1]['value']}」。
警告: 検出済みバイアス {', '.join(b['bias'] for b in biases)} が判断を歪める可能性がある。

## 3. 必要なスキルギャップ
data/knowledge/ の外部知識を参照のこと (LLM無効のため自動要約は省略)。

## 4. 次の一手
{chr(10).join('- ' + r['recommended_action'] for r in rules)}

(注: ローカルLLMが未検出のためルールベース応答。models/ に GGUF を配置すると推論が有効化される)"""

    def stop(self) -> None:
        pass


def select_backend(profile: dict, chunks: list[dict]):
    from .paths import LLAMA_CLI_EXE, LLAMA_SERVER_EXE

    model = find_gguf(role="consult")
    server_exe = LLAMA_SERVER_EXE
    cli_exe = LLAMA_CLI_EXE
    if model and server_exe.exists():
        return LlamaServerBackend(server_exe, model, SERVER_PORT)
    if model and cli_exe.exists():
        return LlamaCliBackend(cli_exe, model)
    return RuleBasedBackend(profile, chunks)


# ============================================================ メイン
def consult(query: str, top_k: int = 3, show_prompt: bool = False) -> str:
    print(f"[app] 相談: {query}")
    qvec = embed_query(query)
    chunks = search_topk(qvec, top_k)
    for c in chunks:
        print(f"[app]   hit: {c['title']} (score={c['score']:.3f})")

    profile = load_profile()
    user_prompt = build_user_prompt(query, profile, chunks)
    if show_prompt:
        print("=" * 60, "\n[PROMPT]\n", user_prompt, "\n", "=" * 60)

    backend = select_backend(profile, chunks)
    print(f"[app] 推論バックエンド: {backend.name}")
    model = find_gguf(role="consult")
    if model:
        print(f"[app] モデル: {model.name}")

    t0 = time.perf_counter()
    answer = backend.generate(SYSTEM_PROMPT, user_prompt)
    dt = time.perf_counter() - t0
    print(f"[app] 生成完了 ({dt:.1f}s)\n")

    LAST_ANSWER_MD.write_text(
        f"# 相談\n{query}\n\n# 回答 ({backend.name})\n\n{answer}\n", encoding="utf-8")
    return answer


def main() -> None:
    ap = argparse.ArgumentParser(description="自己エミュレーション意思決定支援")
    ap.add_argument("query", nargs="?", help="相談内容 (省略時は対話モード)")
    ap.add_argument("--top-k", type=int, default=3)
    ap.add_argument("--show-prompt", action="store_true")
    args = ap.parse_args()

    if args.query:
        print(consult(args.query, args.top_k, args.show_prompt))
        return

    print("自己エミュレーション意思決定支援 (exit で終了)")
    while True:
        try:
            q = input("\n相談 > ").strip()
        except (EOFError, KeyboardInterrupt):
            break
        if not q or q.lower() in ("exit", "quit"):
            break
        print(consult(q, args.top_k, args.show_prompt))


if __name__ == "__main__":
    main()
