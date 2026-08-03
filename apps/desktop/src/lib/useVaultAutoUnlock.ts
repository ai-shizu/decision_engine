//! App-level Vault auto-unlock (friction removal).
//!
//! On app launch and on each foreground restore, if the secure vault is
//! provisioned-but-`locked`, invoke the OS biometric (Face ID / Touch ID)
//! automatically — no manual button press — and let the worker's lifecycle
//! event stream carry the resulting `unlocked` status to the VaultPanel UI.
//!
//! Lives at the App level (always mounted, next to `useForegroundRestore`) so it
//! fires at launch regardless of the active surface. On mobile the VaultPanel
//! itself only mounts on the Settings surface, so it cannot own this.
//!
//! Armed once per launch / foreground: a cancelled/failed Face ID or a
//! deliberate manual lock is never re-spammed (bare visibilitychange does NOT
//! re-arm). Only a genuine `locked` status is auto-attempted — never
//! `unprovisioned` (first run) or `unavailable`/`quarantined` (error states).
//! The VaultPanel's manual "ロック解除" button remains the explicit fallback.

import { useEffect, useRef } from "react";

import { FOREGROUND_RESTORE_EVENT } from "./foregroundRestore";
import { vaultStatus, vaultUnlock } from "./vault";

export function useVaultAutoUnlock(enabled: boolean): void {
  const armed = useRef(true); // armed on mount (launch)
  const inFlight = useRef(false);

  useEffect(() => {
    if (!enabled) {
      return;
    }
    let active = true;

    async function attempt(): Promise<void> {
      if (!armed.current || inFlight.current) {
        return;
      }
      armed.current = false; // one biometric attempt per arm
      inFlight.current = true;
      try {
        const status = await vaultStatus();
        if (active && status === "locked") {
          await vaultUnlock(); // OS biometric; worker pushes the status event
        }
      } catch {
        // Cancelled / unavailable — stay locked; manual button is the fallback.
      } finally {
        inFlight.current = false;
      }
    }

    void attempt(); // launch attempt

    function onRestore(): void {
      armed.current = true; // real background→foreground transition re-arms
      void attempt();
    }
    window.addEventListener(FOREGROUND_RESTORE_EVENT, onRestore);
    return () => {
      active = false;
      window.removeEventListener(FOREGROUND_RESTORE_EVENT, onRestore);
    };
  }, [enabled]);
}
