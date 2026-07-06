# -*- coding: utf-8 -*-
"""NARRATIVE COMPILER (Target Delta D3) の決定論的テスト。

LLM は FakeBackend で差し替え (依存注入)、実 LLM には一切触れない。
実行は一時 PKB_PROJECT_ROOT 上で行い、実データには一切触れない。

対象: docs/SPEC_CHARLIE_DELTA.md §3.5。
  - 素材選択 (証拠の無いギャップは使わない、true_gakuchika 優先)
  - [ref:N] タグの検証・無効段落の破棄・リトライ・最終失敗
  - Recruiter's Eye は別呼び出しで生成され、永続化ファイルには含まれない
"""
from __future__ import annotations

import json
import os
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

_TMP = tempfile.mkdtemp(prefix="pkb_narrative_")
os.environ["PKB_PROJECT_ROOT"] = _TMP
os.environ.setdefault("HF_HUB_OFFLINE", "1")
os.environ.setdefault("TRANSFORMERS_OFFLINE", "1")

from core import narrative_compiler as nc  # noqa: E402
from core.paths import AI_CONSULTATIONS_JSON, DEEP_PROFILE, ES_DIR  # noqa: E402


class SeqFakeBackend:
    """呼び出し順に応じて異なる応答を返す決定論バックエンド。"""

    name = "fake-seq-backend"

    def __init__(self, answers: list[str]):
        self.answers = answers
        self.calls: list[tuple[str, str]] = []

    def generate(self, system: str, prompt: str, **_) -> str:
        self.calls.append((system, prompt))
        idx = min(len(self.calls) - 1, len(self.answers) - 1)
        return self.answers[idx]


class FakeEngine:
    def __init__(self, backend):
        self.backend = backend


def _reset_profile() -> None:
    DEEP_PROFILE.parent.mkdir(parents=True, exist_ok=True)
    if DEEP_PROFILE.exists():
        DEEP_PROFILE.unlink()
    for f in ES_DIR.glob("draft_*.md"):
        f.unlink()


def _write_profile(gaps: list[dict]) -> None:
    DEEP_PROFILE.parent.mkdir(parents=True, exist_ok=True)
    DEEP_PROFILE.write_text(json.dumps(
        {"gap_analysis": {"gaps": gaps}}, ensure_ascii=False), encoding="utf-8")


_GAP_WITH_QUOTES = {
    "theme": "技術開発・ものづくり", "type": "true_gakuchika", "gap": 0.6,
    "insight": "一人で完結するコーディングに閉じこもる傾向",
    "subjective": {"quotes": [{"date": "2026-06-01", "quote": "低レイヤ開発に没頭した"}]},
    "objective": {"evidence": {"events": ["個人開発デモ会"], "line": []}},
}
_GAP_NO_EVIDENCE = {
    "theme": "娯楽・消費", "type": "blind_spot", "gap": 0.4,
    "insight": "娯楽への言及がない", "subjective": {"quotes": []}, "objective": {},
}


def test_no_deep_profile_returns_explicit_reason() -> None:
    _reset_profile()
    eng = FakeEngine(SeqFakeBackend(["should not be called"]))
    result = nc.compile_narrative(eng)
    assert result["ok"] is False and result["reason"] == "no_deep_profile"
    assert eng.backend.calls == [], "素材が無いのに LLM を呼んでいる"
    print("  no deep_profile returns explicit reason OK")


def test_no_material_when_no_evidence() -> None:
    _reset_profile()
    _write_profile([_GAP_NO_EVIDENCE])
    eng = FakeEngine(SeqFakeBackend(["should not be called"]))
    result = nc.compile_narrative(eng)
    assert result["ok"] is False and result["reason"] == "no_material"
    assert eng.backend.calls == [], "証拠なしギャップなのに LLM を呼んでいる"
    print("  no material when no evidence OK")


