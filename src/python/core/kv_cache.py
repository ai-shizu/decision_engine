#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
KV キャッシュのプレフィックス・ピニング (Target Alpha)
========================================================
consult プロンプトの静的プレフィックス (システムプロンプト + プロファイル群) の
KV テンソルを llama-server のスロット save/restore API でディスクに永続化し、
TTFT (Time To First Token) から静的部分の prefill を消し去る。

動作原理:
  - llama-server を `--slot-save-path` 付きで起動すると
    POST /slots/{id}?action=save|restore|erase が有効になる。
  - 生成リクエストに id_slot + cache_prompt を付けると、スロット内 KV と
    新プロンプトの共通トークン接頭辞が再利用される。
  - 静的プレフィックスを常にプロンプト先頭に置く限り、restore 済みスロットでは
    動的サフィックス (検索ヒット + 相談文) の prefill だけで生成が始まる。

決定論的パージ:
  - キャッシュの同一性は「SYSTEM_PROMPT + 静的プレフィックス」の BLAKE2b
    ハッシュで判定する。プロファイル更新でテキストが 1 バイトでも変われば
    ハッシュが変わり、旧スロットファイルは破棄・再計算される。
  - 状態は data/processed/kv_slots/state.json に記録 (現在のハッシュのみ)。

【絶対規則】
  - 全操作は best-effort。キャッシュ機構のいかなる失敗も consult を
    失敗させてはならない (キャッシュは高速化であって機能ではない)。
  - スロット非対応の旧 llama-server では自動的に無効化される
    (起動フラグは llm_config.server_supports_slot_save() のプローブで制御)。
"""

from __future__ import annotations

import hashlib
import json
from datetime import datetime
from typing import Callable

from .paths import KV_SLOTS_DIR

STATE_JSON = KV_SLOTS_DIR / "state.json"
SLOT_ID = 0  # 既定起動 (--parallel 1) ではスロットは 0 のみ

# http_post(url, payload) -> (status_code, response_dict)
HttpPost = Callable[[str, dict], tuple[int, dict]]


def prefix_hash(system: str, static_prefix: str) -> str:
    """静的プレフィックスの同一性を示す決定論的ハッシュ。

    チャットテンプレートは system と user 先頭を連結してトークン化するため、
    どちらが変わっても KV は無効 — 両方をハッシュに含める。"""
    h = hashlib.blake2b(digest_size=16)
    h.update(system.encode("utf-8"))
    h.update(b"\x00")
    h.update(static_prefix.encode("utf-8"))
    return h.hexdigest()


def slot_filename(phash: str) -> str:
    return f"pkb-prefix-{phash}.bin"


def load_state() -> dict:
    if not STATE_JSON.exists():
        return {}
    try:
        data = json.loads(STATE_JSON.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return {}
    return data if isinstance(data, dict) else {}


def _save_state(state: dict) -> None:
    KV_SLOTS_DIR.mkdir(parents=True, exist_ok=True)
    STATE_JSON.write_text(
        json.dumps(state, ensure_ascii=False, indent=2), encoding="utf-8")


def purge_stale(current_hash: str) -> int:
    """現在のハッシュ以外のスロットファイルを削除する (KV は巨大になり得る)。"""
    if not KV_SLOTS_DIR.exists():
        return 0
    keep = slot_filename(current_hash)
    removed = 0
    for f in KV_SLOTS_DIR.glob("pkb-prefix-*.bin"):
        if f.name != keep:
            try:
                f.unlink()
                removed += 1
            except OSError:
                pass
    return removed


def _default_http_post(url: str, payload: dict) -> tuple[int, dict]:
    import urllib.error
    import urllib.request
    req = urllib.request.Request(
        url, data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"}, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            body = r.read().decode("utf-8", errors="replace")
            try:
                return r.status, json.loads(body)
            except json.JSONDecodeError:
                return r.status, {}
    except urllib.error.HTTPError as e:
        return e.code, {}
    except OSError:
        return 0, {}  # 接続不能 (0 = ネットワーク層の失敗)


class SlotCacheClient:
    """llama-server スロット API のクライアント (stdlib のみ・best-effort)。

    状態遷移:
      cold     — このプレフィックスの KV は未計算 (生成後に commit で保存)
      restored — ディスクから KV を復元した (動的部分のみ prefill)
      hit      — サーバー稼働中に既ロード済み (何もしない)
      disabled — サーバーがスロット API 非対応 (以後すべてスキップ)
    """

    def __init__(self, port: int, http_post: HttpPost | None = None):
        self.port = port
        self._http_post = http_post or _default_http_post
        self._loaded: str | None = None
        self._disabled = False

    def _post_action(self, action: str, filename: str) -> int:
        url = f"http://127.0.0.1:{self.port}/slots/{SLOT_ID}?action={action}"
        status, _ = self._http_post(url, {"filename": filename})
        if status in (404, 501):
            self._disabled = True  # スロット API 非対応ビルド
        return status

    def ensure_prefix(self, phash: str) -> str:
        """生成前に呼ぶ。プレフィックス KV の状態を返す (副作用: restore)。"""
        if self._disabled:
            return "disabled"
        if self._loaded == phash:
            return "hit"
        state = load_state()
        fname = slot_filename(phash)
        if state.get("hash") == phash and (KV_SLOTS_DIR / fname).exists():
            if self._post_action("restore", fname) == 200:
                self._loaded = phash
                return "restored"
        return "cold"

    def commit_prefix(self, phash: str) -> bool:
        """生成後に呼ぶ。初回 (ハッシュ変化時) のみスロットを保存し、
        旧ハッシュのファイルをパージする。同一ハッシュ 2 回目以降は no-op。"""
        if self._disabled:
            return False
        state = load_state()
        fname = slot_filename(phash)
        if state.get("hash") == phash and (KV_SLOTS_DIR / fname).exists():
            # 永続化済み — 生成によりスロットは温まっているので記録だけ更新
            self._loaded = phash
            return True
        KV_SLOTS_DIR.mkdir(parents=True, exist_ok=True)
        if self._post_action("save", fname) != 200:
            return False
        purge_stale(phash)
        _save_state({
            "hash": phash,
            "filename": fname,
            "saved_at": datetime.now().isoformat(timespec="seconds"),
        })
        self._loaded = phash
        return True
