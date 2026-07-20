//! Pure helpers for mobile dock selection styling (no React).

/** Active vs idle class pair — never reuse bare `.active` (desktop tab collisions). */
export function dockItemClassName(selected: boolean): string {
  return selected
    ? "mobile-nav-item mobile-nav-item--active"
    : "mobile-nav-item mobile-nav-item--idle";
}

/** Dock primary is selected only when surface matches and menu sheet is closed. */
export function isDockPrimarySelected(
  surface: string,
  itemId: string,
  menuOpen: boolean,
): boolean {
  return !menuOpen && surface === itemId;
}

/** MENU chrome highlights when sheet is open OR a menu-only surface is shown. */
export function isMenuChromeSelected(
  menuOpen: boolean,
  surfaceIsDockPrimary: boolean,
): boolean {
  return menuOpen || !surfaceIsDockPrimary;
}
