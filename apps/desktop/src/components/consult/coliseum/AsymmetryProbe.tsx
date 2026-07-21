/**
 * Phase 14 — I-22 Asymmetry Probe.
 * Visual proof that interviewer LLM receives AbstractTacticSet only:
 * vault raw fossils stay sealed (crimson); compiled tactics are emerald.
 * No external assets — monospace + CSS only.
 */

export type AsymmetryProbeProps = {
  /** Compiled abstract tactic ids for this turn (LLM-visible). */
  activeTactics: string[];
  /** Optional public context fragments shown in the proof line. */
  contextParts?: string[];
  /** Mock sealed vault fossils (never sent to LLM; display-only, blurred). */
  sealedFossils?: { kind: string; preview: string }[];
};

const DEFAULT_CONTEXT = ["ES", "transcript", "company_facts"] as const;

const DEFAULT_FOSSILS: { kind: string; preview: string }[] = [
  { kind: "distortion", preview: "破局視:「また全部ダメになる」 snippet#d-8841" },
  { kind: "purchase", preview: "impulse · 02:17 JST · pur-a9f2 · ¥12,400" },
  { kind: "twin", preview: "r_at_decision=0.28 · active_distortions_json={…}" },
];

export function AsymmetryProbe({
  activeTactics,
  contextParts = [...DEFAULT_CONTEXT],
  sealedFossils = DEFAULT_FOSSILS,
}: AsymmetryProbeProps) {
  const tacticN = activeTactics.length;
  const contextLine = [
    ...contextParts,
    tacticN > 0 ? `tactic#${tacticN}` : "tactic#0",
  ].join(" · ");

  return (
    <section className="asym-probe" aria-label="I-22 asymmetry probe">
      <header className="asym-probe-head">
        <span className="asym-probe-title">I-22 ASYMMETRY</span>
        <div className="asym-probe-flow" aria-hidden="true">
          <span className="term-tag term-tag--danger">[ VAULT SEALED ]</span>
          <span className="asym-flow-arrow">-&gt;</span>
          <span className="term-tag term-tag--ok">[ TACTICS ONLY ]</span>
        </div>
      </header>

      <div className="asym-probe-proof">
        <div className="asym-proof-live">
          Interviewer context this turn: {contextLine}
        </div>
        <div className="asym-proof-excluded">
          Raw distortions / purchases: NOT in context window.
        </div>
      </div>

      <div className="asym-compile-grid">
        <div className="asym-vault-pane hatch-danger" aria-label="Sealed vault raw data">
          <div className="asym-pane-label asym-pane-label-sealed">
            VAULT RAW · SEALED · LLM-OPAQUE
          </div>
          <ul className="asym-fossil-list">
            {sealedFossils.map((f) => (
              <li key={`${f.kind}-${f.preview}`} className="asym-fossil-row">
                <span className="asym-fossil-kind term-tag term-tag--danger">
                  [ {f.kind.toUpperCase()} ]
                </span>
                <span className="asym-fossil-preview" aria-hidden="true">
                  {f.preview}
                </span>
                <span className="sr-only">
                  Sealed {f.kind} fossil (contents hidden from interviewer)
                </span>
              </li>
            ))}
          </ul>
        </div>

        <div className="ascii-flow" role="separator" aria-label="Non-invertible compile">
          -&gt; NON-INVERTIBLE COMPILE -&gt;
        </div>

        <div className="asym-tactics-pane hatch-ok" aria-label="Abstract tactics only">
          <div className="asym-pane-label asym-pane-label-tactics">
            ABSTRACT TACTICS · LLM-VISIBLE
          </div>
          {activeTactics.length === 0 ? (
            <div className="asym-tactics-empty coliseum-text-muted">
              (no tactics compiled this turn)
            </div>
          ) : (
            <ol className="asym-tactics-list">
              {activeTactics.map((t, i) => (
                <li key={`${i}-${t}`} className="asym-tactic-item">
                  <span className="asym-tactic-idx term-tag term-tag--ok">#{i + 1}</span>
                  <span className="asym-tactic-id">{t}</span>
                </li>
              ))}
            </ol>
          )}
        </div>
      </div>
    </section>
  );
}
