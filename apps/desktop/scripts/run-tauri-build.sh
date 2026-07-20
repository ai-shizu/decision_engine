#!/usr/bin/env bash
# macOS / Linux: run-tauri-build.ps1 と等価
set -euo pipefail
cd "$(dirname "$0")/.."

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

echo "=== Step 1/2: Python engine (PyInstaller) ==="
bash ./scripts/build-sidecar.sh

echo "=== Step 2/2: Tauri release bundle ==="
exec node ./node_modules/@tauri-apps/cli/tauri.js build
