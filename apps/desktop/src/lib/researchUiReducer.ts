import type { KnowledgeResearchReceipt } from "./parseEngineResponse";

export type ResearchUiPhase = "idle" | "researching";

export interface ResearchUiState {
  readonly phase: ResearchUiPhase;
  readonly seq: number;
}

export const INITIAL_RESEARCH_UI_STATE: ResearchUiState = {
  phase: "idle",
  seq: 0,
};

export type ResearchUiEvent =
  | { kind: "START"; seq: number }
  | { kind: "DONE"; seq: number }
  | { kind: "FAIL"; seq: number };

export function reduceResearchUi(
  state: ResearchUiState,
  event: ResearchUiEvent,
): ResearchUiState {
  if (event.kind === "START") {
    if (event.seq <= state.seq) {
      return state;
    }
    return { phase: "researching", seq: event.seq };
  }

  if (event.seq !== state.seq) {
    return state;
  }

  return { phase: "idle", seq: state.seq };
}

export function isResearching(state: ResearchUiState): boolean {
  return state.phase === "researching";
}

export interface Provenance {
  source: "wikipedia";
  label: string;
}

export function deriveProvenance(
  receipt: KnowledgeResearchReceipt | null,
): Provenance | null {
  if (receipt === null || receipt.results_persisted <= 0) {
    return null;
  }
  return {
    source: "wikipedia",
    label: "Wikipediaより参照",
  };
}

export function provenanceChipText(provenance: Provenance): string {
  return `🔗 ${provenance.label}`;
}
