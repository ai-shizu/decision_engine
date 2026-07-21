import { invoke } from "@tauri-apps/api/core";
import type { ContextManifestResponseV1 } from "./manifest";
import { parseKnowledgePolicy } from "./parseKnowledgePolicy";
import {
  parseBoolean,
  parseCalendarEventDatesResult,
  parseCalendarSyncResult,
  parseClassifyResult,
  parseDocumentImportResult,
  parseEngineHealth,
  parseEsList,
  parseImportStats,
  parseKnowledgeResearchReceipt,
  parseLineImportResult,
  parseProfilerResult,
  parseRecordData,
  parseRecordSaveResult,
  parseSavedResult,
  parseSettingsData,
  parseSourceCodeView,
  parseTensorRebuildResult,
  type CalendarSyncResult,
  type DocumentImportResult,
  type KnowledgeResearchReceipt,
  type LineImportResult,
  type ProfilerResult,
} from "./parseEngineResponse";
import { parseContextManifestResponseV1 } from "./parseManifest";
import { readTextLenient } from "./textDecode";
import type {
  ClassifyResult,
  EsListItem,
  RecordData,
  SettingsData,
  SourceCodeView,
  SourceStat,
} from "./types";


/** FE-invoked Python sidecar commands only (Coraxis dual-stack wrappers removed). */
type EngineIpcCommand =
  | "engine_ready"
  | "engine_health"
  | "record_load"
  | "record_save"
  | "calendar_event_dates"
  | "import_stats"
  | "es_list"
  | "calendar_sync_ics"
  | "calendar_sync_apple"
  | "import_line_single"
  | "import_line_batch"
  | "import_classify"
  | "import_document"
  | "llm_warm"
  | "settings_get"
  | "settings_save_fixed"
  | "settings_run_profiler"
  | "tensor_rebuild"
  | "profile_source_code"
  | "knowledge_research"
  | "knowledge_policy_get"
  | "knowledge_policy_set"
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


export async function esList(): Promise<EsListItem[]> {
  return invokeEngine("es_list", parseEsList);
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
  companyName?: string,
  opts?: { confirmOverwrite?: boolean; replaceEsId?: string },
): Promise<DocumentImportResult> {
  return invokeEngine(
    "import_document",
    parseDocumentImportResult,
    {
      content,
      filename,
      dest,
      ...(companyName !== undefined && companyName !== ""
        ? { company_name: companyName }
        : {}),
      ...(opts?.confirmOverwrite ? { confirm_overwrite: true } : {}),
      ...(opts?.replaceEsId ? { replace_es_id: opts.replaceEsId } : {}),
    },
    cid,
  );
}


export interface LlmWarmResult {
  embedder_ready: boolean;
  backend_ready: boolean;
  llm_probed: boolean;
  message: string;
}


function parseLlmWarmResult(value: unknown): LlmWarmResult {
  if (typeof value !== "object" || value === null) {
    throw new Error("llm.warm: expected object");
  }
  const o = value as Record<string, unknown>;
  return {
    embedder_ready: Boolean(o.embedder_ready),
    backend_ready: Boolean(o.backend_ready),
    llm_probed: Boolean(o.llm_probed),
    message: typeof o.message === "string" ? o.message : "",
  };
}


export async function warmConsultRuntime(probeLlm = true): Promise<LlmWarmResult> {
  return invokeEngine("llm_warm", parseLlmWarmResult, { probe_llm: probeLlm });
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


export async function tensorRebuild(): Promise<{ rebuilt: boolean; rows: number }> {
  return invokeEngine("tensor_rebuild", parseTensorRebuildResult);
}


export async function sourceCode(): Promise<SourceCodeView> {
  return invokeEngine("profile_source_code", parseSourceCodeView);
}


export async function knowledgeResearch(query: string): Promise<KnowledgeResearchReceipt> {
  return invokeEngine("knowledge_research", parseKnowledgeResearchReceipt, { query });
}


export async function getKnowledgeResearchPolicy() {
  return invokeEngine("knowledge_policy_get", parseKnowledgePolicy);
}


export async function setKnowledgeResearchPolicy(enabled: boolean) {
  return invokeEngine("knowledge_policy_set", parseKnowledgePolicy, { enabled });
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
  LineImportResult,
  ProfilerResult,
};
