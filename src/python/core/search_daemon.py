#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
core/search_daemon.py — C++ 検索エンジン常駐デーモンのクライアント (Target Bravo)
==================================================================================
プロセス生成 + 一時ファイル + stdout 正規表現パースを、常駐デーモンとの
「stdio JSON 制御プレーン + mmap 共有 scratch データプレーン」に置き換える。

C++ 側の唯一の対応物は src/cpp/search_engine.cpp の ScratchBuffer。
レイアウトを変える時は両方を同時に変え、magic のバージョンを上げること
(PKBVEC01 と同規則)。本モジュールは stdlib のみで完結する (numpy 禁止)。

共有 scratch レイアウト (little-endian, 全フィールド自然整列):

  offset    0: char   magic[8]       = "PKBSCR01"
  offset    8: uint64 seq            (Python が書く)
  offset   16: uint32 top_k          (Python が書く, <= SCRATCH_MAX_K)
  offset   20: uint32 result_count   (C++ が書く)
  offset   24: f32    query[384]     (Python が書く)
  offset 1560: (int32 chunk_id, f32 score) × 64   (C++ が書く。未使用は chunk_id=-1)
  合計 2072 bytes

同期モデル (seqlock 簡易版):
  Python が seq / top_k / query を scratch に書く
  → stdio で {"cmd":"search","seq":N,"index":path} を送る
  → C++ が scratch の seq とリクエスト seq の一致を検証して検索
  → results / result_count を書いてから stdio 応答 {"ok":true,"seq":N,...} を返す。
  stdio の 1 行往復がメモリバリアを兼ねるため、ロック不要。

プロトコル規約:
  - リクエストはフラットな JSON オブジェクト 1 行・既知キーのみ・
    ensure_ascii=False + UTF-8 (C++ 側パーサは最小サブセット実装)。
  - デーモンの起動直後は {"event":"ready",...} が 1 行返る。
  - デーモンは stdin の EOF (このプロセスの死 = パイプ切断) で自己終了する。
