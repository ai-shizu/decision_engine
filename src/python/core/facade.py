#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
PKB 共通サービス層
==================
TUI / デスクトップ UI から共有するビジネス操作。
UI 依存は一切含まない。
"""

from __future__ import annotations

import json
import re
import sys
from datetime import date, datetime
from pathlib import Path
from typing import Callable

from . import apple_calendar_sync
from . import calendar_manager as cal
from . import calendar_sync
from . import finance_manager as fin
from .consultation_engine import (
    ConsultationEngine,
    FIXED_ATTRIBUTE_FIELDS,
    USER_PROFILE,
    format_user_profile_summary,
    load_user_profile,
    save_fixed_attributes,
)
from .paths import (
    CALENDAR_JSON,
    DATA_KNOWLEDGE,
    DIARY_MD,
    ES_DIR,
    FINANCE_JSON,
    LINE_HISTORY,
    PROJECT_ROOT,
    PROCESSED,
)

StatusCallback = Callable[[str], None]

_engine: ConsultationEngine | None = None


def get_engine() -> ConsultationEngine:
    global _engine
    if _engine is None:
        _engine = ConsultationEngine()
    return _engine


def shutdown_engine() -> None:
    global _engine
    if _engine is not None:
        _engine.shutdown()
        _engine = None


def load_record(date_str: str) -> dict:
    return {
        "date": date_str,
        "events": cal.get_events_for_date(date_str),
        "transactions": fin.get_transactions_for_date(date_str),
        "diary": cal.load_diary_for_date(date_str),
    }


def save_record(
    date_str: str,
    events: list[dict],
    transactions: list[dict],
    diary: str,
) -> dict:
    date.fromisoformat(date_str)
    cal.set_events_for_date(date_str, events)
    fin.set_transactions_for_date(date_str, transactions)
    body = diary.strip()
    if body:
        cal.save_diary_for_date(date_str, body)
    elif cal.load_diary_for_date(date_str):
        cal.save_diary_for_date(date_str, "")
    rebuilt = get_engine().sync_diary_index(force=True)
    return {"date": date_str, "saved": True, "index_rebuilt": rebuilt}


def consult(
    query: str,
    status: StatusCallback | None = None,
    on_token: StatusCallback | None = None,
    mode: str = "consult",
    personas: list[dict] | None = None,
    response_time_sec: float | None = None,
) -> str:
    return get_engine().consult(
        query, status=status, on_token=on_token, mode=mode,
        personas=personas, response_time_sec=response_time_sec)


def compile_narrative(target_domain: str | None = None) -> dict:
    """NARRATIVE COMPILER (Target Delta D3): gap_analysis の証拠付きギャップから
    ES ドラフト + Recruiter's Eye (メタ解説) を生成する。"""
    from .narrative_compiler import compile_narrative as _compile
    return _compile(get_engine(), target_domain=target_domain)


def oracle_payload(scope: str = "global", alias: str | None = None) -> dict:
    """Target Echo (E4): 無菌化された oracle_payload.v1 のみを返す (LLM 呼び出し
    なし)。UI の数値表示・ui_smoke はこの経路だけで完結し、7B の生成を待たない
    (SPEC_ECHO_GENESIS.md §5.10.5 の E4 実装ノート — oracle.payload/oracle.report
    の cmd 分離)。"""
    from . import oracle as _oracle
    return _oracle.build_oracle_payload(scope, alias=alias)


def oracle_report(scope: str = "global", alias: str | None = None,
                  status: StatusCallback | None = None) -> dict:
    """Target Echo (E4): oracle_payload + LLM による言語化。"""
    from . import oracle as _oracle

    if status:
        status("Echo: テンソル・結合行列・ツインを評価中…")
    payload = _oracle.build_oracle_payload(scope, alias=alias)
    text = _oracle.render_oracle_consult(payload)
    if payload["sufficiency"]["gate_passed"]:
        if status:
            status("Echo: 言語化を生成中…")
        system = ("あなたは本人の物理量データ (LINE 上の観測範囲に限定) を"
                 "解釈するアシスタントである。以下の事実のみを根拠に、"
                 "新しい事実や介入を創作せず簡潔に助言せよ。")
        analysis = get_engine().backend.generate(system, text)
        import re as _re
        analysis = _re.sub(r"<think>.*?</think>\s*", "", analysis, flags=_re.DOTALL).strip()
    else:
        analysis = text
    return {"payload": payload, "analysis": analysis}


