import { invoke } from "@tauri-apps/api/core";
import type { ContextManifestResponseV1 } from "./manifest";
import { parseConsultResponse, type ConsultResponse } from "./parseConsultResponse";
import {
  parseBoolean,
  parseCalendarEventDatesResult,
  parseCalendarSyncResult,
  parseClassifyResult,
  parseDocumentImportResult,
  parseEngineHealth,
  parseEsView,
  parseImportStats,
  parseKnowledgeFetchSummary,
  parseLineImportResult,
  parseNarrativeCompileResult,
  parseOraclePayload,
  parseOracleReport,
  parseProbeAnswerResult,
  parseProbeQuestion,
  parseProbeStatus,
  parseProfilerResult,
  parseRecordData,
  parseRecordSaveResult,
  parseSavedResult,
  parseSettingsData,
  parseSourceCodeView,
  parseTensorRebuildResult,
  parseTwinForecast,
  type CalendarSyncResult,
  type DocumentImportResult,
  type KnowledgeFetchSummary,
  type LineImportResult,
  type NarrativeCompileResult,
  type OraclePayload,
  type OracleReportResult,
  type ProfilerResult,
  type TwinForecast,
} from "./parseEngineResponse";
import { parseContextManifestResponseV1 } from "./parseManifest";
import { readTextLenient } from "./textDecode";
import type {
  ClassifyResult,
  EsView,
  InterviewConfig,
  ProbeAnswerResult,
  ProbeQuestionView,
  ProbeStatus,
  RecordData,
  SettingsData,
  SourceCodeView,
  SourceStat,
} from "./types";


type EngineIpcCommand =
  | "engine_health"
  | "record_load"
  | "record_save"
  | "calendar_event_dates"
  | "import_stats"
  | "es_view"
  | "consult"
  | "calendar_sync_ics"
  | "calendar_sync_apple"
  | "import_line_single"
  | "import_line_batch"
  | "import_classify"
  | "import_document"
  | "settings_get"
  | "settings_save_fixed"
  | "settings_run_profiler"
  | "oracle_payload"
  | "oracle_report"
  | "twin_forecast"
  | "tensor_rebuild"
  | "profile_source_code"
  | "narrative_compile"
  | "knowledge_fetch_pending"
  | "probe_status"
  | "probe_next"
  | "probe_answer"
  | "context_manifest_latest";


async function invokeEngine(
  command: EngineIpcCommand,
  request?: Record<string, unknown>,
  cid?: number,
): Promise<unknown> {
  if (request === undefined) return invoke<unknown>(command);
  return invoke<unknown>(command, { request, cid: cid ?? null });
}


export async function engineReady(): Promise<boolean> {
  return parseBoolean(await invoke<unknown>("engine_ready"));
}


export async function engineHealth(): Promise<{ status: "ok"; offline: true }> {
  return parseEngineHealth(await invokeEngine("engine_health"));
}


export async function loadRecord(date: string): Promise<RecordData> {
  return parseRecordData(await invokeEngine("record_load", { date }));
}


export async function saveRecord(
  date: string,
  events: RecordData["events"],
  transactions: RecordData["transactions"],
  diary: string,
): Promise<{ saved: true; index_rebuilt: boolean }> {
  const result = parseRecordSaveResult(
    await invokeEngine("record_save", { date, events, transactions, diary }),
  );
  return { saved: result.saved, index_rebuilt: result.index_rebuilt };
}


export async function calendarEventDates(): Promise<string[]> {
  return parseCalendarEventDatesResult(await invokeEngine("calendar_event_dates")).dates;
}


export async function importStats(): Promise<Record<string, SourceStat>> {
  return parseImportStats(await invokeEngine("import_stats"));
}


export async function esView(): Promise<EsView> {
  return parseEsView(await invokeEngine("es_view"));
}


export interface ConsultOptions {
  mode?: "consult" | "interview_sim" | "es_review" | "gd_sim" | "romance_analysis";
  personas?: { name: string; trait: string }[];
  response_time_sec?: number;
  config?: InterviewConfig;
}


export type { RomanceAnalysisResult } from "./parseConsultResponse";


export async function consult(
  query: string,
  opts: ConsultOptions = {},
  cid?: number,
): Promise<ConsultResponse> {
  const raw = await invokeEngine("consult", { query, ...opts }, cid);
  return parseConsultResponse(raw);
}


export async function syncIcsContent(
  content: string,
  mode: "append" | "overwrite",
  cid?: number,
): Promise<CalendarSyncResult> {
  return parseCalendarSyncResult(
    await invokeEngine("calendar_sync_ics", { mode, ics_content: content }, cid),
  );
}


export async function syncIcsFiles(
  files: File[],
  mode: "append" | "overwrite",
  cid?: number,
): Promise<CalendarSyncResult | { message: string }> {
  if (files.length === 0) return { message: "ファイルが選択されていません" };
  if (files.length === 1) {
    const content = await readTextLenient(files[0]);
    return syncIcsContent(content, mode, cid);
  }
  const ics_files = await Promise.all(
    files.map(async (file) => ({ content: await readTextLenient(file), filename: file.name })),
  );
  return parseCalendarSyncResult(
    await invokeEngine("calendar_sync_ics", { mode, ics_files }, cid),
  );
}


