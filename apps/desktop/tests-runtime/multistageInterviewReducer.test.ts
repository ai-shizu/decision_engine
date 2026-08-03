import {
  coerceInterviewStage,
  companyFactsReady,
  emptyCompanyFacts,
  isInterviewStage,
  stageIndex,
} from "../src/lib/interviewStage";
import {
  initialMultistageInterviewState,
  multistageInterviewReducer,
} from "../src/lib/multistageInterviewReducer";
import {
  esReviewReducer,
  initialEsReviewState,
} from "../src/lib/esReviewReducer";

type TestFn = () => void;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

test("M18B-01 stage helpers", () => {
  assertOk(isInterviewStage("pressure"), "pressure");
  assertOk(!isInterviewStage("warmup"), "invalid");
  assertOk(coerceInterviewStage("debrief") === "debrief", "coerce");
  assertOk(stageIndex("closed") === 3, "index");
  assertOk(companyFactsReady(emptyCompanyFacts({ companyName: "A" })), "ready");
  assertOk(!companyFactsReady(emptyCompanyFacts()), "not ready");
});

test("M18B-02 start begin → success advances to foundation", () => {
  let s = initialMultistageInterviewState();
  s = multistageInterviewReducer(s, {
    type: "start_begin",
    assistantId: "a1",
  });
  assertOk(s.streaming && s.messages.length === 1, "streaming bubble");
  s = multistageInterviewReducer(s, {
    type: "start_success",
    assistantId: "a1",
    result: {
      session_id: "iv-1",
      stage: "foundation",
      status: "active",
      turn_in_stage: 0,
      total_turns: 1,
      context_ids: [],
      company_name: "Co",
      outcome: "started",
    },
  });
  assertOk(s.sessionId === "iv-1" && s.stage === "foundation", "meta");
  assertOk(s.messages[0].streaming === false, "stream done");
});

test("M18B-03 closed clears empty streaming bubble", () => {
  let s = initialMultistageInterviewState();
  s = multistageInterviewReducer(s, {
    type: "advance_begin",
    userId: "u1",
    assistantId: "a2",
    answer: "終わりです",
  });
  s = multistageInterviewReducer(s, {
    type: "session_closed",
    result: {
      session_id: "iv-1",
      stage: "closed",
      status: "closed",
      turn_in_stage: 0,
      total_turns: 7,
      context_ids: [],
      company_name: "Co",
      outcome: "closed",
    },
  });
  assertOk(s.stage === "closed", "closed");
  assertOk(s.messages.some((m) => m.role === "system"), "system note");
  assertOk(!s.messages.some((m) => m.streaming && m.text === ""), "no empty");
});

test("M18B-04 es review meta on success", () => {
  let s = initialEsReviewState();
  s = esReviewReducer(s, {
    type: "review_begin",
    userId: "u",
    reviewerId: "r",
    draft: "ES本文",
  });
  s = esReviewReducer(s, { type: "token", reviewerId: "r", text: "厳しく" });
  s = esReviewReducer(s, {
    type: "review_success",
    reviewerId: "r",
    result: {
      context_ids: ["c1"],
      context_count: 1,
      company_name: "Co",
      facts_source: "injected",
    },
  });
  assertOk(s.meta?.contextCount === 1, "meta");
  assertOk(s.messages[1].text === "厳しく", "streamed");
  assertOk(s.messages[1].streaming === false, "done");
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
