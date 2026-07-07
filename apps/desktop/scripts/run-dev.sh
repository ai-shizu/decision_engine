#!/usr/bin/env bash
# macOS / Linux 用: tauri dev の beforeDevCommand (run-dev.ps1 と等価)
set -euo pipefail
cd "$(dirname "$0")/.."
exec node ./node_modules/vite/bin/vite.js
