import type {
  ClassifyResult,
  EngineEvent,
  EsView,
  ProbeAnswerResult,
  ProbeAxis,
  ProbeAxisStatus,
  ProbeInsightView,
  ProbeQuestionView,
  ProbeStage,
  ProbeStatus,
  RecordData,
  SettingsData,
  SourceCodeView,
  SourceStat,
} from "./types";


type JsonObject = Record<string, unknown>;

const PROBE_AXES: readonly ProbeAxis[] = [
  "decision_threshold",
  "reward_bias",
  "locus_of_control",
  "unlearning_rate",
  "friction_energy_ledger",
];
const PROBE_STAGES: readonly ProbeStage[] = ["FACT", "CONTEXT", "EMOTION", "MEANING"];


function fail(path: string, reason: string): never {
  throw new Error(`${path}: ${reason}`);
}


function asObject(value: unknown, path: string): JsonObject {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    fail(path, "expected object");
  }
  return value as JsonObject;
}


function exactObject(
  value: unknown,
  required: readonly string[],
  optional: readonly string[],
  path: string,
): JsonObject {
  const object = asObject(value, path);
  const allowed = new Set([...required, ...optional]);
  for (const key of Object.keys(object)) {
    if (!allowed.has(key)) fail(path, "unexpected field");
  }
  for (const key of required) {
    if (!Object.prototype.hasOwnProperty.call(object, key)) fail(path, "missing field");
  }
  return object;
}


function asString(value: unknown, path: string): string {
  if (typeof value !== "string") fail(path, "expected string");
  return value;
}


function asBoolean(value: unknown, path: string): boolean {
  if (typeof value !== "boolean") fail(path, "expected boolean");
  return value;
}


function asFiniteNumber(value: unknown, path: string): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    fail(path, "expected finite number");
  }
  return value;
}


function asSafeInteger(value: unknown, path: string): number {
  const number = asFiniteNumber(value, path);
  if (!Number.isSafeInteger(number)) fail(path, "expected safe integer");
  return number;
}


function asNonNegativeInteger(value: unknown, path: string): number {
  const number = asSafeInteger(value, path);
  if (number < 0) fail(path, "expected non-negative integer");
  return number;
}


function asArray(value: unknown, path: string): unknown[] {
  if (!Array.isArray(value)) fail(path, "expected array");
  return value;
}


function asLiteral<T extends string>(
  value: unknown,
  allowed: readonly T[],
  path: string,
): T {
  if (typeof value !== "string" || !allowed.includes(value as T)) {
    fail(path, "unexpected literal");
  }
  return value as T;
}


function asNullableString(value: unknown, path: string): string | null {
  return value === null ? null : asString(value, path);
}


function asNullableNumber(value: unknown, path: string): number | null {
  return value === null ? null : asFiniteNumber(value, path);
}


function optionalString(object: JsonObject, key: string, path: string): string | undefined {
  return Object.prototype.hasOwnProperty.call(object, key)
    ? asString(object[key], `${path}.${key}`)
    : undefined;
}


function optionalBoolean(object: JsonObject, key: string, path: string): boolean | undefined {
  return Object.prototype.hasOwnProperty.call(object, key)
    ? asBoolean(object[key], `${path}.${key}`)
    : undefined;
}


function optionalNumber(object: JsonObject, key: string, path: string): number | undefined {
  return Object.prototype.hasOwnProperty.call(object, key)
    ? asFiniteNumber(object[key], `${path}.${key}`)
    : undefined;
}


function parseStringArray(value: unknown, path: string): string[] {
  return asArray(value, path).map((item, index) => asString(item, `${path}[${index}]`));
}


function parseNumberArray(value: unknown, path: string): number[] {
  return asArray(value, path).map((item, index) => asFiniteNumber(item, `${path}[${index}]`));
}


function parseNullableNumberArray(value: unknown, path: string): (number | null)[] {
  return asArray(value, path).map((item, index) => asNullableNumber(item, `${path}[${index}]`));
}


function parseNumberRecord(value: unknown, path: string): Record<string, number> {
  const object = asObject(value, path);
  const result: Record<string, number> = {};
  for (const [key, item] of Object.entries(object)) {
    result[key] = asFiniteNumber(item, `${path}.${key}`);
  }
  return result;
}


function parseStringRecord(value: unknown, path: string): Record<string, string> {
  const object = asObject(value, path);
  const result: Record<string, string> = {};
  for (const [key, item] of Object.entries(object)) {
    result[key] = asString(item, `${path}.${key}`);
  }
  return result;
}


export function parseBoolean(value: unknown): boolean {
  return asBoolean(value, "engine_ready");
}


