# -*- coding: utf-8 -*-
"""search_daily の行動ログ(日記+LINE同日)ブーストの検証。"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src" / "python"))

from consultation_engine import ConsultationEngine

e = ConsultationEngine()
q = e.embed("飲み会の誘いを断ってばかりで人間関係が心配")
hits = e.search_daily(q, 3)
for h in hits:
    print(f"{h['title']}  raw={h['score']:.3f}  ranked={h['ranking_score']:.3f}  "
          f"full_day_log={h['is_full_day_log']}  sources={h.get('sources')}")
assert any(h["is_full_day_log"] for h in hits), "行動ログ日がTop-3に入っていない"
print("boost check: PASS")
