// [D] Pocket Brain frontend client (docs/architecture_blueprint.md §3.8).
//
// invoke wrappers + Channel listeners for the M4/M5 on-device LLM commands.
// Command arg keys are camelCase: Tauri v2 converts snake_case Rust parameter
// names to camelCase for the JS payload.

import { Channel, invoke } from "@tauri-apps/api/core";

import type { KakeiboEntryV1 } from "./extractionReducer";

export type { KakeiboEntryV1 } from "./extractionReducer";

export type MemPhase =
  | "baseline"
  | "model_loaded"
  | "ctx_created"
  | "inference"
  | "idle";

export interface MemSample {
  phase: MemPhase;
  phys_footprint_bytes: number;
  delta_from_baseline_bytes: number;
  threshold_bytes: number;
  over_threshold: boolean;
  headroom_bytes: number;
  t_ms: number;
}

/** Known extraction task ids accepted by the Rust worker. */
export type LlmTaskId = "kakeibo_v1";

export const TASK_KAKEIBO_V1: LlmTaskId = "kakeibo_v1";

export interface TokenEvent {
  seq: number;
  text: string;
  done: boolean;
  error: string | null;
  /** Set only on successful kakeibo extraction completion. */
  validated: KakeiboEntryV1 | null;
}

export interface LoadParams {
  n_gpu_layers: number;
  use_mmap: boolean;
}

export interface GenerationParams {
  prompt: string;
  n_ctx: number;
  max_tokens: number;
  temp: number;
  top_k: number;
  top_p: number;
  seed: number;
}

/** Resolve the App-Container model path and mmap-load the GGUF (blocking). */
export function loadModel(params: LoadParams): Promise<void> {
  return invoke("llm_load_model", { params });
}

/**
 * Start a generation; `onToken` receives each streamed `TokenEvent`.
 * Optional `taskId` selects extraction (`"kakeibo_v1"`) vs plain chat (`null`).
 * Existing two-argument callers remain valid.
 */
export function generate(
  params: GenerationParams,
  onToken: (event: TokenEvent) => void,
  taskId: LlmTaskId | null = null,
): Promise<void> {
  const channel = new Channel<TokenEvent>(onToken);
  return invoke("llm_generate", {
    params,
    taskId: taskId ?? null,
    onToken: channel,
  });
}

/** Request cancellation of the in-flight generation. */
export function cancelGeneration(): Promise<void> {
  return invoke("llm_cancel");
}

/**
 * Persistent LLM lifecycle event pushed by the worker (mirrors the Rust
 * `LlmLifecycleEvent`). `memory_purged` means iOS memory pressure forced the
 * model to be dropped; the UI must suspend and await an explicit reload.
 */
export type LlmLifecycleEvent = { readonly kind: "memory_purged" };

/** Strictly parse a lifecycle event; unknown shapes throw and are ignored. */
export function parseLlmLifecycleEvent(value: unknown): LlmLifecycleEvent {
  if (
    typeof value === "object" &&
    value !== null &&
    !Array.isArray(value) &&
    (value as { kind?: unknown }).kind === "memory_purged"
  ) {
    return { kind: "memory_purged" };
  }
  throw new Error("llm.event: unexpected shape");
}

/**
 * Subscribe once to worker-pushed LLM lifecycle events. Creates exactly one
 * `Channel`; call from a mount-only effect so the worker never accumulates
 * sinks. Malformed events are dropped by the strict parser.
 */
export function subscribeLlmEvents(
  onEvent: (event: LlmLifecycleEvent) => void,
): Promise<void> {
  const channel = new Channel<unknown>((raw) => {
    try {
      onEvent(parseLlmLifecycleEvent(raw));
    } catch {
      // Fail-closed: drop malformed / unexpected shapes.
    }
  });
  return invoke("llm_events", { onEvent: channel });
}

/** Start the Jetsam monitor; `onSample` receives each `MemSample`. */
export function startMemoryMonitor(
  onSample: (sample: MemSample) => void,
  intervalMs: number,
  thresholdBytes: number,
): Promise<void> {
  const channel = new Channel<MemSample>(onSample);
  return invoke("memory_monitor_start", {
    onSample: channel,
    intervalMs,
    thresholdBytes,
  });
}

/** Stop the Jetsam monitor sampler thread. */
export function stopMemoryMonitor(): Promise<void> {
  return invoke("memory_monitor_stop");
}
