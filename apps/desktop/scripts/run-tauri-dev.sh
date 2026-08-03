#!/usr/bin/env bash
# macOS / Linux: run-tauri-dev.ps1 と等価（Vite beforeDev + Tauri ウィンドウ）
set -euo pipefail
cd "$(dirname "$0")/.."

export PKB_UNSAFE_DEV_ENGINE="${PKB_UNSAFE_DEV_ENGINE:-1}"
echo "WARNING: UNSAFE DEVELOPMENT ENGINE: system Python is not a production security boundary." >&2

if [[ -z "${PKB_PYTHON:-}" ]]; then
  for candidate in python3.12 python3.13 python3.11 python3; do
    if command -v "$candidate" >/dev/null 2>&1; then
      export PKB_PYTHON
      PKB_PYTHON="$(command -v "$candidate")"
      echo "PKB_PYTHON=$PKB_PYTHON"
      break
    fi
  done
fi

exec node ./node_modules/@tauri-apps/cli/tauri.js dev
