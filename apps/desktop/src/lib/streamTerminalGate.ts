/** Coordinates a Tauri invoke result with its Channel terminal event. */

export class StreamTerminalTimeoutError extends Error {
  constructor() {
    super("stream terminal timeout");
    this.name = "StreamTerminalTimeoutError";
  }
}

export interface StreamTerminalTimers {
  set(callback: () => void, timeoutMs: number): number;
  clear(timerId: number): void;
}

const BROWSER_TIMERS: StreamTerminalTimers = {
  set: (callback, timeoutMs) => window.setTimeout(callback, timeoutMs),
  clear: (timerId) => window.clearTimeout(timerId),
};

export interface StreamTerminalGate {
  readonly promise: Promise<void>;
  isPending(): boolean;
  settle(): void;
  abort(): void;
}

/**
 * Keep frontend stream ownership alive until `done` or `error` is observed.
 * The timeout is a final fail-closed guard against a broken backend/channel
 * contract; `abort` is used when the invoke itself rejects first.
 */
export function createStreamTerminalGate(
  timeoutMs: number,
  timers: StreamTerminalTimers = BROWSER_TIMERS,
): StreamTerminalGate {
  let pending = true;
  let resolvePromise!: () => void;
  let rejectPromise!: (reason: Error) => void;
  const promise = new Promise<void>((resolve, reject) => {
    resolvePromise = resolve;
    rejectPromise = reject;
  });

  const timerId = timers.set(() => {
    if (!pending) return;
    pending = false;
    rejectPromise(new StreamTerminalTimeoutError());
  }, timeoutMs);

  function finish(resolve: boolean): void {
    if (!pending) return;
    pending = false;
    timers.clear(timerId);
    if (resolve) resolvePromise();
  }

  return {
    promise,
    isPending: () => pending,
    settle: () => finish(true),
    // The caller already owns the invoke rejection, so merely retire the gate.
    abort: () => finish(true),
  };
}
