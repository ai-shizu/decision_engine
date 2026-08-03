// [D] Coraxis on-device frontend client (docs/architecture_blueprint.md §3.8).
//
// invoke wrappers + Channel listeners for the M4/M5 on-device LLM commands.
// Command arg keys are camelCase: Tauri v2 converts snake_case Rust parameter
// names to camelCase for the JS payload.

import { Channel, invoke, isTauri } from "@tauri-apps/api/core";

import type { KakeiboEntryV1 } from "./extractionReducer";
import { hapticBiasDetected } from "./haptics";

export type { KakeiboEntryV1 } from "./extractionReducer";

export type MemPhase =
  | "baseline"
  | "model_loaded"
  | "ctx_created"
  | "inference"
  | "idle";

export type DegradationLevel =
  | "nominal"
  | "fair"
  | "serious"
  | "critical";

export interface MemSample {
  phase: MemPhase;
  phys_footprint_bytes: number;
  delta_from_baseline_bytes: number;
  threshold_bytes: number;
  over_threshold: boolean;
  headroom_bytes: number;
  t_ms: number;
  /** Progressive thermal / memory ladder (Phase 4). */
  degradation: DegradationLevel;
}

/** Known extraction task ids accepted by the Rust worker. */
export type LlmTaskId =
  | "kakeibo_v1"
  | "cognitive_distortion_v1"
  | "receipt_ocr_v1"
  | "interview_evaluation_v1"
  | "metacognitive_debrief_v1";

export const TASK_KAKEIBO_V1: LlmTaskId = "kakeibo_v1";
export const TASK_COGNITIVE_DISTORTION_V1: LlmTaskId = "cognitive_distortion_v1";
export const TASK_RECEIPT_OCR_V1: LlmTaskId = "receipt_ocr_v1";
export const TASK_INTERVIEW_EVALUATION_V1: LlmTaskId = "interview_evaluation_v1";
export const TASK_METACOGNITIVE_DEBRIEF_V1: LlmTaskId = "metacognitive_debrief_v1";

/** Burns (1980) / Beck (1976) cognitive distortion category ids. */
export type DistortionCategory =
  | "all_or_nothing"
  | "overgeneralization"
  | "mental_filter"
  | "disqualifying_the_positive"
  | "jumping_to_conclusions"
  | "magnification_minimization"
  | "emotional_reasoning"
  | "should_statements"
  | "labeling"
  | "personalization";

export interface DistortionDetectionV1 {
  category: DistortionCategory;
  snippet: string;
  confidence_score: number;
}

export interface CognitiveDistortionReportV1 {
  detected_distortions: DistortionDetectionV1[];
}

export interface ReceiptLineV1 {
  item_name: string;
  unit_price: number;
  qty: number;
  amount: number;
}

export interface ReceiptOcrV1 {
  merchant: string;
  occurred_at: string;
  tax: number;
  total: number;
  lines: ReceiptLineV1[];
}

/** Layer-1 interview scorecard — turn_id provenance only (Phase 14.3). */
export interface TurnProvenance {
  turn_id: string;
  quote_snippet: string;
}

export interface AxisScoreV1 {
  score: number;
  provenance: TurnProvenance[];
}

export interface InterviewEvaluationV1 {
  schema: string;
  mece_structure: AxisScoreV1;
  hypothesis_thinking: AxisScoreV1;
  quantitative_validity: AxisScoreV1;
  stress_resilience: AxisScoreV1;
  overall_pass: boolean;
  summary: string;
}

/** Layer-2 opt-in metacognitive debrief — never feeds pass/fail. */
export interface MetacognitiveInsightV1 {
  parallel_label: string;
  interview_turn_id: string;
  mirror_kind: string;
  distortion_category: string | null;
  note: string;
}

export interface MetacognitiveDebriefV1 {
  schema: string;
  opted_in: boolean;
  insights: MetacognitiveInsightV1[];
}

