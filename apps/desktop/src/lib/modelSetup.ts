// Offline model setup client (AI_SKILLS §5).
// Existence check + local GGUF import only. Never downloads model bytes.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface ModelExistsStatus {
  exists: boolean;
  relativePath: string;
  recommendedPageUrl: string;
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

export function pickLocalGguf(): Promise<string | null> {
  return invoke<string | null>("pick_local_gguf");
}

export function importLocalModel(sourcePath: string): Promise<void> {
  return invoke("import_local_model", { sourcePath });
}

export function openRecommendedModelPage(): Promise<void> {
  return invoke("open_recommended_model_page");
}

export async function listenModelImportProgress(
  onProgress: (progress: ModelImportProgress) => void,
): Promise<UnlistenFn> {
  return listen<unknown>("model-import-progress", (event) => {
    try {
      onProgress(parseModelImportProgress(event.payload));
    } catch {
      /* ignore malformed */
    }
  });
}
