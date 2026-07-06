#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""PKB デスクトップ用エンジン — stdin/stdout JSON ライン通信。"""

from __future__ import annotations

import io
import json
import sys
import traceback
from pathlib import Path
from typing import Any, Callable, TextIO

_PYTHON_ROOT = Path(__file__).resolve().parent
if str(_PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(_PYTHON_ROOT))


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

    try:
        facade = _import_facade()
    except Exception:
        traceback.print_exc(file=sys.stderr)
        raise

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
        rts = params.get("response_time_sec")
        response_time_sec = float(rts) if isinstance(rts, (int, float)) else None
        # emit があれば進捗 status と生成トークンをイベント行として逐次送出する
        status = (lambda msg: emit({"event": "status", "message": msg})) if emit else None
        on_token = (lambda text: emit({"event": "chunk", "text": text})) if emit else None
        answer = facade.consult(
            query, status=status, on_token=on_token, mode=mode,
            personas=personas, response_time_sec=response_time_sec)
        return {"query": query, "mode": mode, "answer": answer}
    if cmd == "knowledge.fetch_pending":
        return facade.fetch_pending_knowledge()
    if cmd == "calendar.sync":
        source = params["source"]
        mode = params.get("mode", "append")
        if source == "ics" and params.get("ics_files"):
            return facade.sync_calendar_ics_batch(params["ics_files"], mode)
        if source == "ics" and params.get("ics_content"):
            return facade.sync_calendar_ics_content(params["ics_content"], mode)
        if source == "apple":
            return facade.sync_calendar(
                "apple",
                mode,
                db_path=params.get("db_path"),
                days_back=int(params.get("days_back", 365)),
                days_ahead=int(params.get("days_ahead", 365)),
            )
        return facade.sync_calendar(source, mode, ics_path=params.get("ics_path"))
    if cmd == "import.line":
        files = params.get("files")
        if files:
            return facade.import_line_batch(files)
        return facade.import_line_text(
            params["content"],
            params.get("filename", ""),
        )
    if cmd == "settings.run_profiler":
        return facade.run_profiler()
    if cmd == "narrative.compile":
        return facade.compile_narrative(params.get("target_domain"))
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
        try:
            req = json.loads(line)
            req_id = req.get("id")

            def emit_event(payload: dict[str, Any], _id=req_id) -> None:
                _emit({"id": _id, **payload})

            result = dispatch(req["cmd"], req.get("params") or {}, emit=emit_event)
            out: dict[str, Any] = {"id": req_id, "ok": True, "result": result}
        except Exception as exc:  # noqa: BLE001
            traceback.print_exc(file=sys.stderr)
            out = {"id": req_id, "ok": False, "error": f"{type(exc).__name__}: {exc}"}
        _emit(out)


if __name__ == "__main__":
    main()
