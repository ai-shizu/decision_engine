/**
 * BLACKBOX profile VIEW helpers (R-9 PROFILE UI).
 * Pure: no IPC. Unmeasured lanes must render as 「未測定」, never "0".
 */

export type BlackboxLaneView = {
  lane: number;
  axis: string;
  labelJa: string;
  valueMicro: number | null;
  nObs: number;
  sufficiencyMicro: number;
  measured: boolean;
};

export type BlackboxProfileView = {
  schemaVersion: string;
  instrument: string;
  calibration: string;
  pooledCampaigns: number;
  estimatedAt: number;
  poolDigestHex: string;
  lanes: BlackboxLaneView[];
  authorityNote: string;
};

export type BlackboxProfileMetaView = {
  poolDigestHex: string;
  schemaVersion: string;
  instrument: string;
  calibration: string;
  pooledCampaigns: number;
  estimatedAt: number;
  updatedAt: number;
};

const MICRO = 1_000_000;

/** Display value: never coerce null/unmeasured to 0. */
export function formatLaneValue(lane: BlackboxLaneView): string {
  if (!lane.measured || lane.valueMicro === null) {
    return "未測定";
  }
  // Integer-only (N-1 twin of Rust format_value_micro).
  const v = Math.trunc(lane.valueMicro);
  const sign = v < 0 ? "-" : "";
  const abs = Math.abs(v);
  const whole = Math.trunc(abs / MICRO);
  const frac = abs % MICRO;
  return `${sign}${whole}.${String(frac).padStart(6, "0")}`;
}

export function formatSufficiency(lane: BlackboxLaneView): string {
  // Integer-only (N-1 twin of Rust format_sufficiency).
  const clamped = Math.max(0, Math.min(MICRO, Math.trunc(lane.sufficiencyMicro)));
  const whole = Math.trunc(clamped / MICRO);
  const frac = Math.trunc((clamped % MICRO) / 1_000);
  return `${whole}.${String(frac).padStart(3, "0")}`;
}

export function isAbsentProfile(view: BlackboxProfileView): boolean {
  return view.pooledCampaigns === 0 && view.poolDigestHex === "";
}
