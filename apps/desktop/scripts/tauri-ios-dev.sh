#!/usr/bin/env bash
# Physical-device / simulator iOS HMR entrypoint (T4-B-2).
#
# Always merges src-tauri/tauri.ios.dev.conf.json so ATS / Local Network /
# Vite HMR CSP stay on the *dev* path only. Release (`tauri ios build`) must
# NOT pass this overlay — see tauri.ios.conf.json (devCsp/devUrl nulled).
#
# Usage (from apps/desktop):
#   bash scripts/tauri-ios-dev.sh --features pocket-brain,secure-vault
#   npm run tauri:ios-dev -- --features pocket-brain,secure-vault
set -euo pipefail
cd "$(dirname "$0")/.."
exec npx --no-install tauri ios dev \
  --config src-tauri/tauri.ios.dev.conf.json \
  "$@"
