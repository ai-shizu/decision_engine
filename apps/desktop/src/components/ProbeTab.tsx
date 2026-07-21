import { useState } from "react";
import { useIsNarrowViewport } from "../lib/useIsNarrowViewport";
import { PocketProbePanel } from "./PocketProbePanel";
import { PulseRaschDashboard } from "./PulseRaschDashboard";

type ProbeSurface = "pb_probe" | "pulse_rasch";

/**
 * PROBE tab. Coraxis-only: PocketProbe + Pulse/Rasch (Python legacy removed).
 */
export function ProbeTab() {
  const isNarrow = useIsNarrowViewport();
  const [surface, setSurface] = useState<ProbeSurface>("pb_probe");

  if (isNarrow) {
    return (
      <section className="panel probe-panel probe-panel-mobile">
        <h2>
          <span className="term-tag term-tag--info">[ PROBE ]</span>
        </h2>
        <div className="ascii-sep ascii-sep--info" role="separator">
          --- FUNNEL ---
        </div>
        <PocketProbePanel />
      </section>
    );
  }

  return (
    <section className="panel probe-panel">
      <div className="probe-topline">
        <div>
          <h2>
            <span className="desktop-only">
              PROBE <span className="term-tag term-tag--info">[ SELF-PROBE ]</span>
            </span>
            <span className="mobile-only">
              <span className="term-tag term-tag--info">[ PROBE ]</span>
            </span>
          </h2>
          <p className="hint guide">
            Coraxis PROBE ファネルと Romance/Rasch パルス。
          </p>
        </div>
      </div>

      <div className="ascii-sep" role="separator">
        --- SURFACE ---
      </div>

      <div className="sub-tabs sub-tabs-pills" role="tablist" aria-label="PROBE面">
        <button
          type="button"
          role="tab"
          aria-selected={surface === "pb_probe"}
          className={surface === "pb_probe" ? "active" : ""}
          onClick={() => setSurface("pb_probe")}
        >
          <span className="desktop-only">PROBE (PB)</span>
          <span className="mobile-only">PROBE</span>
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={surface === "pulse_rasch"}
          className={surface === "pulse_rasch" ? "active" : ""}
          onClick={() => setSurface("pulse_rasch")}
        >
          <span className="desktop-only">PULSE / RASCH</span>
          <span className="mobile-only">PULSE</span>
        </button>
      </div>

      <div className="ascii-flow" role="separator" aria-hidden="true">
        -&gt; {surface === "pb_probe" ? "FUNNEL" : "PULSE"} -&gt;
      </div>

      {surface === "pb_probe" && <PocketProbePanel />}
      {surface === "pulse_rasch" && <PulseRaschDashboard />}
    </section>
  );
}
