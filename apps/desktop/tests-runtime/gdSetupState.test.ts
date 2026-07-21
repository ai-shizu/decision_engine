import {
  agentCountFromParticipants,
  archetypeToTrait,
  gdSetupReady,
  initialGdSetupConfig,
  syncAgentsToParticipants,
} from "../src/lib/gdSetupState";

type TestFn = () => void;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

test("GD-01 agents = participants - 1", () => {
  const five = syncAgentsToParticipants(5, []);
  assertOk(five.length === 4, "4 agents for 5 seats");
  assertOk(five[0].label === "Agent A", "Agent A");
  assertOk(five[3].label === "Agent D", "Agent D");
  assertOk(agentCountFromParticipants(6) === 5, "6→5");
});

test("GD-02 theme required for ready", () => {
  const cfg = initialGdSetupConfig();
  assertOk(gdSetupReady(cfg) === false, "empty theme blocked");
  cfg.theme = "売上を2倍にする施策";
  assertOk(gdSetupReady(cfg) === true, "theme unlocks");
});

test("GD-03 archetype maps to PRESET traits", () => {
  assertOk(archetypeToTrait("aggressive_crusher") === "クラッシャー", "crusher");
  assertOk(archetypeToTrait("logical_leader") === "論理的", "logical");
  assertOk(archetypeToTrait("passive_harmonizer") === "協調型", "harmonizer");
});

test("GD-04 preserve archetype when growing seats", () => {
  const base = syncAgentsToParticipants(4, []);
  assertOk(base.length === 3, "4 seats → 3 agents");
  base[0].archetype = "framework_zombie";
  const grown = syncAgentsToParticipants(5, base);
  assertOk(grown.length === 4, "5 seats → 4 agents");
  assertOk(grown[0].archetype === "framework_zombie", "preserved");
});

let failed = 0;
for (const t of tests) {
  try {
    t.fn();
    console.log(`PASS ${t.name}`);
  } catch (e) {
    failed += 1;
    console.error(`FAIL ${t.name}`, e);
  }
}
console.log(`RESULT failed=${failed} total=${tests.length}`);
if (failed > 0) {
  throw new Error(`${failed} tests failed`);
}
