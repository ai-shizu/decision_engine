#!/usr/bin/env bash
# macOS / Linux 用: tauri dev の beforeDevCommand (run-dev.ps1 と等価)
set -euo pipefail
cd "$(dirname "$0")/.."

# Collect non-loopback IPv4 addresses currently on this Mac (DHCP can rotate).
local_ips=()
if command -v ifconfig >/dev/null 2>&1; then
  while IFS= read -r ip; do
    [[ -n "$ip" ]] && local_ips+=("$ip")
  done < <(ifconfig 2>/dev/null | awk '/inet / {print $2}' | grep -v '^127\.')
fi

# Physical iOS: Tauri sets TAURI_DEV_HOST to the Mac LAN/TUN IP baked into
# the app's devUrl. Stale IP (DHCP lease change) → "Failed to request
# http://OLD:1420/ … local network permissions?" even when Local Network is ON.
if [[ -n "${TAURI_DEV_HOST:-}" ]]; then
  echo "[pkb-dev] TAURI_DEV_HOST=${TAURI_DEV_HOST}"
  echo "[pkb-dev] Mac IPv4 now: ${local_ips[*]:-none}"
  match=0
  for ip in "${local_ips[@]:-}"; do
    if [[ "$ip" == "$TAURI_DEV_HOST" ]]; then
      match=1
      break
    fi
  done
  # IPv6 TUN (::2) from --force-ip-prompt will not match the IPv4 list — skip warn.
  if [[ $match -eq 0 && "$TAURI_DEV_HOST" != *:* ]]; then
    echo "[pkb-dev] WARNING: TAURI_DEV_HOST=${TAURI_DEV_HOST} is NOT on this Mac."
    echo "[pkb-dev]   DHCP likely rotated the LAN IP. Stop ios dev and relaunch so"
    echo "[pkb-dev]   Tauri re-resolves the host (current en0 is often ${local_ips[0]:-unknown})."
  fi
  echo "[pkb-dev] iPhone: Settings → Privacy → Local Network → Coraxis ON, then relaunch."
fi

exec node ./node_modules/vite/bin/vite.js
