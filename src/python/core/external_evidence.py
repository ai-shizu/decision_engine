# -*- coding: utf-8 -*-
"""E0b external evidence — sanitize, isolated persist, prompt render.

Parent: docs/SPEC_E0B_STEP6_INTEGRATION.md
Scope E cage: no subprocess/ctypes/network/dynamic exec.
"""
from __future__ import annotations

import hashlib
import json
import re
import unicodedata
from typing import Any

from core.durable_persistence import durable_atomic_write_text, read_json_file
from core.paths import DATA_KNOWLEDGE

EXTERNAL_KNOWLEDGE_DIR = DATA_KNOWLEDGE / "external"
SCHEMA = "pkb.external_knowledge.v1"
ORIGIN = "wikipedia"
MAX_ITEMS = 3
MAX_TITLE_BYTES = 256
MAX_CONTENT_BYTES = 2048
_HEX64 = re.compile(r"^[0-9a-f]{64}$")

# Structural / tokenizer trigger → visible fullwidth (NFKC-stable away from ASCII).
_CHAR_MAP: dict[str, str] = {
    "<": "＜",
    ">": "＞",
    "|": "｜",
    "`": "｀",
    "~": "～",
    "[": "［",
    "]": "］",
    "(": "（",
    ")": "）",
    "!": "！",
    "\\": "＼",
    "/": "／",
    "&": "＆",
    "*": "＊",
    "_": "＿",
    "#": "＃",
}

_REMOVE_CPS: frozenset[int] = frozenset(
    {
        *range(0x00, 0x09),
        *range(0x0E, 0x20),
        *range(0x7F, 0xA0),
        0x00AD,
        0x061C,
        0x180E,
        0x200B,
        0x200C,
        0x200D,
        0x200E,
        0x200F,
        0x2028,
        0x2029,
        0x202A,
        0x202B,
        0x202C,
        0x202D,
        0x202E,
        0x2060,
        0x2061,
        0x2062,
        0x2063,
        0x2064,
        0x2066,
        0x2067,
        0x2068,
        0x2069,
        0xFEFF,
    }
)

_NAMED_ENTITIES: dict[str, str] = {
    "amp": "&",
    "lt": "<",
    "gt": ">",
    "quot": '"',
    "apos": "'",
    "nbsp": " ",
}


class E0bRejected(ValueError):
    """Fixed sentinel only — never embed query/title/content/path/URL."""

    def __init__(self, code: str = "E0B_VALIDATION_REJECTED") -> None:
        super().__init__(code)


def _utf8_truncate(s: str, max_bytes: int) -> str:
    raw = s.encode("utf-8")
    if len(raw) <= max_bytes:
        return s
    cut = raw[:max_bytes]
    while cut:
        try:
            return cut.decode("utf-8")
        except UnicodeDecodeError:
            cut = cut[:-1]
    return ""


def _decode_entities(s: str) -> str:
    out: list[str] = []
    i = 0
    n = len(s)
    while i < n:
        if s[i] != "&":
            out.append(s[i])
            i += 1
            continue
        semi = s.find(";", i + 1)
        if semi < 0 or semi - i > 16:
            out.append(s[i])
            i += 1
            continue
        body = s[i + 1 : semi]
        repl: str | None = None
        if body.startswith("#x") or body.startswith("#X"):
            try:
                cp = int(body[2:], 16)
                if 0 <= cp <= 0x10FFFF and not (0xD800 <= cp <= 0xDFFF):
                    repl = chr(cp)
            except ValueError:
                repl = None
        elif body.startswith("#"):
            try:
                cp = int(body[1:], 10)
                if 0 <= cp <= 0x10FFFF and not (0xD800 <= cp <= 0xDFFF):
                    repl = chr(cp)
            except ValueError:
                repl = None
        else:
            repl = _NAMED_ENTITIES.get(body)
        if repl is None:
            out.append(s[i])
            i += 1
            continue
        out.append(repl)
        i = semi + 1
    return "".join(out)


def _strip_tags(s: str) -> str:
    out: list[str] = []
    i = 0
    n = len(s)
    while i < n:
        if s[i] != "<":
            out.append(s[i])
            i += 1
            continue
        j = i + 1
        while j < n and j - i < 64 and s[j] != ">":
            j += 1
        if j < n and s[j] == ">":
            i = j + 1
            continue
        out.append(s[i])
        i += 1
    return "".join(out)


def sanitize_external_text(text: str, *, max_bytes: int) -> str:
    """Neutralize Markdown / HTML / special-token triggers (render-time safe)."""
    if type(text) is not str:
        raise E0bRejected()
    for ch in text:
        cp = ord(ch)
        if 0xD800 <= cp <= 0xDFFF:
            raise E0bRejected()

    s = text
    for _ in range(4):
        nxt = _decode_entities(s)
        if nxt == s:
            break
        s = nxt
    s = _strip_tags(s)
    s = unicodedata.normalize("NFKC", s)

    cleaned: list[str] = []
    for ch in s:
        cp = ord(ch)
        if cp in _REMOVE_CPS:
            continue
        cleaned.append(_CHAR_MAP.get(ch, ch))
    s = "".join(cleaned)
    s = _utf8_truncate(s, max_bytes)
    if not s.strip():
        raise E0bRejected()
    return s


