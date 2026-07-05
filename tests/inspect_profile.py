# -*- coding: utf-8 -*-
"""deep_profile.json の刺激→反応分析とLLM因果分析の内容確認。"""
import json
from pathlib import Path

p = json.loads((Path(__file__).resolve().parents[1] /
                "data/processed/deep_profile.json").read_text(encoding="utf-8"))

ip = p["interaction_patterns"]
print(f"sessions_analyzed: {ip.get('sessions_analyzed', ip.get('pairs_analyzed', 0))}")
for r in ip["stimulus_response_rules"][:6]:
    print(f"- {r['insight']}  (confidence={r['confidence']}, n={r['occurrences']})")

if ip.get("latency_patterns"):
    print("\n--- レイテンシパターン ---")
    for lp in ip["latency_patterns"][:4]:
        print(f"- [{lp['type']}] {lp['insight']}")

llm = p.get("llm_interaction_insights")
if llm:
    print("\n--- LLM因果分析 (冒頭900字) ---")
    print(llm["analysis"][:900])
else:
    print("\n(LLM因果分析なし: --no-llm 実行またはLLM未検出)")