def test_material_selection_prioritizes_true_gakuchika() -> None:
    task_avoid = {"theme": "テーマX", "type": "task_avoidance", "gap": 0.9,
                 "insight": "先延ばし", "subjective": {"quotes": [
                     {"date": "2026-06-02", "quote": "明日やる"}]}, "objective": {}}
    material = nc._select_material({"gaps": [task_avoid, _GAP_WITH_QUOTES]})
    assert material[0]["type"] == "true_gakuchika", \
        "true_gakuchika が優先されていない"
    print("  material selection prioritizes true_gakuchika OK")


def test_compile_success_with_valid_refs() -> None:
    _reset_profile()
    _write_profile([_GAP_WITH_QUOTES])
    answers = [
        "低レイヤ開発への没頭が示す通り、技術力は本物である。[ref:1]",
        "この構成は再現性を重視した。[ref:1]",   # Recruiter's Eye 用
    ]
    eng = FakeEngine(SeqFakeBackend(answers))
    result = nc.compile_narrative(eng, target_domain="バックエンドエンジニア")
    assert result["ok"] is True, result
    assert len(result["claims"]) == 1
    assert result["claims"][0]["node_refs"] == [1]
    assert "[ref:" not in result["es_text"], "参照タグが本文から除去されていない"
    assert result["recruiters_eye"] == answers[1]
    assert len(eng.backend.calls) == 2, "本文生成とRecruiter's Eyeは別呼び出しのはず"

    draft_path = Path(result["draft_path"])
    assert draft_path.exists()
    content = draft_path.read_text(encoding="utf-8")
    assert "再現性を重視した" not in content, \
        "Recruiter's Eye が draft ファイルに混入している (es_review 汚染)"
    assert "低レイヤ開発への没頭" in content

    log = json.loads(AI_CONSULTATIONS_JSON.read_text(encoding="utf-8"))
    entries = [e for day in log.values() for e in day]
    hit = next((e for e in entries
               if e["query"].startswith("[narrative_compile]")), None)
    assert hit is not None, "相談履歴に narrative_compile が記録されていない"
    assert hit["is_simulated_persona"] is True, \
        "narrative_compile は simulated=True で記録すべき (自己申告チャネル汚染防止)"
    print("  compile success with valid refs OK")


def test_compile_retries_then_succeeds() -> None:
    _reset_profile()
    _write_profile([_GAP_WITH_QUOTES])
    answers = [
        "参照タグの無い文章。これは破棄されるべき段落。",   # 1回目: refなし
        "リトライ後は根拠を明示する。[ref:1]",              # 2回目 (リトライ): refあり
        "リトライ後も一貫した戦略だった。[ref:1]",           # Recruiter's Eye
    ]
    eng = FakeEngine(SeqFakeBackend(answers))
    result = nc.compile_narrative(eng, target_domain="テスト職種", max_retries=2)
    assert result["ok"] is True, result
    assert "リトライ後は根拠を明示する" in result["es_text"]
    print("  compile retries then succeeds OK")


def test_compile_gives_up_after_max_retries() -> None:
    _reset_profile()
    _write_profile([_GAP_WITH_QUOTES])
    eng = FakeEngine(SeqFakeBackend(["refなし文章、毎回失敗する。"]))
    result = nc.compile_narrative(eng, target_domain="テスト職種", max_retries=1)
    assert result["ok"] is False and result["reason"] == "no_valid_claims"
    assert result["claims"] == []
    assert not list(ES_DIR.glob("draft_*.md")), \
        "生成失敗なのに draft ファイルが書かれている"
    print("  compile gives up after max retries OK")


if __name__ == "__main__":
    test_no_deep_profile_returns_explicit_reason()
    test_no_material_when_no_evidence()
    test_material_selection_prioritizes_true_gakuchika()
    test_compile_success_with_valid_refs()
    test_compile_retries_then_succeeds()
    test_compile_gives_up_after_max_retries()
    print("test_narrative_compiler: ALL PASS")