"""

from __future__ import annotations

import atexit
import json
import mmap
import os
import queue
import struct
import subprocess
import threading
from pathlib import Path

from .artifact_auth import verify_artifact_path
from .paths import PROCESSED, SEARCH_EXE

# ---------------------------------------------------------------- レイアウト定義
SCRATCH_MAGIC = b"PKBSCR01"
DIM = 384
SCRATCH_MAX_K = 64

_HEADER_FMT = "<8sQII"            # magic, seq, top_k, result_count
_QUERY_FMT = f"<{DIM}f"
_RESULT_FMT = "<if"               # chunk_id, score

HEADER_SIZE = struct.calcsize(_HEADER_FMT)                    # 24
QUERY_OFFSET = HEADER_SIZE                                    # 24
QUERY_SIZE = struct.calcsize(_QUERY_FMT)                      # 1536
RESULTS_OFFSET = QUERY_OFFSET + QUERY_SIZE                    # 1560
RESULT_SIZE = struct.calcsize(_RESULT_FMT)                    # 8
SCRATCH_SIZE = RESULTS_OFFSET + RESULT_SIZE * SCRATCH_MAX_K   # 2072

# C++ 側 static_assert と対をなす相互検証。ここが落ちたら struct フォーマット
# 文字列の変更が C++ と非同期になっている。
assert HEADER_SIZE == 24, HEADER_SIZE
assert RESULTS_OFFSET == 1560, RESULTS_OFFSET
assert SCRATCH_SIZE == 2072, SCRATCH_SIZE

_SEQ_OFFSET = struct.calcsize("<8s")          # 8
_TOPK_OFFSET = struct.calcsize("<8sQ")        # 16
_COUNT_OFFSET = struct.calcsize("<8sQI")      # 20

_READY_TIMEOUT_S = 10.0
_RESPONSE_TIMEOUT_S = 30.0


class SearchDaemonError(RuntimeError):
    """デーモン起動・プロトコル・応答の失敗。呼び出し側はこれを捕まえて
    1-shot exe → NumPy のフォールバック連鎖に落とすこと (Step 2 で配線)。"""


class SearchDaemonClient:
    """search_engine --daemon の生存期間と scratch を管理するクライアント。

    - exe 不在時は start() が SearchDaemonError を投げるだけで、他に副作用はない。
    - spawn を差し替えることで実プロセスなしの決定論的テストが可能
      (transport factoryを注入する既存のテストパターン)。
    - atexit で close() を登録し、どの終了経路でもゾンビを残さない
      (owned childの Terminate → Kill パターンを踏襲)。
    """

    def __init__(self, exe: Path | None = None, scratch_path: Path | None = None,
                 spawn=None):
        self.exe = Path(exe) if exe else SEARCH_EXE
        # pid サフィックス: TUI とデスクトップの 2 エンジン同時起動で
        # scratch を取り合わないため (_ce_query_{pid}.bin と同じ規約)
        self.scratch_path = Path(scratch_path) if scratch_path else (
            PROCESSED / f"_ce_shared_scratch_{os.getpid()}.bin")
        self._spawn = spawn or self._default_spawn
        self.proc = None
        self._mm: mmap.mmap | None = None
        self._scratch_file = None
        self._seq = 0
        self._lines: queue.Queue[str | None] = queue.Queue()
        self._reader: threading.Thread | None = None
        self.max_k = SCRATCH_MAX_K
        self._closed = False
        # scratch とリクエスト seq は 1 組しかないため、search/remap は直列化する
        # (TUI は同期ワーカーと consult が別スレッドで走り得る)
        self._lock = threading.Lock()
        atexit.register(self.close)

    # ---- ライフサイクル ----------------------------------------------------
    def _default_spawn(self, args: list[str]):
        # stderr は親を継承 (engine_stdio 経由で engine.log へ)。
        # stdout は制御プレーン専用線。
        return subprocess.Popen(
            args, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            text=True, encoding="utf-8", errors="replace", bufsize=1)

    def start(self) -> None:
        if self.proc is not None and self.proc.poll() is None:
            return
        if not self.exe.exists():
            raise SearchDaemonError(f"search engine not found: {self.exe}")
        verify_artifact_path("search_engine", self.exe)
        self._init_scratch()
        self.proc = self._spawn([str(self.exe), "--daemon", str(self.scratch_path)])
        self._reader = threading.Thread(target=self._read_loop, daemon=True)
        self._reader.start()
        ready = self._next_line(_READY_TIMEOUT_S)
        if ready.get("event") != "ready":
            raise SearchDaemonError(f"unexpected ready line: {ready}")
        self.max_k = min(int(ready.get("max_k", SCRATCH_MAX_K)), SCRATCH_MAX_K)

    def _init_scratch(self) -> None:
        self.scratch_path.parent.mkdir(parents=True, exist_ok=True)
        # 異常終了 (kill 等で atexit が走らなかった) プロセスの残骸を掃除する。
        # 生きているエンジンの scratch は Python/C++ 両方がオープン中のため
        # Windows では削除に失敗し、自然にスキップされる (POSIX では unlink
        # されても両者の既存マッピングは生きるので無害)。
        for stale in self.scratch_path.parent.glob("_ce_shared_scratch_*.bin"):
            if stale != self.scratch_path:
                try:
                    stale.unlink()
                except OSError:
                    pass
        with open(self.scratch_path, "wb") as f:
            f.write(SCRATCH_MAGIC)
            f.write(b"\x00" * (SCRATCH_SIZE - len(SCRATCH_MAGIC)))
        self._scratch_file = open(self.scratch_path, "r+b")
        self._mm = mmap.mmap(self._scratch_file.fileno(), SCRATCH_SIZE)

    def close(self) -> None:
        """shutdown 送信 → Terminate → Kill の段階的終了 + scratch 掃除。"""
        if self._closed:
            return
        self._closed = True
        proc, self.proc = self.proc, None
        if proc is not None and proc.poll() is None:
            try:
                proc.stdin.write('{"cmd":"shutdown"}\n')
                proc.stdin.flush()
                proc.stdin.close()
                proc.wait(timeout=2)
            except (OSError, ValueError, subprocess.TimeoutExpired):
                pass
            if proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    proc.kill()
        if self._mm is not None:
            self._mm.close()
            self._mm = None
        if self._scratch_file is not None:
            self._scratch_file.close()
            self._scratch_file = None
        try:
            self.scratch_path.unlink(missing_ok=True)
        except OSError:
            pass

    # ---- 制御プレーン --------------------------------------------------------
    def _read_loop(self) -> None:
        proc = self.proc
        if proc is None:
            return
        try:
            for line in proc.stdout:
                self._lines.put(line)
        except (OSError, ValueError):
            pass
        self._lines.put(None)   # EOF 番兵

    def _next_line(self, timeout: float) -> dict:
        try:
            line = self._lines.get(timeout=timeout)
        except queue.Empty:
            raise SearchDaemonError("daemon response timeout") from None
        if line is None:
            raise SearchDaemonError("daemon terminated (stdout EOF)")
        try:
            return json.loads(line)
        except json.JSONDecodeError:
            raise SearchDaemonError(f"daemon protocol violation: {line!r}") from None

    def _request(self, payload: dict, timeout: float = _RESPONSE_TIMEOUT_S) -> dict:
        if self.proc is None or self.proc.poll() is not None:
            raise SearchDaemonError("daemon not running")
        line = json.dumps(payload, ensure_ascii=False)
        try:
            self.proc.stdin.write(line + "\n")
            self.proc.stdin.flush()
        except (OSError, ValueError) as e:
            raise SearchDaemonError(f"daemon stdin broken: {e}") from None
        return self._next_line(timeout)

    # ---- データプレーン + 検索 API -------------------------------------------
    def search(self, bin_path: Path, qvec, top_k: int) -> list[tuple[int, float]]:
        """共有 scratch 経由で Top-K 検索。(chunk_id, score) のリストを返す。

        qvec は f32×384 の bytes、または .tobytes() を持つオブジェクト (numpy 配列)。
        metadata との突合は呼び出し側 (consultation_engine) の責務のまま。
        """
        if self._mm is None:
            raise SearchDaemonError("daemon not started")
        qbytes = qvec.tobytes() if hasattr(qvec, "tobytes") else bytes(qvec)
        if len(qbytes) != QUERY_SIZE:
            raise SearchDaemonError(
                f"query must be {QUERY_SIZE} bytes (f32x{DIM}), got {len(qbytes)}")
        k = max(1, min(int(top_k), self.max_k))

        with self._lock:
            self._seq += 1
            mm = self._mm
            # 書き込み順: query / top_k → seq。seq は C++ 側の一致検証に使われるため最後。
            mm[QUERY_OFFSET:QUERY_OFFSET + QUERY_SIZE] = qbytes
            struct.pack_into("<I", mm, _TOPK_OFFSET, k)
            struct.pack_into("<Q", mm, _SEQ_OFFSET, self._seq)

            resp = self._request(
                {"cmd": "search", "seq": self._seq, "index": str(bin_path)})
            if not resp.get("ok"):
                raise SearchDaemonError(f"daemon search failed: {resp.get('error')}")
            if resp.get("seq") != self._seq:
                raise SearchDaemonError(
                    f"response seq mismatch: {resp.get('seq')} != {self._seq}")

            (count,) = struct.unpack_from("<I", mm, _COUNT_OFFSET)
            count = min(count, k)
            hits: list[tuple[int, float]] = []
            for i in range(count):
                cid, score = struct.unpack_from(
                    _RESULT_FMT, mm, RESULTS_OFFSET + i * RESULT_SIZE)
                if cid >= 0:   # -1 = 未使用スロット (PKBVEC01 の墓標規約と同一)
                    hits.append((cid, score))
            return hits

    def remap(self, bin_path: Path) -> None:
        """指定 index のマッピングを解放させる (次の search で遅延リマップ)。

        Windows ではマップ中のファイルは再構築 (上書き) できないため、
        インデックス再構築の【前】に必ず呼ぶこと。"""
        with self._lock:
            resp = self._request({"cmd": "remap", "index": str(bin_path)})
        if not resp.get("ok"):
            raise SearchDaemonError(f"daemon remap failed: {resp.get('error')}")

    def ping(self) -> bool:
        try:
            with self._lock:
                return bool(self._request({"cmd": "ping"}, timeout=5.0).get("ok"))
        except SearchDaemonError:
            return False
