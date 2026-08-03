/**
 * Phase 14 — Sovereign hard-stop bar (always fixed at viewport bottom).
 * Double-tap arm → confirm within SOVEREIGN_ARM_WINDOW_MS. Crimson only.
 */

import { useEffect, useState } from "react";

import {
  isSovereignArmed,
  resolveSovereignTap,
  SOVEREIGN_ARM_IDLE,
  SOVEREIGN_ARM_WINDOW_MS,
  type SovereignArmState,
} from "../../../lib/sovereignBarLogic";

export function SovereignBar({
  onConfirmHalt,
  disabled = false,
}: {
  onConfirmHalt: () => void;
  disabled?: boolean;
}) {
  const [arm, setArm] = useState<SovereignArmState>(SOVEREIGN_ARM_IDLE);
  const [nowMs, setNowMs] = useState(() => Date.now());

  useEffect(() => {
    if (!arm.armed) return;
    const id = window.setInterval(() => {
      const t = Date.now();
      setNowMs(t);
      if (!isSovereignArmed(arm, t)) {
        setArm(SOVEREIGN_ARM_IDLE);
      }
    }, 200);
    return () => window.clearInterval(id);
  }, [arm]);

  const armed = !disabled && isSovereignArmed(arm, nowMs);
  const remainMs = armed ? Math.max(0, arm.armedUntilMs - nowMs) : 0;

  function handleTap() {
    if (disabled) return;
    const t = Date.now();
    const result = resolveSovereignTap(arm, t, SOVEREIGN_ARM_WINDOW_MS);
    setArm(result.next);
    setNowMs(t);
    if (result.confirmed) {
      onConfirmHalt();
    }
  }

  return (
    <div
      className={
        armed
          ? "coliseum-sovereign coliseum-sovereign-armed"
          : "coliseum-sovereign"
      }
      role="region"
      aria-label="Sovereign hard stop"
    >
      <div className="coliseum-sovereign-meta">
        <span className="coliseum-sovereign-label">SOVEREIGN · HARD STOP</span>
        <span className="coliseum-sovereign-hint">
          {disabled
            ? "HALTED"
            : armed
              ? `ARMED · CONFIRM ${(remainMs / 1000).toFixed(1)}s`
              : "TAP ONCE TO ARM · TAP AGAIN TO HALT"}
        </span>
      </div>
      <button
        type="button"
        className={
          armed
            ? "coliseum-sovereign-btn coliseum-sovereign-btn-armed"
            : "coliseum-sovereign-btn"
        }
        disabled={disabled}
        aria-pressed={armed}
        onClick={handleTap}
      >
        {disabled ? "SURRENDER LOCKED" : armed ? "CONFIRM HALT" : "SURRENDER / HALT"}
      </button>
    </div>
  );
}
