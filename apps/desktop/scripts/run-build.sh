#!/usr/bin/env bash
# macOS / Linux 用: tauri build の beforeBuildCommand (run-build.ps1 と等価)
set -euo pipefail
cd "$(dirname "$0")/.."
node ./node_modules/typescript/bin/tsc
node ./node_modules/vite/bin/vite.js build
