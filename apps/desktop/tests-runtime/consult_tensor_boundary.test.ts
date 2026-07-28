/**
 * Audit finding remediation — interview_report.v1 tensor_profile contract.
 * Boundary tests for parseConsultResponse.ts + tensorProfileView.ts.
 * Harness: sequential registration -> run -> throw if any failure (zero deps).
 */
declare const require: (name: "fs") => {
  readFileSync(path: string, encoding: "utf-8"): string;
};

import {
  ConsultResponseParseError,
  parseConsultResponse,
} from "../src/lib/parseConsultResponse";
import { tensorProfileToRadarData } from "../src/lib/tensorProfileView";
import type {
  InterviewReport,
  TensorDimensionV1,
  TensorProfileReportV1,
} from "../src/lib/types";

type TestFn = () => void;

const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(cond: boolean, msg: string): void {
  if (!cond) throw new Error(msg);
}

function assertEqual(actual: unknown, expected: unknown, msg: string): void {
  if (!deepEqual(actual, expected)) {
    throw new Error(
      `${msg}: expected ${JSON.stringify(expected)} got ${JSON.stringify(actual)}`,
    );
  }
}

function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (typeof a !== typeof b) return false;
  if (a === null || b === null) return a === b;
  if (Array.isArray(a) && Array.isArray(b)) {
    if (a.length !== b.length) return false;
    for (let i = 0; i < a.length; i++) {
      if (!deepEqual(a[i], b[i])) return false;
    }
    return true;
  }
  if (typeof a === "object" && typeof b === "object") {
    const ao = a as Record<string, unknown>;
    const bo = b as Record<string, unknown>;
    const ak = Object.keys(ao).sort();
    const bk = Object.keys(bo).sort();
    if (ak.length !== bk.length) return false;
    for (let i = 0; i < ak.length; i++) {
      if (ak[i] !== bk[i]) return false;
      if (!deepEqual(ao[ak[i]], bo[bk[i]])) return false;
    }
    return true;
  }
  return false;
}

function assertRejects(fn: () => unknown, expectedPathPrefix: string): void {
  let thrown: unknown = undefined;
  try {
    fn();
  } catch (e) {
    thrown = e;
  }
  assertOk(thrown !== undefined, "expected throw");
  assertOk(
    thrown instanceof ConsultResponseParseError,
    `expected ConsultResponseParseError, got ${thrown instanceof Error ? thrown.name + ": " + thrown.message : String(thrown)}`,
  );
  const err = thrown as ConsultResponseParseError;
  assertOk(
    err.path.startsWith(expectedPathPrefix),
    `path prefix: expected startsWith ${JSON.stringify(expectedPathPrefix)} got ${JSON.stringify(err.path)}`,
  );
}

function cloneJson<T>(v: T): T {
  return JSON.parse(JSON.stringify(v)) as T;
}

// ---- fixtures ----

const HEX32_A = "1".repeat(32);
const HEX32_B = "2".repeat(32);

/** problem_structuring evidence yielding a deterministic non-null score/confidence
 * (verified against core/tensor_profile.py via `python -c` during fixture construction):
 * score=0.88, confidence=0.53. */
function measuredDimension(): TensorDimensionV1 {
  return {
    dimension_id: "problem_structuring",
    calculus_axis: "Structural_Decomposition",
    score: 0.88,
    confidence: 0.53,
    evidence: [
      {
        evidence_id: HEX32_A,
        dimension_id: "problem_structuring",
        indicator_id: "ps_clarify_objective",
        level: 3,
        turn_id: "turn-0",
        turn_index: 0,
        speaker_alias: "candidate",
        quote: "quote one",
      },
      {
        evidence_id: HEX32_B,
        dimension_id: "problem_structuring",
        indicator_id: "ps_decompose",
        level: 4,
        turn_id: "turn-1",
        turn_index: 1,
        speaker_alias: "candidate",
        quote: "quote two",
      },
    ],
  };
}

function degenerateDimension(
  dimension_id: TensorDimensionV1["dimension_id"],
  calculus_axis: string,
): TensorDimensionV1 {
  return {
    dimension_id,
    calculus_axis,
    score: null,
    confidence: 0,
    evidence: [],
  };
}

