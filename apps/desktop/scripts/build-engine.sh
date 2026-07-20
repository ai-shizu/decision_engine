#!/usr/bin/env bash
# Alias: Windows build-engine.ps1 ↔ Unix build-sidecar.sh
set -euo pipefail
exec bash "$(dirname "$0")/build-sidecar.sh"
