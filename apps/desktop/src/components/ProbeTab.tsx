import { useState } from "react";
import { useIsNarrowViewport } from "../lib/useIsNarrowViewport";
import { PocketProbePanel } from "./PocketProbePanel";
import { PulseRaschDashboard } from "./PulseRaschDashboard";

type ProbeSurface = "pb_probe" | "pulse_rasch";

/**
 * PROBE tab — MAGI multi-monitor rack + tactical surface array.
 */
export function ProbeTab() {
  const isNarrow = useIsNarrowViewport();
  const [surface, setSurface] = useState<ProbeSurface>("pb_probe");

  if (isNarrow) {
    return (
      <section className="panel probe-panel probe-panel-mobile magi-rack">
        <div className="magi-mod-head" style={{ borderBottom: "1px solid var(--border)" }}>
          <span className="term-tag term-tag--info">[ PROBE ]</span>
          <span className="micro-tel">MOBILE · FUNNEL</span>
        </div>
        <PocketProbePanel />
      </section>
    );
  }

  return (
    <section className="panel probe-panel magi-rack">
      <div className="probe-topline">
        <h2>
          PROBE <span className="term-tag term-tag--info">[ SELF-PROBE ]</span>
        </h2>
        <span className="micro-tel">SYS.NOMINAL · MAGI-RACK</span>
      </div>

      <div
        className="tactical-array"
        role="tablist"
        aria-label="PROBE面"
      >
        <button
          type="button"
          role="tab"
          aria-selected={surface === "pb_probe"}
          className={surface === "pb_probe" ? "active" : ""}
          onClick={() => setSurface("pb_probe")}
        >
          FUNNEL
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={surface === "pulse_rasch"}
          className={surface === "pulse_rasch" ? "active" : ""}
          onClick={() => setSurface("pulse_rasch")}
        >
          PULSE/RASCH
        </button>
      </div>

      <div className="magi-mod">
        <div className="magi-mod-foot" style={{ borderTop: "none" }}>
          <span>-&gt; {surface === "pb_probe" ? "FUNNEL" : "PULSE"} -&gt;</span>
          <span>SURFACE_LOCK</span>
        </div>
      </div>

      {surface === "pb_probe" && <PocketProbePanel />}
      {surface === "pulse_rasch" && (
        <div className="magi-mod">
          <div className="magi-mod-head">
            <span>[ PULSE_RASCH ]</span>
            <span className="micro-tel">TELEMETRY</span>
          </div>
          <div className="magi-mod-body">
            <PulseRaschDashboard />
          </div>
        </div>
      )}
    </section>
  );
}