function baseTensorProfile(): TensorProfileReportV1 {
  return {
    schema: "tensor_profile.6d.v1",
    dimensions: [
      measuredDimension(),
      degenerateDimension("quantitative_rigor", "Quantitative_Agility"),
      degenerateDimension("hypothesis_evidence", "Logical_Rigor"),
      degenerateDimension("synthesis_judgment", "Domain_Adaptability"),
      degenerateDimension("communication", "Communication_Bandwidth"),
      degenerateDimension("collaboration_adaptability", "Cognitive_Flexibility"),
    ],
  };
}

function baseReport(): InterviewReport {
  return {
    schema: "interview_report.v1",
    date: "2026-07-11T10:00:00",
    config: { genre: "case" },
    metrics: [
      { axis: "論理性", score: 70, evidence: "evidence text" },
      { axis: "技術力", score: 60, evidence: "evidence text 2" },
    ],
    summary: "summary text",
    latency: { median_sec: 12.3, max_sec: 20.1, n: 3 },
    simulated: true,
    tensor_profile: baseTensorProfile(),
  };
}

function baseEnvelope(mode: "interview_sim" | "gd_sim" = "interview_sim") {
  return {
    query: "candidate answer text",
    mode,
    answer: "interviewer response text",
    report: baseReport(),
  };
}

function validResponse(): unknown {
  return cloneJson(baseEnvelope());
}

// ---- T-01..T-05: envelope-level acceptance ----

test("T-01 accepts valid interview_sim envelope with tensor_profile", () => {
  const out = parseConsultResponse(validResponse());
  assertEqual(out.mode, "interview_sim", "mode");
  assertOk(out.report !== undefined, "report present");
  assertEqual(out.report?.tensor_profile.schema, "tensor_profile.6d.v1", "tensor schema");
});

test("T-02 accepts valid gd_sim envelope with tensor_profile", () => {
  const out = parseConsultResponse(cloneJson(baseEnvelope("gd_sim")));
  assertEqual(out.mode, "gd_sim", "mode");
  assertOk(out.report !== undefined, "report present");
});

test("T-03 accepts consult mode without report/romance_analysis", () => {
  const raw = { query: "q", mode: "consult", answer: "a" };
  const out = parseConsultResponse(raw);
  assertEqual(out.report, undefined, "no report");
  assertEqual(out.romance_analysis, undefined, "no romance");
});

test("T-04 accepts interview_sim without report (mid-conversation turn)", () => {
  const raw = { query: "q", mode: "interview_sim", answer: "a" };
  const out = parseConsultResponse(raw);
  assertEqual(out.report, undefined, "report optional");
});

test("T-05 accepts romance_analysis mode with romance_analysis payload", () => {
  const raw = {
    query: "q",
    mode: "romance_analysis",
    answer: "a",
    romance_analysis: {
      schema: "romance_analysis.v1",
      affinity_score: 42,
      interaction_tendency: "発話数とターン切り替えは概ね均衡しています",
      next_best_action: "同じ形式で履歴を追加して再分析する",
    },
  };
  const out = parseConsultResponse(raw);
  assertOk(out.romance_analysis !== undefined, "romance present");
});

// ---- T-06..T-10: mode gating ----

test("T-06 rejects report present outside interview_sim/gd_sim", () => {
  const raw = { ...baseEnvelope(), mode: "consult" };
  assertRejects(() => parseConsultResponse(raw), "");
});

test("T-07 rejects report present for es_review", () => {
  const raw = { ...baseEnvelope(), mode: "es_review" };
  assertRejects(() => parseConsultResponse(raw), "");
});

test("T-08 rejects romance_analysis present outside romance_analysis mode", () => {
  const raw = {
    query: "q",
    mode: "consult",
    answer: "a",
    romance_analysis: {
      schema: "romance_analysis.v1",
      affinity_score: null,
      interaction_tendency: "t",
      next_best_action: "t",
    },
  };
  assertRejects(() => parseConsultResponse(raw), "");
});

test("T-09 rejects unknown mode literal", () => {
  const raw = { query: "q", mode: "not_a_mode", answer: "a" };
  assertRejects(() => parseConsultResponse(raw), "mode");
});

test("T-10 rejects extra top-level key", () => {
  const raw = { ...baseEnvelope("gd_sim") as Record<string, unknown>, extra: 1 };
  delete (raw as { report?: unknown }).report;
  assertRejects(() => parseConsultResponse(raw), "");
});

// ---- T-11..T-16: tensor_profile field-level rejection ----

