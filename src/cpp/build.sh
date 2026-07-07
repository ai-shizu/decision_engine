#!/usr/bin/env bash
# C++ 検索コア単体ビルド (macOS / Linux)。
# 実体はリポジトリルートの build.sh に集約している (二重管理禁止)。
# ここからはパイプラインを飛ばして C++ コンパイルのみ実行する。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
exec bash "$ROOT/build.sh" --skip-py "$@"
