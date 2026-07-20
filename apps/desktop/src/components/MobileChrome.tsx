import { useState } from "react";
import type { MobileSurface } from "../lib/types";
import { MOBILE_ALL_SURFACES } from "../lib/mobileNav";
import { useIsNarrowViewport } from "../lib/useIsNarrowViewport";
import { ImportTab } from "./ImportTab";
import { InterviewTab } from "./InterviewTab";
import { MobileBottomNav } from "./MobileBottomNav";
import { PocketBrainPanel } from "./PocketBrainPanel";
import { ProbeTab } from "./ProbeTab";
import { ProfileTab } from "./ProfileTab";
import { RecordTab } from "./RecordTab";
import { SettingsTab } from "./SettingsTab";

function renderMobileSurface(id: MobileSurface, engineReady: boolean) {
  switch (id) {
    case "record":
      return <RecordTab />;
    case "consult":
      // M20-D: RAG tab abolished — messenger UI lives on CONSULT (mobile only).
      return <PocketBrainPanel variant="messenger" />;
    case "interview":
      return <InterviewTab />;
    case "probe":
      return <ProbeTab />;
    case "profile":
      return <ProfileTab />;
    case "import":
      return <ImportTab />;
    case "settings":
      return <SettingsTab engineReady={engineReady} />;
    default: {
      const _exhaustive: never = id;
      throw new Error(`unreachable mobile surface: ${_exhaustive as string}`);
    }
  }
}

export interface MobileChromeProps {
  statusLine: string;
  /** When false (LoadingScreen), engine IPC tabs show wait/retry instead of hard fail. */
  engineReady?: boolean;
}

/**
 * M20-D: default RECORD; dock RECORD/CONSULT/INTERVIEW/PROBE/MENU;
 * Menu = PROFILE/IMPORT/SETTINGS only. Desktop chrome untouched.
 */
export function MobileChrome({
  statusLine,
  engineReady = false,
}: MobileChromeProps) {
  const isNarrow = useIsNarrowViewport();
  const [surface, setSurface] = useState<MobileSurface>("record");
  const [menuOpen, setMenuOpen] = useState(false);

  if (!isNarrow) return null;

  const chatActive = surface === "consult";

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
          chatActive ? "mobile-content mobile-content-rag" : "mobile-content"
        }
      >
        {MOBILE_ALL_SURFACES.map((id) => (
          <div
            key={id}
            id={`mobile-panel-${id}`}
            className={
              id === "consult"
                ? "mobile-panel mobile-panel-rag"
                : "mobile-panel"
            }
            role="tabpanel"
            aria-labelledby={
              id === "settings" || id === "import" || id === "profile"
                ? undefined
                : `mobile-tab-${id}`
            }
            hidden={surface !== id}
          >
            {surface === id && renderMobileSurface(id, engineReady)}
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