export interface TokenEvent {
  seq: number;
  text: string;
  done: boolean;
  error: string | null;
  /** Set only on successful kakeibo extraction completion. */
  validated: KakeiboEntryV1 | null;
  /** Set only on successful CBT distortion extraction completion. */
  validated_distortions: CognitiveDistortionReportV1 | null;
  /** Set only on successful receipt OCR extraction completion. */
  validated_receipt: ReceiptOcrV1 | null;
  /** Deterministic checksum: sum(line.amount)+tax === total. */
  receipt_verified: boolean | null;
  /** Layer-1 interview scorecard (transcript-only). */
  validated_interview_evaluation: InterviewEvaluationV1 | null;
  /** Layer-2 opt-in debrief (never pass/fail). */
  validated_metacognitive_debrief: MetacognitiveDebriefV1 | null;
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
 * Start a generation; `onToken` receives streamed `TokenEvent`s.
 * Phase 9: Rust micro-batches pieces (~30ms / 8 tokens); FE still appends `text`.
 * Optional `taskId` selects extraction (`"kakeibo_v1"`) vs plain chat (`null`).
 */
export function generate(
  params: GenerationParams,
  onToken: (event: TokenEvent) => void,
  taskId: LlmTaskId | null = null,
): Promise<void> {
  const channel = new Channel<TokenEvent>((event) => {
    const report = event.validated_distortions;
    if (
      report &&
      Array.isArray(report.detected_distortions) &&
      report.detected_distortions.length > 0
    ) {
      hapticBiasDetected();
    }
    onToken(event);
  });
  return invoke("llm_generate", {
    params,
    taskId: taskId ?? null,
    onToken: channel,
  });
}

/**
 * Embed text and return a `Float32Array` decoded from little-endian IPC bytes
 * (Phase 9). Avoids JSON-serializing 384 floats.
 */
export async function embedBinary(
  text: string,
  nCtx?: number,
): Promise<Float32Array> {
  const bytes = await invoke<number[] | Uint8Array>("llm_embed_binary", {
    text,
    nCtx: nCtx ?? null,
  });
  return decodeF32Le(bytes);
}

/** Decode little-endian f32 payload from Tauri binary IPC. */
export function decodeF32Le(bytes: ArrayBuffer | Uint8Array | number[]): Float32Array {
  let u8: Uint8Array;
  if (bytes instanceof ArrayBuffer) {
    u8 = new Uint8Array(bytes);
  } else if (Array.isArray(bytes)) {
    u8 = Uint8Array.from(bytes);
  } else {
    u8 = bytes;
  }
  if (u8.byteLength % 4 !== 0) {
    throw new Error("embed binary length not divisible by 4");
  }
  // Copy to guarantee 4-byte alignment for Float32Array view.
  const copy = new Uint8Array(u8.byteLength);
  copy.set(u8);
  return new Float32Array(copy.buffer);
}

/** Cancel an in-flight generation. */
export function cancelGeneration(): Promise<void> {
  return invoke("llm_cancel");
}

/** Phase 10: true when the worker still holds a loaded GGUF. */
export function llmIsLoaded(): Promise<boolean> {
  return invoke<boolean>("llm_is_loaded");
}

/**
 * Persistent LLM lifecycle event pushed by the worker (mirrors the Rust
 * `LlmLifecycleEvent`). `memory_purged` means iOS memory pressure forced the
 * model to be dropped; the UI must suspend and await an explicit reload.
 * `degradation` warns that thermal/memory ladder entered Serious+.
 */
export type LlmLifecycleEvent =
  | { readonly kind: "memory_purged" }
  | { readonly kind: "degradation"; readonly level: DegradationLevel };

const DEGRADATION_LEVELS: readonly DegradationLevel[] = [
  "nominal",
  "fair",
  "serious",
  "critical",
];

function isDegradationLevel(value: unknown): value is DegradationLevel {
  return (
    typeof value === "string" &&
    (DEGRADATION_LEVELS as readonly string[]).includes(value)
  );
}

/** Strictly parse a lifecycle event; unknown shapes throw and are ignored. */
export function parseLlmLifecycleEvent(value: unknown): LlmLifecycleEvent {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("llm.event: unexpected shape");
  }
  const kind = (value as { kind?: unknown }).kind;
  if (kind === "memory_purged") {
    return { kind: "memory_purged" };
  }
  if (kind === "degradation") {
    const level = (value as { level?: unknown }).level;
    if (isDegradationLevel(level)) {
      return { kind: "degradation", level };
    }
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
  // Channel reads window.__TAURI_INTERNALS__.transformCallback synchronously —
  // without the bridge this throws before a Promise exists (.catch is useless).
  if (!isTauri()) {
    return Promise.resolve();
  }
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
  if (!isTauri()) {
    return Promise.resolve();
  }
  const channel = new Channel<MemSample>(onSample);
  return invoke("memory_monitor_start", {
    onSample: channel,
    intervalMs,
    thresholdBytes,
  });
}

/** Stop the Jetsam monitor sampler thread. */
export function memoryMonitorStop(): Promise<void> {
  if (!isTauri()) {
    return Promise.resolve();
  }
  return invoke("memory_monitor_stop");
}