def render_external_evidence(record: dict[str, Any]) -> str:
    """Render one external record into a single prompt lane.

    External text is untrusted. Structural and tokenizer controls are physically
    neutralized and the data cannot trigger further egress, but the fundamental
    RAG limitation remains: plausible plain-language prompt injection can never
    be reduced to zero. The design limits blast radius; it does not prove
    semantic truth or model non-manipulation.
    """
    if type(record) is not dict:
        raise E0bRejected()
    items = record.get("items")
    if type(items) is not list or not items:
        return ""
    rid = record.get("research_id", "")
    origin = record.get("origin", "")
    blocks: list[str] = []
    for item in items:
        if type(item) is not dict:
            raise E0bRejected()
        title = sanitize_external_text(str(item.get("title", "")), max_bytes=MAX_TITLE_BYTES)
        content = sanitize_external_text(
            str(item.get("content", "")), max_bytes=MAX_CONTENT_BYTES
        )
        ordinal = item.get("ordinal", 0)
        digest = item.get("content_sha256", "")
        # One render unit: provenance + text stay in the same block (no ## split).
        blocks.append(
            f"(UNTRUSTED external evidence research_id={rid} origin={origin} "
            f"ordinal={ordinal} digest={digest})\n"
            f"title={title}\ncontent={content}"
        )
    return "\n\n".join(blocks)


def _require_hex64(value: object, *, field: str) -> str:
    if type(value) is not str or not _HEX64.match(value):
        raise E0bRejected()
    return value


def _require_u64(value: object) -> int:
    if type(value) is not int or isinstance(value, bool) or value < 0 or value >= (1 << 64):
        raise E0bRejected()
    return value


def integrate_external_results(params: dict[str, Any]) -> dict[str, Any]:
    """Strict-validate → one atomic sidecar record. 0-or-all."""
    if type(params) is not dict:
        raise E0bRejected()
    allowed = {
        "research_id",
        "origin",
        "policy_epoch",
        "sidecar_generation",
        "dict_hash",
        "fetched_at_unix_ms",
        "query",
        "items",
    }
    if set(params.keys()) != allowed:
        raise E0bRejected()

    research_id = _require_hex64(params["research_id"], field="research_id")
    if params["origin"] != ORIGIN:
        raise E0bRejected()
    policy_epoch = _require_u64(params["policy_epoch"])
    sidecar_generation = _require_u64(params["sidecar_generation"])
    dict_hash = _require_hex64(params["dict_hash"], field="dict_hash")
    fetched_at = _require_u64(params["fetched_at_unix_ms"])
    query = params["query"]
    if type(query) is not str or not query.strip():
        raise E0bRejected()
    query_sha256 = hashlib.sha256(query.encode("utf-8")).hexdigest()

    raw_items = params["items"]
    if type(raw_items) is not list or not raw_items or len(raw_items) > MAX_ITEMS:
        raise E0bRejected()

    items: list[dict[str, Any]] = []
    for idx, raw in enumerate(raw_items):
        if type(raw) is not dict:
            raise E0bRejected()
        if set(raw.keys()) != {"source", "title", "content"}:
            raise E0bRejected()
        if raw["source"] != ORIGIN:
            raise E0bRejected()
        title = sanitize_external_text(raw["title"], max_bytes=MAX_TITLE_BYTES)
        content = sanitize_external_text(raw["content"], max_bytes=MAX_CONTENT_BYTES)
        digest = hashlib.sha256(content.encode("utf-8")).hexdigest()
        items.append(
            {
                "ordinal": idx,
                "source": ORIGIN,
                "title": title,
                "content": content,
                "content_sha256": digest,
            }
        )

    record = {
        "schema": SCHEMA,
        "research_id": research_id,
        "origin": ORIGIN,
        "policy_epoch": policy_epoch,
        "sidecar_generation": sidecar_generation,
        "dict_hash": dict_hash,
        "fetched_at_unix_ms": fetched_at,
        "query_sha256": query_sha256,
        "items": items,
    }
    payload = json.dumps(record, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n"
    path = EXTERNAL_KNOWLEDGE_DIR / f"ext_{research_id}.json"
    if path.is_file():
        existing = path.read_text(encoding="utf-8")
        if existing != payload:
            raise E0bRejected("E0B_RECORD_CONFLICT")
        return {"results_persisted": len(items)}
    durable_atomic_write_text(path, payload)
    return {"results_persisted": len(items)}


def load_external_record(research_id: str) -> dict[str, Any]:
    rid = _require_hex64(research_id, field="research_id")
    path = EXTERNAL_KNOWLEDGE_DIR / f"ext_{rid}.json"
    try:
        data = read_json_file(path)
    except FileNotFoundError as exc:
        raise E0bRejected("E0B_EXTERNAL_CONTEXT_UNAVAILABLE") from exc
    if type(data) is not dict or data.get("schema") != SCHEMA:
        raise E0bRejected("E0B_EXTERNAL_CONTEXT_UNAVAILABLE")
    return data


def list_external_research_ids() -> list[str]:
    EXTERNAL_KNOWLEDGE_DIR.mkdir(parents=True, exist_ok=True)
    ids: list[str] = []
    for path in sorted(EXTERNAL_KNOWLEDGE_DIR.glob("ext_*.json")):
        name = path.name
        if not name.startswith("ext_") or not name.endswith(".json"):
            continue
        rid = name[4:-5]
        if _HEX64.match(rid):
            ids.append(rid)
    return ids


def delete_external_record(research_id: str) -> bool:
    rid = _require_hex64(research_id, field="research_id")
    path = EXTERNAL_KNOWLEDGE_DIR / f"ext_{rid}.json"
    if not path.is_file():
        return False
    path.unlink()
    return True
