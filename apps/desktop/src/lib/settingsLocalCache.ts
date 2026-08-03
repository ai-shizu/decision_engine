//! Local persistence for SETTINGS fixed attributes (mobile seamless mode).
//! Pure helpers — no React. Desktop may ignore; mobile uses as sidecar-free fallback.

import type { SettingsData } from "./types";
import { defaultBirthday } from "./birthdayUtils";

const STORAGE_KEY = "pkb.settings.fixed_attributes.v1";

export function readLocalFixedAttributes(): Record<string, string> | null {
  try {
    if (typeof localStorage === "undefined") return null;
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return null;
    const out: Record<string, string> = {};
    for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof v === "string") out[k] = v;
    }
    return out;
  } catch {
    return null;
  }
}

export function writeLocalFixedAttributes(attrs: Record<string, string>): void {
  try {
    if (typeof localStorage === "undefined") return;
    localStorage.setItem(STORAGE_KEY, JSON.stringify(attrs));
  } catch {
    /* quota / private mode — ignore */
  }
}

/** Skeleton SETTINGS shell for mobile when sidecar IPC is unavailable. */
export function localSettingsShell(
  attrs?: Record<string, string> | null,
): SettingsData {
  const fixed = { ...(attrs ?? {}) };
  if (!fixed.birthday?.trim()) {
    fixed.birthday = defaultBirthday();
  }
  return {
    fixed_fields: [
      { key: "birthday", label: "誕生日" },
      { key: "gender", label: "性別" },
      { key: "height", label: "身長 (cm)" },
      { key: "weight", label: "体重 (kg)" },
      { key: "address", label: "住所" },
      { key: "occupation", label: "勤務先/学校" },
    ],
    fixed_attributes: fixed,
    profile_summary: "",
    apple_calendar_available: false,
  };
}
