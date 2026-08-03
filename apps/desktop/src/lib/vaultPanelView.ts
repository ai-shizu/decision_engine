/** Pure presentation policy for the secure-vault control surface. */

import type { VaultErrorCode, VaultStatus } from "./parseVault";
import { vaultErrorMessage } from "./vaultErrorMessages";

export type VaultPanelTone = "active" | "locked" | "unlocked" | "error";

/** Hardware HUD semantic color: telemetry / sealed / ready / fault. */
export function vaultPanelTone(
  status: VaultStatus,
  error: VaultErrorCode | null,
): VaultPanelTone {
  if (error !== null) {
    return "error";
  }
  if (status === "unlocked") {
    return "unlocked";
  }
  if (status === "unlocking" || status === "locking") {
    return "active";
  }
  return "locked";
}

/** Fixed terminal envelope; native error material never reaches this string. */
export function vaultSystemErrorLine(code: VaultErrorCode): string {
  return `> SYS_ERR :: [${code.toUpperCase()}] ${vaultErrorMessage(code)}`;
}

/**
 * An IPC snapshot is only a fallback. Once the lifecycle channel has delivered
 * an event, applying the earlier snapshot could rewind a newer lifecycle state.
 */
export function shouldApplyVaultSnapshot(
  revisionAtRequest: number,
  currentRevision: number,
): boolean {
  return revisionAtRequest === currentRevision;
}
