import {
  StreamTerminalTimeoutError,
  createStreamTerminalGate,
  type StreamTerminalTimers,
} from "../src/lib/streamTerminalGate";

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

function fakeTimers(): {
  timers: StreamTerminalTimers;
  fire(): void;
  cleared(): boolean;
} {
  let callback: (() => void) | null = null;
  let wasCleared = false;
  return {
    timers: {
      set(next) {
        callback = next;
        return 7;
      },
      clear(timerId) {
        assertOk(timerId === 7, "timer identity");
        wasCleared = true;
      },
    },
    fire() {
      callback?.();
    },
    cleared: () => wasCleared,
  };
}

async function main(): Promise<void> {
  {
    const fake = fakeTimers();
    const gate = createStreamTerminalGate(100, fake.timers);
    assertOk(gate.isPending(), "gate starts pending");
    gate.settle();
    await gate.promise;
    assertOk(!gate.isPending(), "terminal retires gate");
    assertOk(fake.cleared(), "terminal clears timeout");
    gate.settle();
  }

  {
    const fake = fakeTimers();
    const gate = createStreamTerminalGate(100, fake.timers);
    fake.fire();
    let timedOut = false;
    try {
      await gate.promise;
    } catch (error) {
      timedOut = error instanceof StreamTerminalTimeoutError;
    }
    assertOk(timedOut, "missing terminal rejects with controlled timeout");
    assertOk(!gate.isPending(), "timeout retires gate");
  }

  {
    const fake = fakeTimers();
    const gate = createStreamTerminalGate(100, fake.timers);
    gate.abort();
    await gate.promise;
    fake.fire();
    assertOk(!gate.isPending(), "invoke rejection aborts without late timeout");
  }

  console.log("PASS stream terminal gate");
}

void main();
