import { invoke } from "@tauri-apps/api/core";
import type { RecordData, SettingsData } from "./types";

export async function pkbInvoke<T = unknown>(
  cmd: string,
  params?: Record<string, unknown>,
): Promise<T> {
  return invoke<T>("pkb_invoke", { cmd, params: params ?? null });
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

export async function consult(query: string): Promise<{ query: string; answer: string }> {
  return pkbInvoke("consult", { query });
}

export async function syncIcsContent(
  content: string,
  mode: "append" | "overwrite",
): Promise<Record<string, unknown>> {
  return pkbInvoke("calendar.sync", { source: "ics", mode, ics_content: content });
}

export async function syncIcsFiles(
  files: File[],
  mode: "append" | "overwrite",
): Promise<{ message?: string }> {
  if (files.length === 0) return { message: "ファイルが選択されていません" };
  if (files.length === 1) {
    const content = await files[0].text();
    return syncIcsContent(content, mode);
  }
  const ics_files = await Promise.all(
    files.map(async (file) => ({ content: await file.text(), filename: file.name })),
  );
  return pkbInvoke("calendar.sync", { source: "ics", mode, ics_files });
}

export async function syncAppleCalendar(
  mode: "append" | "overwrite",
): Promise<Record<string, unknown>> {
  return pkbInvoke("calendar.sync", { source: "apple", mode });
}

export async function importLineFile(file: File): Promise<{ message?: string; ok?: boolean }> {
  const content = await file.text();
  return pkbInvoke("import.line", { content, filename: file.name });
}

export async function importLineFiles(files: File[]): Promise<{ message?: string; ok?: boolean }> {
  if (files.length === 0) return { message: "ファイルが選択されていません" };
  if (files.length === 1) return importLineFile(files[0]);
  const batch = await Promise.all(
    files.map(async (file) => ({ content: await file.text(), filename: file.name })),
  );
  return pkbInvoke("import.line", { files: batch });
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