export interface EngineHealth {
  status: "ok";
  offline: true;
}


export function parseEngineHealth(value: unknown): EngineHealth {
  const object = exactObject(value, ["status", "offline"], [], "health");
  if (object.status !== "ok" || object.offline !== true) fail("health", "invalid state");
  return { status: "ok", offline: true };
}


export function parseRecordData(value: unknown): RecordData {
  const object = exactObject(value, ["date", "events", "transactions", "diary"], [], "record");
  const events = asArray(object.events, "record.events").map((item, index) => {
    const event = exactObject(item, ["time", "title"], [], `record.events[${index}]`);
    return {
      time: asString(event.time, `record.events[${index}].time`),
      title: asString(event.title, `record.events[${index}].title`),
    };
  });
  const transactions = asArray(object.transactions, "record.transactions").map((item, index) => {
    const transaction = exactObject(
      item,
      ["type", "category", "amount"],
      [],
      `record.transactions[${index}]`,
    );
    return {
      type: asLiteral(
        transaction.type,
        ["expense", "income"] as const,
        `record.transactions[${index}].type`,
      ),
      category: asString(transaction.category, `record.transactions[${index}].category`),
      amount: asFiniteNumber(transaction.amount, `record.transactions[${index}].amount`),
    };
  });
  return {
    date: asString(object.date, "record.date"),
    events,
    transactions,
    diary: asString(object.diary, "record.diary"),
  };
}


export interface RecordSaveResult {
  date: string;
  saved: true;
  index_rebuilt: boolean;
}


export function parseRecordSaveResult(value: unknown): RecordSaveResult {
  const object = exactObject(value, ["date", "saved", "index_rebuilt"], [], "record.save");
  if (object.saved !== true) fail("record.save.saved", "expected true");
  return {
    date: asString(object.date, "record.save.date"),
    saved: true,
    index_rebuilt: asBoolean(object.index_rebuilt, "record.save.index_rebuilt"),
  };
}


export interface CalendarEventDatesResult {
  dates: string[];
}


export function parseCalendarEventDatesResult(value: unknown): CalendarEventDatesResult {
  const object = exactObject(value, ["dates"], [], "calendar.event_dates");
  return { dates: parseStringArray(object.dates, "calendar.event_dates.dates") };
}


function parseSourceStat(value: unknown, path: string): SourceStat {
  const object = exactObject(value, ["exists", "count", "mtime"], [], path);
  return {
    exists: asBoolean(object.exists, `${path}.exists`),
    count: asNonNegativeInteger(object.count, `${path}.count`),
    mtime: asNullableString(object.mtime, `${path}.mtime`),
  };
}


export function parseImportStats(value: unknown): Record<string, SourceStat> {
  const names = ["diary", "line", "calendar", "finance", "es", "knowledge"] as const;
  const object = exactObject(value, names, [], "import.stats");
  const result: Record<string, SourceStat> = {};
  for (const name of names) result[name] = parseSourceStat(object[name], `import.stats.${name}`);
  return result;
}


export function parseEsView(value: unknown): EsView {
  const root = asObject(value, "es.view");
  if (root.exists === false) {
    exactObject(root, ["exists"], [], "es.view");
    return { exists: false };
  }
  const object = exactObject(
    root,
    ["exists", "title", "target_domain", "keywords", "body", "char_count", "mtime"],
    [],
    "es.view",
  );
  if (object.exists !== true) fail("es.view.exists", "expected boolean state");
  return {
    exists: true,
    title: asString(object.title, "es.view.title"),
    target_domain: asString(object.target_domain, "es.view.target_domain"),
    keywords: parseStringArray(object.keywords, "es.view.keywords"),
    body: asString(object.body, "es.view.body"),
    char_count: asNonNegativeInteger(object.char_count, "es.view.char_count"),
    mtime: asNullableString(object.mtime, "es.view.mtime"),
  };
}


export interface CalendarSyncResult {
  source: string;
  mode: "append" | "overwrite";
  imported_events: number;
  imported_dates: number;
  total_events: number;
  index_rebuilt: boolean;
  archived_to?: string;
  db_count?: number;
  count?: number;
  filenames?: string[];
  message?: string;
}


