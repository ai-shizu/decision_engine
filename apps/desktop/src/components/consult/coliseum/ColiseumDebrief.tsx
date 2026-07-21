/**
 * Phase 14 skeleton — Debrief: Layer-1 scorecard vs Layer-2 metacognitive opt-in.
 */

export function ColiseumDebrief({
  halted = false,
}: {
  halted?: boolean;
}) {
  return (
    <section className="coliseum-panel" aria-label="Coliseum debrief">
      <header className="coliseum-panel-head">
        <span className="coliseum-panel-id">VIEW/DEBRIEF</span>
        <span
          className={
            halted
              ? "coliseum-panel-tag coliseum-tag-err"
              : "coliseum-panel-tag coliseum-tag-cyan"
          }
        >
          {halted ? "SOVEREIGN HALT" : "TWO-LAYER EVAL"}
        </span>
      </header>

      <div className="coliseum-grid-2">
        <div className="coliseum-frame coliseum-frame-tall">
          <div className="coliseum-frame-label">
            LAYER-1 · InterviewEvaluationV1 · TRANSCRIPT ONLY
          </div>
          <ul className="coliseum-axis-list">
            {[
              "mece_structure",
              "hypothesis_thinking",
              "quantitative_validity",
              "stress_resilience",
            ].map((axis) => (
              <li key={axis} className="coliseum-axis-row">
                <span className="coliseum-text-cyan">{axis}</span>
                <span className="coliseum-text-muted">score —— · provenance turn_id</span>
              </li>
            ))}
          </ul>
          <div className="coliseum-frame-foot coliseum-text-ok">
            overall_pass · NO vault ids · construct purity
          </div>
        </div>

        <div className="coliseum-frame coliseum-frame-tall">
          <div className="coliseum-frame-label">
            LAYER-2 · MetacognitiveDebriefV1 · OPT-IN MIRROR
          </div>
          <div className="coliseum-optin-box">
            <span className="coliseum-text-amber">OPT-IN REQUIRED</span>
            <span className="coliseum-text-muted">
              VaultMirrorAbstract only · never feeds pass/fail
            </span>
            <button type="button" className="coliseum-btn-ghost" disabled>
              ENABLE MIRROR (PLACEHOLDER)
            </button>
          </div>
          <div className="coliseum-frame-foot coliseum-text-muted">
            artifact fingerprint · frozen evidence bodies
          </div>
        </div>
      </div>
    </section>
  );
}
