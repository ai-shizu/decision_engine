/**
 * Phase 4-A STEP 5 — runtime boundary contract tests (external deps: zero).
 * Harness: sequential registration → run → throw if any failure.
 */
declare const require: (name: "fs") => {
  readFileSync(path: string, encoding: "utf-8"): string;
};

import {
  ManifestParseError,
  parseContextManifestResponseV1,
} from "../src/lib/parseManifest";
import {
  INITIAL_MANIFEST_FETCH_STATE,
  MANIFEST_FETCH_ERROR_MESSAGE,
  reduceManifestFetch,
  type ManifestFetchState,
} from "../src/lib/manifestFetchState";
import type { RetrievalManifestV1 } from "../src/lib/manifest";

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
    thrown instanceof ManifestParseError,
    `expected ManifestParseError, got ${thrown instanceof Error ? thrown.name + ": " + thrown.message : String(thrown)}`,
  );
  const err = thrown as ManifestParseError;
  assertOk(
    err.path.startsWith(expectedPathPrefix),
    `path prefix: expected startsWith ${JSON.stringify(expectedPathPrefix)} got ${JSON.stringify(err.path)}`,
  );
}

function cloneJson<T>(v: T): T {
  return JSON.parse(JSON.stringify(v)) as T;
}

/** Accounting-consistent hand fixture (TS-side). Hash format only — no BLAKE2b recompute. */
function baseManifest(): RetrievalManifestV1 {
  return {
    schema: "retrieval_manifest.v1",
    manifest_id: "a".repeat(64),
    parent_hash: "d".repeat(64),
    sequence_number: 1,
    session_genesis_id: "cd".repeat(64),
    session_id: "boundary-hand-session",
    transcript_version: 2,
    query_hash: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    context_hash: "cccccccccccccccccccccccccccccccc",
    policy_version: "retrieval_policy.v1",
    prompt_version: "pv1",
    model_hash: "ab".repeat(64),
    total_budget_chars: 12000,
    used_chars: 30,
    formatting_overhead_chars: 5,
    candidates: [
      {
        candidate_id: "CURRENT:doc-current",
        document_id: "doc-current",
        content_hash: "11111111111111111111111111111111",
        source_type: "CURRENT_QUERY",
        lane: "CURRENT",
        char_count: 10,
        included_chars: 10,
        status: "ACCEPTED",
        reason_code: "ACCEPTED_REQUIRED_CURRENT",
        selection_rank: null,
        source_index: null,
        speaker_alias: null,
        memory_kind: null,
      },
      {
        candidate_id: "RECENT_TRANSCRIPT:doc-tail",
        document_id: "doc-tail",
        content_hash: "22222222222222222222222222222222",
        source_type: "TRANSCRIPT_TURN",
        lane: "RECENT_TRANSCRIPT",
        char_count: 20,
        included_chars: 8,
        status: "ACCEPTED_TRUNCATED",
        reason_code: "ACCEPTED_TRUNCATED_TO_BUDGET",
        selection_rank: 0,
        source_index: 1,
        speaker_alias: "面接官",
        memory_kind: null,
      },
      {
        candidate_id: "WORKING_MEMORY:atom-1",
        document_id: "atom-1",
        content_hash: "33333333333333333333333333333333",
        source_type: "MEMORY_ATOM",
        lane: "WORKING_MEMORY",
        char_count: 7,
        included_chars: 7,
        status: "ACCEPTED",
        reason_code: "ACCEPTED_WITHIN_BUDGET",
        selection_rank: 0,
        source_index: 0,
        speaker_alias: null,
        memory_kind: "goal",
      },
      {
        candidate_id: "RETRIEVED_EVIDENCE:atom-2",
        document_id: "atom-2",
        content_hash: "44444444444444444444444444444444",
        source_type: "KNOWLEDGE_DOCUMENT",
        lane: "RETRIEVED_EVIDENCE",
        char_count: 100,
        included_chars: 0,
        status: "REJECTED",
        reason_code: "REJECTED_LOW_PRIORITY",
        selection_rank: 1,
        source_index: null,
        speaker_alias: null,
        memory_kind: null,
      },
      {
        candidate_id: "RETRIEVED_EVIDENCE:atom-3",
        document_id: "atom-3",
        content_hash: "55555555555555555555555555555555",
        source_type: "MEMORY_ATOM",
        lane: "RETRIEVED_EVIDENCE",
        char_count: 50,
        included_chars: 0,
        status: "DEDUPLICATED",
        reason_code: "DEDUPLICATED_HIGHER_LANE",
        selection_rank: 2,
        source_index: 0,
        speaker_alias: null,
        memory_kind: "claim",
      },
    ],
    lane_usage: [
      {
        lane: "CURRENT",
        budget_chars: 2400,
        used_chars: 12,
        formatting_chars: 2,
        accepted_count: 1,
        rejected_count: 0,
        deduplicated_count: 0,
      },
      {
        lane: "RECENT_TRANSCRIPT",
        budget_chars: 3600,
        used_chars: 9,
        formatting_chars: 1,
        accepted_count: 1,
        rejected_count: 0,
        deduplicated_count: 0,
      },
      {
        lane: "WORKING_MEMORY",
        budget_chars: 4000,
        used_chars: 9,
        formatting_chars: 2,
        accepted_count: 1,
        rejected_count: 0,
        deduplicated_count: 0,
      },
      {
        lane: "RETRIEVED_EVIDENCE",
        budget_chars: 2000,
        used_chars: 0,
        formatting_chars: 0,
        accepted_count: 0,
        rejected_count: 1,
        deduplicated_count: 1,
      },
    ],
  };
}