export function parseCalendarSyncResult(value: unknown): CalendarSyncResult {
  const object = exactObject(
    value,
    ["source", "mode", "imported_events", "imported_dates", "total_events", "index_rebuilt"],
    ["archived_to", "db_count", "count", "filenames", "message"],
    "calendar.sync",
  );
  const result: CalendarSyncResult = {
    source: asString(object.source, "calendar.sync.source"),
    mode: asLiteral(object.mode, ["append", "overwrite"] as const, "calendar.sync.mode"),
    imported_events: asNonNegativeInteger(object.imported_events, "calendar.sync.imported_events"),
    imported_dates: asNonNegativeInteger(object.imported_dates, "calendar.sync.imported_dates"),
    total_events: asNonNegativeInteger(object.total_events, "calendar.sync.total_events"),
    index_rebuilt: asBoolean(object.index_rebuilt, "calendar.sync.index_rebuilt"),
  };
  const archived = optionalString(object, "archived_to", "calendar.sync");
  if (archived !== undefined) result.archived_to = archived;
  if (Object.prototype.hasOwnProperty.call(object, "db_count")) {
    result.db_count = asNonNegativeInteger(object.db_count, "calendar.sync.db_count");
  }
  if (Object.prototype.hasOwnProperty.call(object, "count")) {
    result.count = asNonNegativeInteger(object.count, "calendar.sync.count");
  }
  if (Object.prototype.hasOwnProperty.call(object, "filenames")) {
    result.filenames = parseStringArray(object.filenames, "calendar.sync.filenames");
  }
  const message = optionalString(object, "message", "calendar.sync");
  if (message !== undefined) result.message = message;
  return result;
}


export interface LineImportResult {
  imported: true;
  ok: boolean;
  message: string;
  filename?: string;
  count?: number;
  filenames?: string[];
}


export function parseLineImportResult(value: unknown): LineImportResult {
  const object = exactObject(
    value,
    ["imported", "ok", "message"],
    ["filename", "count", "filenames"],
    "import.line",
  );
  if (object.imported !== true) fail("import.line.imported", "expected true");
  const result: LineImportResult = {
    imported: true,
    ok: asBoolean(object.ok, "import.line.ok"),
    message: asString(object.message, "import.line.message"),
  };
  const filename = optionalString(object, "filename", "import.line");
  if (filename !== undefined) result.filename = filename;
  if (Object.prototype.hasOwnProperty.call(object, "count")) {
    result.count = asNonNegativeInteger(object.count, "import.line.count");
  }
  if (Object.prototype.hasOwnProperty.call(object, "filenames")) {
    result.filenames = parseStringArray(object.filenames, "import.line.filenames");
  }
  const single = result.filename !== undefined && result.count === undefined && result.filenames === undefined;
  const batch = result.filename === undefined && result.count !== undefined && result.filenames !== undefined;
  if (!single && !batch) fail("import.line", "invalid result variant");
  return result;
}


export function parseClassifyResult(value: unknown): ClassifyResult {
  const object = exactObject(value, ["type", "reasons", "size", "filename"], [], "import.classify");
  return {
    type: asLiteral(
      object.type,
      ["line", "ics", "es", "knowledge", "reject"] as const,
      "import.classify.type",
    ),
    reasons: parseStringArray(object.reasons, "import.classify.reasons"),
    size: asNonNegativeInteger(object.size, "import.classify.size"),
    filename: asString(object.filename, "import.classify.filename"),
  };
}


export interface DocumentImportResult {
  imported: boolean;
  skipped: boolean;
  dest: "es" | "knowledge";
  path?: string;
  message?: string;
  index_rebuilt?: boolean;
}


export function parseDocumentImportResult(value: unknown): DocumentImportResult {
  const object = exactObject(
    value,
    ["imported", "skipped", "dest"],
    ["path", "message", "index_rebuilt"],
    "import.document",
  );
  const result: DocumentImportResult = {
    imported: asBoolean(object.imported, "import.document.imported"),
    skipped: asBoolean(object.skipped, "import.document.skipped"),
    dest: asLiteral(object.dest, ["es", "knowledge"] as const, "import.document.dest"),
  };
  if (result.imported === result.skipped) fail("import.document", "invalid import state");
  const path = optionalString(object, "path", "import.document");
  if (path !== undefined) result.path = path;
  const message = optionalString(object, "message", "import.document");
  if (message !== undefined) result.message = message;
  const rebuilt = optionalBoolean(object, "index_rebuilt", "import.document");
  if (rebuilt !== undefined) result.index_rebuilt = rebuilt;
  return result;
}


