# -*- coding: utf-8 -*-
"""
llama-server (127.0.0.1) HTTP クライアント — 唯一の所有者。

/health と /v1/chat/completions の実装は本モジュールのみ。
model / generation / server command の正本は llm_config.py。
consultation_engine と cli は本クラスを import するだけとする。
"""

from __future__ import annotations

import json
import subprocess
import sys
import time
from pathlib import Path

from .llm_config import generation_params, llama_server_cmd, model_startup_timeout


class LlamaServerBackend:
    """llama-server (127.0.0.1) 経由の推論。プロセス・通信ともに完全ローカル。

    ライフサイクル管理:
      - 自分が spawn したサーバープロセスのみを終了対象とする
        (既存サーバーを再利用した場合は他所有プロセスを殺さない)
      - stop() は Terminate → 5秒待機 → Kill の段階的終了
      - インスタンス生成時に atexit へ登録し、TUI/CLI がどのような経路で
        終了してもゾンビプロセスを残さない
    """

    name = "llama-server (127.0.0.1, ARM64 native)"

    def __init__(self, exe: Path, model: Path, port: int):
        self.exe, self.model, self.port = exe, model, port
        self.proc: subprocess.Popen | None = None
        self._slot_cache = None  # KV プレフィックス・ピニング (遅延生成)
        import atexit
        atexit.register(self.stop)

    def _port_open(self) -> bool:
        import socket
        with socket.socket() as s:
            s.settimeout(0.3)
            return s.connect_ex(("127.0.0.1", self.port)) == 0

    def start(self, timeout_s: int | None = None) -> None:
        import urllib.request
        if timeout_s is None:
            timeout_s = model_startup_timeout(self.model)
        if self.proc is not None and self.proc.poll() is None:
            return  # 自前サーバーが稼働中
        if self._port_open():
            return  # 既存サーバーを再利用 (所有権なし → stop対象外)
        self.proc = subprocess.Popen(
            llama_server_cmd(self.exe, self.model, self.port),
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        deadline = time.time() + timeout_s
        while time.time() < deadline:
            try:
                with urllib.request.urlopen(
                        f"http://127.0.0.1:{self.port}/health", timeout=2) as r:
                    if json.load(r).get("status") == "ok":
                        return
            except OSError:
                time.sleep(1.0)
        self.stop()
        raise RuntimeError("llama-server の起動がタイムアウトしました")

    def _slot_cache_client(self):
        """KV スロットキャッシュのクライアント (遅延生成・サーバー非対応なら不使用)。"""
        from .llm_config import server_supports_slot_save
        if self._slot_cache is None and server_supports_slot_save(self.exe):
            from .kv_cache import SlotCacheClient
            self._slot_cache = SlotCacheClient(self.port)
        return self._slot_cache

    def generate(self, system: str, user: str, max_tokens: int | None = None,
                 on_token=None, prefix_hash: str | None = None) -> str:
        """on_token が渡された場合は SSE ストリーミングでトークン毎に呼ぶ。

        prefix_hash を渡すと KV プレフィックス・ピニングが有効化される:
        生成前にディスクからスロット復元を試み、生成後 (初回のみ) 保存する。
        キャッシュ操作の失敗は無視される (best-effort — 生成は必ず続行)。
        温度・トークン上限は config/model_params.json (generation) で管理。"""
        import urllib.request
        self.start()
        slot_client = self._slot_cache_client() if prefix_hash else None
        if slot_client is not None:
            try:
                outcome = slot_client.ensure_prefix(prefix_hash)
                print(f"[kv_cache] prefix {prefix_hash[:8]}: {outcome}",
                      file=sys.stderr)
            except Exception:  # noqa: BLE001 — キャッシュ不調で生成を止めない
                slot_client = None
        gen = generation_params()
        body: dict = {
            "messages": [{"role": "system", "content": system},
                         {"role": "user", "content": user}],
            "max_tokens": max_tokens if max_tokens is not None else gen["max_tokens"],
            "temperature": gen["temperature"],
            "stream": on_token is not None,
        }
        if prefix_hash is not None:
            from .kv_cache import SLOT_ID
            body["id_slot"] = SLOT_ID       # 復元したスロットで生成する
            body["cache_prompt"] = True     # 共通トークン接頭辞の再利用を明示
        payload = json.dumps(body).encode("utf-8")
        req = urllib.request.Request(
            f"http://127.0.0.1:{self.port}/v1/chat/completions",
            data=payload, headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=600) as r:
            if on_token is None:
                answer = json.load(r)["choices"][0]["message"]["content"].strip()
            else:
                parts: list[str] = []
                for raw in r:  # SSE: "data: {...}\n" 行を逐次読む
                    line = raw.decode("utf-8", errors="replace").strip()
                    if not line.startswith("data:"):
                        continue
                    data = line[len("data:"):].strip()
                    if data == "[DONE]":
                        break
                    try:
                        delta = json.loads(data)["choices"][0].get("delta", {})
                    except (json.JSONDecodeError, KeyError, IndexError):
                        continue
                    piece = delta.get("content")
                    if piece:
                        parts.append(piece)
                        on_token(piece)
                answer = "".join(parts).strip()
        # 生成成功後にのみ保存 (プレフィックスの KV が確実に温まっている状態)
        if slot_client is not None:
            try:
                slot_client.commit_prefix(prefix_hash)
            except Exception:  # noqa: BLE001 — 保存失敗は高速化の機会損失に過ぎない
                pass
        return answer

    def generate_structured(self, system: str, user: str, json_schema: dict,
                            max_tokens: int | None = None) -> str:
        """Non-streaming JSON-only generation with OpenAI-compatible schema hint."""
        import urllib.request
        self.start()
        gen = generation_params()
        body: dict = {
            "messages": [{"role": "system", "content": system},
                         {"role": "user", "content": user}],
            "max_tokens": max_tokens if max_tokens is not None else gen["max_tokens"],
            "temperature": 0,
            "stream": False,
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "structured_output",
                    "strict": True,
                    "schema": json_schema,
                },
            },
        }
        payload = json.dumps(body).encode("utf-8")
        req = urllib.request.Request(
            f"http://127.0.0.1:{self.port}/v1/chat/completions",
            data=payload, headers={"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=600) as r:
                answer = json.load(r)["choices"][0]["message"]["content"].strip()
        except Exception:
            answer = self.generate(system, user, max_tokens=max_tokens, on_token=None)
        return answer

    def stop(self) -> None:
        """自分が起動したサーバーを確実に終了させる (Terminate → Kill)。"""
        proc, self.proc = self.proc, None
        if proc is None or proc.poll() is not None:
            return
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                pass
