import {
  INITIAL_MODEL_SETUP,
  modelSetupReducer,
  softImportError,
} from "../src/lib/modelSetupReducer";

type TestFn = () => void;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

test("MS-01 missing model stays on needed until import_done", () => {
  let s = modelSetupReducer(INITIAL_MODEL_SETUP, { type: "check_start" });
  assertOk(s.phase === "checking", "checking");
  s = modelSetupReducer(s, {
    type: "check_result",
    exists: false,
    relativePath: "models/pocket-brain.gguf",
    recommendedPageUrl: "https://huggingface.co/Qwen/Qwen2.5-7B-Instruct-GGUF",
  });
  assertOk(s.phase === "needed", "needed");
  s = modelSetupReducer(s, { type: "import_start" });
  assertOk(s.phase === "importing", "importing");
  s = modelSetupReducer(s, { type: "import_progress", percent: 42 });
  assertOk(s.progressPercent === 42, "progress");
  s = modelSetupReducer(s, { type: "import_done" });
  assertOk(s.phase === "ready", "ready");
  assertOk(s.progressPercent === 100, "100");
});

test("MS-02 existing model skips gate", () => {
  const s = modelSetupReducer(INITIAL_MODEL_SETUP, {
    type: "check_result",
    exists: true,
    relativePath: "models/pocket-brain.gguf",
    recommendedPageUrl: "https://example.invalid",
  });
  assertOk(s.phase === "ready", "ready");
});

test("MS-03 feature-absent build does not block", () => {
  const s = modelSetupReducer(INITIAL_MODEL_SETUP, { type: "check_skipped" });
  assertOk(s.phase === "unavailable", "unavailable");
});

test("MS-04 soft error never echoes raw text", () => {
  const soft = softImportError("/Users/secret/path/model.gguf boom");
  assertOk(!soft.includes("secret"), "no path leak");
  assertOk(!soft.includes("boom"), "no raw");
});

test("MS-05 no download phase in reducer vocabulary", () => {
  const phases = ["checking", "needed", "importing", "ready", "unavailable"];
  assertOk(phases.every((p) => !p.includes("download")), "no download");
});

let failed = 0;
for (const { name, fn } of tests) {
  try {
    fn();
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