import type { MainTab, MobileSurface } from "./types";

/** Desktop MainTab order (SPEC_FOXTROT) — mobile must expose every id. */
export const MAIN_TAB_ORDER: { id: MainTab; label: string }[] = [
  { id: "record", label: "RECORD" },
  { id: "import", label: "IMPORT" },
  { id: "consult", label: "CONSULT" },
  { id: "interview", label: "INTERVIEW" },
  { id: "probe", label: "PROBE" },
  { id: "profile", label: "PROFILE" },
  { id: "settings", label: "SETTINGS" },
];

/** RAG (Pocket Brain) + full desktop 7-tab parity for mobile. */
export const MOBILE_DESTINATIONS: { id: MobileSurface; label: string; caption: string }[] =
  [
    { id: "rag", label: "RAG", caption: "Home" },
    ...MAIN_TAB_ORDER.map(({ id, label }) => ({
      id,
      label,
      caption:
        id === "interview"
          ? "面接"
          : id === "profile"
            ? "Gap/Tensor"
            : id === "probe"
              ? "Pulse"
              : id === "consult"
                ? "相談"
                : id === "record"
                  ? "記録"
                  : id === "import"
                    ? "取込"
                    : "設定",
    })),
  ];

/**
 * Bottom dock (Approach B): always-visible primaries.
 * Interview is slot 2 — one tap, never buried in Menu.
 */
export const MOBILE_DOCK_PRIMARY: MobileSurface[] = [
  "rag",
  "interview",
  "probe",
  "profile",
];

export function isMobileDockPrimary(id: MobileSurface): boolean {
  return (MOBILE_DOCK_PRIMARY as string[]).includes(id);
}