def twin_forecast(scenario: dict, scope: str = "global", alias: str | None = None) -> dict:
    """Target Echo (E4): モンテカルロ予測のみ (LLM なし)。"""
    from . import digital_twin as _dt
    from . import tensor_store as _ts

    if scope == "dyad":
        return {"gate_passed": False, "reason": "dyad focus 未配線"}
    path = _ts.TENSOR_GLOBAL_BIN
    if not path.exists():
        return {"gate_passed": False, "reason": "tensor 未構築"}
    store = _ts.TensorStore(path)
    try:
        params = _dt.fit_twin(store)
        if not params.gate_passed:
            return {"gate_passed": False, "reason": "walk-forward スキルゲート未達",
                   "bss": params.bss, "n_lapse_test": params.n_lapse_test}
        return {"gate_passed": True, **_dt.simulate(params, store, scenario)}
    finally:
        store.close()


def tensor_rebuild() -> dict:
    """Target Echo (E4): PKBTEN01 の全再構築。自動発火は profiler 実行 /
    import.line からのみ (I-3)。本関数は手動再構築 (Advanced 設定) 用にも公開する。"""
    from . import tensor_store as _ts
    from .data_merger import load_daily_contexts
    from .profiler import load_line_messages

    daily = load_daily_contexts()
    if not daily:
        return {"rebuilt": False, "rows": 0}
    messages = load_line_messages()
    _ts.build_tensor(daily, None, _ts.TENSOR_GLOBAL_BIN, line_messages=messages)
    return {"rebuilt": True, "rows": len(daily)}


def fetch_pending_knowledge() -> dict:
    """<fetch_query> キューを処理し、取得済み知識をインデックスへ統合する。

    ネットワーク取得は PKB_ALLOW_ONLINE_FETCH=1 の時のみ。未許可時は
    pending 件数を返すだけで一切通信しない (完全オフライン維持)。"""
    from .knowledge_fetcher import load_queue, online_fetch_allowed, process_pending

    summary = process_pending()
    if summary["processed"]:
        summary["index_rebuilt"] = get_engine().sync_knowledge_index(force=True)
    pending = sum(1 for e in load_queue() if e["status"] == "pending")
    summary["pending"] = pending
    summary["online_allowed"] = online_fetch_allowed()
    if summary["processed"]:
        summary["message"] = (
            f"{summary['processed']} 件の外部知識を取得し、知識インデックスへ統合しました")
    elif not summary["online_allowed"] and pending:
        summary["message"] = (
            f"pending {pending} 件 — ネットワーク取得は無効です "
            "(PKB_ALLOW_ONLINE_FETCH=1 で許可、または data/knowledge/ に手動配置)")
    else:
        summary["message"] = "処理対象の外部知識リクエストはありません"
    return summary


def sync_diary_index(force: bool = False) -> bool:
    return get_engine().sync_diary_index(force=force)


def sync_calendar(
    source: str,
    mode: str = "append",
    *,
    ics_path: str | Path | None = None,
    db_path: str | Path | None = None,
    days_back: int = 365,
    days_ahead: int = 365,
    status: StatusCallback | None = None,
) -> dict:
    if mode not in ("append", "overwrite"):
        raise ValueError(f"不明なマージモード: {mode}")
    if status:
        status(f"{source} 同期中")
    if source == "ics":
        if not ics_path:
            raise ValueError("ICS 同期には ics_path が必要です")
        summary = calendar_sync.sync_from_ics(ics_path, mode=mode)
    elif source == "apple":
        summary = apple_calendar_sync.sync_from_apple_calendar(
            mode=mode, db_path=db_path,
            days_back=days_back, days_ahead=days_ahead,
        )
    else:
        raise ValueError(f"不明な同期ソース: {source}")
    if status:
        status("DailyContext結晶化+ベクトル同期中")
    summary["index_rebuilt"] = get_engine().sync_diary_index(force=True)
    if status:
        status("完了")
    return summary