export function parseSettingsData(value: unknown): SettingsData {
  const object = exactObject(
    value,
    ["fixed_fields", "fixed_attributes", "profile_summary", "apple_calendar_available"],
    [],
    "settings.get",
  );
  const fixedFields = asArray(object.fixed_fields, "settings.get.fixed_fields").map((item, index) => {
    const field = exactObject(item, ["key", "label"], [], `settings.get.fixed_fields[${index}]`);
    return {
      key: asString(field.key, `settings.get.fixed_fields[${index}].key`),
      label: asString(field.label, `settings.get.fixed_fields[${index}].label`),
    };
  });
  return {
    fixed_fields: fixedFields,
    fixed_attributes: parseStringRecord(object.fixed_attributes, "settings.get.fixed_attributes"),
    profile_summary: asString(object.profile_summary, "settings.get.profile_summary"),
    apple_calendar_available: asBoolean(
      object.apple_calendar_available,
      "settings.get.apple_calendar_available",
    ),
  };
}


export interface SavedResult {
  saved: true;
}


export function parseSavedResult(value: unknown): SavedResult {
  const object = exactObject(value, ["saved"], [], "settings.save_fixed");
  if (object.saved !== true) fail("settings.save_fixed.saved", "expected true");
  return { saved: true };
}


export interface ProfilerResult {
  ok: boolean;
  message: string;
}


export function parseProfilerResult(value: unknown): ProfilerResult {
  const object = exactObject(value, ["ok", "message"], [], "settings.run_profiler");
  return {
    ok: asBoolean(object.ok, "settings.run_profiler.ok"),
    message: asString(object.message, "settings.run_profiler.message"),
  };
}


export interface OraclePayload {
  schema: "oracle_payload.v1";
  generated: string;
  scope: { kind: "global" | "dyad"; alias: string | null };
  sufficiency: {
    days_observed: number;
    coverage: number;
    dead_lanes: string[];
    twin_bss: number;
    n_lapse_test: number;
    gate_passed: boolean;
  };
  state: {
    r_now: number | null;
    r_trend_7d: number | null;
    oii_ema: number | null;
    oii_streak_days: number;
  };
  couplings: {
    src: string;
    dst: string;
    lag_days: number;
    rho: number;
    n_eff: number;
    null_q99: number;
    sig: boolean;
  }[];
  forecast: {
    horizon_days: number;
    r_q10: number[];
    r_q50: number[];
    r_q90: number[];
    p_lapse: number[];
    critical_days: string[];
  };
  findings: { rule_id: string; severity: number; metrics: Record<string, number> }[];
  interventions: {
    bank_id: string;
    trigger_rule: string;
    target_lane: number;
    params: Record<string, number>;
  }[];
}