function validResponse(): { manifest: RetrievalManifestV1; reason: null } {
  return { manifest: baseManifest(), reason: null };
}

function withManifest(
  mutator: (m: Record<string, unknown>) => void,
): unknown {
  const raw = cloneJson(validResponse()) as unknown as {
    manifest: Record<string, unknown>;
    reason: null;
  };
  mutator(raw.manifest);
  return raw;
}

function withCandidate(
  index: number,
  mutator: (c: Record<string, unknown>) => void,
): unknown {
  return withManifest((m) => {
    const cands = m.candidates as Record<string, unknown>[];
    mutator(cands[index]);
  });
}

function withLane(
  index: number,
  mutator: (l: Record<string, unknown>) => void,
): unknown {
  return withManifest((m) => {
    const lanes = m.lane_usage as Record<string, unknown>[];
    mutator(lanes[index]);
  });
}

// ---- T-01..T-03 normal ----

test("T-01 accepts valid manifest response", () => {
  const input = validResponse();
  const out = parseContextManifestResponseV1(input);
  assertEqual(out.reason, null, "reason");
  assertOk(out.manifest !== null, "manifest present");
  assertEqual(out, input, "structural equality");
});

test("T-02 accepts exact NO_MANIFEST", () => {
  const out = parseContextManifestResponseV1({
    manifest: null,
    reason: "NO_MANIFEST",
  });
  assertEqual(out, { manifest: null, reason: "NO_MANIFEST" }, "NO_MANIFEST");
});

test("T-03 preserves candidates order", () => {
  const input = validResponse();
  const ids = input.manifest.candidates.map((c) => c.candidate_id);
  const out = parseContextManifestResponseV1(input);
  assertOk(out.manifest !== null, "manifest");
  if (out.manifest === null) throw new Error("unreachable");
  const outIds = out.manifest.candidates.map((c) => c.candidate_id);
  assertEqual(outIds, ids, "candidate order");
});

// ---- T-04..T-08 top-level ----

test("T-04 rejects non-object roots", () => {
  for (const bad of [null, undefined, 1, "x", [], true]) {
    assertRejects(() => parseContextManifestResponseV1(bad), "");
  }
});

