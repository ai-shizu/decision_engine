/**
 * Build ActionIntentWire from the command-console draft using limits only
 * (no hardcoded numeric bounds).
 */

import {
  buildAbandonProject,
  buildAbstain,
  buildAcceptOffer,
  buildBorrow,
  buildClosePosition,
  buildContinueProject,
  buildDeclineOffer,
  buildForecastInterval,
  buildInvest,
  buildOpenHedge,
  buildOrderInventory,
  buildRepay,
  buildSetPrice,
  BlackboxIntentError,
  type ActionIntentWire,
} from "./blackboxIntent";
import type { ArenaLimitsView } from "./parseBlackboxArena";
import type { BlackboxDraft } from "./blackboxArenaReducer";

function parseIntField(raw: string, label: string): number {
  const trimmed = raw.trim();
  if (trimmed === "") {
    throw new BlackboxIntentError(`${label} is required`);
  }
  const n = Number(trimmed);
  if (!Number.isSafeInteger(n)) {
    throw new BlackboxIntentError(`${label} must be an integer`);
  }
  return n;
}

export function buildIntentFromDraft(
  draft: BlackboxDraft,
  limits: ArenaLimitsView,
): ActionIntentWire {
  switch (draft.kind) {
    case "set_price":
      return buildSetPrice(
        parseIntField(draft.sku, "sku"),
        parseIntField(draft.tickPrice, "tickPrice"),
        limits,
      );
    case "order_inventory":
      return buildOrderInventory(
        parseIntField(draft.sku, "sku"),
        parseIntField(draft.units, "units"),
        limits,
      );
    case "accept_offer":
      return buildAcceptOffer(parseIntField(draft.offerId, "offerId"));
    case "decline_offer":
      return buildDeclineOffer(parseIntField(draft.offerId, "offerId"));
    case "close_position":
      return buildClosePosition(parseIntField(draft.positionId, "positionId"));
    case "open_hedge":
      return buildOpenHedge(
        parseIntField(draft.instrument, "instrument"),
        parseIntField(draft.notionalMinor, "notionalMinor"),
        limits,
      );
    case "invest":
      return buildInvest(
        parseIntField(draft.projectId, "projectId"),
        parseIntField(draft.amountMinor, "amountMinor"),
        limits,
      );
    case "continue_project":
      return buildContinueProject(parseIntField(draft.projectId, "projectId"));
    case "abandon_project":
      return buildAbandonProject(parseIntField(draft.projectId, "projectId"));
    case "borrow":
      return buildBorrow(
        parseIntField(draft.facility, "facility"),
        parseIntField(draft.amountMinor, "amountMinor"),
        limits,
      );
    case "repay":
      return buildRepay(
        parseIntField(draft.facility, "facility"),
        parseIntField(draft.amountMinor, "amountMinor"),
        limits,
      );
    case "forecast_interval":
      return buildForecastInterval(
        parseIntField(draft.loMinor, "loMinor"),
        parseIntField(draft.hiMinor, "hiMinor"),
        limits,
      );
    case "abstain":
      return buildAbstain();
    default: {
      const _exhaustive: never = draft.kind;
      return _exhaustive;
    }
  }
}
