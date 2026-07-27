/**
 * Exact-key parsers for the BLACKBOX Arena IPC boundary (SPEC §15).
 * Mirrors apps/desktop/src-tauri/src/blackbox_arena/view.rs field-for-field.
 * Every successful Tauri response enters as `unknown` and must pass here
 * before reaching application state. No numeric invention, no unknown keys.
 */

type JsonObject = Record<string, unknown>;

export class BlackboxArenaParseError extends Error {
  constructor(path: string, reason: string) {
    super(`${path}: ${reason}`);
    this.name = "BlackboxArenaParseError";
  }
}

function fail(path: string, reason: string): never {
  throw new BlackboxArenaParseError(path, reason);
}

function asObject(value: unknown, path: string): JsonObject {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    fail(path, "expected object");
  }
  return value as JsonObject;
}

function exactObject(
  value: unknown,
  required: readonly string[],
  optional: readonly string[],
  path: string,
): JsonObject {
  const object = asObject(value, path);
  const allowed = new Set([...required, ...optional]);
  for (const key of Object.keys(object)) {
    if (!allowed.has(key)) fail(path, "unexpected field");
  }
  for (const key of required) {
    if (!Object.prototype.hasOwnProperty.call(object, key)) {
      fail(`${path}.${key}`, "missing field");
    }
  }
  return object;
}

function asString(value: unknown, path: string): string {
  if (typeof value !== "string") fail(path, "expected string");
  return value;
}

function asFiniteNumber(value: unknown, path: string): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    fail(path, "expected finite number");
  }
  return value;
}

function asSafeInteger(value: unknown, path: string): number {
  const n = asFiniteNumber(value, path);
  if (!Number.isSafeInteger(n)) fail(path, "expected safe integer");
  return n;
}

function asBoolean(value: unknown, path: string): boolean {
  if (typeof value !== "boolean") fail(path, "expected boolean");
  return value;
}

function asArray(value: unknown, path: string): unknown[] {
  if (!Array.isArray(value)) fail(path, "expected array");
  return value;
}

function asNullableInteger(value: unknown, path: string): number | null {
  if (value === null) return null;
  return asSafeInteger(value, path);
}

function asLiteral<T extends string>(
  value: unknown,
  allowed: readonly T[],
  path: string,
): T {
  if (typeof value !== "string" || !(allowed as readonly string[]).includes(value)) {
    fail(path, `expected one of ${allowed.join(",")}`);
  }
  return value as T;
}

// ---- Closed enums (wire = snake_case) ------------------------------------

export const REGIMES = ["calm", "stress"] as const;
export type Regime = (typeof REGIMES)[number];

export const STIMULUS_KINDS = [
  "gamble_pair",
  "anchor_probe",
  "forecast_elicitation",
  "sunk_cost_pair",
  "crisis_countdown",
  "disposition_window",
] as const;
export type StimulusKind = (typeof STIMULUS_KINDS)[number];

export const TURN_PHASES = [
  "observe",
  "decide",
  "execute",
  "settle",
  "report",
] as const;
export type TurnPhase = (typeof TURN_PHASES)[number];

export const FAILURE_REASONS = [
  "accounting_breach",
  "snapshot_digest_mismatch",
  "replay_divergence",
  "internal_invariant_broken",
] as const;
export type FailureReason = (typeof FAILURE_REASONS)[number];

export const OFFER_KINDS = ["insurance", "expansion"] as const;
export type OfferKind = (typeof OFFER_KINDS)[number];

export const SIM_UI_ERROR_CODES = [
  "campaign_not_found",
  "wrong_phase",
  "campaign_exhausted",
  "business_rule_rejected",
  "generation_not_found",
  "unavailable",
  "internal_fault",
] as const;
export type KnownSimUiErrorCode = (typeof SIM_UI_ERROR_CODES)[number];
export type SimUiErrorCode = KnownSimUiErrorCode | "unknown";

