/**
 * Map closed SimUiErrorCode → sterile UiErrorCode (SPEC §15 two-layer error).
 * Never passes raw diagnostics into the UI message helper.
 */

import type { SimUiErrorCode } from "./parseBlackboxArena";
import { uiErrorMessage, type UiErrorCode } from "./uiErrorMessages";

export function mapSimUiErrorToUiCode(code: SimUiErrorCode): UiErrorCode {
  switch (code) {
    case "business_rule_rejected":
      return "BXS_ARENA_REJECTED";
    case "wrong_phase":
    case "campaign_not_found":
    case "campaign_exhausted":
    case "generation_not_found":
      return "BXS_ARENA_STATE";
    case "unavailable":
    case "internal_fault":
    case "unknown":
      return "BXS_ARENA_FAULT";
    default: {
      const _exhaustive: never = code;
      return _exhaustive;
    }
  }
}

export function blackboxUiErrorMessage(code: SimUiErrorCode): string {
  return uiErrorMessage(mapSimUiErrorToUiCode(code));
}