export async function syncAppleCalendar(
  mode: "append" | "overwrite",
  cid?: number,
): Promise<CalendarSyncResult> {
  return parseCalendarSyncResult(
    await invokeEngine("calendar_sync_apple", { mode }, cid),
  );
}


export async function importLineContent(
  content: string,
  filename: string,
  cid?: number,
): Promise<LineImportResult> {
  return parseLineImportResult(
    await invokeEngine("import_line_single", { content, filename }, cid),
  );
}


export async function importLineFile(file: File, cid?: number): Promise<LineImportResult> {
  return importLineContent(await readTextLenient(file), file.name, cid);
}


export async function importLineFiles(
  files: File[],
  cid?: number,
): Promise<LineImportResult | { message: string }> {
  if (files.length === 0) return { message: "ファイルが選択されていません" };
  if (files.length === 1) return importLineFile(files[0], cid);
  const batch = await Promise.all(
    files.map(async (file) => ({ content: await readTextLenient(file), filename: file.name })),
  );
  return parseLineImportResult(await invokeEngine("import_line_batch", { files: batch }, cid));
}


export async function classifyDocument(file: File): Promise<ClassifyResult> {
  const content = await readTextLenient(file);
  const result = parseClassifyResult(
    await invokeEngine("import_classify", { content, filename: file.name }),
  );
  return { ...result, content };
}


export async function importDocument(
  content: string,
  filename: string,
  dest: "es" | "knowledge",
  cid?: number,
): Promise<DocumentImportResult> {
  return parseDocumentImportResult(
    await invokeEngine("import_document", { content, filename, dest }, cid),
  );
}


export async function loadSettings(): Promise<SettingsData> {
  return parseSettingsData(await invokeEngine("settings_get"));
}


export async function saveFixedAttributes(attributes: Record<string, string>): Promise<void> {
  parseSavedResult(await invokeEngine("settings_save_fixed", { attributes }));
}


export async function runProfiler(): Promise<ProfilerResult> {
  return parseProfilerResult(await invokeEngine("settings_run_profiler"));
}


export async function oraclePayload(
  scope: "global" | "dyad" = "global",
  alias?: string,
): Promise<OraclePayload> {
  return parseOraclePayload(
    await invokeEngine("oracle_payload", { scope, alias: alias ?? null }),
  );
}


export async function oracleReport(
  scope: "global" | "dyad" = "global",
  alias?: string,
): Promise<OracleReportResult> {
  return parseOracleReport(
    await invokeEngine("oracle_report", { scope, alias: alias ?? null }),
  );
}


export interface TwinScenario {
  horizon_days: number;
  calendar: { date: string; time: string; title: string }[];
  mode?: "daily" | "interview";
  interview_turns?: number | null;
}


export async function twinForecast(
  scenario: TwinScenario,
  scope: "global" | "dyad" = "global",
  alias?: string,
): Promise<TwinForecast> {
  return parseTwinForecast(
    await invokeEngine("twin_forecast", { scenario, scope, alias: alias ?? null }),
  );
}


export async function tensorRebuild(): Promise<{ rebuilt: boolean; rows: number }> {
  return parseTensorRebuildResult(await invokeEngine("tensor_rebuild"));
}


export async function sourceCode(): Promise<SourceCodeView> {
  return parseSourceCodeView(await invokeEngine("profile_source_code"));
}


export async function narrativeCompile(targetDomain?: string): Promise<NarrativeCompileResult> {
  return parseNarrativeCompileResult(
    await invokeEngine("narrative_compile", { target_domain: targetDomain ?? null }),
  );
}


export async function knowledgeFetchPending(): Promise<KnowledgeFetchSummary> {
  return parseKnowledgeFetchSummary(await invokeEngine("knowledge_fetch_pending"));
}


export async function probeStatus(today: string): Promise<ProbeStatus> {
  return parseProbeStatus(await invokeEngine("probe_status", { today }));
}


export async function probeNext(today: string): Promise<ProbeQuestionView> {
  return parseProbeQuestion(await invokeEngine("probe_next", { today }));
}


export async function probeAnswer(
  sessionId: string,
  questionId: string,
  answer: string,
  today: string,
): Promise<ProbeAnswerResult> {
  return parseProbeAnswerResult(
    await invokeEngine("probe_answer", {
      session_id: sessionId,
      question_id: questionId,
      answer,
      today,
    }),
  );
}


export async function latestContextManifest(): Promise<ContextManifestResponseV1> {
  return parseContextManifestResponseV1(await invokeEngine("context_manifest_latest"));
}


export type {
  CalendarSyncResult,
  DocumentImportResult,
  KnowledgeFetchSummary,
  LineImportResult,
  NarrativeCompileResult,
  OraclePayload,
  OracleReportResult,
  ProfilerResult,
  TwinForecast,
};
