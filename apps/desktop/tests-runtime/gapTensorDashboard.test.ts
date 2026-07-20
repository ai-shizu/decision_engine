import {
  parseGapPayload,
  sufficiencyLabel,
} from "../src/lib/gapPayloadView";
import {
  buildDaysFromRecords,
  gapTensorDashboardReducer,
  initialGapTensorDashboardState,
} from "../src/lib/gapTensorDashboardReducer";
import { pocketTensorToRadarData } from "../src/lib/pocketBrainTensorView";
import type { TensorProfile } from "../src/lib/pocketBrain/types";
import type { RecordData } from "../src/lib/types";

type TestFn = () => void;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

const sampleTensor: TensorProfile = {
  schema: "tensor_profile.6d.v1",
  model_hash: "no-llm-authority",
  dimensions: [
    {
      dimension_id: "problem_structuring",
      calculus_axis: "Structural_Decomposition",
      score: null,
      confidence: 0,
    },
    {
      dimension_id: "quantitative_rigor",
      calculus_axis: "Quantitative_Agility",
      score: null,
      confidence: 0,
    },
    {
      dimension_id: "hypothesis_evidence",
      calculus_axis: "Logical_Rigor",
      score: null,
      confidence: 0,
    },
    {
      dimension_id: "synthesis_judgment",
      calculus_axis: "Domain_Adaptability",
      score: null,
      confidence: 0,
    },
    {
      dimension_id: "communication",
      calculus_axis: "Communication_Bandwidth",
      score: null,
      confidence: 0,
    },
    {
      dimension_id: "collaboration_adaptability",
      calculus_axis: "Cognitive_Flexibility",
      score: null,
      confidence: 0,
    },
  ],
  evidence: [],
};

test("M18C-01 radar has six N/A points", () => {
  const data = pocketTensorToRadarData(sampleTensor);
  assertOk(data.length === 6, "six dims");
  assertOk(data.every((d) => d.value === null), "all null");
});

test("M18C-02 parse gap payload", () => {
  const view = parseGapPayload({
    schema: "gap_analysis.v3",
    data_sufficiency: 0.55,
    subjective_scores: { career: { score: 0.8 } },
    objective_scores: { career: { score: 0.2, money_spent: 1000 } },
    gaps: [
      {
        type: "intention_gap",
        theme: "career",
        gap: 0.6,
        insight: "test",
      },
    ],
  });
  assertOk(view.dataSufficiency === 0.55, "suff");
  assertOk(view.gaps.length === 1 && view.gaps[0].type === "intention_gap", "gap");
  assertOk(sufficiencyLabel(0.55) === "部分的", "label");
});

test("M20L-03 records build days (no manual draft)", () => {
  const records: RecordData[] = [
    {
      date: "2026-07-20",
      diary: "転職したい",
      events: [{ time: "10:00", title: "面接" }],
      transactions: [{ type: "expense", category: "books", amount: 3000 }],
    },
    {
      date: "2026-07-19",
      diary: "",
      events: [],
      transactions: [],
    },
  ];
  const days = buildDaysFromRecords(records);
  assertOk(days.length === 1, "empty day dropped");
  assertOk(days[0].diaryText === "転職したい", "diary");
  assertOk(days[0].transactions?.length === 1, "tx");
  assertOk(days[0].calendarEvents?.length === 1, "cal");
});

test("M18C-04 reducer load/recalc", () => {
  let s = initialGapTensorDashboardState();
  s = gapTensorDashboardReducer(s, { type: "load_begin" });
  assertOk(s.phase === "loading", "loading");
  s = gapTensorDashboardReducer(s, {
    type: "load_success",
    tensor: sampleTensor,
    gap: null,
  });
  assertOk(s.tensor?.schema === "tensor_profile.6d.v1", "tensor");
  s = gapTensorDashboardReducer(s, { type: "recalc_begin" });
  s = gapTensorDashboardReducer(s, {
    type: "recalc_success",
    result: {
      id: "gap-1",
      data_sufficiency: 0.4,
      gap_count: 0,
      gaps: [],
      languageization_prompt: "",
      payload: { schema: "gap_analysis.v3", gaps: [] },
    },
    tensor: sampleTensor,
    gap: {
      id: "gap-1",
      created_at: 1,
      data_sufficiency: 0.4,
      payload: { schema: "gap_analysis.v3", gaps: [] },
    },
    dayCount: 3,
  });
  assertOk(s.gap?.id === "gap-1" && s.phase === "idle", "recalc done");
  assertOk(s.lastRecalcDayCount === 3, "day count");
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