test("T-05 rejects surplus or missing top-level keys", () => {
  assertRejects(
    () =>
      parseContextManifestResponseV1({
        manifest: null,
        reason: "NO_MANIFEST",
        extra: true,
      }),
    "",
  );
  assertRejects(
    () => parseContextManifestResponseV1({ manifest: baseManifest() }),
    "",
  );
});

test("T-06 rejects {manifest:null, reason:null}", () => {
  assertRejects(
    () => parseContextManifestResponseV1({ manifest: null, reason: null }),
    "reason",
  );
});

test("T-07 rejects {valid manifest, reason:NO_MANIFEST}", () => {
  assertRejects(
    () =>
      parseContextManifestResponseV1({
        manifest: baseManifest(),
        reason: "NO_MANIFEST",
      }),
    "reason",
  );
});

test("T-08 rejects wrong NO_MANIFEST literals", () => {
  assertRejects(
    () =>
      parseContextManifestResponseV1({
        manifest: null,
        reason: "no_manifest",
      }),
    "reason",
  );
  assertRejects(
    () => parseContextManifestResponseV1({ manifest: null, reason: "" }),
    "reason",
  );
});

// ---- T-09..T-21 manifest layer ----

test("T-09 rejects schema v2 or missing", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withManifest((m) => {
      m.schema = "retrieval_manifest.v2";
    })),
    "manifest.schema",
  );
  assertRejects(
    () => parseContextManifestResponseV1(withManifest((m) => {
      delete m.schema;
    })),
    "manifest",
  );
});

test("T-10 rejects surplus/missing manifest keys", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withManifest((m) => {
      m.extra = 1;
    })),
    "manifest",
  );
  assertRejects(
    () => parseContextManifestResponseV1(withManifest((m) => {
      delete m.lane_usage;
    })),
    "manifest",
  );
});

test("T-11 rejects invalid hex hashes", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withManifest((m) => {
      m.manifest_id = "a".repeat(63);
    })),
    "manifest.manifest_id",
  );
  assertRejects(
    () => parseContextManifestResponseV1(withManifest((m) => {
      m.manifest_id = "a".repeat(65);
    })),
    "manifest.manifest_id",
  );
  assertRejects(
    () => parseContextManifestResponseV1(withManifest((m) => {
      m.manifest_id = "A".repeat(64);
    })),
    "manifest.manifest_id",
  );
  assertRejects(
    () => parseContextManifestResponseV1(withManifest((m) => {
      m.manifest_id = "g".repeat(64);
    })),
    "manifest.manifest_id",
  );
  assertRejects(
    () => parseContextManifestResponseV1(withManifest((m) => {
      m.query_hash = "a".repeat(31);
    })),
    "manifest.query_hash",
  );
  assertRejects(
    () => parseContextManifestResponseV1(withManifest((m) => {
      m.context_hash = "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz";
    })),
    "manifest.context_hash",
  );
});

test("T-12 rejects invalid model_hash", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withManifest((m) => {
      m.model_hash = "x";
    })),
    "manifest.model_hash",
  );
  assertRejects(
    () => parseContextManifestResponseV1(withManifest((m) => {
      m.model_hash = null;
    })),
    "manifest.model_hash",
  );
});

test("T-13 rejects budget violations", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withManifest((m) => {
      m.total_budget_chars = 11999;
    })),
    "manifest.total_budget_chars",
  );
  assertRejects(
    () => parseContextManifestResponseV1(withManifest((m) => {
      m.used_chars = 12001;
    })),
    "manifest.used_chars",
  );
});

test("T-14 rejects non-safe-integer transcript_version", () => {
  for (const bad of [1.5, "1", true, NaN, Infinity, -1]) {
    assertRejects(
      () => parseContextManifestResponseV1(withManifest((m) => {
        m.transcript_version = bad;
      })),
      "manifest.transcript_version",
    );
  }
});

test("T-15 rejects invalid session_id", () => {
  for (const bad of ["", "  ", "a\nb", null]) {
    assertRejects(
      () => parseContextManifestResponseV1(withManifest((m) => {
        m.session_id = bad;
      })),
      "manifest.session_id",
    );
  }
});

