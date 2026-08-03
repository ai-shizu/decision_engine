import {
  initialProbePanelState,
  probeBusy,
  probePanelReducer,
} from "../src/lib/probePanelReducer";
import {
  expectedAbility,
  initialPulseViewState,
  parseRaschStateView,
  pulseViewReducer,
  RASCH_GRID_LEN,
} from "../src/lib/pulseViewReducer";

type TestFn = () => void;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

test("M18D-01 probe submit → next question", () => {
  let s = initialProbePanelState();
  s = probePanelReducer(s, { type: "submit_begin" });
  assertOk(probeBusy(s.phase), "busy");
  s = probePanelReducer(s, {
    type: "submit_success",
    result: {
      schema: "probe_answer_result.v1",
      saved: true,
      node_id: "n1",
      session_status: "active",
      next_question: {
        schema: "probe_question.v1",
        session_id: "s1",
        question_id: "q2",
        axis: "decision_threshold",
        stage: "CONTEXT",
        question: "次は？",
        priority: 0.5,
      },
    },
    status: {
      schema: "probe_status.v1",
      today: "2026-07-20",
      progress_percent: 10,
      completed_stages: 1,
      total_stages: 20,
    },
  });
  assertOk(s.phase === "idle" && s.question?.question_id === "q2", "next q");
});

test("M18D-02 probe submit → complete when no next", () => {
  let s = initialProbePanelState();
  s = probePanelReducer(s, {
    type: "submit_success",
    result: {
      schema: "probe_answer_result.v1",
      saved: true,
      node_id: "n1",
      session_status: "closed",
      next_question: null,
    },
    status: {
      schema: "probe_status.v1",
      today: "2026-07-20",
      progress_percent: 100,
      completed_stages: 20,
      total_stages: 20,
    },
  });
  assertOk(s.phase === "complete" && s.question === null, "complete");
});

test("M18D-03 expected ability at center of uniform", () => {
  const uniform = Array.from({ length: RASCH_GRID_LEN }, () => 1 / RASCH_GRID_LEN);
  const theta = expectedAbility(uniform);
  assertOk(theta !== null && Math.abs(theta) < 1e-9, "theta≈0");
});

test("M18D-04 pulse/rasch reducer + parse", () => {
  let s = initialPulseViewState();
  s = pulseViewReducer(s, { type: "pulse_begin" });
  s = pulseViewReducer(s, {
    type: "pulse_success",
    result: {
      id: "p1",
      analysis: {
        schema: "romance_analysis.v1",
        affinity_score: 72,
        interaction_tendency: "往復が安定",
        next_best_action: "短文で返す",
      },
      metrics: {
        total: 8,
        self_count: 4,
        contact_count: 4,
        switches: 6,
        self_to_contact: 3,
        self_turns_with_successor: 3,
        balance: 0.5,
        switch_rate: 0.7,
        reply_coverage: 0.8,
      },
      input_hash: "abc",
    },
  });
  assertOk(s.pulse?.analysis.affinity_score === 72, "affinity");

  const parsed = parseRaschStateView({
    id: "default",
    created_at: 1,
    artifact_sha256: "deadbeef",
    posterior: Array.from({ length: 17 }, () => 1 / 17),
    excluded: ["pq-a"],
    last_selection: { item_id: "pq-b", eig: 0.1, quantized_eig: 100000 },
  });
  assertOk(parsed?.last_selection?.item_id === "pq-b", "parse");
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
