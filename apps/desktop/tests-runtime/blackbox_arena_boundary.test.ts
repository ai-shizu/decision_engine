/**
 * Boundary harness for BLACKBOX Arena parsers / intent / reducer.
 * Zero React deps — run via `npm run test:boundary`.
 */

import {
  buildAbstain,
  buildSetPrice,
  BlackboxIntentError,
  INTENT_KINDS,
} from "../src/lib/blackboxIntent";
import {
  blackboxArenaReducer,
  initialBlackboxArenaState,
} from "../src/lib/blackboxArenaReducer";
import {
  parseAdvanceView,
  parseArenaLimitsView,
  parseObservationView,
  parseSessionState,
  parseSimUiErrorCode,
  SIM_UI_ERROR_CODES,
  type ArenaLimitsView,
  type ObservationView,
} from "../src/lib/parseBlackboxArena";
import { formatLaneValue } from "../src/lib/blackboxProfileView";

type TestFn = () => void;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

function expectReject(fn: () => unknown): void {
  let rejected = false;
  try {
    fn();
  } catch {
    rejected = true;
  }
  assertOk(rejected, "expected strict parser rejection");
}

const LIMITS: ArenaLimitsView = {
  minPriceMinor: 1,
  maxPriceMinor: 10_000_000,
  maxOrderUnits: 1_000_000,
  maxActionAmountMinor: 1_000_000_000_000,
  maxSkus: 3,
  campaignTicks: 52,
  ticksPerQuarter: 13,
};

function sampleObservation(
  overrides: Partial<ObservationView> = {},
): ObservationView {
  return {
    campaignId: "abcd".repeat(16),
    turnsCompleted: 0,
    market: {
      tick: 0,
      regime: "calm",
      commodityPriceMinor: 100,
      equityIndexCenti: 10000,
      demandIndexMicro: 1_000_000,
      rateBp: 100,
      jumpOccurred: false,
    },
    stimuli: [],
    books: {
      cashMinor: 1_000_000,
      inventoryValueMinor: 0,
      seniorDebtMinor: 0,
      mezzanineDebtMinor: 0,
      skus: [
        {
          sku: 0,
          unitPriceMinor: 100,
          inventoryUnits: 0,
          inventoryValueMinor: 0,
        },
      ],
      projects: [],
      offers: [],
      positions: [],
    },
    limits: LIMITS,
    state: { active: { phase: "decide" } },
    ...overrides,
  };
}

test("BXS-01 session state narrows all four shapes", () => {
  assertOk(parseSessionState("genesis") === "genesis", "genesis");
  assertOk(parseSessionState("sealed") === "sealed", "sealed");
  const active = parseSessionState({ active: { phase: "decide" } });
  assertOk(
    typeof active === "object" &&
      "active" in active &&
      active.active.phase === "decide",
    "active",
  );
  const dead = parseSessionState({
    dead: { reason: "accounting_breach" },
  });
  assertOk(
    typeof dead === "object" &&
      "dead" in dead &&
      dead.dead.reason === "accounting_breach",
    "dead",
  );
  expectReject(() => parseSessionState({ active: { phase: "nope" } }));
  expectReject(() => parseSessionState({ weird: true }));
});

test("BXS-02 SimUiErrorCode accepts closed set and falls back", () => {
  for (const code of SIM_UI_ERROR_CODES) {
    assertOk(parseSimUiErrorCode(code) === code, code);
  }
  assertOk(parseSimUiErrorCode("not_a_code") === "unknown", "unknown");
  assertOk(parseSimUiErrorCode(42) === "unknown", "non-string");
});

test("BXS-03 observation parser rejects unknown keys", () => {
  const raw = {
    campaignId: "x",
    turnsCompleted: 0,
    market: {
      tick: 0,
      regime: "calm",
      commodityPriceMinor: 1,
      equityIndexCenti: 1,
      demandIndexMicro: 1,
      rateBp: 1,
      jumpOccurred: false,
    },
    stimuli: [],
    books: {
      cashMinor: 0,
      inventoryValueMinor: 0,
      seniorDebtMinor: 0,
      mezzanineDebtMinor: 0,
      skus: [],
      projects: [],
      offers: [],
      positions: [],
    },
    limits: LIMITS,
    state: "sealed",
  };
  const ok = parseObservationView(raw);
  assertOk(ok.campaignId === "x", "ok parse");
  expectReject(() => parseObservationView({ ...raw, leak: true }));
  expectReject(() =>
    parseObservationView({
      ...raw,
      market: { ...raw.market, extra: 1 },
    }),
  );
});

