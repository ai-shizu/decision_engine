/**
 * Runtime boundary for the `consult` IPC command (audit finding remediation:
 * interview_report.v1 tensor_profile was reaching the frontend and being
 * silently discarded by a generic-cast `pkbInvoke<T>()`). Mirrors the Python
 * validators in core/interview_report.py, core/tensor_profile.py, and
 * core/romance_analysis.py — read-only reference, no core code imported or
 * reimplemented differently. No cast, clamp, delete, defaulting, or silent
 * fallback. Errors never carry the violating value, quote, or answer text.
 */
import type {
  InterviewConfig,
  InterviewReport,
  InterviewReportLatency,
  InterviewReportMetric,
  TensorDimensionId,
  TensorDimensionV1,
  TensorEvidenceV1,
  TensorProfileReportV1,
} from "./types";

export class ConsultResponseParseError extends Error {
  readonly path: string;
  constructor(path: string, expected: string) {
    super(`consult response parse error at ${path}: expected ${expected}`);
    this.name = "ConsultResponseParseError";
    this.path = path;
  }
}

// ---- shared strict primitives (self-contained; no cross-import from
// parseManifest.ts — different domain, avoids coupling unrelated boundaries) ----

const HEX32 = /^[0-9a-f]{32}$/;

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
    throw new ConsultResponseParseError(path, `exact keys ${expectedSorted.join(",")}`);
  }
  for (let i = 0; i < keys.length; i++) {
    if (keys[i] !== expectedSorted[i]) {
      throw new ConsultResponseParseError(path, `exact keys ${expectedSorted.join(",")}`);
    }
    if (obj[keys[i]] === undefined) {
      throw new ConsultResponseParseError(`${path}.${keys[i]}`, "defined value");
    }
  }
}

function parseStrictStr(v: unknown, path: string): string {
  if (typeof v !== "string" || v.length === 0) {
    throw new ConsultResponseParseError(path, "non-empty string");
  }
  return v;
}

function parseAnyStr(v: unknown, path: string): string {
  if (typeof v !== "string") {
    throw new ConsultResponseParseError(path, "string");
  }
  return v;
}

function parseHex32(v: unknown, path: string): string {
  if (typeof v !== "string" || !HEX32.test(v)) {
    throw new ConsultResponseParseError(path, "32-char lowercase hex");
  }
  return v;
}

function parseSafeInt(v: unknown, path: string, min: number, max: number): number {
  if (typeof v !== "number" || !Number.isSafeInteger(v) || v < min || v > max) {
    throw new ConsultResponseParseError(path, `safe integer ${min}..${max}`);
  }
  return v;
}

function parseFiniteNumber(v: unknown, path: string, min: number, max: number): number {
  if (typeof v !== "number" || !Number.isFinite(v) || v < min || v > max) {
    throw new ConsultResponseParseError(path, `finite number ${min}..${max}`);
  }
  return v;
}

// ---- ROUND_HALF_UP mirror of Python's Decimal(str(x)).quantize(Decimal("0.01"))
// See core/tensor_profile.py `_round2`. Both runtimes produce the shortest
// round-trip decimal string for the same IEEE754 double, so string-based
// half-up rounding on that string reproduces Python's Decimal result exactly. ----

function round2HalfUp(x: number): number {
  const s = x.toString();
  const neg = s.startsWith("-");
  const abs = neg ? s.slice(1) : s;
  const dotIdx = abs.indexOf(".");
  let intPart: string;
  let fracPart: string;
  if (dotIdx === -1) {
    intPart = abs;
    fracPart = "";
  } else {
    intPart = abs.slice(0, dotIdx);
    fracPart = abs.slice(dotIdx + 1);
  }
  while (fracPart.length < 3) fracPart += "0";
  const keep = fracPart.slice(0, 2);
  const nextDigit = fracPart.charCodeAt(2) - 48;
  let keepNum = parseInt(intPart + keep, 10);
  if (nextDigit >= 5) keepNum += 1;
  const result = keepNum / 100;
  return neg ? -result : result;
}

