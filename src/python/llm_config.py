#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""ローカルLLM (llama.cpp) のモデル選択とサーバー起動引数 (Snapdragon X / ARM64 向け)。"""

from __future__ import annotations

import os
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MODELS_DIR = Path(os.environ.get("PKB_MODELS_DIR", ROOT / "models"))
LLAMA_DIR = Path(os.environ.get("PKB_LLAMA_DIR", ROOT / "tools" / "llama-arm64"))
SERVER_PORT = int(os.environ.get("PKB_LLM_PORT", "18081"))

# 7Bクラスを優先 (未配置時は models/ 内の任意 GGUF にフォールバック)
PREFERRED_7B = MODELS_DIR / "Qwen2.5-7B-Instruct-Q4_K_M.gguf"
PREFERRED_7B_ALT = MODELS_DIR / "qwen2.5-7b-instruct-q4_k_m.gguf"
# Q4_K_M 7B の正常サイズは約 4.4GB 以上。これ未満は不完全ダウンロードとみなす
MIN_7B_BYTES = 4_000_000_000

# Snapdragon X (ARM64): スレッド数は環境変数で上書き可
LLAMA_THREADS = int(os.environ.get("PKB_LLAMA_THREADS", "8"))
LLAMA_CTX = int(os.environ.get("PKB_LLAMA_CTX", "8192"))
LLAMA_BATCH = int(os.environ.get("PKB_LLAMA_BATCH", "512"))


def _is_valid_gguf(path: Path) -> bool:
    if not path.exists():
        return False
    size = path.stat().st_size
    if size < 1_000_000:
        return False
    if "7b" in path.name.lower() and size < MIN_7B_BYTES:
        return False
    return True


def find_gguf() -> Path | None:
    """7B GGUF を最優先 (完全ファイルのみ)。無ければ models/ 内の最大 GGUF。"""
    for p in (PREFERRED_7B, PREFERRED_7B_ALT):
        if _is_valid_gguf(p):
            return p
    if not MODELS_DIR.exists():
        return None
    candidates = [g for g in MODELS_DIR.glob("*.gguf") if _is_valid_gguf(g)]
    candidates.sort(key=lambda p: p.stat().st_size, reverse=True)
    for g in candidates:
        if "7b" in g.name.lower():
            return g
    return candidates[0] if candidates else None


def model_startup_timeout(model: Path) -> int:
    size_gb = model.stat().st_size / (1024 ** 3)
    if size_gb >= 3.5:
        return 300
    if size_gb >= 1.0:
        return 120
    return 90


def llama_server_cmd(exe: Path, model: Path, port: int = SERVER_PORT) -> list[str]:
    """llama-server 起動コマンド (7B向け最適化引数)。遅延初期化: 呼び出し時のみ spawn。"""
    return [
        str(exe), "-m", str(model),
        "--host", "127.0.0.1", "--port", str(port),
        "--ctx-size", str(LLAMA_CTX),
        "--threads", str(LLAMA_THREADS),
        "--batch-size", str(LLAMA_BATCH),
        "--no-webui",
    ]
