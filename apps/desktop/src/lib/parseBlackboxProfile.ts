/**
 * Exact-key parser for `bxs_latest_profile` / `bxs_list_profiles` wire DTOs.
 * Closed set — unknown keys fail closed (E0b / arena parser twin).
 */

import type {
  BlackboxLaneView,
  BlackboxProfileMetaView,
  BlackboxProfileView,
} from "./blackboxProfileView";

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function expectKeys(obj: Record<string, unknown>, keys: readonly string[]): void {
  const got = Object.keys(obj).sort().join(",");
  const want = [...keys].sort().join(",");
  if (got !== want) {
    throw new Error(`blackbox profile wire key mismatch: got [${got}] want [${want}]`);
  }
}

const LANE_KEYS = [
  "lane",
  "axis",
  "labelJa",
  "valueMicro",
  "nObs",
  "sufficiencyMicro",
  "measured",
] as const;

const PROFILE_KEYS = [
  "schemaVersion",
  "instrument",
  "calibration",
  "pooledCampaigns",
  "estimatedAt",
  "poolDigestHex",
  "lanes",
  "authorityNote",
] as const;

const META_KEYS = [
  "poolDigestHex",
  "schemaVersion",
  "instrument",
  "calibration",
  "pooledCampaigns",
  "estimatedAt",
  "updatedAt",
] as const;

function parseLane(raw: unknown): BlackboxLaneView {
  if (!isRecord(raw)) throw new Error("lane must be object");
  expectKeys(raw, LANE_KEYS);
  const valueMicro =
    raw.valueMicro === null
      ? null
      : typeof raw.valueMicro === "number"
        ? raw.valueMicro
        : (() => {
            throw new Error("valueMicro must be number|null");
          })();
  if (typeof raw.measured !== "boolean") throw new Error("measured must be bool");
  if (!raw.measured && valueMicro !== null) {
    throw new Error("unmeasured lane must have valueMicro=null");
  }
  return {
    lane: Number(raw.lane),
    axis: String(raw.axis),
    labelJa: String(raw.labelJa),
    valueMicro,
    nObs: Number(raw.nObs),
    sufficiencyMicro: Number(raw.sufficiencyMicro),
    measured: raw.measured,
  };
}

export function parseBlackboxProfileView(raw: unknown): BlackboxProfileView {
  if (!isRecord(raw)) throw new Error("profile must be object");
  expectKeys(raw, PROFILE_KEYS);
  if (!Array.isArray(raw.lanes)) throw new Error("lanes must be array");
  return {
    schemaVersion: String(raw.schemaVersion),
    instrument: String(raw.instrument),
    calibration: String(raw.calibration),
    pooledCampaigns: Number(raw.pooledCampaigns),
    estimatedAt: Number(raw.estimatedAt),
    poolDigestHex: String(raw.poolDigestHex),
    lanes: raw.lanes.map(parseLane),
    authorityNote: String(raw.authorityNote),
  };
}

export function parseBlackboxProfileMetaList(raw: unknown): BlackboxProfileMetaView[] {
  if (!Array.isArray(raw)) throw new Error("meta list must be array");
  return raw.map((item) => {
    if (!isRecord(item)) throw new Error("meta must be object");
    expectKeys(item, META_KEYS);
    return {
      poolDigestHex: String(item.poolDigestHex),
      schemaVersion: String(item.schemaVersion),
      instrument: String(item.instrument),
      calibration: String(item.calibration),
      pooledCampaigns: Number(item.pooledCampaigns),
      estimatedAt: Number(item.estimatedAt),
      updatedAt: Number(item.updatedAt),
    };
  });
}