// ---- tensor_profile.py mirrors (fixed tables — read-only reference) ----

const CANONICAL_DIMENSION_IDS: readonly TensorDimensionId[] = [
  "problem_structuring",
  "quantitative_rigor",
  "hypothesis_evidence",
  "synthesis_judgment",
  "communication",
  "collaboration_adaptability",
];

const CALCULUS_AXIS_MAP: Record<TensorDimensionId, string> = {
  problem_structuring: "Structural_Decomposition",
  quantitative_rigor: "Quantitative_Agility",
  hypothesis_evidence: "Logical_Rigor",
  synthesis_judgment: "Domain_Adaptability",
  communication: "Communication_Bandwidth",
  collaboration_adaptability: "Cognitive_Flexibility",
};

const INDICATOR_IDS: Record<TensorDimensionId, readonly string[]> = {
  problem_structuring: ["ps_clarify_objective", "ps_decompose", "ps_prioritize"],
  quantitative_rigor: ["qr_units_assumptions", "qr_calculations", "qr_interpret_data"],
  hypothesis_evidence: ["he_form_hypothesis", "he_disconfirm", "he_update_facts"],
  synthesis_judgment: ["sj_implications", "sj_tradeoffs", "sj_recommendations"],
  communication: ["cm_signposting", "cm_conclusions", "cm_delivery"],
  collaboration_adaptability: ["ca_listens", "ca_builds_on", "ca_handles_challenge"],
};

const CANDIDATE_ALIAS = "candidate";
const FIRST_FIVE_DIMENSIONS: ReadonlySet<TensorDimensionId> = new Set(
  CANONICAL_DIMENSION_IDS.slice(0, 5),
);

function isTensorDimensionId(v: string): v is TensorDimensionId {
  return (CANONICAL_DIMENSION_IDS as readonly string[]).includes(v);
}

const TENSOR_EVIDENCE_KEYS = [
  "evidence_id",
  "dimension_id",
  "indicator_id",
  "level",
  "turn_id",
  "turn_index",
  "speaker_alias",
  "quote",
].sort();

const TENSOR_DIMENSION_KEYS = [
  "dimension_id",
  "calculus_axis",
  "score",
  "confidence",
  "evidence",
].sort();

const TENSOR_PROFILE_KEYS = ["schema", "dimensions"].sort();

function medianInt(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const n = sorted.length;
  const mid = Math.floor(n / 2);
  if (n % 2 === 1) return sorted[mid];
  return (sorted[mid - 1] + sorted[mid]) / 2;
}

function parseTensorEvidence(
  raw: unknown,
  path: string,
  parentDimensionId: TensorDimensionId,
): TensorEvidenceV1 {
  if (!isPlainObject(raw)) throw new ConsultResponseParseError(path, "object");
  expectExactKeys(raw, TENSOR_EVIDENCE_KEYS, path);

  const evidence_id = parseHex32(raw.evidence_id, `${path}.evidence_id`);

  if (typeof raw.dimension_id !== "string" || !isTensorDimensionId(raw.dimension_id)) {
    throw new ConsultResponseParseError(`${path}.dimension_id`, "TensorDimensionId");
  }
  if (raw.dimension_id !== parentDimensionId) {
    throw new ConsultResponseParseError(`${path}.dimension_id`, "match parent dimension_id");
  }
  const dimension_id = raw.dimension_id;

  const allowedIndicators = INDICATOR_IDS[dimension_id];
  if (typeof raw.indicator_id !== "string" || !allowedIndicators.includes(raw.indicator_id)) {
    throw new ConsultResponseParseError(`${path}.indicator_id`, `one of ${allowedIndicators.join(",")}`);
  }
  const indicator_id = raw.indicator_id;

  const level = parseSafeInt(raw.level, `${path}.level`, 0, 4);
  const turn_id = parseStrictStr(raw.turn_id, `${path}.turn_id`);
  const turn_index = parseSafeInt(raw.turn_index, `${path}.turn_index`, 0, Number.MAX_SAFE_INTEGER);
  const speaker_alias = parseStrictStr(raw.speaker_alias, `${path}.speaker_alias`);

  if (FIRST_FIVE_DIMENSIONS.has(dimension_id) && speaker_alias !== CANDIDATE_ALIAS) {
    throw new ConsultResponseParseError(`${path}.speaker_alias`, `"${CANDIDATE_ALIAS}" for first-five dimensions`);
  }

  if (typeof raw.quote !== "string" || raw.quote.length === 0 || raw.quote.length > 120) {
    throw new ConsultResponseParseError(`${path}.quote`, "non-empty string, max 120 chars");
  }
  const quote = raw.quote;

  return { evidence_id, dimension_id, indicator_id, level, turn_id, turn_index, speaker_alias, quote };
}

