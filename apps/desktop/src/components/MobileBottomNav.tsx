import { useEffect, useId, useRef } from "react";
import {
  isMobileDockPrimary,
  MOBILE_DESTINATIONS,
  MOBILE_DOCK_PRIMARY,
} from "../lib/mobileNav";
import type { MobileSurface } from "../lib/types";

type IconKind = "rag" | "interview" | "probe" | "profile" | "menu" | "generic";

function iconFor(id: MobileSurface | "menu"): IconKind {
  if (id === "menu") return "menu";
  if (id === "rag") return "rag";
  if (id === "interview") return "interview";
  if (id === "probe") return "probe";
  if (id === "profile") return "profile";
  return "generic";
}

function NavIcon({ kind }: { kind: IconKind }) {
  // Inline SVG only — no icon packages.
  if (kind === "rag") {
    return (
      <svg className="mobile-nav-icon" viewBox="0 0 24 24" aria-hidden="true">
        <path
          fill="currentColor"
          d="M12 3.2 3.5 10.2V21h6.2v-6.5h4.6V21h6.2V10.2L12 3.2zm0 2.3 6.5 5.3V19h-3.2v-6.5H8.7V19H5.5v-8.2L12 5.5z"
        />
      </svg>
    );
  }
  if (kind === "interview") {
    return (
      <svg className="mobile-nav-icon" viewBox="0 0 24 24" aria-hidden="true">
        <path
          fill="currentColor"
          d="M4 4h16v12H7.5L4 19.5V4zm2 2v9.2l1.8-1.7H18V6H6zm2 2h8v2H8V8zm0 3h6v2H8v-2z"
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
  if (kind === "profile") {
    return (
      <svg className="mobile-nav-icon" viewBox="0 0 24 24" aria-hidden="true">
        <path
          fill="currentColor"
          d="M4 4h7v7H4V4zm9 0h7v5h-7V4zM4 13h7v7H4v-7zm9 3h7v4h-7v-4zm0-6h7v4h-7V10z"
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

function destMeta(id: MobileSurface) {
  return MOBILE_DESTINATIONS.find((d) => d.id === id);
}

export interface MobileBottomNavProps {
  active: MobileSurface;
  menuOpen: boolean;
  onSelect: (id: MobileSurface) => void;
  onMenuOpenChange: (open: boolean) => void;
}

/**
 * M20-C: bottom dock + Menu drawer only (chip rail removed — nav congestion fix).
 * Interview remains a primary dock slot.
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

  const menuHighlights =
    menuOpen || !isMobileDockPrimary(active);

  return (
    <>
      <nav
        className="mobile-bottom-nav"
        role="tablist"
        aria-label="モバイル主要タブ"
        aria-orientation="horizontal"
      >
        {MOBILE_DOCK_PRIMARY.map((id) => {
          const meta = destMeta(id);
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
                  : id === "interview"
                    ? "mobile-nav-item mobile-nav-item-priority"
                    : "mobile-nav-item"
              }
              onClick={() => select(id)}
            >
              <NavIcon kind={iconFor(id)} />
              <span className="mobile-nav-label">{meta?.label ?? id}</span>
              <span className="mobile-nav-caption">{meta?.caption ?? ""}</span>
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
          <span className="mobile-nav-label">Menu</span>
          <span className="mobile-nav-caption">すべて</span>
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
              <h2 id={titleId}>Navigate</h2>
              <button
                ref={closeRef}
                type="button"
                className="mobile-menu-close"
                aria-label="メニューを閉じる"
                onClick={() => onMenuOpenChange(false)}
              >
                Close
              </button>
            </div>
            <p className="hint mobile-menu-hint">
              デスクトップと同じ全タブ（RAG + 7）。Interview は下ドックからも即開きます。
            </p>
            <ul className="mobile-menu-list">
              {MOBILE_DESTINATIONS.map(({ id, label, caption }) => {
                const selected = active === id;
                return (
                  <li key={`menu-${id}`}>
                    <button
                      type="button"
                      className={
                        selected
                          ? "mobile-menu-item active"
                          : id === "interview"
                            ? "mobile-menu-item mobile-menu-item-priority"
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