def calendar_event_dates() -> list[str]:
    """カレンダーマーク用: 予定・日記・家計簿・AI相談のいずれかが存在する日付。"""
    from .consultation_log import load_consultations

    dates = set(cal.dates_with_events())
    dates |= cal.dates_with_diary()
    dates |= {d for d, txs in fin.load_finance().items() if txs}
    dates |= {d for d, entries in load_consultations().items() if entries}
    return sorted(dates)


def _mtime_iso(path: Path) -> str | None:
    if not path.exists():
        return None
    return datetime.fromtimestamp(path.stat().st_mtime).isoformat()


def _diary_entry_count(path: Path) -> int:
    text = path.read_text(encoding="utf-8")
    return len(re.findall(r"^##\s+\d{4}-\d{2}-\d{2}", text, re.MULTILINE))


def _line_export_count(path: Path) -> int:
    return path.read_text(encoding="utf-8").count("[LINE]")


def _json_entry_count(path: Path) -> int:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return 0
    return sum(len(v) for v in data.values()) if isinstance(data, dict) else 0


def _dir_file_count(path: Path) -> int:
    if not path.is_dir():
        return 0
    return sum(1 for p in path.iterdir() if p.is_file())


def data_source_stats() -> dict:
    """F2 (SPEC_FOXTROT_UI.md §2.2.1 裁定4): IMPORT タブの SourceTable 用。
    stdlib のみの軽量 stat ({exists, count, mtime})。LLM/埋め込みは一切
    使わない (遅延初期化 (AI_SKILLS §1) を壊さないこと)。
    """
    sources = {
        "diary": (DIARY_MD, _diary_entry_count),
        "line": (LINE_HISTORY, _line_export_count),
        "calendar": (CALENDAR_JSON, _json_entry_count),
        "finance": (FINANCE_JSON, _json_entry_count),
    }
    result: dict[str, dict] = {}
    for name, (path, count_fn) in sources.items():
        exists = path.exists()
        result[name] = {
            "exists": exists,
            "count": count_fn(path) if exists else 0,
            "mtime": _mtime_iso(path),
        }
    for name, path in (("es", ES_DIR), ("knowledge", DATA_KNOWLEDGE)):
        exists = path.is_dir()
        result[name] = {
            "exists": exists,
            "count": _dir_file_count(path) if exists else 0,
            "mtime": _mtime_iso(path),
        }
    return result


def _run_profiler() -> dict:
    try:
        from core import profiler  # noqa: WPS433

        profiler.main()
        return {
            "ok": True,
            "message": "再分析完了 (deep_profile.json / user_profile.json 更新)",
        }
    except Exception as exc:  # noqa: BLE001
        import traceback

        traceback.print_exc(file=sys.stderr)
        return {"ok": False, "message": f"profiler失敗: {type(exc).__name__}: {exc}"}


def run_profiler() -> dict:
    return _run_profiler()


def format_line_import(text: str, filename: str = "") -> str:
    """T-23 (IMP-2): [LINE] ヘッダの無い追記はエクスポートのブロック境界を
    消し、profiler.load_line_messages() の多重集合デデュープ (max) を
    ブロック内 sum に退化させる (docs/AI_SKILLS.md §14)。取込側でヘッダを
    補ってから追記することで、全 append 経路が必ずブロック境界を持つ。
    """
    body = text.strip()
    if "[LINE]" not in body:
        label = Path(filename).stem if filename else "不明"
        body = f"[LINE] {label}とのトーク履歴\n{body}"
    return body


def _append_line_text(text: str, filename: str = "") -> None:
    head = text[:2000]
    stem = Path(filename).stem.lower() if filename else ""
    if "[LINE]" not in head and "line" not in stem and not filename.lower().endswith(".txt"):
        raise ValueError("LINE履歴 (.txt) のみ取り込めます")
    with open(LINE_HISTORY, "a", encoding="utf-8") as f:
        f.write("\n" + format_line_import(text, filename) + "\n")


