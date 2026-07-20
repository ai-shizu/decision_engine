import type { MainTab } from "./types";

/** Mobile surface = desktop MainTab (RAG id abolished in M20-D). */
export type MobileSurface = MainTab;

export const MOBILE_DOCK_PRIMARY: {
  id: MobileSurface;
  label: string;
  caption: string;
}[] = [
  { id: "record", label: "RECORD", caption: "記録" },
  { id: "consult", label: "CONSULT", caption: "チャット" },
  { id: "interview", label: "INTERVIEW", caption: "面接" },
  { id: "probe", label: "PROBE", caption: "Pulse" },
];

/** Menu-only destinations — must not duplicate dock primaries. */
export const MOBILE_MENU_DESTINATIONS: {
  id: MobileSurface;
  label: string;
  caption: string;
}[] = [
  { id: "profile", label: "PROFILE", caption: "Gap/Tensor" },
  { id: "import", label: "IMPORT", caption: "取込" },
  { id: "settings", label: "SETTINGS", caption: "設定" },
];

/** All mountable mobile panels (dock ∪ menu). */
export const MOBILE_ALL_SURFACES: MobileSurface[] = [
  ...MOBILE_DOCK_PRIMARY.map((d) => d.id),
  ...MOBILE_MENU_DESTINATIONS.map((d) => d.id),
];

export function isMobileDockPrimary(id: MobileSurface): boolean {
  return MOBILE_DOCK_PRIMARY.some((d) => d.id === id);
}

export function mobileDestMeta(id: MobileSurface): {
  label: string;
  caption: string;
} {
  const hit =
    MOBILE_DOCK_PRIMARY.find((d) => d.id === id) ??
    MOBILE_MENU_DESTINATIONS.find((d) => d.id === id);
  return hit ?? { label: id.toUpperCase(), caption: "" };
}