test("BXS-04 limits parser is exact", () => {
  const limits = parseArenaLimitsView(LIMITS);
  assertOk(limits.campaignTicks === 52, "ticks");
  expectReject(() => parseArenaLimitsView({ ...LIMITS, extra: 1 }));
  expectReject(() =>
    parseArenaLimitsView({ ...LIMITS, maxSkus: "3" as unknown as number }),
  );
});

test("BXS-05 advance parser handles null nextObservation", () => {
  const advance = parseAdvanceView({
    tick: 51,
    revenueMinor: 10,
    cogsMinor: 5,
    unmetUnits: 0,
    periodClose: null,
    state: "sealed",
    nextObservation: null,
  });
  assertOk(advance.nextObservation === null, "sealed next");
  assertOk(advance.state === "sealed", "sealed state");
});

test("BXS-06 intent builders produce wire shapes and reject OOB", () => {
  const price = buildSetPrice(0, 100, LIMITS);
  assertOk(
    typeof price === "object" &&
      "set_price" in price &&
      price.set_price.sku === 0 &&
      price.set_price.tickPrice === 100,
    "set_price wire",
  );
  assertOk(buildAbstain() === "abstain", "abstain");
  let threw = false;
  try {
    buildSetPrice(9, 100, LIMITS);
  } catch (e) {
    threw = e instanceof BlackboxIntentError;
  }
  assertOk(threw, "sku OOB");
  threw = false;
  try {
    buildSetPrice(0, 0, LIMITS);
  } catch (e) {
    threw = e instanceof BlackboxIntentError;
  }
  assertOk(threw, "price below min");
  assertOk(INTENT_KINDS.length === 13, "13 player intents");
  assertOk(
    !INTENT_KINDS.includes("forced_default" as never),
    "no forced_default",
  );
});

test("BXS-07 reducer lifecycle start → advance → sealed clears busy", () => {
  let state = initialBlackboxArenaState();
  const obs = sampleObservation();
  state = blackboxArenaReducer(state, {
    type: "campaign_started",
    observation: obs,
  });
  assertOk(state.campaignId === obs.campaignId, "campaign id");
  assertOk(state.busy === false, "not busy after start");

  state = blackboxArenaReducer(state, { type: "submit_begin" });
  assertOk(state.busy === true, "busy on submit");

  state = blackboxArenaReducer(state, {
    type: "submit_ok",
    outcome: { ledgerEffect: false, journalSeq: null },
  });
  assertOk(state.busy === true, "still busy after submit_ok");

  state = blackboxArenaReducer(state, {
    type: "advance_ok",
    advance: {
      tick: 0,
      revenueMinor: 1,
      cogsMinor: 0,
      unmetUnits: 0,
      periodClose: null,
      state: "sealed",
      nextObservation: null,
    },
  });
  assertOk(state.busy === false, "busy cleared");
  assertOk(state.sealed === true, "sealed");
  assertOk(state.turnLog.length === 1, "log entry");
});

test("BXS-08 command_failed clears busy", () => {
  let state = blackboxArenaReducer(initialBlackboxArenaState(), {
    type: "submit_begin",
  });
  state = blackboxArenaReducer(state, {
    type: "command_failed",
    message: "x",
  });
  assertOk(state.busy === false && state.error === "x", "failure path");
});

test("R9 unmeasured lane never formats as bare zero", () => {
  const unmeasured = {
    lane: 0,
    axis: "loss_aversion",
    labelJa: "損失回避",
    valueMicro: null as number | null,
    nObs: 0,
    sufficiencyMicro: 0,
    measured: false,
  };
  assertOk(formatLaneValue(unmeasured) === "未測定", "null → 未測定");
  assertOk(formatLaneValue(unmeasured) !== "0", "must not coerce to 0");
  const measured = { ...unmeasured, measured: true, valueMicro: 0, nObs: 1 };
  assertOk(formatLaneValue(measured) === "0.000000", "measured zero stays numeric");
});

let failed = 0;
for (const { name, fn } of tests) {
  try {
    fn();
    console.log(`PASS ${name}`);
  } catch (error: unknown) {
    failed += 1;
    console.error(`FAIL ${name}`, error);
  }
}
console.log(`RESULT failed=${failed} total=${tests.length}`);
if (failed > 0) {
  throw new Error(`${failed} tests failed`);
}