def import_line_text(text: str, filename: str = "", *, status: StatusCallback | None = None) -> dict:
    """F2 (SPEC_FOXTROT_UI.md §2.2.1): status は UI への逐次進捗通知のみに
    使う任意コールバック。ロジックは無変更 (配線のみ)。W-32: ファイル名は
    UI の一時イベントに含めてよいが、ログ・永続化には書かない。
    """
    label = filename or "line_history.txt"
    if status:
        status(f"{label} を受信")
    _append_line_text(text, filename)
    if status:
        status("追記完了 — profiler 再分析中 (テレメトリ/テンソル同期含む)")
    result = _run_profiler()
    msg = result["message"]
    if result["ok"]:
        msg = f"{label} を取り込み — {msg}"
    if status:
        status("完了")
    return {"imported": True, "filename": label, **result, "message": msg}


def import_line_batch(files: list[dict], *, status: StatusCallback | None = None) -> dict:
    names: list[str] = []
    if status:
        status(f"{len(files)} 件を受信")
    for item in files:
        content = str(item.get("content", ""))
        name = str(item.get("filename", ""))
        _append_line_text(content, name)
        names.append(name or f"file_{len(names) + 1}.txt")
    if status:
        status("追記完了 — profiler 再分析中 (テレメトリ/テンソル同期含む)")
    result = _run_profiler()
    msg = result["message"]
    if result["ok"]:
        msg = f"{len(names)} 件の LINE 履歴を取り込み — {msg}"
    if status:
        status("完了")
    return {
        "imported": True,
        "count": len(names),
        "filenames": names,
        **result,
        "message": msg,
    }


def sync_calendar_ics_batch(
    files: list[dict], mode: str = "append", *, status: StatusCallback | None = None,
) -> dict:
    if mode not in ("append", "overwrite"):
        raise ValueError(f"不明なマージモード: {mode}")
    names: list[str] = []
    last_summary: dict = {}
    if status:
        status(f"{len(files)} 件を受信")
    for item in files:
        content = str(item.get("content", ""))
        name = str(item.get("filename", "calendar.ics"))
        last_summary = sync_calendar_ics_content(content, mode, status=status)
        names.append(name)
    message = f"{len(names)} 件の ICS を同期しました ({mode})"
    if last_summary.get("index_rebuilt"):
        message += " — DailyContext結晶化+ベクトル同期完了"
    return {**last_summary, "count": len(names), "filenames": names, "message": message}


def sync_calendar_ics_content(
    content: str, mode: str = "append", *, status: StatusCallback | None = None,
) -> dict:
    import tempfile

    with tempfile.NamedTemporaryFile(
        mode="w", suffix=".ics", delete=False, encoding="utf-8",
    ) as tf:
        tf.write(content)
        tmp_path = tf.name
    try:
        summary = sync_calendar("ics", mode, ics_path=tmp_path, status=status)
    finally:
        Path(tmp_path).unlink(missing_ok=True)
    return summary


def get_settings() -> dict:
    from .settings_api import get_settings as _get_settings

    return _get_settings()


def import_line_history(path: str | Path) -> None:
    path = Path(path)
    text = path.read_text(encoding="utf-8", errors="replace")
    with open(LINE_HISTORY, "a", encoding="utf-8") as f:
        f.write("\n" + format_line_import(text, path.name) + "\n")


def profiler_script_path() -> Path:
    return PROJECT_ROOT / "src" / "python" / "core" / "profiler.py"


__all__ = [
    "DIARY_MD",
    "FIXED_ATTRIBUTE_FIELDS",
    "LINE_HISTORY",
    "PROJECT_ROOT",
    "PROCESSED",
    "USER_PROFILE",
    "calendar_event_dates",
    "compile_narrative",
    "consult",
    "data_source_stats",
    "fetch_pending_knowledge",
    "format_line_import",
    "format_user_profile_summary",
    "get_engine",
    "get_settings",
    "import_line_batch",
    "import_line_text",
    "oracle_payload",
    "oracle_report",
    "run_profiler",
    "tensor_rebuild",
    "twin_forecast",
    "sync_calendar_ics_batch",
    "sync_calendar_ics_content",
    "load_record",
    "load_user_profile",
    "profiler_script_path",
    "save_fixed_attributes",
    "save_record",
    "shutdown_engine",
    "sync_calendar",
    "sync_diary_index",
]