export type SessionState =
  | "genesis"
  | "sealed"
  | { readonly active: { readonly phase: TurnPhase } }
  | { readonly dead: { readonly reason: FailureReason } };

// ---- View DTOs (wire = camelCase) ----------------------------------------

export interface MarketTickView {
  readonly tick: number;
  readonly regime: Regime;
  readonly commodityPriceMinor: number;
  readonly equityIndexCenti: number;
  readonly demandIndexMicro: number;
  readonly rateBp: number;
  readonly jumpOccurred: boolean;
}

export interface StimulusView {
  readonly kind: StimulusKind;
  readonly stimulusSeq: number;
  readonly offerId: number | null;
  readonly projectId: number | null;
  readonly positionId: number | null;
  readonly anchorMinor: number | null;
  readonly gainMinor: number | null;
  readonly lossMinor: number | null;
  readonly winProbabilityMicro: number | null;
  readonly premiumMinor: number | null;
  readonly ticksRemaining: number | null;
  readonly unrealisedMinor: number | null;
}

export interface SkuLineView {
  readonly sku: number;
  readonly unitPriceMinor: number;
  readonly inventoryUnits: number;
  readonly inventoryValueMinor: number;
}

export interface ProjectLineView {
  readonly id: number;
  readonly committedMinor: number;
  readonly continueCount: number;
}

export interface OfferLineView {
  readonly id: number;
  readonly kind: OfferKind;
  readonly costMinor: number;
}

export interface PositionLineView {
  readonly id: number;
  readonly instrument: number;
  readonly notionalMinor: number;
  readonly entryIndexCenti: number;
}

export interface BooksView {
  readonly cashMinor: number;
  readonly inventoryValueMinor: number;
  readonly seniorDebtMinor: number;
  readonly mezzanineDebtMinor: number;
  readonly skus: readonly SkuLineView[];
  readonly projects: readonly ProjectLineView[];
  readonly offers: readonly OfferLineView[];
  readonly positions: readonly PositionLineView[];
}

export interface ArenaLimitsView {
  readonly minPriceMinor: number;
  readonly maxPriceMinor: number;
  readonly maxOrderUnits: number;
  readonly maxActionAmountMinor: number;
  readonly maxSkus: number;
  readonly campaignTicks: number;
  readonly ticksPerQuarter: number;
}

export interface ObservationView {
  readonly campaignId: string;
  readonly turnsCompleted: number;
  readonly market: MarketTickView;
  readonly stimuli: readonly StimulusView[];
  readonly books: BooksView;
  readonly limits: ArenaLimitsView;
  readonly state: SessionState;
}

export interface DecisionOutcomeView {
  readonly ledgerEffect: boolean;
  readonly journalSeq: number | null;
}

export interface PeriodCloseView {
  readonly netIncomeMinor: number;
  readonly netCashChangeMinor: number;
  readonly depreciationMinor: number;
  readonly interestMinor: number;
  readonly taxMinor: number;
}

export interface AdvanceView {
  readonly tick: number;
  readonly revenueMinor: number;
  readonly cogsMinor: number;
  readonly unmetUnits: number;
  readonly periodClose: PeriodCloseView | null;
  readonly state: SessionState;
  readonly nextObservation: ObservationView | null;
}

export type ArenaDifficulty = "standard" | "hard";

export interface StartCampaignRequest {
  readonly scenarioId: number;
  readonly difficulty: ArenaDifficulty;
  readonly campaignIndex: number;
  readonly createdDate: string;
}

// ---- Parsers -------------------------------------------------------------

export function parseSimUiErrorCode(value: unknown): SimUiErrorCode {
  if (
    typeof value === "string" &&
    (SIM_UI_ERROR_CODES as readonly string[]).includes(value)
  ) {
    return value as KnownSimUiErrorCode;
  }
  return "unknown";
}

