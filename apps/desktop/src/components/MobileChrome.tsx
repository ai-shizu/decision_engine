import { useState } from "react";
import type { MobileSurface } from "../lib/types";
import { useIsNarrowViewport } from "../lib/useIsNarrowViewport";
import { GapTensorDashboard } from "./GapTensorDashboard";
import { MobileBottomNav } from "./MobileBottomNav";
import { PocketBrainPanel } from "./PocketBrainPanel";
import { ProbeTab } from "./ProbeTab";
import { VaultPanel } from "./VaultPanel";

/**
 * M20-A mobile shell: RAG / Gap·Tensor / Probe.
 * CSS hides desktop chrome ≤768px; matchMedia skips mounting this tree on desktop
 * so Pocket Brain / Probe do not double-invoke beside the 7-tab UI.
 */
export function MobileChrome({ statusLine }: { statusLine: string }) {
  const isNarrow = useIsNarrowViewport();
  const [surface, setSurface] = useState<MobileSurface>("rag");

  if (!isNarrow) return null;

  return (
    <div className="mobile-chrome">
      <header className="mobile-topbar">
        <div>
          <h1>PKB</h1>
          <p className="subtitle">{statusLine}</p>
        </div>
      </header>
      <main className="mobile-content">
        <div
          id="mobile-panel-rag"
          className="mobile-panel"
          role="tabpanel"
          aria-labelledby="mobile-tab-rag"
          hidden={surface !== "rag"}
        >
          {surface === "rag" && (
            <>
              <PocketBrainPanel />
              <VaultPanel />
            </>
          )}
        </div>
        <div
          id="mobile-panel-dashboard"
          className="mobile-panel"
          role="tabpanel"
          aria-labelledby="mobile-tab-dashboard"
          hidden={surface !== "dashboard"}
        >
          {surface === "dashboard" && (
            <section className="panel">
              <h2>Gap / Tensor</h2>
              <p className="hint">M18-C ダッシュボード（モバイル導線）</p>
              <GapTensorDashboard />
            </section>
          )}
        </div>
        <div
          id="mobile-panel-probe"
          className="mobile-panel"
          role="tabpanel"
          aria-labelledby="mobile-tab-probe"
          hidden={surface !== "probe"}
        >
          {surface === "probe" && <ProbeTab />}
        </div>
      </main>
      <MobileBottomNav active={surface} onSelect={setSurface} />
    </div>
  );
}