export function parseOraclePayload(value: unknown): OraclePayload {
  const object = exactObject(
    value,
    ["schema", "generated", "scope", "sufficiency", "state", "couplings", "forecast", "findings", "interventions"],
    [],
    "oracle.payload",
  );
  if (object.schema !== "oracle_payload.v1") fail("oracle.payload.schema", "unexpected literal");
  const scope = exactObject(object.scope, ["kind", "alias"], [], "oracle.payload.scope");
  const sufficiency = exactObject(
    object.sufficiency,
    ["days_observed", "coverage", "dead_lanes", "twin_bss", "n_lapse_test", "gate_passed"],
    [],
    "oracle.payload.sufficiency",
  );
  const state = exactObject(
    object.state,
    ["r_now", "r_trend_7d", "oii_ema", "oii_streak_days"],
    [],
    "oracle.payload.state",
  );
  const couplings = asArray(object.couplings, "oracle.payload.couplings").map((item, index) => {
    const path = `oracle.payload.couplings[${index}]`;
    const entry = exactObject(
      item,
      ["src", "dst", "lag_days", "rho", "n_eff", "null_q99", "sig"],
      [],
      path,
    );
    return {
      src: asString(entry.src, `${path}.src`),
      dst: asString(entry.dst, `${path}.dst`),
      lag_days: asSafeInteger(entry.lag_days, `${path}.lag_days`),
      rho: asFiniteNumber(entry.rho, `${path}.rho`),
      n_eff: asNonNegativeInteger(entry.n_eff, `${path}.n_eff`),
      null_q99: asFiniteNumber(entry.null_q99, `${path}.null_q99`),
      sig: asBoolean(entry.sig, `${path}.sig`),
    };
  });
  const forecast = exactObject(
    object.forecast,
    ["horizon_days", "r_q10", "r_q50", "r_q90", "p_lapse", "critical_days"],
    [],
    "oracle.payload.forecast",
  );
  const findings = asArray(object.findings, "oracle.payload.findings").map((item, index) => {
    const path = `oracle.payload.findings[${index}]`;
    const entry = exactObject(item, ["rule_id", "severity", "metrics"], [], path);
    return {
      rule_id: asString(entry.rule_id, `${path}.rule_id`),
      severity: asFiniteNumber(entry.severity, `${path}.severity`),
      metrics: parseNumberRecord(entry.metrics, `${path}.metrics`),
    };
  });
  const interventions = asArray(object.interventions, "oracle.payload.interventions").map(
    (item, index) => {
      const path = `oracle.payload.interventions[${index}]`;
      const entry = exactObject(
        item,
        ["bank_id", "trigger_rule", "target_lane", "params"],
        [],
        path,
      );
      return {
        bank_id: asString(entry.bank_id, `${path}.bank_id`),
        trigger_rule: asString(entry.trigger_rule, `${path}.trigger_rule`),
        target_lane: asSafeInteger(entry.target_lane, `${path}.target_lane`),
        params: parseNumberRecord(entry.params, `${path}.params`),
      };
    },
  );
  return {
    schema: "oracle_payload.v1",
    generated: asString(object.generated, "oracle.payload.generated"),
    scope: {
      kind: asLiteral(scope.kind, ["global", "dyad"] as const, "oracle.payload.scope.kind"),
      alias: asNullableString(scope.alias, "oracle.payload.scope.alias"),
    },
    sufficiency: {
      days_observed: asNonNegativeInteger(
        sufficiency.days_observed,
        "oracle.payload.sufficiency.days_observed",
      ),
      coverage: asFiniteNumber(sufficiency.coverage, "oracle.payload.sufficiency.coverage"),
      dead_lanes: parseStringArray(
        sufficiency.dead_lanes,
        "oracle.payload.sufficiency.dead_lanes",
      ),
      twin_bss: asFiniteNumber(sufficiency.twin_bss, "oracle.payload.sufficiency.twin_bss"),
      n_lapse_test: asNonNegativeInteger(
        sufficiency.n_lapse_test,
        "oracle.payload.sufficiency.n_lapse_test",
      ),
      gate_passed: asBoolean(
        sufficiency.gate_passed,
        "oracle.payload.sufficiency.gate_passed",
      ),
    },
    state: {
      r_now: asNullableNumber(state.r_now, "oracle.payload.state.r_now"),
      r_trend_7d: asNullableNumber(state.r_trend_7d, "oracle.payload.state.r_trend_7d"),
      oii_ema: asNullableNumber(state.oii_ema, "oracle.payload.state.oii_ema"),
      oii_streak_days: asNonNegativeInteger(
        state.oii_streak_days,
        "oracle.payload.state.oii_streak_days",
      ),
    },
    couplings,
    forecast: {
      horizon_days: asNonNegativeInteger(
        forecast.horizon_days,
        "oracle.payload.forecast.horizon_days",
      ),
      r_q10: parseNumberArray(forecast.r_q10, "oracle.payload.forecast.r_q10"),
      r_q50: parseNumberArray(forecast.r_q50, "oracle.payload.forecast.r_q50"),
      r_q90: parseNumberArray(forecast.r_q90, "oracle.payload.forecast.r_q90"),
      p_lapse: parseNumberArray(forecast.p_lapse, "oracle.payload.forecast.p_lapse"),
      critical_days: parseStringArray(
        forecast.critical_days,
        "oracle.payload.forecast.critical_days",
      ),
    },
    findings,
    interventions,
  };
}


export interface OracleReportResult {
  payload: OraclePayload;
  analysis: string;
}


export function parseOracleReport(value: unknown): OracleReportResult {
  const object = exactObject(value, ["payload", "analysis"], [], "oracle.report");
  return {
    payload: parseOraclePayload(object.payload),
    analysis: asString(object.analysis, "oracle.report.analysis"),
  };
}


export interface TwinForecast {
  gate_passed: boolean;
  reason?: string;
  bss?: number;
  n_lapse_test?: number;
  horizon_days?: number;
  r_q10?: number[];
  r_q50?: number[];
  r_q90?: number[];
  p_lapse?: (number | null)[];
  critical_days?: string[];
}


