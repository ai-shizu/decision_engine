import { useEffect, useState } from "react";
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

function renderMobileSurface(
  id: MobileSurface,
  engineReady: boolean,
  onOpenProfile: () => void,
) {
  switch (id) {
    case "record":
      return <RecordTab />;
    case "consult":
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
      return (
        <SettingsTab engineReady={engineReady} onOpenProfile={onOpenProfile} />
      );
    default: {
      const _exhaustive: never = id;
      throw new Error(`unreachable mobile surface: ${_exhaustive as string}`);
    }
  }
}

function isAlarmStatus(line: string): boolean {
  return /失敗|エラー|再起動/.test(line);
}

export interface MobileChromeProps {
  statusLine: string;
  engineReady?: boolean;
}

/**
 * M20-G: dismissible status banner; CONSULT composer stays fixed above dock.
 */
export function MobileChrome({
  statusLine,
  engineReady = false,
}: MobileChromeProps) {
  const isNarrow = useIsNarrowViewport();
  const [surface, setSurface] = useState<MobileSurface>("record");
  const [menuOpen, setMenuOpen] = useState(false);
  const [bannerDismissed, setBannerDismissed] = useState(false);

  useEffect(() => {
    setBannerDismissed(false);
  }, [statusLine]);

  useEffect(() => {
    if (!isAlarmStatus(statusLine) || bannerDismissed) return;
    const t = window.setTimeout(() => setBannerDismissed(true), 8000);
    return () => window.clearTimeout(t);
  }, [statusLine, bannerDismissed]);

  if (!isNarrow) return null;

  const chatActive = surface === "consult";
  const showBanner =
    Boolean(statusLine) && !bannerDismissed && isAlarmStatus(statusLine);

  return (
    <div className="mobile-chrome">
      <header className="mobile-topbar">
        <div className="mobile-topbar-row">
          <h1>Coraxis</h1>
          {!showBanner && statusLine && !isAlarmStatus(statusLine) ? (
            <p className="subtitle">{statusLine}</p>
          ) : null}
        </div>
        {showBanner ? (
          <div className="mobile-status-banner" role="status">
            <p className="mobile-status-banner-text">{statusLine}</p>
            <button
              type="button"
              className="mobile-status-banner-dismiss"
              aria-label="バナーを閉じる"
              onClick={() => setBannerDismissed(true)}
            >
              ×
            </button>
          </div>
        ) : null}
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
            {surface === id &&
              renderMobileSurface(id, engineReady, () => {
                setMenuOpen(false);
                setSurface("profile");
              })}
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
