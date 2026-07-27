/**
 * Pure reducer for the BLACKBOX Arena FE. Never talks to IPC — the container
 * dispatches actions after invoke results (gdArenaReducer pattern / E0b).
 */

import type {
  AdvanceView,
  ArenaLimitsView,
  DecisionOutcomeView,
  ObservationView,
} from "./parseBlackboxArena";
import type { IntentKind } from "./blackboxIntent";

export interface BlackboxDraft {
  readonly kind: IntentKind;
  readonly sku: string;
  readonly tickPrice: string;
  readonly units: string;
  readonly offerId: string;
  readonly positionId: string;
  readonly instrument: string;
  readonly notionalMinor: string;
  readonly projectId: string;
  readonly amountMinor: string;
  readonly facility: string;
  readonly loMinor: string;
  readonly hiMinor: string;
}

export interface TurnLogEntry {
  readonly tick: number;
  readonly revenueMinor: number;
  readonly cogsMinor: number;
  readonly unmetUnits: number;
  readonly periodClose: AdvanceView["periodClose"];
}

export interface BlackboxArenaState {
  readonly campaignId: string | null;
  readonly observation: ObservationView | null;
  readonly limits: ArenaLimitsView | null;
  readonly lastAdvance: AdvanceView | null;
  readonly lastOutcome: DecisionOutcomeView | null;
  readonly turnLog: readonly TurnLogEntry[];
  readonly draft: BlackboxDraft;
  readonly busy: boolean;
  readonly sealed: boolean;
  readonly error: string | null;
}

export type BlackboxArenaAction =
  | { type: "campaign_started"; observation: ObservationView }
  | { type: "observation_loaded"; observation: ObservationView }
  | { type: "draft_changed"; draft: Partial<BlackboxDraft> }
  | { type: "submit_begin" }
  | { type: "submit_ok"; outcome: DecisionOutcomeView }
  | { type: "advance_ok"; advance: AdvanceView }
  | { type: "command_failed"; message: string }
  | { type: "sealed" }
  | { type: "reset" };

export function initialBlackboxDraft(): BlackboxDraft {
  return {
    kind: "abstain",
    sku: "0",
    tickPrice: "",
    units: "",
    offerId: "",
    positionId: "",
    instrument: "0",
    notionalMinor: "",
    projectId: "",
    amountMinor: "",
    facility: "0",
    loMinor: "",
    hiMinor: "",
  };
}

export function initialBlackboxArenaState(): BlackboxArenaState {
  return {
    campaignId: null,
    observation: null,
    limits: null,
    lastAdvance: null,
    lastOutcome: null,
    turnLog: [],
    draft: initialBlackboxDraft(),
    busy: false,
    sealed: false,
    error: null,
  };
}

function advanceTick(advance: AdvanceView): number {
  return advance.tick;
}

function appendTurnLog(
  log: readonly TurnLogEntry[],
  advance: AdvanceView,
  campaignTicks: number,
): readonly TurnLogEntry[] {
  const next: TurnLogEntry = {
    tick: advanceTick(advance),
    revenueMinor: advance.revenueMinor,
    cogsMinor: advance.cogsMinor,
    unmetUnits: advance.unmetUnits,
    periodClose: advance.periodClose,
  };
  const capped = [...log, next];
  if (capped.length > campaignTicks) {
    return capped.slice(capped.length - campaignTicks);
  }
  return capped;
}

export function blackboxArenaReducer(
  state: BlackboxArenaState,
  action: BlackboxArenaAction,
): BlackboxArenaState {
  switch (action.type) {
    case "campaign_started":
      return {
        ...initialBlackboxArenaState(),
        campaignId: action.observation.campaignId,
        observation: action.observation,
        limits: action.observation.limits,
        draft: initialBlackboxDraft(),
      };
    case "observation_loaded":
      return {
        ...state,
        campaignId: action.observation.campaignId,
        observation: action.observation,
        limits: action.observation.limits,
        busy: false,
        error: null,
      };
    case "draft_changed":
      return {
        ...state,
        draft: { ...state.draft, ...action.draft },
      };
    case "submit_begin":
      return { ...state, busy: true, error: null };
    case "submit_ok":
      return {
        ...state,
        lastOutcome: action.outcome,
        // still busy until advance completes
      };
    case "advance_ok": {
      const campaignTicks =
        state.limits?.campaignTicks ??
        action.advance.nextObservation?.limits.campaignTicks ??
        null;
      const nextObs = action.advance.nextObservation;
      const sealed =
        action.advance.state === "sealed" ||
        (typeof action.advance.state === "object" &&
          "dead" in action.advance.state) ||
        nextObs === null;
      const entry: TurnLogEntry = {
        tick: advanceTick(action.advance),
        revenueMinor: action.advance.revenueMinor,
        cogsMinor: action.advance.cogsMinor,
        unmetUnits: action.advance.unmetUnits,
        periodClose: action.advance.periodClose,
      };
      const turnLog =
        campaignTicks === null
          ? [...state.turnLog, entry]
          : appendTurnLog(state.turnLog, action.advance, campaignTicks);
      return {
        ...state,
        lastAdvance: action.advance,
        turnLog,
        observation: nextObs,
        limits: nextObs?.limits ?? state.limits,
        campaignId: nextObs?.campaignId ?? state.campaignId,
        busy: false,
        sealed,
        error: null,
        draft: sealed ? state.draft : initialBlackboxDraft(),
      };
    }
    case "command_failed":
      return { ...state, busy: false, error: action.message };
    case "sealed":
      return { ...state, sealed: true, busy: false };
    case "reset":
      return initialBlackboxArenaState();
    default: {
      const _exhaustive: never = action;
      return _exhaustive;
    }
  }
}