function parseTensorDimension(
  raw: unknown,
  path: string,
  expectedDimensionId: TensorDimensionId,
): TensorDimensionV1 {
  if (!isPlainObject(raw)) throw new ConsultResponseParseError(path, "object");
  expectExactKeys(raw, TENSOR_DIMENSION_KEYS, path);

  if (typeof raw.dimension_id !== "string" || raw.dimension_id !== expectedDimensionId) {
    throw new ConsultResponseParseError(`${path}.dimension_id`, `"${expectedDimensionId}" (fixed order)`);
  }
  const dimension_id = expectedDimensionId;

  if (typeof raw.calculus_axis !== "string" || raw.calculus_axis !== CALCULUS_AXIS_MAP[dimension_id]) {
    throw new ConsultResponseParseError(`${path}.calculus_axis`, `"${CALCULUS_AXIS_MAP[dimension_id]}"`);
  }
  const calculus_axis = raw.calculus_axis;

  let score: number | null;
  if (raw.score === null) {
    score = null;
  } else {
    score = parseFiniteNumber(raw.score, `${path}.score`, 0, 1);
    if (round2HalfUp(score) !== score) {
      throw new ConsultResponseParseError(`${path}.score`, "rounded to 2 decimals");
    }
  }

  const confidence = parseFiniteNumber(raw.confidence, `${path}.confidence`, 0, 1);
  if (round2HalfUp(confidence) !== confidence) {
    throw new ConsultResponseParseError(`${path}.confidence`, "rounded to 2 decimals");
  }

  if (!Array.isArray(raw.evidence)) {
    throw new ConsultResponseParseError(`${path}.evidence`, "array");
  }
  const evidence: TensorEvidenceV1[] = raw.evidence.map((item, i) =>
    parseTensorEvidence(item, `${path}.evidence[${i}]`, dimension_id),
  );

  const seenIndicators = new Set<string>();
  for (const ev of evidence) {
    if (seenIndicators.has(ev.indicator_id)) {
      throw new ConsultResponseParseError(`${path}.evidence`, "no duplicate indicator_id within one dimension");
    }
    seenIndicators.add(ev.indicator_id);
  }

  // ---- score/confidence accounting: recomputed from raw evidence, mirroring
  // core/tensor_profile.py::_aggregate_dimension exactly (not range-checked
  // only — a genuine independent recomputation). ----
  const byIndicator = new Map<string, number[]>();
  const candidateTurns = new Set<number>();
  for (const ev of evidence) {
    const levels = byIndicator.get(ev.indicator_id) ?? [];
    levels.push(ev.level);
    byIndicator.set(ev.indicator_id, levels);
    if (ev.speaker_alias === CANDIDATE_ALIAS) candidateTurns.add(ev.turn_index);
  }
  const indicatorScores: number[] = [];
  for (const levels of byIndicator.values()) {
    indicatorScores.push(medianInt(levels) / 4);
  }
  const observedIndicators = byIndicator.size;
  const observedTurns = candidateTurns.size;
  const acceptedCount = evidence.length;

  const scoreValid = observedIndicators >= 2 && observedTurns >= 2;
  if (scoreValid) {
    const expectedScore = round2HalfUp(
      indicatorScores.reduce((a, b) => a + b, 0) / indicatorScores.length,
    );
    if (score === null || score !== expectedScore) {
      throw new ConsultResponseParseError(`${path}.score`, "match recomputed accounting");
    }
  } else if (score !== null) {
    throw new ConsultResponseParseError(`${path}.score`, "null (validity condition unmet)");
  }

  const indicatorCoverage = observedIndicators / 3;
  const turnCoverage = Math.min(observedTurns, 4) / 4;
  const evidenceCoverage = Math.min(acceptedCount, 6) / 6;
  const expectedConfidence = round2HalfUp(
    Math.min(1, 0.4 * indicatorCoverage + 0.35 * turnCoverage + 0.25 * evidenceCoverage),
  );
  if (confidence !== expectedConfidence) {
    throw new ConsultResponseParseError(`${path}.confidence`, "match recomputed accounting");
  }

  return { dimension_id, calculus_axis, score, confidence, evidence };
}

