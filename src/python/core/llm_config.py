#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""ローカルLLM (llama.cpp) のモデル選択とサーバー起動引数。

モデル戦略・チューニング値は config/model_params.json で管理する:
  - roles.default : 標準モデル (profiler / 汎用) — Qwen2.5-7B-Instruct (IQ4_XS 優先)
  - roles.consult : 相談専用 — DeepSeek-R1-Distill-Qwen-7B 優先
  - policy        : 7B 未満モデルの原則禁止 (min_model_bytes)
  - server / generation : llama-server 起動引数と生成パラメータ

優先順位: 環境変数 > config/model_params.json > 組み込みデフォルト。
config が無い環境 (旧構成) でも組み込みデフォルトで従来同様に動作する。
"""

from __future__ import annotations

import json
import os
import subprocess
from pathlib import Path

from .paths import KV_SLOTS_DIR, LLAMA_DIR, MODELS_DIR, PROJECT_ROOT as ROOT

MODEL_PARAMS_JSON = ROOT / "config" / "model_params.json"

# 7B クラスの正常サイズ下限。IQ4_XS 量子化の 7B は約 3.9GB のため
# 4GB にすると正規モデルを不完全ダウンロード扱いで弾いてしまう。
MIN_7B_BYTES = 3_500_000_000

_DEFAULT_PARAMS: dict = {
    "schema": "model_params.v1",
    "roles": {
        "default": {
            "preferred": [
                "Qwen2.5-7B-Instruct-IQ4_XS.gguf",
                "qwen2.5-7b-instruct-iq4_xs.gguf",
                "Qwen2.5-7B-Instruct-Q4_K_M.gguf",
                "qwen2.5-7b-instruct-q4_k_m.gguf",
            ],
        },
        "consult": {
            "preferred": [
                "DeepSeek-R1-Distill-Qwen-7B*.gguf",
                "deepseek-r1-distill-qwen-7b*.gguf",
            ],
        },
    },
    "policy": {
        # 7B 未満 (≒ 3.5GB 未満) のモデルは原則使用禁止。
        # 検証用に PKB_ALLOW_SMALL_LLM=1 または allow_small_models で解除可。
        "min_model_bytes": MIN_7B_BYTES,
        "allow_small_models": False,
    },
    "server": {"port": 18081, "threads": 8, "ctx": 8192, "batch": 512},
    "generation": {"temperature": 0.6, "max_tokens": 900},
}


def load_model_params() -> dict:
    """config/model_params.json を読み、欠損キーはデフォルトで補完する。"""
    params = json.loads(json.dumps(_DEFAULT_PARAMS))  # deep copy
    if MODEL_PARAMS_JSON.exists():
        try:
            user = json.loads(MODEL_PARAMS_JSON.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, OSError):
            return params
        if isinstance(user, dict):
            for key in ("roles", "policy", "server", "generation"):
                section = user.get(key)
                if isinstance(section, dict):
                    if key == "roles":
                        params["roles"].update(section)
                    else:
                        params[key].update(section)
    return params


def _server_int(env_name: str, config_key: str) -> int:
    env = os.environ.get(env_name)
    if env is not None:
        return int(env)
    return int(load_model_params()["server"][config_key])


# 旧 API 互換の モジュール定数 (環境変数 > config > デフォルト)
SERVER_PORT = _server_int("PKB_LLM_PORT", "port")
LLAMA_THREADS = _server_int("PKB_LLAMA_THREADS", "threads")
LLAMA_CTX = _server_int("PKB_LLAMA_CTX", "ctx")
LLAMA_BATCH = _server_int("PKB_LLAMA_BATCH", "batch")

# 旧 API 互換 (docs / 既存コード参照用)
PREFERRED_7B = MODELS_DIR / "Qwen2.5-7B-Instruct-Q4_K_M.gguf"
PREFERRED_7B_ALT = MODELS_DIR / "qwen2.5-7b-instruct-q4_k_m.gguf"


def _allow_small_models(params: dict) -> bool:
    if os.environ.get("PKB_ALLOW_SMALL_LLM") == "1":
        return True
    return bool(params["policy"].get("allow_small_models"))


def _min_model_bytes(params: dict) -> int:
    return int(params["policy"].get("min_model_bytes", MIN_7B_BYTES))


def _is_valid_gguf(path: Path, params: dict) -> bool:
    if not path.exists():
        return False
    size = path.stat().st_size
    if size < 1_000_000:
        return False
    # 名前が 7B を名乗るのにサイズ下限未満 = 不完全ダウンロード
    if "7b" in path.name.lower() and size < _min_model_bytes(params):
        return False
    return True


def _expand_pattern(pattern: str) -> list[Path]:
    """preferred エントリを実ファイル候補に展開 (glob パターン対応)。"""
    if any(ch in pattern for ch in "*?["):
        if not MODELS_DIR.exists():
            return []
        return sorted(MODELS_DIR.glob(pattern))
    return [MODELS_DIR / pattern]


def find_gguf(role: str = "default") -> Path | None:
    """役割別の優先 GGUF を返す。

    1. roles[role].preferred を順に探す (glob 可)
    2. role が consult 等で見つからなければ default の preferred へフォールバック
    3. それでも無ければ models/ 内でポリシーを満たす最大 GGUF
       (7B 未満は PKB_ALLOW_SMALL_LLM=1 か allow_small_models 無しでは選ばない)
    """
    params = load_model_params()
    roles = params["roles"]

    patterns = list(roles.get(role, {}).get("preferred", []))
    if role != "default":
        for p in roles.get("default", {}).get("preferred", []):
            if p not in patterns:
                patterns.append(p)

    for pattern in patterns:
        for candidate in _expand_pattern(pattern):
            if _is_valid_gguf(candidate, params):
                return candidate

    if not MODELS_DIR.exists():
        return None
    candidates = [
        g for g in MODELS_DIR.glob("*.gguf") if _is_valid_gguf(g, params)
    ]
    if not _allow_small_models(params):
        candidates = [
            g for g in candidates if g.stat().st_size >= _min_model_bytes(params)
        ]
    candidates.sort(key=lambda p: p.stat().st_size, reverse=True)
    for g in candidates:
        if "7b" in g.name.lower():
            return g
    return candidates[0] if candidates else None


def generation_params() -> dict:
    """生成パラメータ (temperature / max_tokens)。"""
    gen = load_model_params()["generation"]
    return {
        "temperature": float(gen.get("temperature", 0.6)),
        "max_tokens": int(gen.get("max_tokens", 900)),
    }


def model_startup_timeout(model: Path) -> int:
    size_gb = model.stat().st_size / (1024 ** 3)
    if size_gb >= 3.5:
        return 300
    if size_gb >= 1.0:
        return 120
    return 90


# --slot-save-path 対応のプローブ結果キャッシュ (exe パス毎)
_SLOT_SUPPORT_CACHE: dict[str, bool] = {}


def server_supports_slot_save(exe: Path) -> bool:
    """llama-server が KV スロット保存 (--slot-save-path) に対応しているか。

    旧ビルドに未知フラグを渡すと起動自体が失敗するため、--help の出力を
    プローブして対応時のみフラグを付与する (完全オフライン・決定論)。"""
    key = str(exe)
    if key not in _SLOT_SUPPORT_CACHE:
        if os.environ.get("PKB_KV_CACHE") == "0":
            _SLOT_SUPPORT_CACHE[key] = False
        else:
            try:
                out = subprocess.run(
                    [str(exe), "--help"], capture_output=True, text=True,
                    timeout=15, encoding="utf-8", errors="replace")
                _SLOT_SUPPORT_CACHE[key] = "--slot-save-path" in (
                    (out.stdout or "") + (out.stderr or ""))
            except (OSError, subprocess.SubprocessError):
                _SLOT_SUPPORT_CACHE[key] = False
    return _SLOT_SUPPORT_CACHE[key]


def llama_server_cmd(exe: Path, model: Path, port: int = SERVER_PORT) -> list[str]:
    """llama-server 起動コマンド (7B向け最適化引数)。遅延初期化: 呼び出し時のみ spawn。"""
    cmd = [
        str(exe), "-m", str(model),
        "--host", "127.0.0.1", "--port", str(port),
        "--ctx-size", str(LLAMA_CTX),
        "--threads", str(LLAMA_THREADS),
        "--batch-size", str(LLAMA_BATCH),
        "--no-webui",
    ]
    if server_supports_slot_save(exe):
        KV_SLOTS_DIR.mkdir(parents=True, exist_ok=True)
        cmd += ["--slot-save-path", str(KV_SLOTS_DIR)]
    return cmd
