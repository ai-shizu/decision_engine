//! Unified Pocket Brain invoke error + typed invoke helper.

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
    throw new PocketBrainInvokeError(command, cause);
  }
}
