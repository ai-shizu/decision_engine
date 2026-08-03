import { pollForFlavor } from "../src/lib/flavorPoll";

type TestFn = () => Promise<void> | void;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

/** Deterministic clock: sleeps resolve immediately and are only counted. */
function harness(takes: (string | null)[], cancelAfter = Number.MAX_SAFE_INTEGER) {
  const state = { calls: 0, sleeps: 0 };
  return {
    state,
    deps: {
      take: () => {
        const v = takes[state.calls] ?? null;
        state.calls += 1;
        return Promise.resolve(v);
      },
      sleep: () => {
        state.sleeps += 1;
        return Promise.resolve();
      },
      cancelled: () => state.calls >= cancelAfter,
    },
  };
}

test("FP-01 empty slot then prose — the measured device shape", async () => {
  // ~267ms generation against a 200ms interval: the first pull misses, the
  // second finds it. A single pull is exactly what shipped and always lost it.
  const h = harness([null, "静かな朝だった。"]);
  const got = await pollForFlavor(h.deps, 12, 200);
  assertOk(got === "静かな朝だった。", `got ${String(got)}`);
  assertOk(h.state.calls === 2, `calls=${h.state.calls}`);
});

test("FP-02 gives up after the budget instead of polling forever", async () => {
  const h = harness([]);
  const got = await pollForFlavor(h.deps, 4, 10);
  assertOk(got === null, "null after budget");
  assertOk(h.state.calls === 4, `calls=${h.state.calls}`);
  // No trailing sleep after the final attempt.
  assertOk(h.state.sleeps === 3, `sleeps=${h.state.sleeps}`);
});

test("FP-03 first attempt hit costs no sleep", async () => {
  const h = harness(["もう届いていた。"]);
  const got = await pollForFlavor(h.deps, 12, 200);
  assertOk(got === "もう届いていた。", "immediate");
  assertOk(h.state.sleeps === 0, `sleeps=${h.state.sleeps}`);
});

test("FP-04 cancellation stops the loop and issues no further take", async () => {
  // `take` consumes the slot, so a superseded poll that keeps pulling would
  // swallow the next tick's value — the failure this check exists to prevent.
  const h = harness([null, null, "遅れて来た。"], 2);
  const got = await pollForFlavor(h.deps, 12, 10);
  assertOk(got === null, "cancelled yields null");
  assertOk(h.state.calls === 2, `no take after cancel, calls=${h.state.calls}`);
});

test("FP-05 a take that rejects propagates rather than looping", async () => {
  let calls = 0;
  let threw = false;
  try {
    await pollForFlavor(
      {
        take: () => {
          calls += 1;
          return Promise.reject(new Error("ipc down"));
        },
        sleep: () => Promise.resolve(),
        cancelled: () => false,
      },
      12,
      10,
    );
  } catch {
    threw = true;
  }
  assertOk(threw, "error surfaces to the caller");
  assertOk(calls === 1, `stopped at first failure, calls=${calls}`);
});

void (async () => {
  let failed = 0;
  for (const { name, fn } of tests) {
    try {
      await fn();
      console.log(`PASS ${name}`);
    } catch (err) {
      failed += 1;
      console.error(`FAIL ${name}`, err);
    }
  }
  console.log(`RESULT failed=${failed} total=${tests.length}`);
  if (failed > 0) {
    throw new Error(`${failed} tests failed`);
  }
})();
