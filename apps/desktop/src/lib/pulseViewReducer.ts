//! Pure helpers + reducer for Romance Pulse / Rasch Pulse View (M18-D).

import type {
  CalculatePulseResult,
  EvaluateRaschResult,
  ItemSelection,
  ProbeQuestionOut,
} from "./pocketBrain/types";

/** Ability grid matching Rust `ABILITY_GRID` (17 points). */
export const RASCH_ABILITY_GRID: readonly number[] = [
  -4.0, -3.5, -3.0, -2.5, -2.0, -1.5, -1.0, -0.5, 0.0, 0.5, 1.0, 1.5, 2.0, 2.5,
  3.0, 3.5, 4.0,
] as const;

export const RASCH_GRID_LEN = 17;

export interface RaschStateView {
  id: string;
  created_at: number;
  artifact_sha256: string;
  posterior: number[];
  excluded: string[];
  last_selection: ItemSelection | null;
}

export interface PulseViewState {
  phase: "idle" | "loading_rasch" | "computing_pulse" | "updating_rasch";
  transcript: string;
  pulse: CalculatePulseResult | null;
  rasch: EvaluateRaschResult | null;
  raschPersisted: RaschStateView | null;
  bank: ProbeQuestionOut[];
  selectedItemId: string;
  response: number;
  error: string | null;
}

export type PulseViewAction =
  | { type: "set_transcript"; value: string }
  | { type: "set_item"; itemId: string }
  | { type: "set_response"; value: number }
  | { type: "clear_error" }
  | { type: "load_rasch_begin" }
  | {
      type: "load_rasch_success";
      persisted: RaschStateView | null;
      bank: ProbeQuestionOut[];
    }
  | { type: "load_rasch_failure"; message: string }
  | { type: "pulse_begin" }
  | { type: "pulse_success"; result: CalculatePulseResult }
  | { type: "pulse_failure"; message: string }
  | { type: "rasch_begin" }
  | { type: "rasch_success"; result: EvaluateRaschResult }
  | { type: "rasch_failure"; message: string };

export function initialPulseViewState(): PulseViewState {
  return {
    phase: "idle",
    transcript: "",
    pulse: null,
    rasch: null,
    raschPersisted: null,
    bank: [],
    selectedItemId: "",
    response: 2,
    error: null,
  };
}

export function pulseViewBusy(phase: PulseViewState["phase"]): boolean {
  return phase !== "idle";
}

/** EAP θ̂ from discrete posterior (same grid as Rust). */
export function expectedAbility(posterior: number[]): number | null {
  if (posterior.length !== RASCH_GRID_LEN) return null;
  let mass = 0;
  let sum = 0;
  for (let i = 0; i < RASCH_GRID_LEN; i += 1) {
    const p = posterior[i];
    if (!Number.isFinite(p) || p < 0) return null;
    mass += p;
    sum += RASCH_ABILITY_GRID[i] * p;
  }
  if (!(mass > 0)) return null;
  return sum / mass;
}

export function parseRaschStateView(raw: unknown): RaschStateView | null {
  if (!raw || typeof raw !== "object") return null;
  const o = raw as Record<string, unknown>;
  if (typeof o.id !== "string") return null;
  if (typeof o.created_at !== "number") return null;
  if (typeof o.artifact_sha256 !== "string") return null;
  if (!Array.isArray(o.posterior)) return null;
  const posterior = o.posterior.filter(
    (x): x is number => typeof x === "number" && Number.isFinite(x),
  );
  if (posterior.length !== o.posterior.length) return null;
  if (!Array.isArray(o.excluded)) return null;
  const excluded = o.excluded.filter((x): x is string => typeof x === "string");
  if (excluded.length !== o.excluded.length) return null;

  let last_selection: ItemSelection | null = null;
  if (o.last_selection && typeof o.last_selection === "object") {
    const sel = o.last_selection as Record<string, unknown>;
    if (
      typeof sel.item_id === "string" &&
      typeof sel.eig === "number" &&
      typeof sel.quantized_eig === "number"
    ) {
      last_selection = {
        item_id: sel.item_id,
        eig: sel.eig,
        quantized_eig: sel.quantized_eig,
      };
    }
  }

  return {
    id: o.id,
    created_at: o.created_at,
    artifact_sha256: o.artifact_sha256,
    posterior,
    excluded,
    last_selection,
  };
}

export function pulseViewReducer(
  state: PulseViewState,
  action: PulseViewAction,
): PulseViewState {
  switch (action.type) {
    case "set_transcript":
      return { ...state, transcript: action.value };
    case "set_item":
      return { ...state, selectedItemId: action.itemId };
    case "set_response":
      return {
        ...state,
        response: Math.max(0, Math.min(4, Math.round(action.value))),
      };
    case "clear_error":
      return { ...state, error: null };
    case "load_rasch_begin":
      return { ...state, phase: "loading_rasch", error: null };
    case "load_rasch_success": {
      const nextItem =
        action.persisted?.last_selection?.item_id ??
        action.bank[0]?.id ??
        "";
      return {
        ...state,
        phase: "idle",
        raschPersisted: action.persisted,
        bank: action.bank,
        selectedItemId: state.selectedItemId || nextItem,
        rasch: action.persisted
          ? {
              schema: "dynamic_ordinal_rasch.v1",
              artifact_sha256: action.persisted.artifact_sha256,
              posterior: action.persisted.posterior,
              excluded: action.persisted.excluded,
              next: action.persisted.last_selection,
            }
          : state.rasch,
      };
    }
    case "load_rasch_failure":
      return { ...state, phase: "idle", error: action.message };
    case "pulse_begin":
      return { ...state, phase: "computing_pulse", error: null };
    case "pulse_success":
      return { ...state, phase: "idle", pulse: action.result };
    case "pulse_failure":
      return { ...state, phase: "idle", error: action.message };
    case "rasch_begin":
      return { ...state, phase: "updating_rasch", error: null };
    case "rasch_success":
      return {
        ...state,
        phase: "idle",
        rasch: action.result,
        selectedItemId: action.result.next?.item_id ?? state.selectedItemId,
        raschPersisted: {
          id: "default",
          created_at: state.raschPersisted?.created_at ?? 0,
          artifact_sha256: action.result.artifact_sha256,
          posterior: action.result.posterior,
          excluded: action.result.excluded,
          last_selection: action.result.next,
        },
      };
    case "rasch_failure":
      return { ...state, phase: "idle", error: action.message };
    default: {
      const _exhaustive: never = action;
      return _exhaustive;
    }
  }
}
