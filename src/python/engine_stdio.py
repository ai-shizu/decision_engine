#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""PKB デスクトップ用エンジン — stdin/stdout JSON ライン通信。"""

from __future__ import annotations

import io
import json
import sys
from pathlib import Path
from typing import Any, Callable, TextIO

_PYTHON_ROOT = Path(__file__).resolve().parent
if str(_PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(_PYTHON_ROOT))

from core.offline_runtime import enforce_offline_environment

enforce_offline_environment()

# Exact sterile diagnostic persisted by the Rust engine-log allowlist (Finding 12).
_PKB_DIAG_REQUEST_FAILED = "[PKB_DIAG_V1] REQUEST_FAILED"


def _open_json_stdout() -> TextIO:
    """Rust との JSON 通信用 stdout を UTF-8 に固定 (Windows cp932 回避)。"""
    buf = getattr(sys.__stdout__, "buffer", None)
    if buf is not None:
        return io.TextIOWrapper(
            buf,
            encoding="utf-8",
            newline="\n",
            write_through=True,
        )
    if hasattr(sys.__stdout__, "reconfigure"):
        sys.__stdout__.reconfigure(encoding="utf-8")
    return sys.__stdout__


# stdout は Rust との JSON 専用。print() 等は stderr へ退避する。
_JSON_OUT: TextIO = _open_json_stdout()
sys.stdout = sys.stderr  # noqa: E501


def _emit(payload: dict[str, Any]) -> None:
    from core.text_utils import sanitize_obj

    safe = sanitize_obj(payload)
    _JSON_OUT.write(json.dumps(safe, ensure_ascii=False) + "\n")
    _JSON_OUT.flush()


def _emit_request_failed_diag() -> None:
    """Write the sole allowlisted persistent diagnostic to stderr (no exception data)."""
    sys.stderr.write(_PKB_DIAG_REQUEST_FAILED + "\n")
    sys.stderr.flush()


def _import_facade():
    from core import facade  # noqa: WPS433

    return facade


EventEmitter = Callable[[dict[str, Any]], None]