export function parseTwinForecast(value: unknown): TwinForecast {
  const object = exactObject(
    value,
    ["gate_passed"],
    ["reason", "bss", "n_lapse_test", "horizon_days", "r_q10", "r_q50", "r_q90", "p_lapse", "critical_days"],
    "twin.forecast",
  );
  const result: TwinForecast = {
    gate_passed: asBoolean(object.gate_passed, "twin.forecast.gate_passed"),
  };
  const reason = optionalString(object, "reason", "twin.forecast");
  if (reason !== undefined) result.reason = reason;
  const bss = optionalNumber(object, "bss", "twin.forecast");
  if (bss !== undefined) result.bss = bss;
  if (Object.prototype.hasOwnProperty.call(object, "n_lapse_test")) {
    result.n_lapse_test = asNonNegativeInteger(object.n_lapse_test, "twin.forecast.n_lapse_test");
  }
  if (Object.prototype.hasOwnProperty.call(object, "horizon_days")) {
    result.horizon_days = asNonNegativeInteger(object.horizon_days, "twin.forecast.horizon_days");
  }
  for (const key of ["r_q10", "r_q50", "r_q90"] as const) {
    if (Object.prototype.hasOwnProperty.call(object, key)) {
      result[key] = parseNumberArray(object[key], `twin.forecast.${key}`);
    }
  }
  if (Object.prototype.hasOwnProperty.call(object, "p_lapse")) {
    result.p_lapse = parseNullableNumberArray(object.p_lapse, "twin.forecast.p_lapse");
  }
  if (Object.prototype.hasOwnProperty.call(object, "critical_days")) {
    result.critical_days = parseStringArray(object.critical_days, "twin.forecast.critical_days");
  }
  return result;
}


export interface TensorRebuildResult {
  rebuilt: boolean;
  rows: number;
}


export function parseTensorRebuildResult(value: unknown): TensorRebuildResult {
  const object = exactObject(value, ["rebuilt", "rows"], [], "tensor.rebuild");
  return {
    rebuilt: asBoolean(object.rebuilt, "tensor.rebuild.rebuilt"),
    rows: asNonNegativeInteger(object.rows, "tensor.rebuild.rows"),
  };
}


export function parseSourceCodeView(value: unknown): SourceCodeView {
  const object = exactObject(value, ["schema", "axes"], ["progress", "updated"], "profile.source_code");
  const axesObject = asObject(object.axes, "profile.source_code.axes");
  const axes: SourceCodeView["axes"] = {};
  for (const [name, rawAxis] of Object.entries(axesObject)) {
    const path = `profile.source_code.axes.${name}`;
    const axis = exactObject(rawAxis, ["score", "confidence", "evidence", "updated"], [], path);
    axes[name] = {
      score: asNullableNumber(axis.score, `${path}.score`),
      confidence: asFiniteNumber(axis.confidence, `${path}.confidence`),
      evidence: asArray(axis.evidence, `${path}.evidence`).map((item, index) => {
        const evidencePath = `${path}.evidence[${index}]`;
        const evidence = exactObject(
          item,
          ["kind", "date", "quote", "value"],
          [],
          evidencePath,
        );
        return {
          kind: asString(evidence.kind, `${evidencePath}.kind`),
          date: asString(evidence.date, `${evidencePath}.date`),
          quote: asString(evidence.quote, `${evidencePath}.quote`),
          value: asNullableNumber(evidence.value, `${evidencePath}.value`),
        };
      }),
      updated: asString(axis.updated, `${path}.updated`),
    };
  }
  const result: SourceCodeView = {
    schema: asString(object.schema, "profile.source_code.schema"),
    axes,
  };
  const progress = optionalNumber(object, "progress", "profile.source_code");
  if (progress !== undefined) result.progress = progress;
  const updated = optionalString(object, "updated", "profile.source_code");
  if (updated !== undefined) result.updated = updated;
  return result;
}


export interface NarrativeClaim {
  text: string;
  node_refs: number[];
}


export interface NarrativeCompileResult {
  ok: boolean;
  es_text?: string;
  recruiters_eye?: string;
  claims?: NarrativeClaim[];
  compiled_from?: string;
  target_domain?: string;
  draft_path?: string;
  reason?: string;
}


export function parseNarrativeCompileResult(value: unknown): NarrativeCompileResult {
  const object = exactObject(
    value,
    ["ok"],
    ["es_text", "recruiters_eye", "claims", "compiled_from", "target_domain", "draft_path", "reason"],
    "narrative.compile",
  );
  const result: NarrativeCompileResult = {
    ok: asBoolean(object.ok, "narrative.compile.ok"),
  };
  for (const key of [
    "es_text",
    "recruiters_eye",
    "compiled_from",
    "target_domain",
    "draft_path",
    "reason",
  ] as const) {
    const parsed = optionalString(object, key, "narrative.compile");
    if (parsed !== undefined) result[key] = parsed;
  }
  if (Object.prototype.hasOwnProperty.call(object, "claims")) {
    result.claims = asArray(object.claims, "narrative.compile.claims").map((item, index) => {
      const claimPath = `narrative.compile.claims[${index}]`;
      const claim = exactObject(item, ["text", "node_refs"], [], claimPath);
      const nodeRefs = asArray(claim.node_refs, `${claimPath}.node_refs`).map((node, refIndex) => {
        const parsed = asSafeInteger(node, `${claimPath}.node_refs[${refIndex}]`);
        if (parsed < 1) fail(`${claimPath}.node_refs[${refIndex}]`, "expected positive integer");
        return parsed;
      });
      return {
        text: asString(claim.text, `${claimPath}.text`),
        node_refs: nodeRefs,
      };
    });
  }
  return result;
}


