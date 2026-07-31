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
 * Send a precise failure to the Rust log (OSLog on iOS). The on-screen message
 * stays the fixed sentence required by AI_SKILLS §5.1 — this is the developer
 * channel that sentence used to destroy. Never throws: a diagnostic must not
 * replace the failure it is describing.
 */
export async function reportImportDiagnostic(
  step: string,
  detail: string,
): Promise<void> {
  try {
    await invoke("report_import_diagnostic", { step, detail });
  } catch {
    /* diagnostics are best-effort by design */
  }
}

/**
 * Provider shape of the picked URL — which Files backend handed it to us — using
 * fixed markers only. The raw path is deliberately not logged: it can carry
 * account and filename detail, and §5.1 forbids putting it in user-visible text.
 * Which provider it came from is the part that actually discriminates a
 * simulator-only success (Tier 2) from a real-provider failure.
 */
export function describeSourceShape(p: string): string {
  const markers: [string, string][] = [
    ["/Mobile Documents/", "icloud"],
    ["/File Provider Storage/", "fileprovider"],
    ["/Inbox/", "inbox"],
    ["/tmp/", "tmp"],
    ["/Downloads/", "downloads"],
    ["/var/mobile/Containers/Data/", "appcontainer"],
  ];
  const hit = markers.find(([m]) => p.includes(m));
  const segments = p.split("/").filter(Boolean).length;
  const ext = p.slice(p.lastIndexOf(".") + 1).toLowerCase();
  return `provider=${hit ? hit[1] : "other"} segments=${segments} ext=${ext} len=${p.length}`;
}

/**
 * Run one import step, naming it if it throws. Without this every failure
 * arrived as one indistinguishable sentence; the step marker is what turns a
 * device re-run into a diagnosis instead of another guess.
 */
async function withStep<T>(name: string, fn: () => Promise<T>): Promise<T> {
  try {
    return await fn();
  } catch (e) {
    const detail = e instanceof Error ? e.message : String(e);
    await reportImportDiagnostic(name, detail);
    throw e;
  }
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

  const selected = await withStep("dialog", () =>
    open({
      multiple: false,
      directory: false,
      filters: [{ name: "GGUF model", extensions: ["gguf"] }],
      title: "Coraxis — ローカル GGUF を選択",
      // Required, not cosmetic. The picker defaults to `copy`, which makes iOS
      // duplicate the whole GGUF into our own container tmp and hand back a URL
      // we already own — not security-scoped, so `startAccessingSecurityScoped
      // Resource` fails on it (measured on device: provider=tmp). `scoped`
      // leaves the file in place with system-managed access, which is what §5.1
      // describes, and avoids a 1.1 GB duplicate that the API docs make *our*
      // responsibility to delete.
      fileAccessMode: "scoped",
    }),
  );
  if (selected === null) {
    return false;
  }
  const sourcePath = Array.isArray(selected) ? selected[0] : selected;
  if (typeof sourcePath !== "string" || !sourcePath) {
    return false;
  }

  // Which Files provider handed us this URL, recorded before anything can fail.
  // Tier 2 only ever exercised the simulator's local provider.
  await reportImportDiagnostic("source_shape", describeSourceShape(sourcePath));

  options?.onProgress?.(5);
  const dest = await withStep("prepare_dest", () => prepareModelImportDest());
  options?.onProgress?.(15);

  await withStep("mkdir", () =>
    mkdir("models", {
      baseDir: BaseDirectory.AppData,
      recursive: true,
    }),
  );

  let scoped = false;
  try {
    // Non-fatal by design. Under `scoped` this succeeds; under a `copy`-mode URL
    // (our own container tmp) it fails even though access is already ours. A
    // throw here aborted the whole import before the copy was ever attempted —
    // that, not the copy, is what failed on device. If access is genuinely
    // missing the failure now surfaces at `copy` with the OS error attached,
    // which is strictly more informative than dying one step early.
    try {
      await startAccessingSecurityScopedResource(sourcePath);
      scoped = true;
    } catch (e) {
      await reportImportDiagnostic(
        "scope_start_skipped",
        e instanceof Error ? e.message : String(e),
      );
    }
    options?.onProgress?.(25);
    // Prefer relative AppData dest so scope stays inside $APPDATA.
    await withStep("copy", () =>
      copyFile(sourcePath, dest.relativePath, {
        toPathBaseDir: BaseDirectory.AppData,
      }),
    );
    options?.onProgress?.(90);
    await withStep("confirm", () => confirmModelImported());
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