test("T-11 rejects tensor_profile missing entirely", () => {
  const report = baseReport() as unknown as Record<string, unknown>;
  delete report.tensor_profile;
  const raw = { ...baseEnvelope(), report };
  // caught by the exact-key-set check on `report` itself (tensor_profile is
  // a required key of interview_report.v1) — same precedent as
  // parseManifest.ts's missing-required-key rejection.
  assertRejects(() => parseConsultResponse(raw), "report");
});

test("T-12 rejects tensor_profile schema violation", () => {
  const report = baseReport();
  (report.tensor_profile as { schema: string }).schema = "tensor_profile.6d.v2";
  assertRejects(() => parseConsultResponse({ ...baseEnvelope(), report }), "report.tensor_profile.schema");
});

test("T-13 rejects dimension field missing", () => {
  const report = baseReport();
  const dim0 = report.tensor_profile.dimensions[0] as unknown as Record<string, unknown>;
  delete dim0.confidence;
  assertRejects(() => parseConsultResponse({ ...baseEnvelope(), report }), "report.tensor_profile.dimensions[0]");
});

test("T-14 rejects extra key on dimension", () => {
  const report = baseReport();
  (report.tensor_profile.dimensions[0] as unknown as Record<string, unknown>).extra = 1;
  assertRejects(() => parseConsultResponse({ ...baseEnvelope(), report }), "report.tensor_profile.dimensions[0]");
});

test("T-15 rejects bool as score", () => {
  const report = baseReport();
  (report.tensor_profile.dimensions[1] as unknown as Record<string, unknown>).score = true;
  assertRejects(() => parseConsultResponse({ ...baseEnvelope(), report }), "report.tensor_profile.dimensions[1].score");
});

test("T-16 rejects NaN/Infinity confidence", () => {
  const report = baseReport();
  (report.tensor_profile.dimensions[0] as unknown as Record<string, unknown>).confidence = Infinity;
  assertRejects(() => parseConsultResponse({ ...baseEnvelope(), report }), "report.tensor_profile.dimensions[0].confidence");
});

// ---- T-17..T-20: range / order / duplication ----

test("T-17 rejects score out of [0,1] range", () => {
  const report = baseReport();
  (report.tensor_profile.dimensions[0] as unknown as Record<string, unknown>).score = 1.01;
  assertRejects(() => parseConsultResponse({ ...baseEnvelope(), report }), "report.tensor_profile.dimensions[0].score");
});

test("T-18 rejects dimensions array with fewer than 6 entries", () => {
  const report = baseReport();
  report.tensor_profile.dimensions.pop();
  assertRejects(() => parseConsultResponse({ ...baseEnvelope(), report }), "report.tensor_profile.dimensions");
});

test("T-19 rejects duplicate dimension_id", () => {
  const report = baseReport();
  report.tensor_profile.dimensions[1] = cloneJson(report.tensor_profile.dimensions[0]);
  assertRejects(() => parseConsultResponse({ ...baseEnvelope(), report }), "report.tensor_profile.dimensions[1].dimension_id");
});

test("T-20 rejects out-of-order dimensions", () => {
  const report = baseReport();
  const dims = report.tensor_profile.dimensions;
  [dims[0], dims[1]] = [dims[1], dims[0]];
  assertRejects(() => parseConsultResponse({ ...baseEnvelope(), report }), "report.tensor_profile.dimensions[0].dimension_id");
});

// ---- T-21..T-26: evidence-level rejection ----

test("T-21 rejects evidence dimension_id mismatch with parent", () => {
  const report = baseReport();
  (report.tensor_profile.dimensions[0].evidence[0] as unknown as Record<string, unknown>).dimension_id =
    "quantitative_rigor";
  assertRejects(
    () => parseConsultResponse({ ...baseEnvelope(), report }),
    "report.tensor_profile.dimensions[0].evidence[0].dimension_id",
  );
});

test("T-22 rejects duplicate indicator_id within one dimension", () => {
  const report = baseReport();
  report.tensor_profile.dimensions[0].evidence[1].indicator_id = "ps_clarify_objective";
  assertRejects(
    () => parseConsultResponse({ ...baseEnvelope(), report }),
    "report.tensor_profile.dimensions[0].evidence",
  );
});

test("T-23 rejects indicator_id outside allowlist for dimension", () => {
  const report = baseReport();
  report.tensor_profile.dimensions[0].evidence[0].indicator_id = "qr_units_assumptions";
  assertRejects(
    () => parseConsultResponse({ ...baseEnvelope(), report }),
    "report.tensor_profile.dimensions[0].evidence[0].indicator_id",
  );
});