def dispatch(cmd: str, params: dict[str, Any], emit: EventEmitter | None = None) -> Any:
    if cmd == "health":
        return {"status": "ok", "offline": True}

    if cmd == "settings.get":
        from core.settings_api import get_settings

        return get_settings()
    if cmd == "settings.save_fixed":
        from core.settings_api import save_settings_fixed

        save_settings_fixed(params.get("attributes", {}))
        return {"saved": True}

    facade = _import_facade()

    if cmd == "record.load":
        return facade.load_record(params["date"])
    if cmd == "record.save":
        return facade.save_record(
            params["date"],
            params.get("events", []),
            params.get("transactions", []),
            params.get("diary", ""),
        )
    if cmd == "calendar.event_dates":
        return {"dates": facade.calendar_event_dates()}
    if cmd == "consult":
        query = str(params.get("query", "")).strip()
        if not query:
            raise ValueError("query is required")
        mode = str(params.get("mode", "consult"))
        personas = params.get("personas")
        if not isinstance(personas, list):
            personas = None
        config = params.get("config")
        if not isinstance(config, dict):
            config = None
        rts = params.get("response_time_sec")
        response_time_sec = float(rts) if isinstance(rts, (int, float)) else None
        ext_id = params.get("external_research_id")
        if ext_id is not None and type(ext_id) is not str:
            raise ValueError("E0B_VALIDATION_REJECTED")
        if ext_id is not None and mode != "consult":
            raise ValueError("E0B_VALIDATION_REJECTED")
        # emit があれば進捗 status と生成トークンをイベント行として逐次送出する
        status = (lambda msg: emit({"event": "status", "message": msg})) if emit else None
        on_token = (lambda text: emit({"event": "chunk", "text": text})) if emit else None
        answer = facade.consult(
            query, status=status, on_token=on_token, mode=mode,
            personas=personas, response_time_sec=response_time_sec, config=config,
            external_research_id=ext_id)
        result: dict = {"query": query, "mode": mode, "answer": answer}
        # F4b (W-37): 検証済みの interview_report.v1 のみを構造体として渡す。
        # UI は JSON.parse(LLM出力) を絶対に書かない。
        report = facade.last_interview_report()
        if report is not None:
            result["report"] = report
        romance = facade.last_romance_analysis()
        if romance is not None:
            result["romance_analysis"] = romance
        return result
    if cmd == "knowledge.fetch_pending":
        return facade.fetch_pending_knowledge()
    if cmd == "calendar.sync":
        source = params["source"]
        mode = params.get("mode", "append")
        # F2 (SPEC_FOXTROT_UI.md §2.2.1): consult と同型の status 逐次通知。
        status = (lambda msg: emit({"event": "status", "message": msg})) if emit else None
        if source == "ics" and params.get("ics_files"):
            return facade.sync_calendar_ics_batch(params["ics_files"], mode, status=status)
        if source == "ics" and params.get("ics_content"):
            return facade.sync_calendar_ics_content(params["ics_content"], mode, status=status)
        if source == "apple":
            return facade.sync_calendar(
                "apple",
                mode,
                db_path=params.get("db_path"),
                days_back=int(params.get("days_back", 365)),
                days_ahead=int(params.get("days_ahead", 365)),
                status=status,
            )
        return facade.sync_calendar(source, mode, ics_path=params.get("ics_path"), status=status)
    if cmd == "import.line":
        status = (lambda msg: emit({"event": "status", "message": msg})) if emit else None
        files = params.get("files")
        if files:
            return facade.import_line_batch(files, status=status)
        return facade.import_line_text(
            params["content"],
            params.get("filename", ""),
            status=status,
        )
    if cmd == "import.stats":
        return facade.data_source_stats()
    if cmd == "es.view":
        return facade.active_es()
    if cmd == "es.list":
        return facade.list_es()
    if cmd == "import.classify":
        return facade.classify_document(params["content"], params.get("filename", ""))
    if cmd == "import.document":
        status = (lambda msg: emit({"event": "status", "message": msg})) if emit else None
        return facade.import_document(
            params["content"],
            params.get("filename", ""),
            params["dest"],
            company_name=params.get("company_name"),
            confirm_overwrite=bool(params.get("confirm_overwrite", False)),
            replace_es_id=params.get("replace_es_id"),
            status=status,
        )
    if cmd == "llm.warm":
        return facade.warm_consult_runtime(
            probe_llm=bool(params.get("probe_llm", True)),
        )
    if cmd == "settings.run_profiler":
        return facade.run_profiler()
    if cmd == "narrative.compile":
        return facade.compile_narrative(params.get("target_domain"))
    if cmd == "oracle.payload":
        return facade.oracle_payload(params.get("scope", "global"), alias=params.get("alias"))
    if cmd == "oracle.report":
        status = (lambda msg: emit({"event": "status", "message": msg})) if emit else None
        return facade.oracle_report(
            params.get("scope", "global"), alias=params.get("alias"), status=status)
    if cmd == "twin.forecast":
        return facade.twin_forecast(
            params.get("scenario", {}), scope=params.get("scope", "global"),
            alias=params.get("alias"))
    if cmd == "tensor.rebuild":
        return facade.tensor_rebuild()
    if cmd == "profile.source_code":
        return facade.get_source_code()
    if cmd == "probe.status":
        return facade.probe_status(params.get("today"))
    if cmd == "probe.next":
        return facade.probe_next(params["today"])
    if cmd == "probe.answer":
        return facade.probe_answer(
            params["session_id"],
            params["question_id"],
            params["answer"],
            params["today"],
        )
    if cmd == "context.manifest.latest":
        if type(params) is not dict or params:
            raise ValueError("context.manifest.latest accepts no params")
        return facade.latest_context_manifest()
    if cmd == "knowledge.intent.build":
        if type(params) is not dict:
            raise ValueError("E0B_VALIDATION_REJECTED")
        return facade.knowledge_intent_build(params)
    if cmd == "knowledge.integrate":
        if type(params) is not dict:
            raise ValueError("E0B_VALIDATION_REJECTED")
        return facade.knowledge_integrate(params)
    if cmd == "shutdown":
        facade.shutdown_engine()
        return {"status": "stopped"}

    raise ValueError(f"unknown command: {cmd}")


def main() -> None:
    _emit({"event": "ready", "offline": True})
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        req_id = None
        cid = None
        try:
            req = json.loads(line)
            req_id = req.get("id")
            cid = req.get("cid")

            # W-48 (SPEC_FOXTROT_UI.md §9.3): cid の刻印はこの emit_event
            # ラッパー1箇所のみ。dispatch 内の各コマンドはこの emit の中身を
            # 意識しない (status/on_token ラムダが emit(...) を呼ぶだけ)。
            def emit_event(payload: dict[str, Any], _id=req_id, _cid=cid) -> None:
                _emit({"id": _id, "cid": _cid, **payload})

            result = dispatch(req["cmd"], req.get("params") or {}, emit=emit_event)
            out: dict[str, Any] = {
                "id": req_id,
                "cid": cid,
                "ok": True,
                "result": result,
            }
        except Exception as exc:  # noqa: BLE001
            _emit_request_failed_diag()
            out = {
                "id": req_id,
                "cid": cid,
                "ok": False,
                "error": f"{type(exc).__name__}: {exc}",
            }
        _emit(out)


if __name__ == "__main__":
    main()
