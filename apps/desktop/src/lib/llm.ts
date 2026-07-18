// [D] Pocket Brain frontend client (docs/architecture_blueprint.md §3.8).
//
// invoke wrappers + Channel listeners for the M4 on-device LLM commands. Command
// arg keys are snake_case to match the Rust parameter names (Tauri default
// RenamePolicy::Keep — verified against tauri-macros 2.6.3).

import { Channel, invoke } from "@tauri-apps/api/core";

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

export interface TokenEvent {
  seq: number;
  text: string;
  done: boolean;
  error: string | null;
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

/** Start a generation; `onToken` receives each streamed `TokenEvent`. */
export function generate(
  params: GenerationParams,
  onToken: (event: TokenEvent) => void,
): Promise<void> {
  const channel = new Channel<TokenEvent>(onToken);
  return invoke("llm_generate", { params, on_token: channel });
}

/** Request cancellation of the in-flight generation. */
export function cancelGeneration(): Promise<void> {
  return invoke("llm_cancel");
}

/** Start the Jetsam monitor; `onSample` receives each `MemSample`. */
export function startMemoryMonitor(
  onSample: (sample: MemSample) => void,
  intervalMs: number,
  thresholdBytes: number,
): Promise<void> {
  const channel = new Channel<MemSample>(onSample);
  return invoke("memory_monitor_start", {
    on_sample: channel,
    interval_ms: intervalMs,
    threshold_bytes: thresholdBytes,
  });
}

/** Stop the Jetsam monitor sampler thread. */
export function stopMemoryMonitor(): Promise<void> {
  return invoke("memory_monitor_stop");
}