export function parseSessionState(value: unknown, path = "state"): SessionState {
  if (value === "genesis" || value === "sealed") return value;
  const object = asObject(value, path);
  const keys = Object.keys(object);
  if (keys.length !== 1) fail(path, "expected single-tag enum");
  const tag = keys[0];
  if (tag === "active") {
    const body = exactObject(object.active, ["phase"], [], `${path}.active`);
    return {
      active: {
        phase: asLiteral(body.phase, TURN_PHASES, `${path}.active.phase`),
      },
    };
  }
  if (tag === "dead") {
    const body = exactObject(object.dead, ["reason"], [], `${path}.dead`);
    return {
      dead: {
        reason: asLiteral(body.reason, FAILURE_REASONS, `${path}.dead.reason`),
      },
    };
  }
  fail(path, "unexpected session state tag");
}

export function parseMarketTickView(
  value: unknown,
  path = "market",
): MarketTickView {
  const object = exactObject(
    value,
    [
      "tick",
      "regime",
      "commodityPriceMinor",
      "equityIndexCenti",
      "demandIndexMicro",
      "rateBp",
      "jumpOccurred",
    ],
    [],
    path,
  );
  return {
    tick: asSafeInteger(object.tick, `${path}.tick`),
    regime: asLiteral(object.regime, REGIMES, `${path}.regime`),
    commodityPriceMinor: asSafeInteger(
      object.commodityPriceMinor,
      `${path}.commodityPriceMinor`,
    ),
    equityIndexCenti: asSafeInteger(
      object.equityIndexCenti,
      `${path}.equityIndexCenti`,
    ),
    demandIndexMicro: asSafeInteger(
      object.demandIndexMicro,
      `${path}.demandIndexMicro`,
    ),
    rateBp: asSafeInteger(object.rateBp, `${path}.rateBp`),
    jumpOccurred: asBoolean(object.jumpOccurred, `${path}.jumpOccurred`),
  };
}

export function parseStimulusView(
  value: unknown,
  path = "stimulus",
): StimulusView {
  const object = exactObject(
    value,
    [
      "kind",
      "stimulusSeq",
      "offerId",
      "projectId",
      "positionId",
      "anchorMinor",
      "gainMinor",
      "lossMinor",
      "winProbabilityMicro",
      "premiumMinor",
      "ticksRemaining",
      "unrealisedMinor",
    ],
    [],
    path,
  );
  return {
    kind: asLiteral(object.kind, STIMULUS_KINDS, `${path}.kind`),
    stimulusSeq: asSafeInteger(object.stimulusSeq, `${path}.stimulusSeq`),
    offerId: asNullableInteger(object.offerId, `${path}.offerId`),
    projectId: asNullableInteger(object.projectId, `${path}.projectId`),
    positionId: asNullableInteger(object.positionId, `${path}.positionId`),
    anchorMinor: asNullableInteger(object.anchorMinor, `${path}.anchorMinor`),
    gainMinor: asNullableInteger(object.gainMinor, `${path}.gainMinor`),
    lossMinor: asNullableInteger(object.lossMinor, `${path}.lossMinor`),
    winProbabilityMicro: asNullableInteger(
      object.winProbabilityMicro,
      `${path}.winProbabilityMicro`,
    ),
    premiumMinor: asNullableInteger(object.premiumMinor, `${path}.premiumMinor`),
    ticksRemaining: asNullableInteger(
      object.ticksRemaining,
      `${path}.ticksRemaining`,
    ),
    unrealisedMinor: asNullableInteger(
      object.unrealisedMinor,
      `${path}.unrealisedMinor`,
    ),
  };
}

function parseSkuLineView(value: unknown, path: string): SkuLineView {
  const object = exactObject(
    value,
    ["sku", "unitPriceMinor", "inventoryUnits", "inventoryValueMinor"],
    [],
    path,
  );
  return {
    sku: asSafeInteger(object.sku, `${path}.sku`),
    unitPriceMinor: asSafeInteger(object.unitPriceMinor, `${path}.unitPriceMinor`),
    inventoryUnits: asSafeInteger(object.inventoryUnits, `${path}.inventoryUnits`),
    inventoryValueMinor: asSafeInteger(
      object.inventoryValueMinor,
      `${path}.inventoryValueMinor`,
    ),
  };
}