function parseTensorProfile(raw: unknown, path: string): TensorProfileReportV1 {
  if (!isPlainObject(raw)) throw new ConsultResponseParseError(path, "object");
  expectExactKeys(raw, TENSOR_PROFILE_KEYS, path);

  if (raw.schema !== "tensor_profile.6d.v1") {
    throw new ConsultResponseParseError(`${path}.schema`, '"tensor_profile.6d.v1"');
  }

  const rawDimensions = raw.dimensions;
  if (!Array.isArray(rawDimensions) || rawDimensions.length !== 6) {
    throw new ConsultResponseParseError(`${path}.dimensions`, "array of exactly 6 entries");
  }
  const dimensions = CANONICAL_DIMENSION_IDS.map((id, i) =>
    parseTensorDimension(rawDimensions[i], `${path}.dimensions[${i}]`, id),
  );

  return { schema: "tensor_profile.6d.v1", dimensions };
}

// ---- interview_report.py mirrors ----

const AXIS_WHITELIST = ["論理性", "技術力", "構成力", "具体性"] as const;
const INTERVIEW_REPORT_KEYS = [
  "schema",
  "date",
  "config",
  "metrics",
  "summary",
  "latency",
  "simulated",
  "tensor_profile",
].sort();
const METRIC_KEYS = ["axis", "score", "evidence"].sort();
const LATENCY_KEYS = ["median_sec", "max_sec", "n"].sort();
const DATE_RE = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}$/;

// ---- report.config strict field-by-field reconstruction (review remediation
// 1: replaces a bare `as InterviewReport["config"]` cast). config is a
// free-form dict on the Python side (Partial<InterviewConfig> for
// interview_sim, {"genre": ...} for gd_sim, `{}` in the legacy report path —
// see core/interview_report.py `config or {}`), so only the allowed-key
// subset and each present field's own type are validated; unknown keys are
// rejected outright rather than passed through. ----

const CONFIG_ALLOWED_KEYS: ReadonlySet<string> = new Set([
  "industry",
  "genre",
  "difficulty",
  "stance",
  "esId",
]);
const DIFFICULTY_VALUES = ["standard", "hard", "extreme"] as const;
const STANCE_VALUES = ["adversarial", "standard"] as const;

function isDifficultyValue(raw: unknown): raw is InterviewConfig["difficulty"] {
  return raw === "standard" || raw === "hard" || raw === "extreme";
}

function isStanceValue(raw: unknown): raw is InterviewConfig["stance"] {
  return raw === "adversarial" || raw === "standard";
}

function parseConfigStringField(raw: unknown, path: string): string {
  if (typeof raw !== "string") {
    throw new ConsultResponseParseError(path, "string (empty string allowed)");
  }
  return raw;
}