test("T-24 rejects level outside 0..4", () => {
  const report = baseReport();
  (report.tensor_profile.dimensions[0].evidence[0] as unknown as Record<string, unknown>).level = 5;
  assertRejects(
    () => parseConsultResponse({ ...baseEnvelope(), report }),
    "report.tensor_profile.dimensions[0].evidence[0].level",
  );
});

test("T-25 rejects non-hex32 evidence_id", () => {
  const report = baseReport();
  report.tensor_profile.dimensions[0].evidence[0].evidence_id = "not-hex";
  assertRejects(
    () => parseConsultResponse({ ...baseEnvelope(), report }),
    "report.tensor_profile.dimensions[0].evidence[0].evidence_id",
  );
});

test("T-26 rejects non-candidate speaker on first-five dimension", () => {
  const report = baseReport();
  report.tensor_profile.dimensions[0].evidence[0].speaker_alias = "面接官";
  assertRejects(
    () => parseConsultResponse({ ...baseEnvelope(), report }),
    "report.tensor_profile.dimensions[0].evidence[0].speaker_alias",
  );
});

// ---- T-27..T-30: accounting hard validation ----

test("T-27 rejects score present when validity condition unmet (single indicator)", () => {
  const report = baseReport();
  report.tensor_profile.dimensions[0].evidence.pop(); // only 1 indicator now
  // score/confidence still claim the 2-indicator values -> must be rejected
  assertRejects(
    () => parseConsultResponse({ ...baseEnvelope(), report }),
    "report.tensor_profile.dimensions[0].score",
  );
});

test("T-28 rejects score null when validity condition is met", () => {
  const report = baseReport();
  (report.tensor_profile.dimensions[0] as unknown as Record<string, unknown>).score = null;
  assertRejects(
    () => parseConsultResponse({ ...baseEnvelope(), report }),
    "report.tensor_profile.dimensions[0].score",
  );
});

test("T-29 rejects score value mismatch vs recomputed accounting", () => {
  const report = baseReport();
  report.tensor_profile.dimensions[0].score = 0.5;
  assertRejects(
    () => parseConsultResponse({ ...baseEnvelope(), report }),
    "report.tensor_profile.dimensions[0].score",
  );
});

test("T-30 rejects confidence value mismatch vs recomputed accounting", () => {
  const report = baseReport();
  report.tensor_profile.dimensions[0].confidence = 0.99;
  assertRejects(
    () => parseConsultResponse({ ...baseEnvelope(), report }),
    "report.tensor_profile.dimensions[0].confidence",
  );
});

// ---- T-31: error must not leak violating values ----

test("T-31 parser error does not leak violating value or quote text", () => {
  const report = baseReport();
  report.tensor_profile.dimensions[0].evidence[0].quote = "SECRET_QUOTE_VALUE_MUST_NOT_LEAK";
  (report.tensor_profile.dimensions[0] as unknown as Record<string, unknown>).confidence = "SECRET_BAD_TYPE_STRING";
  try {
    parseConsultResponse({ ...baseEnvelope(), report });
    throw new Error("expected throw");
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    assertOk(!msg.includes("SECRET_QUOTE_VALUE_MUST_NOT_LEAK"), "quote must not leak");
    assertOk(!msg.includes("SECRET_BAD_TYPE_STRING"), "violating value must not leak");
  }
});

// ---- T-32: pure UI transform ----

test("T-32 tensorProfileToRadarData preserves fixed order and null-as-null", () => {
  const out = parseConsultResponse(validResponse());
  if (out.report === undefined) throw new Error("unreachable");
  const radar = tensorProfileToRadarData(out.report.tensor_profile);
  assertEqual(radar.length, 6, "radar length");
  assertEqual(radar[0].id, "problem_structuring", "order[0]");
  assertEqual(radar[5].id, "collaboration_adaptability", "order[5]");
  assertEqual(radar[0].value, 0.88, "measured value preserved");
  assertEqual(radar[1].value, null, "null preserved, not coerced to 0");
});

// ---- T-33: cross-boundary golden ----