function parseProjectLineView(value: unknown, path: string): ProjectLineView {
  const object = exactObject(
    value,
    ["id", "committedMinor", "continueCount"],
    [],
    path,
  );
  return {
    id: asSafeInteger(object.id, `${path}.id`),
    committedMinor: asSafeInteger(object.committedMinor, `${path}.committedMinor`),
    continueCount: asSafeInteger(object.continueCount, `${path}.continueCount`),
  };
}

function parseOfferLineView(value: unknown, path: string): OfferLineView {
  const object = exactObject(value, ["id", "kind", "costMinor"], [], path);
  return {
    id: asSafeInteger(object.id, `${path}.id`),
    kind: asLiteral(object.kind, OFFER_KINDS, `${path}.kind`),
    costMinor: asSafeInteger(object.costMinor, `${path}.costMinor`),
  };
}

function parsePositionLineView(value: unknown, path: string): PositionLineView {
  const object = exactObject(
    value,
    ["id", "instrument", "notionalMinor", "entryIndexCenti"],
    [],
    path,
  );
  return {
    id: asSafeInteger(object.id, `${path}.id`),
    instrument: asSafeInteger(object.instrument, `${path}.instrument`),
    notionalMinor: asSafeInteger(object.notionalMinor, `${path}.notionalMinor`),
    entryIndexCenti: asSafeInteger(
      object.entryIndexCenti,
      `${path}.entryIndexCenti`,
    ),
  };
}

export function parseBooksView(value: unknown, path = "books"): BooksView {
  const object = exactObject(
    value,
    [
      "cashMinor",
      "inventoryValueMinor",
      "seniorDebtMinor",
      "mezzanineDebtMinor",
      "skus",
      "projects",
      "offers",
      "positions",
    ],
    [],
    path,
  );
  return {
    cashMinor: asSafeInteger(object.cashMinor, `${path}.cashMinor`),
    inventoryValueMinor: asSafeInteger(
      object.inventoryValueMinor,
      `${path}.inventoryValueMinor`,
    ),
    seniorDebtMinor: asSafeInteger(
      object.seniorDebtMinor,
      `${path}.seniorDebtMinor`,
    ),
    mezzanineDebtMinor: asSafeInteger(
      object.mezzanineDebtMinor,
      `${path}.mezzanineDebtMinor`,
    ),
    skus: asArray(object.skus, `${path}.skus`).map((row, i) =>
      parseSkuLineView(row, `${path}.skus[${i}]`),
    ),
    projects: asArray(object.projects, `${path}.projects`).map((row, i) =>
      parseProjectLineView(row, `${path}.projects[${i}]`),
    ),
    offers: asArray(object.offers, `${path}.offers`).map((row, i) =>
      parseOfferLineView(row, `${path}.offers[${i}]`),
    ),
    positions: asArray(object.positions, `${path}.positions`).map((row, i) =>
      parsePositionLineView(row, `${path}.positions[${i}]`),
    ),
  };
}

export function parseArenaLimitsView(
  value: unknown,
  path = "limits",
): ArenaLimitsView {
  const object = exactObject(
    value,
    [
      "minPriceMinor",
      "maxPriceMinor",
      "maxOrderUnits",
      "maxActionAmountMinor",
      "maxSkus",
      "campaignTicks",
      "ticksPerQuarter",
    ],
    [],
    path,
  );
  return {
    minPriceMinor: asSafeInteger(object.minPriceMinor, `${path}.minPriceMinor`),
    maxPriceMinor: asSafeInteger(object.maxPriceMinor, `${path}.maxPriceMinor`),
    maxOrderUnits: asSafeInteger(object.maxOrderUnits, `${path}.maxOrderUnits`),
    maxActionAmountMinor: asSafeInteger(
      object.maxActionAmountMinor,
      `${path}.maxActionAmountMinor`,
    ),
    maxSkus: asSafeInteger(object.maxSkus, `${path}.maxSkus`),
    campaignTicks: asSafeInteger(object.campaignTicks, `${path}.campaignTicks`),
    ticksPerQuarter: asSafeInteger(
      object.ticksPerQuarter,
      `${path}.ticksPerQuarter`,
    ),
  };
}

