//! Unified Coraxis on-device invoke error + typed invoke helper.

import { invoke } from "@tauri-apps/api/core";

export class PocketBrainInvokeError extends Error {
  readonly command: string;
  readonly cause: unknown;

  constructor(command: string, cause: unknown) {
    const message =
      cause instanceof Error
        ? cause.message
        : typeof cause === "string"
          ? cause
          : `invoke failed: ${String(cause)}`;
    super(`[${command}] ${message}`);
    this.name = "PocketBrainInvokeError";
    this.command = command;
    this.cause = cause;
  }
}

export function isPocketBrainInvokeError(
  value: unknown,
): value is PocketBrainInvokeError {
  return value instanceof PocketBrainInvokeError;
}

/**
 * Wrap `@tauri-apps/api/core` invoke with a consistent error type.
 * Never swallows failures — callers must handle `PocketBrainInvokeError`.
 */
export async function pocketInvoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (cause) {
    // Lessons-learned rule (2026-07-24 context-budget hunt): always surface the
    // RAW IPC error before it is re-wrapped into a sterile PocketBrainInvokeError.
    // The generic wrapped message alone cost hours of debugging when the true
    // cause ("prompt exceeds context budget: 2394 > 1792") was fully available.
    // eslint-disable-next-line no-console -- intentional diagnostic (see above)
    console.error(`[pocketInvoke] invoke("${command}") failed:`, cause);
    throw new PocketBrainInvokeError(command, cause);
  }
}
