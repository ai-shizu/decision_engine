#!/usr/bin/env bash
# ============================================================================
# M19-C — iOS Simulator GGUF model injection (dev/test helper only)
#
# Resolves the booted simulator's data container for com.ai-shizu.pkb, creates
# the M19-B user_data_root models dir, and copies a local GGUF into place as
# pocket-brain.gguf (matches src-tauri MODEL_FILENAME / resolve_model_path).
#
# Usage:
#   bash apps/desktop/scripts/inject_ios_model.sh
#   bash apps/desktop/scripts/inject_ios_model.sh /path/to/model.gguf
#
# Prerequisites:
#   - A simulator is booted
#   - The PKB app (com.ai-shizu.pkb) has been installed at least once on it
# ============================================================================
set -euo pipefail

BUNDLE_ID="com.ai-shizu.pkb"
# Destination filename must match apps/desktop/src-tauri/src/llm/model_path.rs
DEST_FILENAME="pocket-brain.gguf"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DESKTOP_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Colors (disable if not a TTY)
if [[ -t 1 ]]; then
  C_RESET=$'\033[0m'
  C_BOLD=$'\033[1m'
  C_DIM=$'\033[2m'
  C_GREEN=$'\033[32m'
  C_YELLOW=$'\033[33m'
  C_RED=$'\033[31m'
  C_CYAN=$'\033[36m'
else
  C_RESET="" C_BOLD="" C_DIM="" C_GREEN="" C_YELLOW="" C_RED="" C_CYAN=""
fi

log_info()  { printf '%s==>%s %s\n' "${C_CYAN}${C_BOLD}" "${C_RESET}" "$*"; }
log_ok()    { printf '%s[ok]%s  %s\n' "${C_GREEN}${C_BOLD}" "${C_RESET}" "$*"; }
log_warn()  { printf '%s[warn]%s %s\n' "${C_YELLOW}${C_BOLD}" "${C_RESET}" "$*"; }
log_err()   { printf '%s[err]%s  %s\n' "${C_RED}${C_BOLD}" "${C_RESET}" "$*" >&2; }

die() {
  log_err "$*"
  exit 1
}

# ---- Resolve source GGUF ----------------------------------------------------
resolve_source() {
  if [[ $# -ge 1 && -n "${1:-}" ]]; then
    printf '%s\n' "$1"
    return
  fi
  # Prefer the filename the app loads; fall back to the M19-C brief default.
  local candidates=(
    "$DESKTOP_ROOT/models/pocket-brain.gguf"
    "$DESKTOP_ROOT/models/pocketbrain.gguf"
  )
  local c
  for c in "${candidates[@]}"; do
    if [[ -f "$c" ]]; then
      printf '%s\n' "$c"
      return
    fi
  done
  # Documented default when nothing exists yet (clear error path).
  printf '%s\n' "$DESKTOP_ROOT/models/pocketbrain.gguf"
}

SOURCE="$(resolve_source "${1:-}")"

log_info "M19-C iOS Simulator model injection"
printf '     %sbundle%s  %s\n' "${C_DIM}" "${C_RESET}" "$BUNDLE_ID"
printf '     %ssource%s  %s\n' "${C_DIM}" "${C_RESET}" "$SOURCE"

if [[ ! -f "$SOURCE" ]]; then
  die "GGUF not found: $SOURCE
     Pass an explicit path, or place a file at:
       $DESKTOP_ROOT/models/pocket-brain.gguf
       $DESKTOP_ROOT/models/pocketbrain.gguf"
fi

if ! command -v xcrun >/dev/null 2>&1; then
  die "xcrun not found (install Xcode Command Line Tools)"
fi

# ---- 1. Resolve booted simulator data container -----------------------------
log_info "Resolving data container via simctl…"
CONTAINER_RAW="$(
  xcrun simctl get_app_container booted "$BUNDLE_ID" data 2>&1
)" || true

if [[ -z "$CONTAINER_RAW" ]]; then
  die "simctl returned empty path for $BUNDLE_ID (is a simulator booted?)"
fi

# simctl prints the path on success; on failure it prints an error message.
if [[ ! -d "$CONTAINER_RAW" ]]; then
  die "Failed to get app data container for $BUNDLE_ID:
     $CONTAINER_RAW
     Ensure a simulator is booted and the app has been installed once
     (e.g. after \`npm run tauri -- ios dev\`)."
fi

CONTAINER="$CONTAINER_RAW"
log_ok "Container: $CONTAINER"

# ---- 2. Create M19-B models directory ---------------------------------------
# user_data_root (iOS) = $HOME/Library/Application Support/com.ai-shizu.pkb
# Inside the data container, $HOME == container root.
MODELS_DIR="$CONTAINER/Library/Application Support/$BUNDLE_ID/models"
log_info "Ensuring models directory…"
mkdir -p "$MODELS_DIR"
log_ok "Models dir: $MODELS_DIR"

# ---- 3. Copy GGUF as pocket-brain.gguf --------------------------------------
DEST="$MODELS_DIR/$DEST_FILENAME"
log_info "Copying GGUF → $DEST_FILENAME…"
# cp -f: overwrite prior injection without prompt
cp -f "$SOURCE" "$DEST"

BYTES="$(wc -c < "$DEST" | tr -d ' ')"
log_ok "Copied ${BYTES} bytes"
printf '     %sfrom%s  %s\n' "${C_DIM}" "${C_RESET}" "$SOURCE"
printf '     %sto%s    %s\n' "${C_DIM}" "${C_RESET}" "$DEST"

log_ok "Injection complete. Restart the app (or reload the model) on the simulator."
