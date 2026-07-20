import type { MobileSurface } from "../lib/types";

const ITEMS: {
  id: MobileSurface;
  label: string;
  caption: string;
  icon: "home" | "dashboard" | "probe";
}[] = [
  { id: "rag", label: "RAG", caption: "Home", icon: "home" },
  { id: "dashboard", label: "Dashboard", caption: "Gap/Tensor", icon: "dashboard" },
  { id: "probe", label: "Probe", caption: "Pulse", icon: "probe" },
];

function NavIcon({ kind }: { kind: "home" | "dashboard" | "probe" }) {
  // Inline SVG only — no icon packages (Foxtrot / offline discipline).
  if (kind === "home") {
    return (
      <svg className="mobile-nav-icon" viewBox="0 0 24 24" aria-hidden="true">
        <path
          fill="currentColor"
          d="M12 3.2 3.5 10.2V21h6.2v-6.5h4.6V21h6.2V10.2L12 3.2zm0 2.3 6.5 5.3V19h-3.2v-6.5H8.7V19H5.5v-8.2L12 5.5z"
        />
      </svg>
    );
  }
  if (kind === "dashboard") {
    return (
      <svg className="mobile-nav-icon" viewBox="0 0 24 24" aria-hidden="true">
        <path
          fill="currentColor"
          d="M4 4h7v7H4V4zm9 0h7v5h-7V4zM4 13h7v7H4v-7zm9 3h7v4h-7v-4zm0-6h7v4h-7V10z"
        />
      </svg>
    );
  }
  return (
    <svg className="mobile-nav-icon" viewBox="0 0 24 24" aria-hidden="true">
      <path
        fill="currentColor"
        d="M12 2a7 7 0 0 0-7 7c0 2.4 1.2 4.5 3 5.8V17h8v-2.2c1.8-1.3 3-3.4 3-5.8a7 7 0 0 0-7-7zm-3 17h6v2H9v-2z"
      />
    </svg>
  );
}

export interface MobileBottomNavProps {
  active: MobileSurface;
  onSelect: (id: MobileSurface) => void;
}

/**
 * M20-A: iOS-like bottom tab bar (CSS-shown ≤768px only). Dumb view — no router.
 */
export function MobileBottomNav({ active, onSelect }: MobileBottomNavProps) {
  return (
    <nav
      className="mobile-bottom-nav"
      role="tablist"
      aria-label="モバイルメインタブ"
      aria-orientation="horizontal"
    >
      {ITEMS.map(({ id, label, caption, icon }) => {
        const selected = active === id;
        return (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={selected}
            aria-controls={`mobile-panel-${id}`}
            id={`mobile-tab-${id}`}
            className={selected ? "mobile-nav-item active" : "mobile-nav-item"}
            onClick={() => onSelect(id)}
          >
            <NavIcon kind={icon} />
            <span className="mobile-nav-label">{label}</span>
            <span className="mobile-nav-caption">{caption}</span>
          </button>
        );
      })}
    </nav>
  );
}
