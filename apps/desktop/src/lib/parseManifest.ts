import type {
  CandidateStatus,
  ContextLane,
  ContextManifestResponseV1,
  LaneUsageV1,
  ReasonCode,
  RetrievalCandidateV1,
  RetrievalManifestV1,
  SourceType,
} from "./manifest";

export class ManifestParseError extends Error {
  readonly path: string;
  constructor(path: string, expected: string) {
    super(`manifest parse error at ${path}: expected ${expected}`);
    this.name = "ManifestParseError";
    this.path = path;
  }
}

const HEX32 = /^[0-9a-f]{32}$/;
const HEX128 = /^[0-9a-f]{128}$/;
const CONTACT_ALIAS = /^C-[0-9a-f]{8}$/;

const FIXED_INTERNAL_ALIASES = new Set([
  "candidate",
  "面接官",
  "参加者",
  "メンター",
]);

const MEMORY_KINDS = new Set([
  "goal",
  "claim",
  "datum",
  "assumption",
  "constraint",
  "decision",
  "open_question",
  "contradiction",
  "recommendation",
]);

const LANE_ORDER: readonly ContextLane[] = [
  "CURRENT",
  "RECENT_TRANSCRIPT",
  "WORKING_MEMORY",
  "RETRIEVED_EVIDENCE",
];

const LANE_BUDGETS: Record<ContextLane, number> = {
  CURRENT: 2400,
  RECENT_TRANSCRIPT: 3600,
  WORKING_MEMORY: 4000,
  RETRIEVED_EVIDENCE: 2000,
};

const STATUS_REASONS: Record<CandidateStatus, ReadonlySet<ReasonCode>> = {
  ACCEPTED: new Set([
    "ACCEPTED_REQUIRED_CURRENT",
    "ACCEPTED_WITHIN_BUDGET",
  ]),
  ACCEPTED_TRUNCATED: new Set(["ACCEPTED_TRUNCATED_TO_BUDGET"]),
  REJECTED: new Set([
    "REJECTED_BUDGET_LIMIT",
    "REJECTED_TOTAL_BUDGET",
    "REJECTED_LOW_PRIORITY",
    "REJECTED_INELIGIBLE_SOURCE",
    "REJECTED_EMPTY",
    "REJECTED_SUPERSEDED",
    "REJECTED_PRIVACY_POLICY",
  ]),
  DEDUPLICATED: new Set([
    "DEDUPLICATED_DOCUMENT_ID",
    "DEDUPLICATED_CONTENT_HASH",
    "DEDUPLICATED_HIGHER_LANE",
  ]),
};

const MANIFEST_KEYS = [
  "schema",
  "manifest_id",
  "session_id",
  "transcript_version",
  "query_hash",
  "context_hash",
  "policy_version",
  "prompt_version",
  "model_hash",
  "total_budget_chars",
  "used_chars",
  "formatting_overhead_chars",
  "candidates",
  "lane_usage",
].sort();

const CANDIDATE_KEYS = [
  "candidate_id",
  "document_id",
  "content_hash",
  "source_type",
  "lane",
  "char_count",
  "included_chars",
  "status",
  "reason_code",
  "selection_rank",
  "source_index",
  "speaker_alias",
  "memory_kind",
].sort();

const LANE_USAGE_KEYS = [
  "lane",
  "budget_chars",
  "used_chars",
  "formatting_chars",
  "accepted_count",
  "rejected_count",
  "deduplicated_count",
].sort();

const RESPONSE_KEYS = ["manifest", "reason"].sort();