test("T-16 rejects bad candidates/lane_usage shapes", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withManifest((m) => {
      m.candidates = {};
    })),
    "manifest.candidates",
  );
  assertRejects(
    () => parseContextManifestResponseV1(withManifest((m) => {
      m.lane_usage = (m.lane_usage as unknown[]).slice(0, 3);
    })),
    "manifest.lane_usage",
  );
  assertRejects(
    () => parseContextManifestResponseV1(withManifest((m) => {
      const lanes = m.lane_usage as unknown[];
      m.lane_usage = [...lanes, lanes[0]];
    })),
    "manifest.lane_usage",
  );
});

test("T-17 rejects lane_usage order violation", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withManifest((m) => {
      const lanes = m.lane_usage as Record<string, unknown>[];
      const tmp = lanes[0];
      lanes[0] = lanes[1];
      lanes[1] = tmp;
    })),
    "manifest.lane_usage",
  );
});

test("T-18 rejects wrong fixed budget_chars", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withLane(0, (l) => {
      l.budget_chars = 2401;
    })),
    "manifest.lane_usage[0].budget_chars",
  );
});

test("T-19 rejects included+formatting !== used_chars", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withManifest((m) => {
      m.formatting_overhead_chars = (m.formatting_overhead_chars as number) + 1;
    })),
    "manifest",
  );
});

test("T-20 rejects sum(lane.used_chars) !== used_chars", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withLane(0, (l) => {
      l.used_chars = (l.used_chars as number) + 1;
      l.formatting_chars = (l.formatting_chars as number) + 1;
    })),
    "manifest",
  );
});

test("T-21 rejects duplicate candidate_id", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withCandidate(1, (c) => {
      c.candidate_id = "CURRENT:doc-current";
    })),
    "manifest.candidates",
  );
});

// ---- T-22..T-31 candidate layer ----

test("T-22 rejects surplus/missing candidate keys", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withCandidate(0, (c) => {
      c.extra = 1;
    })),
    "manifest.candidates[0]",
  );
  assertRejects(
    () => parseContextManifestResponseV1(withCandidate(0, (c) => {
      delete c.reason_code;
    })),
    "manifest.candidates[0]",
  );
});

test("T-23 rejects enum allowlist violations", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withCandidate(0, (c) => {
      c.source_type = "WEB_SEARCH";
    })),
    "manifest.candidates[0].source_type",
  );
  assertRejects(
    () => parseContextManifestResponseV1(withCandidate(0, (c) => {
      c.lane = "current";
    })),
    "manifest.candidates[0].lane",
  );
  assertRejects(
    () => parseContextManifestResponseV1(withCandidate(0, (c) => {
      c.status = "OK";
    })),
    "manifest.candidates[0].status",
  );
  assertRejects(
    () => parseContextManifestResponseV1(withCandidate(0, (c) => {
      c.reason_code = "BECAUSE";
    })),
    "manifest.candidates[0].reason_code",
  );
});

test("T-24 rejects status/reason mismatch", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withCandidate(0, (c) => {
      c.status = "ACCEPTED";
      c.reason_code = "REJECTED_EMPTY";
      c.included_chars = c.char_count;
    })),
    "manifest.candidates[0]",
  );
  assertRejects(
    () => parseContextManifestResponseV1(withCandidate(3, (c) => {
      c.status = "REJECTED";
      c.reason_code = "ACCEPTED_WITHIN_BUDGET";
      c.included_chars = 0;
    })),
    "manifest.candidates[3]",
  );
});

test("T-25 rejects included_chars > char_count", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withCandidate(0, (c) => {
      c.included_chars = (c.char_count as number) + 1;
    })),
    "manifest.candidates[0].included_chars",
  );
});

test("T-26 rejects ACCEPTED with included !== char", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withCandidate(0, (c) => {
      c.status = "ACCEPTED";
      c.reason_code = "ACCEPTED_REQUIRED_CURRENT";
      c.included_chars = (c.char_count as number) - 1;
    })),
    "manifest.candidates[0]",
  );
});

