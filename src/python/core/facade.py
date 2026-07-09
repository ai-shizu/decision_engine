#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
PKB 共通サービス層
==================
TUI / デスクトップ UI から共有するビジネス操作。
UI 依存は一切含まない。
"""

from __future__ import annotations

import hashlib
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
    ACTIVE_ES,
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
    config: dict | None = None,
) -> str:
    return get_engine().consult(
        query, status=status, on_token=on_token, mode=mode,
        personas=personas, response_time_sec=response_time_sec, config=config)


def last_interview_report() -> dict | None:
    """F4b: 直前の consult() 呼び出しが面接講評 (interview_report.v1) を
    生成していればそれを返す (それ以外は None)。stdio 層が応答へ検証済み
    構造体 (`report`) を含めるかどうかを判定するための薄いアクセサ
    (W-37: UI は JSON.parse を書かない — 構造体はここ経由でのみ渡る)。"""
    return get_engine()._last_interview_report


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
    # F-16 (SPEC_FOXTROT_UI.md §10.2改定): 保持ESは active_es.md ただ1件。
    # レガシーが ES_DIR に物理的に残っていてもカウントには数えない
    # (読み手側整合 — es_manager と同じ「真実の源は ACTIVE_ES のみ」)。
    es_exists = ACTIVE_ES.exists()
    result["es"] = {
        "exists": es_exists,
        "count": 1 if es_exists else 0,
        "mtime": _mtime_iso(ACTIVE_ES),
    }
    knowledge_exists = DATA_KNOWLEDGE.is_dir()
    result["knowledge"] = {
        "exists": knowledge_exists,
        "count": _dir_file_count(DATA_KNOWLEDGE) if knowledge_exists else 0,
        "mtime": _mtime_iso(DATA_KNOWLEDGE),
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


# ============================================================ F2-EXT: 汎用インポート
# SPEC_FOXTROT_UI.md §2.2.2 (Rev.6): 「判別は提案、書き込みは明示」の権限分離。
# classify_document は読み取り専用の純関数。import_document は UI が明示した
# dest (es/knowledge の2値のみ) にのみ書く — バックエンドは自分の推測に基づいて
# 書き込まない。IMP-2 の教訓 (取込 API の冪等性・データ爆弾トリップワイヤ) を
# 汎用口にも適用する。

_ALLOWED_DOCUMENT_EXTENSIONS = {".txt", ".md", ".csv", ".json", ".ics"}
_MAX_DOCUMENT_BYTES = 10 * 1024 * 1024  # データ爆弾トリップワイヤ (IMP の系譜)
_ES_SIGNAL_RE = re.compile(r"志望動機|自己PR|自己ＰＲ|ガクチカ|志望職種|志望業界|応募職種")
_DOCUMENT_DEST_DIRS = {"es": ES_DIR, "knowledge": DATA_KNOWLEDGE}


def classify_document(content: str, filename: str = "") -> dict:
    """決定論的・順序固定・先勝ちの分類 (§2.2.2 裁定1)。読み取り専用の純関数
    — 一切の書き込みを行わない。戻り値の reasons はUIが判定根拠を表示する
    ためのものであり、ブラックボックス化を防ぐ防止線。
    """
    size = len(content.encode("utf-8"))
    ext = Path(filename).suffix.lower()

    if ext not in _ALLOWED_DOCUMENT_EXTENSIONS:
        return {"type": "reject", "filename": filename, "size": size,
                "reasons": [f"拡張子 {ext or '(なし)'} は許可されていません"
                           f" (許可: {', '.join(sorted(_ALLOWED_DOCUMENT_EXTENSIONS))})"]}
    if b"\x00" in content.encode("utf-8")[:8192]:
        return {"type": "reject", "filename": filename, "size": size,
                "reasons": ["先頭8KBにNULバイトを検出 (バイナリ混入の疑い)"]}
    if size > _MAX_DOCUMENT_BYTES:
        return {"type": "reject", "filename": filename, "size": size,
                "reasons": [f"サイズ {size:,} バイトが上限 {_MAX_DOCUMENT_BYTES:,} を超過"]}

    if "[LINE]" in content:
        return {"type": "line", "filename": filename, "size": size,
                "reasons": ['本文に "[LINE]" ヘッダを検出']}
    if "BEGIN:VCALENDAR" in content:
        return {"type": "ics", "filename": filename, "size": size,
                "reasons": ['本文に "BEGIN:VCALENDAR" を検出']}
    es_hits = sorted(set(_ES_SIGNAL_RE.findall(content)))
    if es_hits:
        return {"type": "es", "filename": filename, "size": size,
                "reasons": [f"ES語彙を検出: {', '.join(es_hits)}"]}
    return {"type": "knowledge", "filename": filename, "size": size,
            "reasons": ["LINE/ICS/ESいずれの語彙にも一致せず (既定の受け皿)"]}


def _sanitize_document_filename(filename: str) -> str:
    name = Path(filename).name or "untitled.txt"  # パス成分除去 (トラバーサル対策)
    name = re.sub(r"[^\w.\-ぁ-んァ-ヶ一-鿿]", "_", name)  # 危険文字除去
    return name or "untitled.txt"


def _import_es_document(content: str, *, status: StatusCallback | None = None) -> dict:
    """F-16 (SPEC_FOXTROT_UI.md §10.2改定・指揮官裁定): 保持ESは
    active_es.md ただ1件に収束させる。ES_DIR 内の他ファイル (レガシー) は
    削除しない (破壊操作は禁止 — 読み手側の不可視化 (es_manager) のみで
    単一性を保証する)。
    """
    ES_DIR.mkdir(parents=True, exist_ok=True)
    digest = hashlib.blake2b(content.encode("utf-8"), digest_size=16).hexdigest()
    if ACTIVE_ES.exists():
        try:
            existing_digest = hashlib.blake2b(
                ACTIVE_ES.read_bytes(), digest_size=16).hexdigest()
        except OSError:
            existing_digest = None
        if existing_digest == digest:
            if status:
                status("完了 (同一内容が既に存在するためスキップ)")
            return {
                "imported": False, "skipped": True, "dest": "es",
                "message": "同一内容が既に存在するためスキップしました",
            }

    # write_bytes (write_text ではない): Windows の text モードは "\n"→"\r\n"
    # 変換を行い、read_bytes() ベースの上記ハッシュ比較と食い違って冪等性
    # 判定が壊れる (既知の罠 — import_document 本流と同じ理由)。
    ACTIVE_ES.write_bytes(content.encode("utf-8"))

    if status:
        status("追記完了")
        status("完了")

    return {
        "imported": True, "skipped": False, "dest": "es", "path": ACTIVE_ES.name,
        "message": f"active_es.md へ取り込みました ({ACTIVE_ES.name})",
    }


def active_es() -> dict:
    """F-16 (SPEC_FOXTROT_UI.md §10.2): ImportTab の ES_ACTIVE パネル用View。
    未登録なら {"exists": False}。W-32: filename はログ/永続化に書かない
    (本文は本人の書類なので本人UIへの一時表示は可)。
    """
    from . import es_manager

    doc = es_manager.get_active_es()
    if doc is None:
        return {"exists": False}
    return {
        "exists": True,
        "title": doc["title"],
        "target_domain": doc["target_domain"],
        "keywords": doc["keywords"],
        "body": doc["body"],
        "char_count": doc["char_count"],
        "mtime": _mtime_iso(ACTIVE_ES),
    }


def import_document(
    content: str, filename: str, dest: str, *, status: StatusCallback | None = None,
) -> dict:
    """dest は "es"/"knowledge" の2値ホワイトリストのみ (§2.2.2 裁定1)。
    UI の分類結果を信用せず拒絶ゲートをここで再検証する — バックエンドは
    自分の推測でも UI の確認でも盲信せず、最終防衛はここに置く。
    """
    if dest not in _DOCUMENT_DEST_DIRS:
        raise ValueError(f"不正な dest: {dest} (許可: es, knowledge)")

    classification = classify_document(content, filename)
    if classification["type"] == "reject":
        raise ValueError(f"取込を拒否: {'; '.join(classification['reasons'])}")
    if classification["type"] in ("line", "ics"):
        raise ValueError(
            f"{filename} は {classification['type']} 形式と判定されました — "
            "専用の取込 (LINE/ICS) を使ってください。"
        )

    if status:
        status(f"{filename} を検証中")

    if dest == "es":
        return _import_es_document(content, status=status)

    target_dir = _DOCUMENT_DEST_DIRS[dest]
    target_dir.mkdir(parents=True, exist_ok=True)

    # 冪等性 (T-20 の直接適用): 同一内容が既に存在すれば skip。
    digest = hashlib.blake2b(content.encode("utf-8"), digest_size=16).hexdigest()
    for existing in target_dir.iterdir():
        if not existing.is_file():
            continue
        try:
            existing_digest = hashlib.blake2b(existing.read_bytes(), digest_size=16).hexdigest()
        except OSError:
            continue
        if existing_digest == digest:
            if status:
                status("完了 (同一内容が既に存在するためスキップ)")
            return {
                "imported": False, "skipped": True, "dest": dest,
                "message": "同一内容が既に存在するためスキップしました",
            }

    # 名前衝突は上書きせずハッシュ接尾辞で別名保存する。
    safe_name = _sanitize_document_filename(filename)
    target_path = target_dir / safe_name
    if target_path.exists():
        target_path = target_dir / f"{target_path.stem}_{digest[:8]}{target_path.suffix}"
    # write_bytes (write_text ではない): Windows の text モードは "\n"→"\r\n"
    # 変換を行い、read_bytes() ベースの上記ハッシュ比較と食い違って冪等性
    # 判定が壊れる (実測で踏んだ罠)。
    target_path.write_bytes(content.encode("utf-8"))

    if status:
        status("追記完了")

    result: dict = {"imported": True, "skipped": False, "dest": dest, "path": target_path.name}
    if dest == "knowledge":
        if status:
            status("知識インデックス同期中")
        result["index_rebuilt"] = get_engine().sync_knowledge_index(force=True)
    if status:
        status("完了")
    result["message"] = f"{dest} へ取り込みました ({target_path.name})"
    return result


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
    "active_es",
    "calendar_event_dates",
    "classify_document",
    "compile_narrative",
    "consult",
    "data_source_stats",
    "fetch_pending_knowledge",
    "format_line_import",
    "format_user_profile_summary",
    "get_engine",
    "get_settings",
    "import_document",
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