function parseInterviewConfig(raw: unknown, path: string): Partial<InterviewConfig> {
  if (!isPlainObject(raw)) {
    throw new ConsultResponseParseError(path, "object");
  }
  for (const key of Object.keys(raw)) {
    if (!CONFIG_ALLOWED_KEYS.has(key)) {
      throw new ConsultResponseParseError(path, `keys subset of ${[...CONFIG_ALLOWED_KEYS].join(",")}`);
    }
  }

  const result: Partial<InterviewConfig> = {};

  if ("industry" in raw) {
    result.industry = parseConfigStringField(raw.industry, `${path}.industry`);
  }
  if ("genre" in raw) {
    result.genre = parseConfigStringField(raw.genre, `${path}.genre`);
  }
  if ("difficulty" in raw) {
    if (!isDifficultyValue(raw.difficulty)) {
      throw new ConsultResponseParseError(`${path}.difficulty`, `one of ${DIFFICULTY_VALUES.join(",")}`);
    }
    result.difficulty = raw.difficulty;
  }
  if ("stance" in raw) {
    if (!isStanceValue(raw.stance)) {
      throw new ConsultResponseParseError(`${path}.stance`, `one of ${STANCE_VALUES.join(",")}`);
    }
    result.stance = raw.stance;
  }
  if ("esId" in raw) {
    result.esId = parseConfigStringField(raw.esId, `${path}.esId`);
  }

  return result;
}

function parseMetric(raw: unknown, path: string, seenAxes: Set<string>): InterviewReportMetric {
  if (!isPlainObject(raw)) throw new ConsultResponseParseError(path, "object");
  expectExactKeys(raw, METRIC_KEYS, path);

  if (typeof raw.axis !== "string" || !(AXIS_WHITELIST as readonly string[]).includes(raw.axis)) {
    throw new ConsultResponseParseError(`${path}.axis`, `one of ${AXIS_WHITELIST.join(",")}`);
  }
  if (seenAxes.has(raw.axis)) {
    throw new ConsultResponseParseError(path, "no duplicate axis");
  }
  seenAxes.add(raw.axis);
  const axis = raw.axis;

  const score = parseSafeInt(raw.score, `${path}.score`, 0, 100);
  const evidence = parseStrictStr(raw.evidence, `${path}.evidence`);

  return { axis, score, evidence };
}

function parseLatency(raw: unknown, path: string): InterviewReportLatency {
  if (!isPlainObject(raw)) throw new ConsultResponseParseError(path, "object");
  expectExactKeys(raw, LATENCY_KEYS, path);

  const n = parseSafeInt(raw.n, `${path}.n`, 0, Number.MAX_SAFE_INTEGER);
  const median_sec = parseFiniteNumber(raw.median_sec, `${path}.median_sec`, 0, Number.MAX_VALUE);
  const max_sec = parseFiniteNumber(raw.max_sec, `${path}.max_sec`, 0, Number.MAX_VALUE);

  if (n === 0) {
    if (median_sec !== 0 || max_sec !== 0) {
      throw new ConsultResponseParseError(path, "median_sec=0 and max_sec=0 when n=0");
    }
  } else if (median_sec > max_sec) {
    throw new ConsultResponseParseError(path, "median_sec <= max_sec");
  }

  return { median_sec, max_sec, n };
}

function parseInterviewReport(raw: unknown, path: string): InterviewReport {
  if (!isPlainObject(raw)) throw new ConsultResponseParseError(path, "object");
  expectExactKeys(raw, INTERVIEW_REPORT_KEYS, path);

  if (raw.schema !== "interview_report.v1") {
    throw new ConsultResponseParseError(`${path}.schema`, '"interview_report.v1"');
  }

  const date = parseStrictStr(raw.date, `${path}.date`);
  if (!DATE_RE.test(date)) {
    throw new ConsultResponseParseError(`${path}.date`, "ISO 8601 seconds precision");
  }

  const config = parseInterviewConfig(raw.config, `${path}.config`);

  if (!Array.isArray(raw.metrics)) {
    throw new ConsultResponseParseError(`${path}.metrics`, "array");
  }
  const seenAxes = new Set<string>();
  const metrics = raw.metrics.map((m, i) => parseMetric(m, `${path}.metrics[${i}]`, seenAxes));

  const summary = parseAnyStr(raw.summary, `${path}.summary`);
  const latency = parseLatency(raw.latency, `${path}.latency`);

  if (raw.simulated !== true) {
    throw new ConsultResponseParseError(`${path}.simulated`, "true");
  }

  const tensor_profile = parseTensorProfile(raw.tensor_profile, `${path}.tensor_profile`);

  return { schema: "interview_report.v1", date, config, metrics, summary, latency, simulated: true, tensor_profile };
}

