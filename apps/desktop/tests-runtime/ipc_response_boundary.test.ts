import {
  parseBoolean,
  parseCalendarEventDatesResult,
  parseClassifyResult,
  parseEngineEvent,
  parseEngineHealth,
  parseImportStats,
  parseKnowledgeResearchReceipt,
  parseNarrativeCompileResult,
  parseProbeQuestion,
  parseRecordData,
  parseSettingsData,
  parseSourceCodeView,
  parseTensorRebuildResult,
  parseTwinForecast,
} from "../src/lib/parseEngineResponse";

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

test("I-01 boolean parser rejects truthy coercion", () => {
  assertOk(parseBoolean(true) === true, "true");
  expectReject(() => parseBoolean(1));
  expectReject(() => parseBoolean("true"));
});

test("I-02 health parser is exact", () => {
  const health = parseEngineHealth({ status: "ok", offline: true });
  assertOk(health.status === "ok" && health.offline, "health");
  expectReject(() => parseEngineHealth({ status: "ok", offline: true, leak: "x" }));
  expectReject(() => parseEngineHealth({ status: "ok", offline: 1 }));
});

test("I-03 record parser validates nested enums and exact keys", () => {
  const record = parseRecordData({
    date: "2026-07-14",
    events: [{ time: "09:00", title: "work" }],
    transactions: [{ type: "expense", category: "food", amount: 100 }],
    diary: "text",
  });
  assertOk(record.transactions[0].type === "expense", "record");
  expectReject(() => parseRecordData({
    date: "2026-07-14",
    events: [],
    transactions: [{ type: "other", category: "x", amount: 1 }],
    diary: "",
  }));
  expectReject(() => parseRecordData({
    date: "2026-07-14", events: [], transactions: [], diary: "", extra: true,
  }));
});

test("I-04 calendar dates reject invalid members", () => {
  assertOk(parseCalendarEventDatesResult({ dates: ["2026-07-14"] }).dates.length === 1, "dates");
  expectReject(() => parseCalendarEventDatesResult({ dates: [null] }));
});

test("I-05 import stats require the fixed source set", () => {
  const stat = { exists: false, count: 0, mtime: null };
  const parsed = parseImportStats({
    diary: stat,
    line: stat,
    calendar: stat,
    finance: stat,
    es: stat,
    knowledge: stat,
  });
  assertOk(parsed.diary.count === 0, "stats");
  expectReject(() => parseImportStats({ diary: stat }));
});

test("I-06 classify parser rejects unknown literals", () => {
  const parsed = parseClassifyResult({
    type: "knowledge", reasons: ["fixed"], size: 4, filename: "a.md",
  });
  assertOk(parsed.type === "knowledge", "classification");
  expectReject(() => parseClassifyResult({
    type: "execute", reasons: [], size: 0, filename: "x",
  }));
});

test("I-07 settings parser validates every nested field", () => {
  const parsed = parseSettingsData({
    fixed_fields: [{ key: "role", label: "Role" }],
    fixed_attributes: { role: "engineer" },
    profile_summary: "summary",
    apple_calendar_available: false,
  });
  assertOk(parsed.fixed_fields[0].key === "role", "settings");
  expectReject(() => parseSettingsData({
    fixed_fields: [], fixed_attributes: {}, profile_summary: "", apple_calendar_available: 0,
  }));
});

test("I-08 event parser binds id cid and event-specific payload", () => {
  const status = parseEngineEvent({ id: 1, cid: 2, event: "status", message: "working" });
  assertOk(status.event === "status", "status event");
  const chunk = parseEngineEvent({ id: 1, cid: 2, event: "chunk", text: "token" });
  assertOk(chunk.event === "chunk", "chunk event");
  expectReject(() => parseEngineEvent({ id: 1, cid: 2, event: "status", text: "wrong" }));
  expectReject(() => parseEngineEvent({ id: 1, cid: 2, event: "unknown" }));
});