test("T-27 rejects ACCEPTED_TRUNCATED with included === char", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withCandidate(1, (c) => {
      c.status = "ACCEPTED_TRUNCATED";
      c.reason_code = "ACCEPTED_TRUNCATED_TO_BUDGET";
      c.included_chars = c.char_count;
    })),
    "manifest.candidates[1]",
  );
});

test("T-28 rejects REJECTED with included_chars !== 0", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withCandidate(3, (c) => {
      c.status = "REJECTED";
      c.reason_code = "REJECTED_LOW_PRIORITY";
      c.included_chars = 1;
    })),
    "manifest.candidates[3]",
  );
});

test("T-29 rejects unapproved speaker_alias without leaking value", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withCandidate(0, (c) => {
      c.speaker_alias = "山田太郎";
    })),
    "manifest.candidates[0].speaker_alias",
  );
  try {
    parseContextManifestResponseV1(
      withCandidate(0, (c) => {
        c.speaker_alias = "山田太郎";
      }),
    );
    throw new Error("expected throw");
  } catch (e) {
    assertOk(e instanceof ManifestParseError, "ManifestParseError");
    assertOk(
      !(e as Error).message.includes("山田太郎"),
      "must not leak speaker_alias value",
    );
  }
  assertRejects(
    () => parseContextManifestResponseV1(withCandidate(0, (c) => {
      c.speaker_alias = "C-XYZ";
    })),
    "manifest.candidates[0].speaker_alias",
  );
});

test("T-30 rejects memory_kind outside allowlist", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withCandidate(2, (c) => {
      c.memory_kind = "secret";
    })),
    "manifest.candidates[2].memory_kind",
  );
});

test("T-31 rejects invalid selection_rank", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withCandidate(1, (c) => {
      c.selection_rank = -1;
    })),
    "manifest.candidates[1].selection_rank",
  );
  assertRejects(
    () => parseContextManifestResponseV1(withCandidate(1, (c) => {
      c.selection_rank = 0.5;
    })),
    "manifest.candidates[1].selection_rank",
  );
});

// ---- T-32..T-35 lane_usage ----

test("T-32 rejects formatting_chars > used_chars", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withLane(0, (l) => {
      l.formatting_chars = (l.used_chars as number) + 1;
    })),
    "manifest.lane_usage[0]",
  );
});

test("T-33 rejects lane content exceeding budget", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withLane(0, (l) => {
      l.budget_chars = 2400;
      l.used_chars = 2401;
      l.formatting_chars = 0;
    })),
    "manifest.lane_usage[0]",
  );
});

test("T-34 rejects lane count mismatches vs candidates", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withLane(0, (l) => {
      l.accepted_count = (l.accepted_count as number) + 1;
    })),
    "manifest.lane_usage[0].accepted_count",
  );
  assertRejects(
    () => parseContextManifestResponseV1(withLane(3, (l) => {
      l.rejected_count = (l.rejected_count as number) + 1;
    })),
    "manifest.lane_usage[3].rejected_count",
  );
  assertRejects(
    () => parseContextManifestResponseV1(withLane(3, (l) => {
      l.deduplicated_count = (l.deduplicated_count as number) + 1;
    })),
    "manifest.lane_usage[3].deduplicated_count",
  );
});

test("T-35 rejects lane included+formatting !== used", () => {
  assertRejects(
    () => parseContextManifestResponseV1(withLane(0, (l) => {
      // keep used_chars; inflate formatting so included+formatting !== used
      l.formatting_chars = (l.formatting_chars as number) + 1;
    })),
    "manifest.lane_usage[0]",
  );
});

// ---- T-36..T-41 reducer ----

test("T-36 REQUEST_START clears prior and sets loading", () => {
  const prior: ManifestFetchState = {
    phase: "ready",
    manifest: baseManifest(),
    errorMessage: "old",
    activeSeq: 1,
  };
  const next = reduceManifestFetch(prior, { kind: "REQUEST_START", seq: 2 });
  assertEqual(next.phase, "loading", "phase");
  assertEqual(next.manifest, null, "manifest cleared");
  assertEqual(next.errorMessage, "", "error cleared");
  assertEqual(next.activeSeq, 2, "seq");
});

