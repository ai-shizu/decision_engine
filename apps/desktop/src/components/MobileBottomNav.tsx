import { useEffect, useId, useRef } from "react";
import {
  isMobileDockPrimary,
  MOBILE_DOCK_PRIMARY,
  MOBILE_MENU_DESTINATIONS,
} from "../lib/mobileNav";
import type { MobileSurface } from "../lib/types";

type IconKind =
  | "record"
  | "consult"
  | "interview"
  | "probe"
  | "menu"
  | "generic";

function iconFor(id: MobileSurface | "menu"): IconKind {
  if (id === "menu") return "menu";
  if (id === "record") return "record";
  if (id === "consult") return "consult";
  if (id === "interview") return "interview";
  if (id === "probe") return "probe";
  return "generic";
}

function NavIcon({ kind }: { kind: IconKind }) {
  if (kind === "record") {
    return (
      <svg className="mobile-nav-icon" viewBox="0 0 24 24" aria-hidden="true">
        <path
          fill="currentColor"
          d="M5 3h11l3 3v15H5V3zm2 2v14h10V7.8L14.2 5H7zm2 3h8v2H9V8zm0 4h8v2H9v-2zm0 4h5v2H9v-2z"
        />
      </svg>
    );
  }
  if (kind === "consult") {
    return (
      <svg className="mobile-nav-icon" viewBox="0 0 24 24" aria-hidden="true">
        <path
          fill="currentColor"
          d="M4 4h16v12H7.5L4 19.5V4zm2 2v9.2l1.8-1.7H18V6H6z"
        />
      </svg>
    );
  }
  if (kind === "interview") {
    return (
      <svg className="mobile-nav-icon" viewBox="0 0 24 24" aria-hidden="true">
        <path
          fill="currentColor"
          d="M12 3a4 4 0 1 1 0 8 4 4 0 0 1 0-8zm0 10c3.9 0 7 2 7 4.5V20H5v-2.5C5 15 8.1 13 12 13z"
        />
      </svg>
    );
  }
  if (kind === "probe") {
    return (
      <svg className="mobile-nav-icon" viewBox="0 0 24 24" aria-hidden="true">
        <path
          fill="currentColor"
          d="M12 2a7 7 0 0 0-7 7c0 2.4 1.2 4.5 3 5.8V17h8v-2.2c1.8-1.3 3-3.4 3-5.8a7 7 0 0 0-7-7zm-3 17h6v2H9v-2z"
        />
      </svg>
    );
  }
  if (kind === "menu") {
    return (
      <svg className="mobile-nav-icon" viewBox="0 0 24 24" aria-hidden="true">
        <path
          fill="currentColor"
          d="M4 6h16v2H4V6zm0 5h16v2H4v-2zm0 5h16v2H4v-2z"
        />
      </svg>
    );
  }
  return (
    <svg className="mobile-nav-icon" viewBox="0 0 24 24" aria-hidden="true">
      <path fill="currentColor" d="M5 5h6v6H5V5zm8 0h6v6h-6V5zM5 13h6v6H5v-6zm8 0h6v6h-6v-6z" />
    </svg>
  );
}

export interface MobileBottomNavProps {
  active: MobileSurface;
  menuOpen: boolean;
  onSelect: (id: MobileSurface) => void;
  onMenuOpenChange: (open: boolean) => void;
}

/**
 * M20-D: dock [RECORD, CONSULT, INTERVIEW, PROBE, MENU].
 * Menu lists only PROFILE / IMPORT / SETTINGS (no dock duplicates).
 */
export function MobileBottomNav({
  active,
  menuOpen,
  onSelect,
  onMenuOpenChange,
}: MobileBottomNavProps) {
  const titleId = useId();
  const closeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!menuOpen) return;
    closeRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onMenuOpenChange(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [menuOpen, onMenuOpenChange]);

  function select(id: MobileSurface) {
    onSelect(id);
    onMenuOpenChange(false);
  }

  const menuHighlights = menuOpen || !isMobileDockPrimary(active);

  return (
    <>
      <nav
        className="mobile-bottom-nav"
        role="tablist"
        aria-label="モバイル主要タブ"
        aria-orientation="horizontal"
      >
        {MOBILE_DOCK_PRIMARY.map(({ id, label, caption }) => {
          const selected = active === id && !menuOpen;
          return (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={selected}
              aria-controls={`mobile-panel-${id}`}
              id={`mobile-tab-${id}`}
              className={
                selected
                  ? "mobile-nav-item active"
                  : id === "record" || id === "interview"
                    ? "mobile-nav-item mobile-nav-item-priority"
                    : "mobile-nav-item"
              }
              onClick={() => select(id)}
            >
              <NavIcon kind={iconFor(id)} />
              <span className="mobile-nav-label">{label}</span>
              <span className="mobile-nav-caption">{caption}</span>
            </button>
          );
        })}
        <button
          type="button"
          className={
            menuHighlights ? "mobile-nav-item active" : "mobile-nav-item"
          }
          aria-haspopup="dialog"
          aria-expanded={menuOpen}
          aria-controls="mobile-menu-sheet"
          onClick={() => onMenuOpenChange(!menuOpen)}
        >
          <NavIcon kind="menu" />
          <span className="mobile-nav-label">MENU</span>
          <span className="mobile-nav-caption">他</span>
        </button>
      </nav>

      {menuOpen && (
        <div
          className="mobile-menu-backdrop"
          role="presentation"
          onClick={() => onMenuOpenChange(false)}
        >
          <div
            id="mobile-menu-sheet"
            className="mobile-menu-sheet"
            role="dialog"
            aria-modal="true"
            aria-labelledby={titleId}
            onClick={(e) => e.stopPropagation()}
          >
            <div className="mobile-menu-sheet-head">
              <h2 id={titleId}>メニュー</h2>
              <button
                ref={closeRef}
                type="button"
                className="mobile-menu-close"
                aria-label="メニューを閉じる"
                onClick={() => onMenuOpenChange(false)}
              >
                閉じる
              </button>
            </div>
            <p className="hint mobile-menu-hint">
              PROFILE / IMPORT / SETTINGS（ドックと重複しない項目のみ）
            </p>
            <ul className="mobile-menu-list">
              {MOBILE_MENU_DESTINATIONS.map(({ id, label, caption }) => {
                const selected = active === id;
                return (
                  <li key={`menu-${id}`}>
                    <button
                      type="button"
                      className={
                        selected
                          ? "mobile-menu-item active"
                          : "mobile-menu-item"
                      }
                      aria-current={selected ? "page" : undefined}
                      onClick={() => select(id)}
                    >
                      <span className="mobile-menu-item-label">{label}</span>
                      <span className="mobile-menu-item-caption">{caption}</span>
                    </button>
                  </li>
                );
              })}
            </ul>
          </div>
        </div>
      )}
    </>
  );
}
