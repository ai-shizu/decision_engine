import { useState } from "react";
import { useIsNarrowViewport } from "../lib/useIsNarrowViewport";
import { PocketProbePanel } from "./PocketProbePanel";
import { PulseRaschDashboard } from "./PulseRaschDashboard";

type ProbeSurface = "pb_probe" | "pulse_rasch";

/**
 * PROBE tab — stoic instrument rack (Japanese-first, no flavor noise).
 */
export function ProbeTab() {
  const isNarrow = useIsNarrowViewport();
  const [surface, setSurface] = useState<ProbeSurface>("pb_probe");

  if (isNarrow) {
    return (
      <section className="panel probe-panel probe-panel-mobile magi-rack">
        <div className="magi-mod-head" style={{ borderBottom: "1px solid var(--border)" }}>
          <span>自己探索</span>
        </div>
        <PocketProbePanel />
      </section>
    );
  }

  return (
    <section className="panel probe-panel magi-rack">
      <div className="probe-topline">
        <h2>自己探索</h2>
      </div>

      <div className="tactical-array" role="tablist" aria-label="PROBE面">
        <button
          type="button"
          role="tab"
          aria-selected={surface === "pb_probe"}
          className={surface === "pb_probe" ? "active" : ""}
          onClick={() => setSurface("pb_probe")}
        >
          [ 探索 ]
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={surface === "pulse_rasch"}
          className={surface === "pulse_rasch" ? "active" : ""}
          onClick={() => setSurface("pulse_rasch")}
        >
          [ パルス ]
        </button>
      </div>

      {surface === "pb_probe" && <PocketProbePanel />}
      {surface === "pulse_rasch" && (
        <div className="magi-mod">
          <div className="magi-mod-head">
            <span>パルス / Rasch</span>
          </div>
          <div className="magi-mod-body">
            <PulseRaschDashboard />
          </div>
        </div>
      )}
    </section>
  );
}