test("T-37 REQUEST_SUCCESS with manifest -> ready", () => {
  let state = reduceManifestFetch(INITIAL_MANIFEST_FETCH_STATE, {
    kind: "REQUEST_START",
    seq: 1,
  });
  const response = validResponse();
  state = reduceManifestFetch(state, {
    kind: "REQUEST_SUCCESS",
    seq: 1,
    response,
  });
  assertEqual(state.phase, "ready", "phase");
  assertEqual(state.manifest, response.manifest, "manifest");
  assertEqual(state.errorMessage, "", "error");
});

test("T-38 REQUEST_SUCCESS NO_MANIFEST -> empty", () => {
  let state = reduceManifestFetch(INITIAL_MANIFEST_FETCH_STATE, {
    kind: "REQUEST_START",
    seq: 1,
  });
  state = reduceManifestFetch(state, {
    kind: "REQUEST_SUCCESS",
    seq: 1,
    response: { manifest: null, reason: "NO_MANIFEST" },
  });
  assertEqual(state.phase, "empty", "phase");
  assertEqual(state.manifest, null, "manifest");
  assertEqual(state.errorMessage, "", "error");
});

test("T-39 REQUEST_FAILURE -> error with fixed message", () => {
  let state = reduceManifestFetch(INITIAL_MANIFEST_FETCH_STATE, {
    kind: "REQUEST_START",
    seq: 1,
  });
  state = reduceManifestFetch(state, { kind: "REQUEST_FAILURE", seq: 1 });
  assertEqual(state.phase, "error", "phase");
  assertEqual(state.manifest, null, "manifest");
  assertEqual(state.errorMessage, MANIFEST_FETCH_ERROR_MESSAGE, "msg");
});

test("T-40 stale SUCCESS/FAILURE ignored (same reference)", () => {
  let state = reduceManifestFetch(INITIAL_MANIFEST_FETCH_STATE, {
    kind: "REQUEST_START",
    seq: 2,
  });
  const afterStart = state;
  const staleSuccess = reduceManifestFetch(state, {
    kind: "REQUEST_SUCCESS",
    seq: 1,
    response: validResponse(),
  });
  assertOk(staleSuccess === afterStart, "stale success same ref");
  const staleFail = reduceManifestFetch(state, {
    kind: "REQUEST_FAILURE",
    seq: 1,
  });
  assertOk(staleFail === afterStart, "stale failure same ref");
});

test("T-41 non-monotonic REQUEST_START ignored", () => {
  let state = reduceManifestFetch(INITIAL_MANIFEST_FETCH_STATE, {
    kind: "REQUEST_START",
    seq: 3,
  });
  const after = state;
  const same = reduceManifestFetch(state, { kind: "REQUEST_START", seq: 3 });
  assertOk(same === after, "seq== ignored");
  const older = reduceManifestFetch(state, { kind: "REQUEST_START", seq: 2 });
  assertOk(older === after, "seq< ignored");
});

// ---- T-42 golden ----

test("T-42 accepts Python golden fixture", () => {
  const fs = require("fs");
  let text: string;
  try {
    text = fs.readFileSync(".boundary-tests-out/golden.json", "utf-8");
  } catch {
    throw new Error("golden fixture missing: .boundary-tests-out/golden.json");
  }
  const raw: unknown = JSON.parse(text);
  const out = parseContextManifestResponseV1(raw);
  assertOk(out.manifest !== null, "golden manifest present");
  if (out.manifest === null) throw new Error("unreachable");
  assertEqual(out.reason, null, "golden reason");
  const expectedId = (raw as { manifest: { manifest_id: string } }).manifest
    .manifest_id;
  assertEqual(out.manifest.manifest_id, expectedId, "golden manifest_id");
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
