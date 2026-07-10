import { invoke } from "@tauri-apps/api/core";
import { readTextLenient } from "./textDecode";
import type {
  ClassifyResult,
  EsView,
  InterviewConfig,
  InterviewReport,
  RecordData,
  SettingsData,
  SourceCodeView,
  SourceStat,
  ProbeAnswerResult,
  ProbeQuestionView,
  ProbeStatus,
} from "./types";

/**
 * SPEC_FOXTROT_UI.md §9 (Rev.10): cid はリクエストエンベロープの独立引数
 * として渡す (params には混ぜない = F-13 準拠)。省略時は Rust 側で
 * null (cid 不要なコマンド、例: health) として扱われる。
 */
export async function pkbInvoke<T = unknown>(
  cmd: string,
  params?: Record<string, unknown>,
  cid?: number,
): Promise<T> {
  return invoke<T>("pkb_invoke", { cmd, params: params ?? null, cid: cid ?? null });
}

export async function engineReady(): Promise<boolean> {
  return invoke<boolean>("pkb_engine_ready");
}

export async function engineHealth(): Promise<{ status: string; offline: boolean }> {
  return pkbInvoke("health");
}

export async function loadRecord(date: string): Promise<RecordData> {
  return pkbInvoke("record.load", { date });
}

export async function saveRecord(
  date: string,
  events: RecordData["events"],
  transactions: RecordData["transactions"],
  diary: string,
): Promise<{ saved: boolean; index_rebuilt?: boolean }> {
  return pkbInvoke("record.save", { date, events, transactions, diary });
}

export async function calendarEventDates(): Promise<string[]> {
  const res = await pkbInvoke<{ dates: string[] }>("calendar.event_dates");
  return res.dates;
}

/** F2 (SPEC_FOXTROT_UI.md §2.2.1 裁定4): IMPORT SourceTable 用の軽量 stat */
export async function importStats(): Promise<Record<string, SourceStat>> {
  return pkbInvoke("import.stats");
}

/** F-16 (SPEC_FOXTROT_UI.md §10.2): 保持ES (active_es.md) の View。
 * 状態取得の純クエリのため cid は不要。 */
export async function esView(): Promise<EsView> {
  return pkbInvoke("es.view");
}

export interface ConsultOptions {
  mode?: "consult" | "interview_sim" | "es_review" | "gd_sim" | "romance_analysis";
  personas?: { name: string; trait: string }[];
  /** AI 表示 → ユーザー送信までの経過秒 (面接/GD の思考速度評価用) */
  response_time_sec?: number;
  /** F4a: interview_sim のセッション設定 (開始ターンのみ送信すれば十分) */
  config?: InterviewConfig;
}

export interface RomanceAnalysisResult {
  schema: string;
  affinity_score: number | null;
  interaction_tendency: string;
  next_best_action: string;
}

export async function consult(
  query: string,
  opts: ConsultOptions = {},
  cid?: number,
): Promise<{
  query: string;
  mode?: string;
  answer: string;
  report?: InterviewReport;
  romance_analysis?: RomanceAnalysisResult;
}> {
  return pkbInvoke("consult", { query, ...opts }, cid);
}

export async function syncIcsContent(
  content: string,
  mode: "append" | "overwrite",
  cid?: number,
): Promise<Record<string, unknown>> {
  return pkbInvoke("calendar.sync", { source: "ics", mode, ics_content: content }, cid);
}

export async function syncIcsFiles(
  files: File[],
  mode: "append" | "overwrite",
  cid?: number,
): Promise<{ message?: string }> {
  if (files.length === 0) return { message: "ファイルが選択されていません" };
  if (files.length === 1) {
    const content = await readTextLenient(files[0]);
    return syncIcsContent(content, mode, cid);
  }
  const ics_files = await Promise.all(
    files.map(async (file) => ({ content: await readTextLenient(file), filename: file.name })),
  );
  return pkbInvoke("calendar.sync", { source: "ics", mode, ics_files }, cid);
}

export async function syncAppleCalendar(
  mode: "append" | "overwrite",
  cid?: number,
): Promise<Record<string, unknown>> {
  return pkbInvoke("calendar.sync", { source: "apple", mode }, cid);
}

export async function importLineFile(
  file: File,
  cid?: number,
): Promise<{ message?: string; ok?: boolean }> {
  const content = await readTextLenient(file);
  return pkbInvoke("import.line", { content, filename: file.name }, cid);
}

export async function importLineFiles(
  files: File[],
  cid?: number,
): Promise<{ message?: string; ok?: boolean }> {
  if (files.length === 0) return { message: "ファイルが選択されていません" };
  if (files.length === 1) return importLineFile(files[0], cid);
  const batch = await Promise.all(
    files.map(async (file) => ({ content: await readTextLenient(file), filename: file.name })),
  );
  return pkbInvoke("import.line", { files: batch }, cid);
}

