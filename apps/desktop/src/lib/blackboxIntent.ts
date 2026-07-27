/**
 * Closed ActionIntent wire builders for BLACKBOX Arena.
 * Shape mirrors blackbox_sim::telemetry::ActionIntent serde:
 * externally-tagged snake_case variants, camelCase fields.
 * Director-injected timeout defaults have no FE builder (SPEC §3.4).
 */

import type { ArenaLimitsView } from "./parseBlackboxArena";

export type ActionIntentWire =
  | { readonly set_price: { readonly sku: number; readonly tickPrice: number } }
  | { readonly order_inventory: { readonly sku: number; readonly units: number } }
  | { readonly accept_offer: { readonly offerId: number } }
  | { readonly decline_offer: { readonly offerId: number } }
  | { readonly close_position: { readonly positionId: number } }
  | {
      readonly open_hedge: {
        readonly instrument: number;
        readonly notionalMinor: number;
      };
    }
  | {
      readonly invest: {
        readonly projectId: number;
        readonly amountMinor: number;
      };
    }
  | { readonly continue_project: { readonly projectId: number } }
  | { readonly abandon_project: { readonly projectId: number } }
  | {
      readonly borrow: {
        readonly facility: number;
        readonly amountMinor: number;
      };
    }
  | {
      readonly repay: {
        readonly facility: number;
        readonly amountMinor: number;
      };
    }
  | {
      readonly forecast_interval: {
        readonly loMinor: number;
        readonly hiMinor: number;
      };
    }
  | "abstain";

export type IntentKind =
  | "set_price"
  | "order_inventory"
  | "accept_offer"
  | "decline_offer"
  | "close_position"
  | "open_hedge"
  | "invest"
  | "continue_project"
  | "abandon_project"
  | "borrow"
  | "repay"
  | "forecast_interval"
  | "abstain";

export const INTENT_KINDS: readonly IntentKind[] = [
  "set_price",
  "order_inventory",
  "accept_offer",
  "decline_offer",
  "close_position",
  "open_hedge",
  "invest",
  "continue_project",
  "abandon_project",
  "borrow",
  "repay",
  "forecast_interval",
  "abstain",
] as const;

export class BlackboxIntentError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "BlackboxIntentError";
  }
}

function requireNonNegInt(value: number, label: string): number {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new BlackboxIntentError(`${label} must be a non-negative safe integer`);
  }
  return value;
}

function requirePosInt(value: number, label: string): number {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new BlackboxIntentError(`${label} must be a positive safe integer`);
  }
  return value;
}

function requireAmount(
  value: number,
  limits: ArenaLimitsView,
  label: string,
): number {
  const n = requirePosInt(value, label);
  if (n > limits.maxActionAmountMinor) {
    throw new BlackboxIntentError(`${label} exceeds maxActionAmountMinor`);
  }
  return n;
}

export function buildSetPrice(
  sku: number,
  tickPrice: number,
  limits: ArenaLimitsView,
): ActionIntentWire {
  const s = requireNonNegInt(sku, "sku");
  if (s >= limits.maxSkus) {
    throw new BlackboxIntentError("sku out of range");
  }
  if (
    !Number.isSafeInteger(tickPrice) ||
    tickPrice < limits.minPriceMinor ||
    tickPrice > limits.maxPriceMinor
  ) {
    throw new BlackboxIntentError("tickPrice out of limits");
  }
  return { set_price: { sku: s, tickPrice } };
}

export function buildOrderInventory(
  sku: number,
  units: number,
  limits: ArenaLimitsView,
): ActionIntentWire {
  const s = requireNonNegInt(sku, "sku");
  if (s >= limits.maxSkus) {
    throw new BlackboxIntentError("sku out of range");
  }
  const u = requirePosInt(units, "units");
  if (u > limits.maxOrderUnits) {
    throw new BlackboxIntentError("units exceeds maxOrderUnits");
  }
  return { order_inventory: { sku: s, units: u } };
}

export function buildAcceptOffer(offerId: number): ActionIntentWire {
  return { accept_offer: { offerId: requireNonNegInt(offerId, "offerId") } };
}

export function buildDeclineOffer(offerId: number): ActionIntentWire {
  return { decline_offer: { offerId: requireNonNegInt(offerId, "offerId") } };
}

export function buildClosePosition(positionId: number): ActionIntentWire {
  return {
    close_position: { positionId: requireNonNegInt(positionId, "positionId") },
  };
}

export function buildOpenHedge(
  instrument: number,
  notionalMinor: number,
  limits: ArenaLimitsView,
): ActionIntentWire {
  return {
    open_hedge: {
      instrument: requireNonNegInt(instrument, "instrument"),
      notionalMinor: requireAmount(notionalMinor, limits, "notionalMinor"),
    },
  };
}

export function buildInvest(
  projectId: number,
  amountMinor: number,
  limits: ArenaLimitsView,
): ActionIntentWire {
  return {
    invest: {
      projectId: requireNonNegInt(projectId, "projectId"),
      amountMinor: requireAmount(amountMinor, limits, "amountMinor"),
    },
  };
}

export function buildContinueProject(projectId: number): ActionIntentWire {
  return {
    continue_project: { projectId: requireNonNegInt(projectId, "projectId") },
  };
}

export function buildAbandonProject(projectId: number): ActionIntentWire {
  return {
    abandon_project: { projectId: requireNonNegInt(projectId, "projectId") },
  };
}

export function buildBorrow(
  facility: number,
  amountMinor: number,
  limits: ArenaLimitsView,
): ActionIntentWire {
  return {
    borrow: {
      facility: requireNonNegInt(facility, "facility"),
      amountMinor: requireAmount(amountMinor, limits, "amountMinor"),
    },
  };
}

export function buildRepay(
  facility: number,
  amountMinor: number,
  limits: ArenaLimitsView,
): ActionIntentWire {
  return {
    repay: {
      facility: requireNonNegInt(facility, "facility"),
      amountMinor: requireAmount(amountMinor, limits, "amountMinor"),
    },
  };
}

export function buildForecastInterval(
  loMinor: number,
  hiMinor: number,
  limits: ArenaLimitsView,
): ActionIntentWire {
  if (!Number.isSafeInteger(loMinor) || !Number.isSafeInteger(hiMinor)) {
    throw new BlackboxIntentError("forecast bounds must be safe integers");
  }
  if (loMinor > hiMinor) {
    throw new BlackboxIntentError("loMinor must be <= hiMinor");
  }
  if (
    Math.abs(loMinor) > limits.maxActionAmountMinor ||
    Math.abs(hiMinor) > limits.maxActionAmountMinor
  ) {
    throw new BlackboxIntentError("forecast bounds exceed maxActionAmountMinor");
  }
  return { forecast_interval: { loMinor, hiMinor } };
}

export function buildAbstain(): ActionIntentWire {
  return "abstain";
}