test("T-33 accepts Python golden fixture (real generate_report shape)", () => {
  const fs = require("fs");
  let text: string;
  try {
    text = fs.readFileSync(".boundary-tests-out/tensor_golden.json", "utf-8");
  } catch {
    throw new Error("golden fixture missing: .boundary-tests-out/tensor_golden.json");
  }
  const raw: unknown = JSON.parse(text);
  const out = parseConsultResponse(raw);
  assertOk(out.report !== undefined, "golden report present");
  if (out.report === undefined) throw new Error("unreachable");
  assertEqual(out.report.tensor_profile.dimensions.length, 6, "golden dimensions length");
  assertEqual(out.report.tensor_profile.schema, "tensor_profile.6d.v1", "golden tensor schema");
});

// ---- T-34..T-43: report.config strict field-by-field reconstruction ----
// (review remediation 1: bare `as` cast replaced by parseInterviewConfig)

test("T-34 accepts empty config object", () => {
  const report = baseReport();
  (report as unknown as { config: unknown }).config = {};
  const out = parseConsultResponse({ ...baseEnvelope(), report });
  assertEqual(out.report?.config, {}, "empty config");
});

test("T-35 accepts partial config (gd_sim genre-only shape)", () => {
  const report = baseReport();
  (report as unknown as { config: unknown }).config = { genre: "case" };
  const out = parseConsultResponse({ ...baseEnvelope("gd_sim"), report });
  assertEqual(out.report?.config, { genre: "case" }, "partial config");
});

test("T-36 accepts full config with empty-string fields (UI blank = valid)", () => {
  const report = baseReport();
  (report as unknown as { config: unknown }).config = {
    industry: "",
    genre: "",
    difficulty: "standard",
    stance: "adversarial",
  };
  const out = parseConsultResponse({ ...baseEnvelope(), report });
  assertEqual(
    out.report?.config,
    { industry: "", genre: "", difficulty: "standard", stance: "adversarial" },
    "full config with blanks",
  );
});

test("T-37 rejects extra key on config", () => {
  const report = baseReport();
  (report as unknown as { config: unknown }).config = { industry: "it", extra: 1 };
  assertRejects(() => parseConsultResponse({ ...baseEnvelope(), report }), "report.config");
});

test("T-38 rejects non-string industry field", () => {
  const report = baseReport();
  (report as unknown as { config: unknown }).config = { industry: 42 };
  assertRejects(() => parseConsultResponse({ ...baseEnvelope(), report }), "report.config.industry");
});

test("T-39 rejects unknown difficulty enum value", () => {
  const report = baseReport();
  (report as unknown as { config: unknown }).config = { difficulty: "impossible" };
  assertRejects(() => parseConsultResponse({ ...baseEnvelope(), report }), "report.config.difficulty");
});

test("T-40 rejects unknown stance enum value", () => {
  const report = baseReport();
  (report as unknown as { config: unknown }).config = { stance: "friendly" };
  assertRejects(() => parseConsultResponse({ ...baseEnvelope(), report }), "report.config.stance");
});

test("T-41 rejects explicit undefined field value", () => {
  const report = baseReport();
  (report as unknown as { config: unknown }).config = { industry: undefined };
  assertRejects(() => parseConsultResponse({ ...baseEnvelope(), report }), "report.config.industry");
});

test("T-42 output config is a fresh reference, not the raw object", () => {
  const report = baseReport();
  const rawConfig = { genre: "case" };
  (report as unknown as { config: unknown }).config = rawConfig;
  const out = parseConsultResponse({ ...baseEnvelope(), report });
  assertOk(out.report?.config !== rawConfig, "config must be reconstructed, not the same reference");
});

test("T-43 config violation error does not leak the bad value", () => {
  const report = baseReport();
  (report as unknown as { config: unknown }).config = { industry: "SECRET_BAD_INDUSTRY_VALUE" as unknown, difficulty: "SECRET_BAD_DIFFICULTY" };
  try {
    parseConsultResponse({ ...baseEnvelope(), report });
    throw new Error("expected throw");
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    assertOk(!msg.includes("SECRET_BAD_DIFFICULTY"), "bad config value must not leak");
  }
});

// ---- T-44..T-54: romance_analysis strict allowlist + required-when-mode ----
// (review remediation 2/3: fixed tendency/action allowlist, null<->insufficient
// coupling, and required-when-romance_analysis-mode)

function romanceEnvelope(payload: unknown) {
  const env: Record<string, unknown> = {
    query: "q",
    mode: "romance_analysis",
    answer: "a",
  };
  if (payload !== undefined) env.romance_analysis = payload;
  return env;
}