export interface KnowledgeFetchSummary {
  processed: number;
  pending: number;
  online_allowed: false;
  index_rebuilt?: boolean;
  message?: string;
}


export function parseKnowledgeFetchSummary(value: unknown): KnowledgeFetchSummary {
  const object = exactObject(
    value,
    ["processed", "pending", "online_allowed"],
    ["index_rebuilt", "message"],
    "knowledge.fetch_pending",
  );
  if (object.online_allowed !== false) {
    fail("knowledge.fetch_pending.online_allowed", "expected false");
  }
  const result: KnowledgeFetchSummary = {
    processed: asNonNegativeInteger(object.processed, "knowledge.fetch_pending.processed"),
    pending: asNonNegativeInteger(object.pending, "knowledge.fetch_pending.pending"),
    online_allowed: false,
  };
  const rebuilt = optionalBoolean(object, "index_rebuilt", "knowledge.fetch_pending");
  if (rebuilt !== undefined) result.index_rebuilt = rebuilt;
  const message = optionalString(object, "message", "knowledge.fetch_pending");
  if (message !== undefined) result.message = message;
  return result;
}


const RESEARCH_ID_RE = /^[0-9a-f]{64}$/;


export interface KnowledgeResearchReceipt {
  schema: "knowledge_research_receipt.v1";
  research_id: string;
  results_persisted: number;
}


export function parseKnowledgeResearchReceipt(value: unknown): KnowledgeResearchReceipt {
  const object = exactObject(
    value,
    ["schema", "research_id", "results_persisted"],
    [],
    "knowledge_research_receipt",
  );
  if (object.schema !== "knowledge_research_receipt.v1") {
    fail("knowledge_research_receipt.schema", "expected knowledge_research_receipt.v1");
  }
  if (typeof object.research_id !== "string" || !RESEARCH_ID_RE.test(object.research_id)) {
    fail("knowledge_research_receipt.research_id", "expected 64 lowercase hex");
  }
  // Reject any accidental leakage of raw external payload fields.
  for (const banned of ["query", "title", "content", "snippet", "url", "message", "error"]) {
    if (Object.prototype.hasOwnProperty.call(object, banned)) {
      fail(`knowledge_research_receipt.${banned}`, "forbidden field");
    }
  }
  return {
    schema: "knowledge_research_receipt.v1",
    research_id: object.research_id,
    results_persisted: asNonNegativeInteger(
      object.results_persisted,
      "knowledge_research_receipt.results_persisted",
    ),
  };
}


function parseProbeAxis(value: unknown, path: string): ProbeAxis {
  return asLiteral(value, PROBE_AXES, path);
}


function parseProbeStage(value: unknown, path: string): ProbeStage {
  return asLiteral(value, PROBE_STAGES, path);
}


function parseProbeQuestionAt(value: unknown, path: string): ProbeQuestionView {
  const object = exactObject(
    value,
    ["schema", "session_id", "question_id", "axis", "stage", "question", "priority"],
    [],
    path,
  );
  if (object.schema !== "probe_question.v1") fail(`${path}.schema`, "unexpected literal");
  return {
    schema: "probe_question.v1",
    session_id: asString(object.session_id, `${path}.session_id`),
    question_id: asString(object.question_id, `${path}.question_id`),
    axis: parseProbeAxis(object.axis, `${path}.axis`),
    stage: parseProbeStage(object.stage, `${path}.stage`),
    question: asString(object.question, `${path}.question`),
    priority: asFiniteNumber(object.priority, `${path}.priority`),
  };
}


export function parseProbeQuestion(value: unknown): ProbeQuestionView {
  return parseProbeQuestionAt(value, "probe.next");
}


function parseProbeAxisStatus(value: unknown, path: string): ProbeAxisStatus {
  const object = exactObject(
    value,
    ["axis", "score", "confidence", "priority", "stage", "node_count", "open_session_id"],
    [],
    path,
  );
  return {
    axis: parseProbeAxis(object.axis, `${path}.axis`),
    score: asNullableNumber(object.score, `${path}.score`),
    confidence: asFiniteNumber(object.confidence, `${path}.confidence`),
    priority: asFiniteNumber(object.priority, `${path}.priority`),
    stage: parseProbeStage(object.stage, `${path}.stage`),
    node_count: asNonNegativeInteger(object.node_count, `${path}.node_count`),
    open_session_id: asNullableString(object.open_session_id, `${path}.open_session_id`),
  };
}