/** F2-EXT (SPEC_FOXTROT_UI.md §2.2.2): 読み取り専用の分類。書き込みなし。 */
export async function classifyDocument(file: File): Promise<ClassifyResult> {
  const content = await readTextLenient(file);
  const result = await pkbInvoke<ClassifyResult>("import.classify", {
    content,
    filename: file.name,
  });
  return { ...result, content };
}

/** F2-EXT: dest はユーザーが確定した "es" | "knowledge" のみ。 */
export async function importDocument(
  content: string,
  filename: string,
  dest: "es" | "knowledge",
  cid?: number,
): Promise<{
  imported: boolean;
  skipped: boolean;
  dest: string;
  path?: string;
  message?: string;
  index_rebuilt?: boolean;
}> {
  return pkbInvoke("import.document", { content, filename, dest }, cid);
}

export async function loadSettings(): Promise<SettingsData> {
  return pkbInvoke("settings.get");
}

export async function saveFixedAttributes(
  attributes: Record<string, string>,
): Promise<void> {
  await pkbInvoke("settings.save_fixed", { attributes });
}

export async function runProfiler(): Promise<{ ok: boolean; message: string }> {
  return pkbInvoke("settings.run_profiler");
}

export async function readFileAsText(file: File): Promise<string> {
  return file.text();
}

// ---------------------------------------------------------------- Target Echo (E4)
// oracle.payload は無菌 JSON のみ (LLM 呼び出しなし)。oracle.report は言語化込み
// (7B の生成を待つ)。UI の数値表示は必ず oraclePayload() を使うこと — 7B の
// 生成待ちで PROFILE ペインの表示が遅延するのは不合格 (SPEC_ECHO §5.10.5)。
export interface OraclePayload {
  schema: string;
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

export async function oraclePayload(
  scope: "global" | "dyad" = "global",
  alias?: string,
): Promise<OraclePayload> {
  return pkbInvoke("oracle.payload", { scope, alias: alias ?? null });
}

export async function oracleReport(
  scope: "global" | "dyad" = "global",
  alias?: string,
): Promise<{ payload: OraclePayload; analysis: string }> {
  return pkbInvoke("oracle.report", { scope, alias: alias ?? null });
}

export interface TwinScenario {
  horizon_days: number;
  calendar: { date: string; time: string; title: string }[];
  mode?: "daily" | "interview";
  interview_turns?: number | null;
}

export interface TwinForecast {
  gate_passed: boolean;
  reason?: string;
  horizon_days?: number;
  r_q10?: number[];
  r_q50?: number[];
  r_q90?: number[];
  p_lapse?: (number | null)[];
  critical_days?: string[];
}

export async function twinForecast(
  scenario: TwinScenario,
  scope: "global" | "dyad" = "global",
  alias?: string,
): Promise<TwinForecast> {
  return pkbInvoke("twin.forecast", { scenario, scope, alias: alias ?? null });
}

export async function tensorRebuild(): Promise<{ rebuilt: boolean; rows: number }> {
  return pkbInvoke("tensor.rebuild");
}

export async function sourceCode(): Promise<SourceCodeView> {
  return pkbInvoke("profile.source_code");
}

export interface NarrativeCompileResult {
  ok: boolean;
  es_text?: string;
  recruiters_eye?: string;
  claims?: unknown[];
  compiled_from?: string;
  target_domain?: string;
  draft_path?: string;
  reason?: string;
}

export async function narrativeCompile(
  targetDomain?: string,
): Promise<NarrativeCompileResult> {
  return pkbInvoke("narrative.compile", { target_domain: targetDomain ?? null });
}

export interface KnowledgeFetchSummary {
  processed: number;
  pending: number;
  online_allowed: boolean;
  index_rebuilt?: boolean;
  message?: string;
  [key: string]: unknown;
}

export async function knowledgeFetchPending(): Promise<KnowledgeFetchSummary> {
  return pkbInvoke("knowledge.fetch_pending");
}

export async function probeStatus(today: string): Promise<ProbeStatus> {
  return pkbInvoke("probe.status", { today });
}

export async function probeNext(today: string): Promise<ProbeQuestionView> {
  return pkbInvoke("probe.next", { today });
}

export async function probeAnswer(
  sessionId: string,
  questionId: string,
  answer: string,
  today: string,
): Promise<ProbeAnswerResult> {
  return pkbInvoke("probe.answer", {
    session_id: sessionId,
    question_id: questionId,
    answer,
    today,
  });
}