function isPlainObject(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function expectExactKeys(
  obj: Record<string, unknown>,
  expectedSorted: string[],
  path: string,
): void {
  const keys = Object.keys(obj).sort();
  if (keys.length !== expectedSorted.length) {
    throw new ManifestParseError(path, `exact keys ${expectedSorted.join(",")}`);
  }
  for (let i = 0; i < keys.length; i++) {
    if (keys[i] !== expectedSorted[i]) {
      throw new ManifestParseError(path, `exact keys ${expectedSorted.join(",")}`);
    }
    if (obj[keys[i]] === undefined) {
      throw new ManifestParseError(`${path}.${keys[i]}`, "defined value");
    }
  }
}

function parseNonNegInt(v: unknown, path: string): number {
  if (typeof v !== "number" || !Number.isSafeInteger(v) || v < 0) {
    throw new ManifestParseError(path, "nonnegative safe integer");
  }
  return v;
}

function parseStrictStr(v: unknown, path: string): string {
  if (typeof v !== "string" || v.trim() === "" || v.includes("\n") || v.includes("\r")) {
    throw new ManifestParseError(path, "non-empty string without newlines");
  }
  return v;
}

function parseHex32(v: unknown, path: string): string {
  if (typeof v !== "string" || !HEX32.test(v)) {
    throw new ManifestParseError(path, "32-char lowercase hex");
  }
  return v;
}

function parseModelHash(v: unknown, path: string): string {
  if (typeof v !== "string" || !HEX128.test(v)) {
    throw new ManifestParseError(
      path,
      "128-char lowercase hex canonical runtime identity",
    );
  }
  return v;
}

function isContextLane(v: string): v is ContextLane {
  return (
    v === "CURRENT" ||
    v === "RECENT_TRANSCRIPT" ||
    v === "WORKING_MEMORY" ||
    v === "RETRIEVED_EVIDENCE"
  );
}

function isSourceType(v: string): v is SourceType {
  return (
    v === "CURRENT_QUERY" ||
    v === "TRANSCRIPT_TURN" ||
    v === "MEMORY_ATOM" ||
    v === "KNOWLEDGE_DOCUMENT"
  );
}

function isCandidateStatus(v: string): v is CandidateStatus {
  return (
    v === "ACCEPTED" ||
    v === "ACCEPTED_TRUNCATED" ||
    v === "REJECTED" ||
    v === "DEDUPLICATED"
  );
}

function isReasonCode(v: string): v is ReasonCode {
  return (
    v === "ACCEPTED_REQUIRED_CURRENT" ||
    v === "ACCEPTED_WITHIN_BUDGET" ||
    v === "ACCEPTED_TRUNCATED_TO_BUDGET" ||
    v === "REJECTED_BUDGET_LIMIT" ||
    v === "REJECTED_TOTAL_BUDGET" ||
    v === "REJECTED_LOW_PRIORITY" ||
    v === "REJECTED_INELIGIBLE_SOURCE" ||
    v === "REJECTED_EMPTY" ||
    v === "REJECTED_SUPERSEDED" ||
    v === "REJECTED_PRIVACY_POLICY" ||
    v === "DEDUPLICATED_DOCUMENT_ID" ||
    v === "DEDUPLICATED_CONTENT_HASH" ||
    v === "DEDUPLICATED_HIGHER_LANE"
  );
}

function parseNullableNonNegInt(v: unknown, path: string): number | null {
  if (v === null) return null;
  return parseNonNegInt(v, path);
}

function parseSpeakerAlias(v: unknown, path: string): string | null {
  if (v === null) return null;
  if (typeof v !== "string") {
    throw new ManifestParseError(path, "null or approved alias");
  }
  if (FIXED_INTERNAL_ALIASES.has(v) || CONTACT_ALIAS.test(v)) {
    return v;
  }
  throw new ManifestParseError(path, "null or approved alias");
}

function parseMemoryKind(v: unknown, path: string): string | null {
  if (v === null) return null;
  if (typeof v !== "string" || !MEMORY_KINDS.has(v)) {
    throw new ManifestParseError(path, "null or allowlisted memory_kind");
  }
  return v;
}

function parseCandidate(
  raw: unknown,
  path: string,
): RetrievalCandidateV1 {
  if (!isPlainObject(raw)) {
    throw new ManifestParseError(path, "object");
  }
  expectExactKeys(raw, CANDIDATE_KEYS, path);

  const candidate_id = parseStrictStr(raw.candidate_id, `${path}.candidate_id`);
  const document_id = parseStrictStr(raw.document_id, `${path}.document_id`);
  const content_hash = parseHex32(raw.content_hash, `${path}.content_hash`);

  if (typeof raw.source_type !== "string" || !isSourceType(raw.source_type)) {
    throw new ManifestParseError(`${path}.source_type`, "SourceType");
  }
  const source_type = raw.source_type;

  if (typeof raw.lane !== "string" || !isContextLane(raw.lane)) {
    throw new ManifestParseError(`${path}.lane`, "ContextLane");
  }
  const lane = raw.lane;

  const char_count = parseNonNegInt(raw.char_count, `${path}.char_count`);
  const included_chars = parseNonNegInt(
    raw.included_chars,
    `${path}.included_chars`,
  );
  if (included_chars > char_count) {
    throw new ManifestParseError(
      `${path}.included_chars`,
      "included_chars <= char_count",
    );
  }

  if (typeof raw.status !== "string" || !isCandidateStatus(raw.status)) {
    throw new ManifestParseError(`${path}.status`, "CandidateStatus");
  }
  const status = raw.status;

  if (typeof raw.reason_code !== "string" || !isReasonCode(raw.reason_code)) {
    throw new ManifestParseError(`${path}.reason_code`, "ReasonCode");
  }
  const reason_code = raw.reason_code;

  if (!STATUS_REASONS[status].has(reason_code)) {
    throw new ManifestParseError(path, "status/reason compatibility");
  }

  if (status === "ACCEPTED") {
    if (included_chars !== char_count) {
      throw new ManifestParseError(path, "ACCEPTED included_chars === char_count");
    }
  } else if (status === "ACCEPTED_TRUNCATED") {
    if (!(0 < included_chars && included_chars < char_count)) {
      throw new ManifestParseError(
        path,
        "ACCEPTED_TRUNCATED 0 < included_chars < char_count",
      );
    }
  } else if (included_chars !== 0) {
    throw new ManifestParseError(path, "terminal status included_chars === 0");
  }

  const selection_rank = parseNullableNonNegInt(
    raw.selection_rank,
    `${path}.selection_rank`,
  );
  const source_index = parseNullableNonNegInt(
    raw.source_index,
    `${path}.source_index`,
  );
  const speaker_alias = parseSpeakerAlias(
    raw.speaker_alias,
    `${path}.speaker_alias`,
  );
  const memory_kind = parseMemoryKind(raw.memory_kind, `${path}.memory_kind`);

  return {
    candidate_id,
    document_id,
    content_hash,
    source_type,
    lane,
    char_count,
    included_chars,
    status,
    reason_code,
    selection_rank,
    source_index,
    speaker_alias,
    memory_kind,
  };
}

function parseLaneUsage(raw: unknown, path: string): LaneUsageV1 {
  if (!isPlainObject(raw)) {
    throw new ManifestParseError(path, "object");
  }
  expectExactKeys(raw, LANE_USAGE_KEYS, path);

  if (typeof raw.lane !== "string" || !isContextLane(raw.lane)) {
    throw new ManifestParseError(`${path}.lane`, "ContextLane");
  }
  const lane = raw.lane;
  const budget_chars = parseNonNegInt(raw.budget_chars, `${path}.budget_chars`);
  if (budget_chars !== LANE_BUDGETS[lane]) {
    throw new ManifestParseError(
      `${path}.budget_chars`,
      `fixed budget ${LANE_BUDGETS[lane]}`,
    );
  }
  const used_chars = parseNonNegInt(raw.used_chars, `${path}.used_chars`);
  const formatting_chars = parseNonNegInt(
    raw.formatting_chars,
    `${path}.formatting_chars`,
  );
  const accepted_count = parseNonNegInt(
    raw.accepted_count,
    `${path}.accepted_count`,
  );
  const rejected_count = parseNonNegInt(
    raw.rejected_count,
    `${path}.rejected_count`,
  );
  const deduplicated_count = parseNonNegInt(
    raw.deduplicated_count,
    `${path}.deduplicated_count`,
  );

  if (formatting_chars > used_chars) {
    throw new ManifestParseError(path, "formatting_chars <= used_chars");
  }
  if (used_chars - formatting_chars > budget_chars) {
    throw new ManifestParseError(path, "lane content within budget_chars");
  }

  return {
    lane,
    budget_chars,
    used_chars,
    formatting_chars,
    accepted_count,
    rejected_count,
    deduplicated_count,
  };
}

function parseManifest(raw: unknown, path: string): RetrievalManifestV1 {
  if (!isPlainObject(raw)) {
    throw new ManifestParseError(path, "object");
  }
  expectExactKeys(raw, MANIFEST_KEYS, path);

  if (raw.schema !== "retrieval_manifest.v1") {
    throw new ManifestParseError(`${path}.schema`, '"retrieval_manifest.v1"');
  }

  const manifest_id = parseHex32(raw.manifest_id, `${path}.manifest_id`);
  const session_id = parseStrictStr(raw.session_id, `${path}.session_id`);
  const transcript_version = parseNonNegInt(
    raw.transcript_version,
    `${path}.transcript_version`,
  );
  const query_hash = parseHex32(raw.query_hash, `${path}.query_hash`);
  const context_hash = parseHex32(raw.context_hash, `${path}.context_hash`);
  const policy_version = parseStrictStr(
    raw.policy_version,
    `${path}.policy_version`,
  );
  const prompt_version = parseStrictStr(
    raw.prompt_version,
    `${path}.prompt_version`,
  );
  const model_hash = parseModelHash(raw.model_hash, `${path}.model_hash`);

  const total_budget_chars = parseNonNegInt(
    raw.total_budget_chars,
    `${path}.total_budget_chars`,
  );
  if (total_budget_chars !== 12000) {
    throw new ManifestParseError(`${path}.total_budget_chars`, "12000");
  }
  const used_chars = parseNonNegInt(raw.used_chars, `${path}.used_chars`);
  if (used_chars > total_budget_chars) {
    throw new ManifestParseError(
      `${path}.used_chars`,
      "used_chars <= total_budget_chars",
    );
  }
  const formatting_overhead_chars = parseNonNegInt(
    raw.formatting_overhead_chars,
    `${path}.formatting_overhead_chars`,
  );

  if (!Array.isArray(raw.candidates)) {
    throw new ManifestParseError(`${path}.candidates`, "array");
  }
  const candidates: RetrievalCandidateV1[] = [];
  for (let i = 0; i < raw.candidates.length; i++) {
    candidates.push(parseCandidate(raw.candidates[i], `${path}.candidates[${i}]`));
  }
  const ids = new Set<string>();
  for (const c of candidates) {
    if (ids.has(c.candidate_id)) {
      throw new ManifestParseError(`${path}.candidates`, "unique candidate_id");
    }
    ids.add(c.candidate_id);
  }

  if (!Array.isArray(raw.lane_usage)) {
    throw new ManifestParseError(`${path}.lane_usage`, "array of length 4");
  }
  if (raw.lane_usage.length !== 4) {
    throw new ManifestParseError(`${path}.lane_usage`, "array of length 4");
  }
  const lane_usage: LaneUsageV1[] = [];
  for (let i = 0; i < raw.lane_usage.length; i++) {
    lane_usage.push(parseLaneUsage(raw.lane_usage[i], `${path}.lane_usage[${i}]`));
  }
  for (let i = 0; i < 4; i++) {
    if (lane_usage[i].lane !== LANE_ORDER[i]) {
      throw new ManifestParseError(`${path}.lane_usage`, "fixed lane order");
    }
  }

  let includedSum = 0;
  for (const c of candidates) {
    includedSum += c.included_chars;
  }
  if (includedSum + formatting_overhead_chars !== used_chars) {
    throw new ManifestParseError(path, "budget accounting match");
  }

  let laneSum = 0;
  for (const lu of lane_usage) {
    laneSum += lu.used_chars;
  }
  if (laneSum !== used_chars) {
    throw new ManifestParseError(path, "lane usage sum match");
  }

  for (let i = 0; i < lane_usage.length; i++) {
    const lu = lane_usage[i];
    const laneCands = candidates.filter((c) => c.lane === lu.lane);
    let accepted = 0;
    let rejected = 0;
    let deduped = 0;
    let laneIncluded = 0;
    for (const c of laneCands) {
      laneIncluded += c.included_chars;
      if (
        (c.status === "ACCEPTED" || c.status === "ACCEPTED_TRUNCATED") &&
        c.included_chars > 0
      ) {
        accepted += 1;
      } else if (c.status === "REJECTED") {
        rejected += 1;
      } else if (c.status === "DEDUPLICATED") {
        deduped += 1;
      }
    }
    if (lu.accepted_count !== accepted) {
      throw new ManifestParseError(
        `${path}.lane_usage[${i}].accepted_count`,
        "match candidates",
      );
    }
    if (lu.rejected_count !== rejected) {
      throw new ManifestParseError(
        `${path}.lane_usage[${i}].rejected_count`,
        "match candidates",
      );
    }
    if (lu.deduplicated_count !== deduped) {
      throw new ManifestParseError(
        `${path}.lane_usage[${i}].deduplicated_count`,
        "match candidates",
      );
    }
    if (laneIncluded + lu.formatting_chars !== lu.used_chars) {
      throw new ManifestParseError(
        `${path}.lane_usage[${i}]`,
        "lane formatting accounting",
      );
    }
  }

  const manifest: RetrievalManifestV1 = {
    schema: "retrieval_manifest.v1",
    manifest_id,
    session_id,
    transcript_version,
    query_hash,
    context_hash,
    policy_version,
    prompt_version,
    model_hash,
    total_budget_chars,
    used_chars,
    formatting_overhead_chars,
    candidates,
    lane_usage,
  };
  return manifest;
}

export function parseContextManifestResponseV1(
  raw: unknown,
): ContextManifestResponseV1 {
  if (!isPlainObject(raw)) {
    throw new ManifestParseError("", "object with keys manifest,reason");
  }
  expectExactKeys(raw, RESPONSE_KEYS, "");

  if (raw.manifest === null) {
    if (raw.reason !== "NO_MANIFEST") {
      throw new ManifestParseError("reason", '"NO_MANIFEST"');
    }
    return { manifest: null, reason: "NO_MANIFEST" };
  }

  if (raw.reason !== null) {
    throw new ManifestParseError("reason", "null");
  }

  if (!isPlainObject(raw.manifest)) {
    throw new ManifestParseError("manifest", "object");
  }

  return {
    manifest: parseManifest(raw.manifest, "manifest"),
    reason: null,
  };
}