// ---- romance_analysis.py mirror (review remediation 2: fixed tendency/action
// allowlist and the null<->insufficient coupling are now hard-validated —
// not just schema + score range. Causal agreement between a *specific*
// non-null score and a *specific* allowed tendency/action is out of scope:
// the frontend has no access to the underlying metrics validate_result()
// uses for that check (expected_tendency/expected_action), only the final
// payload.) ----

const ROMANCE_SCHEMA = "romance_analysis.v1" as const;

const ROMANCE_INSUFFICIENT_TENDENCY = "判定に必要な観測量が不足しています" as const;
const ROMANCE_INSUFFICIENT_ACTION = "会話履歴を追加して再分析する" as const;

const ROMANCE_ALLOWED_TENDENCIES = [
  "発話数とターン切り替えは概ね均衡しています",
  "発話数に偏りがあります",
  "ターン切り替えが少ない状態です",
  "観測範囲では中程度の往復です",
] as const;

const ROMANCE_ALLOWED_ACTIONS = [
  "同じ形式で履歴を追加して再分析する",
  "往復のバランスを意識して記録を続ける",
  "ターン切り替えを意識して記録を続ける",
] as const;

type RomanceTendency = (typeof ROMANCE_ALLOWED_TENDENCIES)[number] | typeof ROMANCE_INSUFFICIENT_TENDENCY;
type RomanceAction = (typeof ROMANCE_ALLOWED_ACTIONS)[number] | typeof ROMANCE_INSUFFICIENT_ACTION;

export interface RomanceAnalysisResult {
  schema: typeof ROMANCE_SCHEMA;
  affinity_score: number | null;
  interaction_tendency: RomanceTendency;
  next_best_action: RomanceAction;
}

const ROMANCE_KEYS = ["schema", "affinity_score", "interaction_tendency", "next_best_action"].sort();

function isRomanceTendency(v: string): v is RomanceTendency {
  return v === ROMANCE_INSUFFICIENT_TENDENCY || (ROMANCE_ALLOWED_TENDENCIES as readonly string[]).includes(v);
}

function isRomanceAction(v: string): v is RomanceAction {
  return v === ROMANCE_INSUFFICIENT_ACTION || (ROMANCE_ALLOWED_ACTIONS as readonly string[]).includes(v);
}

function parseRomanceAnalysis(raw: unknown, path: string): RomanceAnalysisResult {
  if (!isPlainObject(raw)) throw new ConsultResponseParseError(path, "object");
  expectExactKeys(raw, ROMANCE_KEYS, path);

  if (raw.schema !== ROMANCE_SCHEMA) {
    throw new ConsultResponseParseError(`${path}.schema`, `"${ROMANCE_SCHEMA}"`);
  }
  const schema = raw.schema;

  let affinity_score: number | null;
  if (raw.affinity_score === null) {
    affinity_score = null;
  } else {
    affinity_score = parseSafeInt(raw.affinity_score, `${path}.affinity_score`, 0, 100);
  }

  if (typeof raw.interaction_tendency !== "string" || !isRomanceTendency(raw.interaction_tendency)) {
    throw new ConsultResponseParseError(
      `${path}.interaction_tendency`,
      `one of ${[...ROMANCE_ALLOWED_TENDENCIES, ROMANCE_INSUFFICIENT_TENDENCY].join(",")}`,
    );
  }
  const interaction_tendency = raw.interaction_tendency;

  if (typeof raw.next_best_action !== "string" || !isRomanceAction(raw.next_best_action)) {
    throw new ConsultResponseParseError(
      `${path}.next_best_action`,
      `one of ${[...ROMANCE_ALLOWED_ACTIONS, ROMANCE_INSUFFICIENT_ACTION].join(",")}`,
    );
  }
  const next_best_action = raw.next_best_action;

  // score<->text coupling (mirrors validate_result: null iff insufficient).
  if (affinity_score === null) {
    if (interaction_tendency !== ROMANCE_INSUFFICIENT_TENDENCY || next_best_action !== ROMANCE_INSUFFICIENT_ACTION) {
      throw new ConsultResponseParseError(path, "insufficient tendency/action when affinity_score is null");
    }
  } else if (
    interaction_tendency === ROMANCE_INSUFFICIENT_TENDENCY ||
    next_best_action === ROMANCE_INSUFFICIENT_ACTION
  ) {
    throw new ConsultResponseParseError(path, "non-insufficient tendency/action when affinity_score is non-null");
  }

  return { schema, affinity_score, interaction_tendency, next_best_action };
}