export function parseObservationView(
  value: unknown,
  path = "observation",
): ObservationView {
  const object = exactObject(
    value,
    [
      "campaignId",
      "turnsCompleted",
      "market",
      "stimuli",
      "books",
      "limits",
      "state",
    ],
    [],
    path,
  );
  return {
    campaignId: asString(object.campaignId, `${path}.campaignId`),
    turnsCompleted: asSafeInteger(
      object.turnsCompleted,
      `${path}.turnsCompleted`,
    ),
    market: parseMarketTickView(object.market, `${path}.market`),
    stimuli: asArray(object.stimuli, `${path}.stimuli`).map((row, i) =>
      parseStimulusView(row, `${path}.stimuli[${i}]`),
    ),
    books: parseBooksView(object.books, `${path}.books`),
    limits: parseArenaLimitsView(object.limits, `${path}.limits`),
    state: parseSessionState(object.state, `${path}.state`),
  };
}

export function parseDecisionOutcomeView(
  value: unknown,
  path = "decisionOutcome",
): DecisionOutcomeView {
  const object = exactObject(value, ["ledgerEffect", "journalSeq"], [], path);
  return {
    ledgerEffect: asBoolean(object.ledgerEffect, `${path}.ledgerEffect`),
    journalSeq: asNullableInteger(object.journalSeq, `${path}.journalSeq`),
  };
}

export function parsePeriodCloseView(
  value: unknown,
  path = "periodClose",
): PeriodCloseView {
  const object = exactObject(
    value,
    [
      "netIncomeMinor",
      "netCashChangeMinor",
      "depreciationMinor",
      "interestMinor",
      "taxMinor",
    ],
    [],
    path,
  );
  return {
    netIncomeMinor: asSafeInteger(
      object.netIncomeMinor,
      `${path}.netIncomeMinor`,
    ),
    netCashChangeMinor: asSafeInteger(
      object.netCashChangeMinor,
      `${path}.netCashChangeMinor`,
    ),
    depreciationMinor: asSafeInteger(
      object.depreciationMinor,
      `${path}.depreciationMinor`,
    ),
    interestMinor: asSafeInteger(object.interestMinor, `${path}.interestMinor`),
    taxMinor: asSafeInteger(object.taxMinor, `${path}.taxMinor`),
  };
}

export function parseAdvanceView(value: unknown, path = "advance"): AdvanceView {
  const object = exactObject(
    value,
    [
      "tick",
      "revenueMinor",
      "cogsMinor",
      "unmetUnits",
      "periodClose",
      "state",
      "nextObservation",
    ],
    [],
    path,
  );
  return {
    tick: asSafeInteger(object.tick, `${path}.tick`),
    revenueMinor: asSafeInteger(object.revenueMinor, `${path}.revenueMinor`),
    cogsMinor: asSafeInteger(object.cogsMinor, `${path}.cogsMinor`),
    unmetUnits: asSafeInteger(object.unmetUnits, `${path}.unmetUnits`),
    periodClose:
      object.periodClose === null
        ? null
        : parsePeriodCloseView(object.periodClose, `${path}.periodClose`),
    state: parseSessionState(object.state, `${path}.state`),
    nextObservation:
      object.nextObservation === null
        ? null
        : parseObservationView(
            object.nextObservation,
            `${path}.nextObservation`,
          ),
  };
}