const INSUFFICIENT_TENDENCY = "判定に必要な観測量が不足しています";
const INSUFFICIENT_ACTION = "会話履歴を追加して再分析する";
const ALLOWED_TENDENCY_0 = "発話数とターン切り替えは概ね均衡しています";
const ALLOWED_ACTION_0 = "同じ形式で履歴を追加して再分析する";

function insufficientRomance(): Record<string, unknown> {
  return {
    schema: "romance_analysis.v1",
    affinity_score: null,
    interaction_tendency: INSUFFICIENT_TENDENCY,
    next_best_action: INSUFFICIENT_ACTION,
  };
}

function measuredRomance(): Record<string, unknown> {
  return {
    schema: "romance_analysis.v1",
    affinity_score: 42,
    interaction_tendency: ALLOWED_TENDENCY_0,
    next_best_action: ALLOWED_ACTION_0,
  };
}

test("T-44 rejects romance_analysis mode with payload missing entirely", () => {
  assertRejects(() => parseConsultResponse(romanceEnvelope(undefined)), "");
});

test("T-45 rejects romance schema violation", () => {
  const payload = { ...insufficientRomance(), schema: "romance_analysis.v2" };
  assertRejects(() => parseConsultResponse(romanceEnvelope(payload)), "romance_analysis.schema");
});

test("T-46 rejects extra key on romance payload", () => {
  const payload = { ...insufficientRomance(), extra: 1 };
  assertRejects(() => parseConsultResponse(romanceEnvelope(payload)), "romance_analysis");
});

test("T-47 rejects tendency outside fixed allowlist", () => {
  const payload = { ...measuredRomance(), interaction_tendency: "not a real tendency" };
  assertRejects(
    () => parseConsultResponse(romanceEnvelope(payload)),
    "romance_analysis.interaction_tendency",
  );
});

test("T-48 rejects action outside fixed allowlist", () => {
  const payload = { ...measuredRomance(), next_best_action: "not a real action" };
  assertRejects(() => parseConsultResponse(romanceEnvelope(payload)), "romance_analysis.next_best_action");
});

test("T-49 rejects bool/float affinity_score", () => {
  const payload1 = { ...measuredRomance(), affinity_score: true };
  assertRejects(() => parseConsultResponse(romanceEnvelope(payload1)), "romance_analysis.affinity_score");
  const payload2 = { ...measuredRomance(), affinity_score: 42.5 };
  assertRejects(() => parseConsultResponse(romanceEnvelope(payload2)), "romance_analysis.affinity_score");
});

test("T-50 rejects affinity_score out of 0..100 range", () => {
  const payload = { ...measuredRomance(), affinity_score: 101 };
  assertRejects(() => parseConsultResponse(romanceEnvelope(payload)), "romance_analysis.affinity_score");
});

test("T-51 rejects score=null paired with a normal (non-insufficient) tendency/action", () => {
  const payload = {
    schema: "romance_analysis.v1",
    affinity_score: null,
    interaction_tendency: ALLOWED_TENDENCY_0,
    next_best_action: INSUFFICIENT_ACTION,
  };
  assertRejects(() => parseConsultResponse(romanceEnvelope(payload)), "romance_analysis");
});

test("T-52 rejects non-null score paired with insufficient tendency/action", () => {
  const payload = {
    schema: "romance_analysis.v1",
    affinity_score: 42,
    interaction_tendency: INSUFFICIENT_TENDENCY,
    next_best_action: ALLOWED_ACTION_0,
  };
  assertRejects(() => parseConsultResponse(romanceEnvelope(payload)), "romance_analysis");
});

test("T-53 accepts valid insufficient-observation payload", () => {
  const out = parseConsultResponse(romanceEnvelope(insufficientRomance()));
  assertOk(out.romance_analysis !== undefined, "romance present");
  assertEqual(out.romance_analysis?.affinity_score, null, "score null");
});

test("T-54 accepts valid measured payload", () => {
  const out = parseConsultResponse(romanceEnvelope(measuredRomance()));
  assertOk(out.romance_analysis !== undefined, "romance present");
  assertEqual(out.romance_analysis?.affinity_score, 42, "score 42");
});

// ---- run ----

let failed = 0;
for (const t of tests) {
  try {
    t.fn();
    console.log(`PASS ${t.name}`);
  } catch (e) {
    failed += 1;
    const msg = e instanceof Error ? e.message : String(e);
    console.log(`FAIL ${t.name}: ${msg}`);
  }
}
console.log(`RESULT failed=${failed} total=${tests.length}`);
if (failed > 0) {
  throw new Error(`${failed} tests failed`);
}