function parseProbeInsight(value: unknown, path: string): ProbeInsightView {
  const object = exactObject(
    value,
    ["kind", "axis", "stage", "priority", "message_code"],
    [],
    path,
  );
  return {
    kind: asLiteral(
      object.kind,
      ["low_confidence", "under_probed", "stage_complete"] as const,
      `${path}.kind`,
    ),
    axis: parseProbeAxis(object.axis, `${path}.axis`),
    stage: parseProbeStage(object.stage, `${path}.stage`),
    priority: asFiniteNumber(object.priority, `${path}.priority`),
    message_code: asLiteral(
      object.message_code,
      ["probe.low_confidence", "probe.under_probed", "probe.stage_complete"] as const,
      `${path}.message_code`,
    ),
  };
}


export function parseProbeStatus(value: unknown): ProbeStatus {
  const object = exactObject(
    value,
    ["schema", "today", "axes", "active_session", "insights", "progress"],
    [],
    "probe.status",
  );
  if (object.schema !== "probe_status.v1") fail("probe.status.schema", "unexpected literal");
  let activeSession: ProbeStatus["active_session"] = null;
  if (object.active_session !== null) {
    const active = exactObject(
      object.active_session,
      ["id", "axis", "stage", "status"],
      [],
      "probe.status.active_session",
    );
    activeSession = {
      id: asString(active.id, "probe.status.active_session.id"),
      axis: parseProbeAxis(active.axis, "probe.status.active_session.axis"),
      stage: parseProbeStage(active.stage, "probe.status.active_session.stage"),
      status: asLiteral(
        active.status,
        ["active", "closed"] as const,
        "probe.status.active_session.status",
      ),
    };
  }
  const progress = exactObject(
    object.progress,
    ["completed_stages", "total_stages", "percent"],
    [],
    "probe.status.progress",
  );
  return {
    schema: "probe_status.v1",
    today: asString(object.today, "probe.status.today"),
    axes: asArray(object.axes, "probe.status.axes").map((item, index) =>
      parseProbeAxisStatus(item, `probe.status.axes[${index}]`),
    ),
    active_session: activeSession,
    insights: asArray(object.insights, "probe.status.insights").map((item, index) =>
      parseProbeInsight(item, `probe.status.insights[${index}]`),
    ),
    progress: {
      completed_stages: asNonNegativeInteger(
        progress.completed_stages,
        "probe.status.progress.completed_stages",
      ),
      total_stages: asNonNegativeInteger(
        progress.total_stages,
        "probe.status.progress.total_stages",
      ),
      percent: asFiniteNumber(progress.percent, "probe.status.progress.percent"),
    },
  };
}


export function parseProbeAnswerResult(value: unknown): ProbeAnswerResult {
  const object = exactObject(
    value,
    ["schema", "saved", "node_id", "session_status", "next_question", "status"],
    [],
    "probe.answer",
  );
  if (object.schema !== "probe_answer_result.v1") fail("probe.answer.schema", "unexpected literal");
  return {
    schema: "probe_answer_result.v1",
    saved: asBoolean(object.saved, "probe.answer.saved"),
    node_id: asString(object.node_id, "probe.answer.node_id"),
    session_status: asLiteral(
      object.session_status,
      ["active", "closed"] as const,
      "probe.answer.session_status",
    ),
    next_question:
      object.next_question === null
        ? null
        : parseProbeQuestionAt(object.next_question, "probe.answer.next_question"),
    status: parseProbeStatus(object.status),
  };
}


function parseEventId(value: unknown, path: string): number | undefined {
  if (value === null || value === undefined) return undefined;
  return asNonNegativeInteger(value, path);
}


export function parseEngineEvent(value: unknown): EngineEvent {
  const root = asObject(value, "engine.event");
  const event = asLiteral(root.event, ["status", "chunk"] as const, "engine.event.event");
  if (event === "status") {
    const object = exactObject(root, ["id", "cid", "event", "message"], [], "engine.event");
    return {
      id: parseEventId(object.id, "engine.event.id"),
      cid: parseEventId(object.cid, "engine.event.cid"),
      event,
      message: asString(object.message, "engine.event.message"),
    };
  }
  const object = exactObject(root, ["id", "cid", "event", "text"], [], "engine.event");
  return {
    id: parseEventId(object.id, "engine.event.id"),
    cid: parseEventId(object.cid, "engine.event.cid"),
    event,
    text: asString(object.text, "engine.event.text"),
  };
}