test("I-09 probe question parser rejects surplus fields", () => {
  const question = parseProbeQuestion({
    schema: "probe_question.v1",
    session_id: "s",
    question_id: "q",
    axis: "decision_threshold",
    stage: "FACT",
    question: "question",
    priority: 1,
  });
  assertOk(question.stage === "FACT", "probe");
  expectReject(() => parseProbeQuestion({ ...question, hidden: "x" }));
});

test("I-10 compact result parsers reject coercion", () => {
  assertOk(parseTensorRebuildResult({ rebuilt: true, rows: 3 }).rows === 3, "tensor");
  expectReject(() => parseTensorRebuildResult({ rebuilt: true, rows: 3.5 }));
  const twin = parseTwinForecast({ gate_passed: false, reason: "not ready" });
  assertOk(twin.gate_passed === false, "twin");
  expectReject(() => parseTwinForecast({ gate_passed: "false" }));
});

test("I-11 narrative parser uses an exact option set", () => {
  const parsed = parseNarrativeCompileResult({ ok: false, reason: "insufficient" });
  assertOk(parsed.ok === false, "narrative");
  expectReject(() => parseNarrativeCompileResult({ ok: false, reason: "x", secret: "y" }));
});

test("I-12 nested evidence and claims are strict objects", () => {
  const source = {
    schema: "human_source_code.v1",
    axes: {
      decision_threshold: {
        score: 0.5,
        confidence: 0.7,
        evidence: [{ kind: "diary", date: "2026-07-14", quote: "q", value: null }],
        updated: "2026-07-14",
      },
    },
  };
  assertOk(parseSourceCodeView(source).axes.decision_threshold.evidence.length === 1, "evidence");
  expectReject(() => parseSourceCodeView({
    ...source,
    axes: {
      decision_threshold: {
        ...source.axes.decision_threshold,
        evidence: [{ kind: "diary", date: "2026-07-14", quote: "q", value: null, leak: true }],
      },
    },
  }));
  assertOk(parseNarrativeCompileResult({
    ok: true,
    claims: [{ text: "claim", node_refs: [1, 2] }],
  }).claims?.length === 1, "claims");
  expectReject(() => parseNarrativeCompileResult({
    ok: true,
    claims: [{ text: "claim", node_refs: [1], hidden: "x" }],
  }));
});

test("I-12 knowledge research receipt is id/count only (no raw external payload)", () => {
  const rid = "a".repeat(64);
  const ok = parseKnowledgeResearchReceipt({
    schema: "knowledge_research_receipt.v1",
    research_id: rid,
    results_persisted: 2,
  });
  assertOk(ok.research_id === rid && ok.results_persisted === 2, "receipt");
  expectReject(() => parseKnowledgeResearchReceipt({
    schema: "knowledge_research_receipt.v1",
    research_id: rid,
    results_persisted: 1,
    snippet: "leak",
  }));
  expectReject(() => parseKnowledgeResearchReceipt({
    schema: "knowledge_research_receipt.v1",
    research_id: rid,
    results_persisted: 1,
    url: "https://example.invalid",
  }));
  expectReject(() => parseKnowledgeResearchReceipt({
    schema: "knowledge_research_receipt.v1",
    research_id: rid,
    results_persisted: 1,
    content: "body",
  }));
  expectReject(() => parseKnowledgeResearchReceipt({
    schema: "knowledge_research_receipt.v1",
    research_id: "A".repeat(64),
    results_persisted: 1,
  }));
  expectReject(() => parseKnowledgeResearchReceipt({
    schema: "knowledge_research_receipt.v1",
    research_id: rid,
    results_persisted: -1,
  }));
});

let failed = 0;
for (const entry of tests) {
  try {
    entry.fn();
    console.log(`PASS ${entry.name}`);
  } catch (error) {
    failed += 1;
    console.log(`FAIL ${entry.name}: ${error instanceof Error ? error.message : String(error)}`);
  }
}
console.log(`RESULT failed=${failed} total=${tests.length}`);
if (failed > 0) throw new Error(`${failed} tests failed`);