// ---- envelope (engine_stdio.py `consult` cmd handler) ----
// review remediation 3: mode-discriminated union — `romance_analysis` is
// required (not merely optionally-allowed) on the romance_analysis branch,
// matching consultation_engine.py always assigning `_last_romance_analysis`
// before returning from that mode.

const KNOWN_MODES = ["consult", "interview_sim", "es_review", "gd_sim", "romance_analysis"] as const;
type KnownMode = (typeof KNOWN_MODES)[number];

interface ConsultResponseBase {
  query: string;
  answer: string;
}

export type ConsultResponse =
  | (ConsultResponseBase & {
      mode: "consult" | "es_review";
      report?: never;
      romance_analysis?: never;
    })
  | (ConsultResponseBase & {
      mode: "interview_sim" | "gd_sim";
      report?: InterviewReport;
      romance_analysis?: never;
    })
  | (ConsultResponseBase & {
      mode: "romance_analysis";
      report?: never;
      romance_analysis: RomanceAnalysisResult;
    });

export function parseConsultResponse(raw: unknown): ConsultResponse {
  if (!isPlainObject(raw)) {
    throw new ConsultResponseParseError("", "object");
  }

  if (typeof raw.mode !== "string" || !(KNOWN_MODES as readonly string[]).includes(raw.mode)) {
    throw new ConsultResponseParseError("mode", `one of ${KNOWN_MODES.join(",")}`);
  }
  const mode = raw.mode as KnownMode;

  const hasReport = Object.prototype.hasOwnProperty.call(raw, "report");
  const hasRomance = Object.prototype.hasOwnProperty.call(raw, "romance_analysis");

  const reportAllowed = mode === "interview_sim" || mode === "gd_sim";
  const romanceRequired = mode === "romance_analysis";

  if (hasReport && !reportAllowed) {
    throw new ConsultResponseParseError("report", "absent (report only allowed for interview_sim/gd_sim)");
  }
  if (hasRomance && !romanceRequired) {
    throw new ConsultResponseParseError(
      "romance_analysis",
      "absent (romance_analysis only allowed for romance_analysis mode)",
    );
  }
  if (romanceRequired && !hasRomance) {
    throw new ConsultResponseParseError(
      "romance_analysis",
      "present (romance_analysis is required for romance_analysis mode)",
    );
  }

  const expectedKeys = ["query", "mode", "answer"];
  if (hasReport) expectedKeys.push("report");
  if (hasRomance) expectedKeys.push("romance_analysis");
  expectExactKeys(raw, expectedKeys.sort(), "");

  const query = parseAnyStr(raw.query, "query");
  const answer = parseAnyStr(raw.answer, "answer");

  if (mode === "consult" || mode === "es_review") {
    return { query, mode, answer };
  }
  if (mode === "interview_sim" || mode === "gd_sim") {
    return {
      query,
      mode,
      answer,
      ...(hasReport ? { report: parseInterviewReport(raw.report, "report") } : {}),
    };
  }
  return {
    query,
    mode,
    answer,
    romance_analysis: parseRomanceAnalysis(raw.romance_analysis, "romance_analysis"),
  };
}
