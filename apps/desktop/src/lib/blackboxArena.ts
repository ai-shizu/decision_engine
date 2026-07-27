/** The sole frontend owner of BLACKBOX Arena Tauri IPC calls (SPEC §15). */

import { invoke } from "@tauri-apps/api/core";

import {
  parseAdvanceView,
  parseDecisionOutcomeView,
  parseObservationView,
  parseSimUiErrorCode,
  type AdvanceView,
  type DecisionOutcomeView,
  type ObservationView,
  type SimUiErrorCode,
  type StartCampaignRequest,
} from "./parseBlackboxArena";
import type { ActionIntentWire } from "./blackboxIntent";

export type {
  AdvanceView,
  ArenaDifficulty,
  ArenaLimitsView,
  BooksView,
  DecisionOutcomeView,
  ObservationView,
  SimUiErrorCode,
  StartCampaignRequest,
} from "./parseBlackboxArena";

type BlackboxIpcCommand =
  | "bxs_start_campaign"
  | "bxs_get_view"
  | "bxs_submit_decision"
  | "bxs_advance"
  | "bxs_abort";

type BlackboxParser<T> = (value: unknown) => T;

export class BlackboxArenaIpcError extends Error {
  readonly code: SimUiErrorCode;

  constructor(code: SimUiErrorCode) {
    super("Blackbox arena request failed");
    this.name = "BlackboxArenaIpcError";
    this.code = code;
  }
}

async function invokeBlackbox<T>(
  command: BlackboxIpcCommand,
  parser: BlackboxParser<T> | null,
  args?: Record<string, unknown>,
): Promise<T> {
  let raw: unknown;
  try {
    raw =
      args === undefined
        ? await invoke<unknown>(command)
        : await invoke<unknown>(command, args);
  } catch (cause: unknown) {
    // Lessons-learned rule (2026-07-24 context-budget hunt): always surface the
    // RAW IPC error before it is re-wrapped into a sterile typed error.
    console.error(`[blackboxArena] invoke("${command}") failed:`, cause);
    throw new BlackboxArenaIpcError(parseSimUiErrorCode(cause));
  }
  if (parser === null) {
    return undefined as T;
  }
  try {
    return parser(raw);
  } catch (cause: unknown) {
    console.error(
      `[blackboxArena] parse of "${command}" response failed:`,
      cause,
      raw,
    );
    throw cause;
  }
}

export function bxsStartCampaign(
  request: StartCampaignRequest,
): Promise<ObservationView> {
  return invokeBlackbox("bxs_start_campaign", parseObservationView, {
    request,
  });
}

export function bxsGetView(campaignId: string): Promise<ObservationView> {
  return invokeBlackbox("bxs_get_view", parseObservationView, {
    campaignId,
  });
}

export function bxsSubmitDecision(
  campaignId: string,
  intent: ActionIntentWire,
  latencyMs: number | null,
): Promise<DecisionOutcomeView> {
  return invokeBlackbox("bxs_submit_decision", parseDecisionOutcomeView, {
    campaignId,
    intent,
    latencyMs,
  });
}

export function bxsAdvance(campaignId: string): Promise<AdvanceView> {
  return invokeBlackbox("bxs_advance", parseAdvanceView, { campaignId });
}

export function bxsAbort(campaignId: string): Promise<void> {
  return invokeBlackbox("bxs_abort", null, { campaignId });
}
