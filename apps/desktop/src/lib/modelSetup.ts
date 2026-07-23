// Offline model setup client (AI_SKILLS §5 / §4.72).
// Existence check + local GGUF import via plugin-dialog + plugin-fs.
// Never downloads model bytes. Rust never reads Security-Scoped picker paths.

import { invoke, isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  copyFile,
  mkdir,
  startAccessingSecurityScopedResource,
  stopAccessingSecurityScopedResource,
} from "@tauri-apps/plugin-fs";
import { BaseDirectory } from "@tauri-apps/api/path";

export interface ModelExistsStatus {
  exists: boolean;
  relativePath: string;
  recommendedPageUrl: string;
}

export interface ModelImportDest {
  absolutePath: string;
  relativePath: string;
}

export interface ModelImportProgress {
  percent: number;
  bytesCopied: number;
  totalBytes: number;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
}

export function parseModelExistsStatus(value: unknown): ModelExistsStatus {
  const o = asRecord(value);
  if (!o) throw new Error("model exists payload invalid");
  const exists = o.exists;
  const relativePath = o.relativePath;
  const recommendedPageUrl = o.recommendedPageUrl;
  if (typeof exists !== "boolean") throw new Error("exists invalid");
  if (typeof relativePath !== "string") throw new Error("relativePath invalid");
  if (typeof recommendedPageUrl !== "string") {
    throw new Error("recommendedPageUrl invalid");
  }
  return { exists, relativePath, recommendedPageUrl };
}

export function parseModelImportDest(value: unknown): ModelImportDest {
  const o = asRecord(value);
  if (!o) throw new Error("model import dest invalid");
  const absolutePath = o.absolutePath;
  const relativePath = o.relativePath;
  if (typeof absolutePath !== "string" || !absolutePath) {
    throw new Error("absolutePath invalid");
  }
  if (typeof relativePath !== "string" || !relativePath) {
    throw new Error("relativePath invalid");
  }
  return { absolutePath, relativePath };
}

export function parseModelImportProgress(value: unknown): ModelImportProgress {
  const o = asRecord(value);
  if (!o) throw new Error("import progress payload invalid");
  const percent = o.percent;
  const bytesCopied = o.bytesCopied;
  const totalBytes = o.totalBytes;
  if (typeof percent !== "number" || !Number.isFinite(percent)) {
    throw new Error("percent invalid");
  }
  if (typeof bytesCopied !== "number" || !Number.isFinite(bytesCopied)) {
    throw new Error("bytesCopied invalid");
  }
  if (typeof totalBytes !== "number" || !Number.isFinite(totalBytes)) {
    throw new Error("totalBytes invalid");
  }
  return {
    percent: Math.max(0, Math.min(100, Math.floor(percent))),
    bytesCopied,
    totalBytes,
  };
}

/**
 * Soft probe: when `pocket-brain` is not compiled in, the command is missing —
 * treat as "already ready" so the default desktop shell is not blocked.
 */
export async function checkModelExists(): Promise<ModelExistsStatus | null> {
  try {
    const raw = await invoke<unknown>("check_model_exists");
    return parseModelExistsStatus(raw);
  } catch {
    return null;
  }
}

export async function prepareModelImportDest(): Promise<ModelImportDest> {
  const raw = await invoke<unknown>("prepare_model_import_dest");
  return parseModelImportDest(raw);
}

export function confirmModelImported(): Promise<void> {
  return invoke("confirm_model_imported");
}

export function openRecommendedModelPage(): Promise<void> {
  return invoke("open_recommended_model_page");
}

/**
 * Dialog → Security-Scoped access → copyFile into AppData/models/pocket-brain.gguf
 * → Rust confirm. Never loads the whole GGUF into JS heap.
 */
export async function importLocalGgufViaFs(options?: {
  onProgress?: (percent: number) => void;
}): Promise<boolean> {
  if (!isTauri()) {
    throw new Error("not tauri");
  }

  const selected = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "GGUF model", extensions: ["gguf"] }],
    title: "Coraxis — ローカル GGUF を選択",
  });
  if (selected === null) {
    return false;
  }
  const sourcePath = Array.isArray(selected) ? selected[0] : selected;
  if (typeof sourcePath !== "string" || !sourcePath) {
    return false;
  }

  options?.onProgress?.(5);
  const dest = await prepareModelImportDest();
  options?.onProgress?.(15);

  await mkdir("models", {
    baseDir: BaseDirectory.AppData,
    recursive: true,
  });

  let scoped = false;
  try {
    await startAccessingSecurityScopedResource(sourcePath);
    scoped = true;
    options?.onProgress?.(25);
    // Prefer relative AppData dest so scope stays inside $APPDATA.
    await copyFile(sourcePath, dest.relativePath, {
      toPathBaseDir: BaseDirectory.AppData,
    });
    options?.onProgress?.(90);
    await confirmModelImported();
    // Best-effort warm load — file is already confirmed in AppData.
    try {
      await invoke("brain_load_gguf", {
        params: { n_gpu_layers: 999, use_mmap: true },
      });
    } catch {
      /* PocketBrainPanel retries */
    }
    options?.onProgress?.(100);
    return true;
  } finally {
    if (scoped) {
      try {
        await stopAccessingSecurityScopedResource(sourcePath);
      } catch {
        /* best-effort */
      }
    }
  }
}
