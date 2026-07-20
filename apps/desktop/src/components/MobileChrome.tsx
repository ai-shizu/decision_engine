import { useState } from "react";
import type { MainTab, MobileSurface } from "../lib/types";
import { MOBILE_DESTINATIONS } from "../lib/mobileNav";
import { useIsNarrowViewport } from "../lib/useIsNarrowViewport";
import { ConsultTab } from "./ConsultTab";
import { ImportTab } from "./ImportTab";
import { InterviewTab } from "./InterviewTab";
import { MobileBottomNav } from "./MobileBottomNav";
import { PocketBrainPanel } from "./PocketBrainPanel";
import { ProbeTab } from "./ProbeTab";
import { ProfileTab } from "./ProfileTab";
import { RecordTab } from "./RecordTab";
import { SettingsTab } from "./SettingsTab";

function renderMainTab(id: MainTab) {
  switch (id) {
    case "record":
      return <RecordTab />;
    case "import":
      return <ImportTab />;
    case "consult":
      return <ConsultTab />;
    case "interview":
      return <InterviewTab />;
    case "probe":
      return <ProbeTab />;
    case "profile":
      return <ProfileTab />;
    case "settings":
      return <SettingsTab />;
    default: {
      const _exhaustive: never = id;
      throw new Error(`unreachable main tab: ${_exhaustive as string}`);
    }
  }
}

function renderMobileSurface(id: MobileSurface) {
  if (id === "rag") {
    // Vault lives in messenger action sheet (+) — keep chat uncluttered.
    return <PocketBrainPanel variant="messenger" />;
  }
  return renderMainTab(id);
}

/**
 * M20-C mobile shell: dock + Menu only; RAG uses messenger layout.
 * CSS hides desktop chrome ≤768px; matchMedia skips mounting on desktop.
 */
export function MobileChrome({ statusLine }: { statusLine: string }) {
  const isNarrow = useIsNarrowViewport();
  const [surface, setSurface] = useState<MobileSurface>("rag");
  const [menuOpen, setMenuOpen] = useState(false);

  if (!isNarrow) return null;

  const ragActive = surface === "rag";

  return (
    <div className="mobile-chrome">
      <header className="mobile-topbar">
        <div>
          <h1>PKB</h1>
          <p className="subtitle">{statusLine}</p>
        </div>
      </header>
      <main
        className={
          ragActive ? "mobile-content mobile-content-rag" : "mobile-content"
        }
      >
        {MOBILE_DESTINATIONS.map(({ id }) => (
          <div
            key={id}
            id={`mobile-panel-${id}`}
            className={
              id === "rag" ? "mobile-panel mobile-panel-rag" : "mobile-panel"
            }
            role="tabpanel"
            aria-labelledby={`mobile-tab-${id}`}
            hidden={surface !== id}
          >
            {surface === id && renderMobileSurface(id)}
          </div>
        ))}
      </main>
      <MobileBottomNav
        active={surface}
        menuOpen={menuOpen}
        onSelect={setSurface}
        onMenuOpenChange={setMenuOpen}
      />
    </div>
  );
}
