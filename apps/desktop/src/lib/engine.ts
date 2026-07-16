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
  parseKnowledgeResearchReceipt,
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
  type KnowledgeResearchReceipt,
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
  | "engine_ready"
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
  | "knowledge_research"
  | "probe_status"
  | "probe_next"
  | "probe_answer"
  | "context_manifest_latest";


type RuntimeParser<T> = (value: unknown) => T;


async function invokeEngine<T>(
  command: EngineIpcCommand,
  parser: RuntimeParser<T>,
  request?: Record<string, unknown>,
  cid?: number,
): Promise<T> {
  const raw = request === undefined
    ? await invoke<unknown>(command)
    : await invoke<unknown>(command, { request, cid: cid ?? null });
  return parser(raw);
}


export async function engineReady(): Promise<boolean> {
  return invokeEngine("engine_ready", parseBoolean);
}


export async function engineHealth(): Promise<{ status: "ok"; offline: true }> {
  return invokeEngine("engine_health", parseEngineHealth);
}


export async function loadRecord(date: string): Promise<RecordData> {
  return invokeEngine("record_load", parseRecordData, { date });
}


export async function saveRecord(
  date: string,
  events: RecordData["events"],
  transactions: RecordData["transactions"],
  diary: string,
): Promise<{ saved: true; index_rebuilt: boolean }> {
  const result = await invokeEngine(
    "record_save",
    parseRecordSaveResult,
    { date, events, transactions, diary },
  );
  return { saved: result.saved, index_rebuilt: result.index_rebuilt };
}


export async function calendarEventDates(): Promise<string[]> {
  return (await invokeEngine(
    "calendar_event_dates",
    parseCalendarEventDatesResult,
  )).dates;
}


export async function importStats(): Promise<Record<string, SourceStat>> {
  return invokeEngine("import_stats", parseImportStats);
}


export async function esView(): Promise<EsView> {
  return invokeEngine("es_view", parseEsView);
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
  return invokeEngine("consult", parseConsultResponse, { query, ...opts }, cid);
}


export async function syncIcsContent(
  content: string,
  mode: "append" | "overwrite",
  cid?: number,
): Promise<CalendarSyncResult> {
  return invokeEngine(
    "calendar_sync_ics",
    parseCalendarSyncResult,
    { mode, ics_content: content },
    cid,
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
  return invokeEngine(
    "calendar_sync_ics",
    parseCalendarSyncResult,
    { mode, ics_files },
    cid,
  );
}


export async function syncAppleCalendar(
  mode: "append" | "overwrite",
  cid?: number,
): Promise<CalendarSyncResult> {
  return invokeEngine(
    "calendar_sync_apple",
    parseCalendarSyncResult,
    { mode },
    cid,
  );
}


export async function importLineContent(
  content: string,
  filename: string,
  cid?: number,
): Promise<LineImportResult> {
  return invokeEngine(
    "import_line_single",
    parseLineImportResult,
    { content, filename },
    cid,
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
  return invokeEngine("import_line_batch", parseLineImportResult, { files: batch }, cid);
}


export async function classifyDocument(file: File): Promise<ClassifyResult> {
  const content = await readTextLenient(file);
  const result = await invokeEngine(
    "import_classify",
    parseClassifyResult,
    { content, filename: file.name },
  );
  return { ...result, content };
}


export async function importDocument(
  content: string,
  filename: string,
  dest: "es" | "knowledge",
  cid?: number,
): Promise<DocumentImportResult> {
  return invokeEngine(
    "import_document",
    parseDocumentImportResult,
    { content, filename, dest },
    cid,
  );
}


export async function loadSettings(): Promise<SettingsData> {
  return invokeEngine("settings_get", parseSettingsData);
}


export async function saveFixedAttributes(attributes: Record<string, string>): Promise<void> {
  await invokeEngine("settings_save_fixed", parseSavedResult, { attributes });
}


export async function runProfiler(): Promise<ProfilerResult> {
  return invokeEngine("settings_run_profiler", parseProfilerResult);
}


export async function oraclePayload(
  scope: "global" | "dyad" = "global",
  alias?: string,
): Promise<OraclePayload> {
  return invokeEngine(
    "oracle_payload",
    parseOraclePayload,
    { scope, alias: alias ?? null },
  );
}


export async function oracleReport(
  scope: "global" | "dyad" = "global",
  alias?: string,
): Promise<OracleReportResult> {
  return invokeEngine(
    "oracle_report",
    parseOracleReport,
    { scope, alias: alias ?? null },
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
  return invokeEngine(
    "twin_forecast",
    parseTwinForecast,
    { scenario, scope, alias: alias ?? null },
  );
}


export async function tensorRebuild(): Promise<{ rebuilt: boolean; rows: number }> {
  return invokeEngine("tensor_rebuild", parseTensorRebuildResult);
}


export async function sourceCode(): Promise<SourceCodeView> {
  return invokeEngine("profile_source_code", parseSourceCodeView);
}


export async function narrativeCompile(targetDomain?: string): Promise<NarrativeCompileResult> {
  return invokeEngine(
    "narrative_compile",
    parseNarrativeCompileResult,
    { target_domain: targetDomain ?? null },
  );
}


export async function knowledgeFetchPending(): Promise<KnowledgeFetchSummary> {
  return invokeEngine("knowledge_fetch_pending", parseKnowledgeFetchSummary);
}


export async function knowledgeResearch(query: string): Promise<KnowledgeResearchReceipt> {
  return invokeEngine("knowledge_research", parseKnowledgeResearchReceipt, { query });
}


export async function probeStatus(today: string): Promise<ProbeStatus> {
  return invokeEngine("probe_status", parseProbeStatus, { today });
}


export async function probeNext(today: string): Promise<ProbeQuestionView> {
  return invokeEngine("probe_next", parseProbeQuestion, { today });
}


export async function probeAnswer(
  sessionId: string,
  questionId: string,
  answer: string,
  today: string,
): Promise<ProbeAnswerResult> {
  return invokeEngine(
    "probe_answer",
    parseProbeAnswerResult,
    {
      session_id: sessionId,
      question_id: questionId,
      answer,
      today,
    },
  );
}


export async function latestContextManifest(): Promise<ContextManifestResponseV1> {
  return invokeEngine(
    "context_manifest_latest",
    parseContextManifestResponseV1,
  );
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
