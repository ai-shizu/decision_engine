/**
 * Phase 14 — Debrief: Layer-1 scorecard (always visible) + Layer-2 sealed mirror
 * (two-step opt-in) + immutable artifact footer.
 */

import { useState } from "react";

import {
  applyLayer2Consent,
  applyLayer2Reveal,
  canRevealLayer2,
  formatTurnBadge,
  type Layer2OptInPhase,
} from "../../../lib/debriefOptInLogic";

export type Layer1AxisRow = {
  axis: string;
  score: number;
  turnId: string;
  quote: string;
};

export type Layer2InsightRow = {
  parallelLabel: string;
  interviewTurnId: string;
  vaultPattern: string;
  note: string;
};

export type ArtifactMeta = {
  frozenLabel: string;
  fingerprint: string;
  modelHash: string;
  seeds: string[];
};

export type ColiseumDebriefProps = {
  halted?: boolean;
  overallPass?: boolean;
  layer1?: Layer1AxisRow[];
  layer2?: Layer2InsightRow[];
  artifact?: ArtifactMeta;
};

const MOCK_LAYER1: Layer1AxisRow[] = [
  {
    axis: "MECE_STRUCTURE",
    score: 0.82,
    turnId: "t-1",
    quote: "需要・供給・規制の三層で切ります。",
  },
  {
    axis: "HYPOTHESIS_THINKING",
    score: 0.74,
    turnId: "t-3",
    quote: "10^7 規模。感度は価格弾性に依存。",
  },
  {
    axis: "QUANTITATIVE_VALIDITY",
    score: 0.71,
    turnId: "t-3",
    quote: "10^7 規模。感度は価格弾性に依存。",
  },
  {
    axis: "STRESS_RESILIENCE",
    score: 0.68,
    turnId: "t-5",
    quote: "需要側が半減すればオーダーは一桁下がる。",
  },
];

const MOCK_LAYER2: Layer2InsightRow[] = [
  {
    parallelLabel: "圧迫下の視野狭窄",
    interviewTurnId: "t-4",
    vaultPattern: "catastrophizing",
    note: "面接での一問詰まりは、先月の破局視ログと同型の縮退パターン。",
  },
  {
    parallelLabel: "定量回避の再発",
    interviewTurnId: "t-2",
    vaultPattern: "mental_filter",
    note: "数字要求への遅延は、Vault 上の filter 傾向と位相が一致。",
  },
];

const MOCK_ARTIFACT: ArtifactMeta = {
  frozenLabel: "FROZEN @ ENTRY SNAPSHOT",
  fingerprint: "8f4e…2a1b",
  modelHash: "tensor_v3_hash",
  seeds: ["0x_C41", "0x_D92"],
};

function EvalLayer1({
  rows,
  overallPass,
}: {
  rows: Layer1AxisRow[];
  overallPass: boolean;
}) {
  const [openTurn, setOpenTurn] = useState<string | null>(null);

  function toggle(turnId: string) {
    setOpenTurn((prev) => (prev === turnId ? null : turnId));
  }

  return (
    <section className="debrief-l1" aria-label="Layer 1 interview evaluation">
      <header className="debrief-layer-head">
        <span className="debrief-layer-id">LAYER-1 · InterviewEvaluationV1</span>
        <span className="debrief-layer-tag coliseum-text-cyan">ALWAYS VISIBLE</span>
      </header>
      <p className="debrief-layer-sub coliseum-text-muted">
        Transcript provenance only · construct purity · no vault ids
      </p>
      <ul className="debrief-l1-list">
        {rows.map((row) => {
          const badge = formatTurnBadge(row.turnId);
          const open = openTurn === row.turnId;
          return (
            <li key={row.axis} className="debrief-l1-row">
              <div className="debrief-l1-main">
                <span className="debrief-l1-axis">{row.axis}</span>
                <span className="debrief-l1-score coliseum-text-cyan">
                  {row.score.toFixed(2)}
                </span>
                <button
                  type="button"
                  className={
                    open
                      ? "debrief-turn-badge debrief-turn-badge-open"
                      : "debrief-turn-badge"
                  }
                  aria-expanded={open}
                  aria-controls={`quote-${row.turnId}`}
                  title="Toggle transcript evidence"
                  onClick={() => toggle(row.turnId)}
                  onMouseEnter={() => setOpenTurn(row.turnId)}
                >
                  {badge}
                </button>
              </div>
              {open && (
                <blockquote
                  id={`quote-${row.turnId}`}
                  className="debrief-quote"
                  cite={`#${row.turnId}`}
                >
                  <span className="debrief-quote-meta">{badge} EVIDENCE</span>
                  <span className="debrief-quote-text">{row.quote}</span>
                </blockquote>
              )}
            </li>
          );
        })}
      </ul>
      <div
        className={
          overallPass
            ? "debrief-l1-foot coliseum-text-ok"
            : "debrief-l1-foot coliseum-text-amber"
        }
      >
        overall_pass={overallPass ? "true" : "false"}
      </div>
    </section>
  );
}

function EvalLayer2({ insights }: { insights: Layer2InsightRow[] }) {
  const [phase, setPhase] = useState<Layer2OptInPhase>("sealed");
  const revealed = phase === "revealed";
  const consented = phase === "consented" || revealed;

  return (
    <section className="debrief-l2" aria-label="Layer 2 metacognitive debrief">
      <header className="debrief-layer-head">
        <span className="debrief-layer-id">LAYER-2 · MetacognitiveDebriefV1</span>
        <span className="debrief-layer-tag coliseum-text-amber">OPT-IN MIRROR</span>
      </header>
      <p className="debrief-layer-sub coliseum-text-muted">
        Never feeds pass/fail · VaultMirrorAbstract only
      </p>

      <div
        className={
          revealed
            ? "debrief-l2-veil debrief-l2-veil-clear"
            : "debrief-l2-veil debrief-l2-veil-sealed"
        }
        data-testid="debrief-l2-veil"
      >
        <ul className="debrief-l2-list" aria-hidden={!revealed}>
          {insights.map((ins) => (
            <li key={`${ins.interviewTurnId}-${ins.vaultPattern}`} className="debrief-l2-row">
              <div className="debrief-l2-link">
                <span className="coliseum-text-cyan">
                  {formatTurnBadge(ins.interviewTurnId)}
                </span>
                <span className="debrief-l2-arrow">↔</span>
                <span className="coliseum-text-ok">{ins.vaultPattern}</span>
              </div>
              <div className="debrief-l2-label">{ins.parallelLabel}</div>
              <div className="debrief-l2-note">{ins.note}</div>
            </li>
          ))}
        </ul>
        {!revealed && (
          <div className="debrief-l2-hatch" aria-hidden="true" />
        )}
      </div>

      {!revealed && (
        <div className="debrief-l2-gate">
          <label className="debrief-l2-consent">
            <input
              type="checkbox"
              checked={consented}
              onChange={(e) =>
                setPhase((p) => applyLayer2Consent(p, e.target.checked))
              }
            />
            <span>I consent to view private cognitive patterns</span>
          </label>
          <button
            type="button"
            className="debrief-reveal-btn"
            disabled={!canRevealLayer2(phase)}
            onClick={() => setPhase((p) => applyLayer2Reveal(p))}
          >
            [ REVEAL ]
          </button>
        </div>
      )}
      {revealed && (
        <div className="debrief-l2-unlocked coliseum-text-ok" role="status">
          MIRROR UNLOCKED · ONE-WAY · SESSION-SCOPED
        </div>
      )}
    </section>
  );
}

function ArtifactFooter({ meta }: { meta: ArtifactMeta }) {
  return (
    <footer className="debrief-artifact" aria-label="Immutable session artifact">
      <span>{meta.frozenLabel}</span>
      <span>FINGERPRINT: {meta.fingerprint}</span>
      <span>MODEL: {meta.modelHash}</span>
      <span>SEEDS: {meta.seeds.join(", ")}</span>
    </footer>
  );
}

export function ColiseumDebrief({
  halted = false,
  overallPass = true,
  layer1 = MOCK_LAYER1,
  layer2 = MOCK_LAYER2,
  artifact = MOCK_ARTIFACT,
}: ColiseumDebriefProps) {
  return (
    <section className="coliseum-panel coliseum-debrief" aria-label="Coliseum debrief">
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
        <div className="coliseum-frame coliseum-frame-tall debrief-frame">
          <EvalLayer1 rows={layer1} overallPass={overallPass} />
        </div>
        <div className="coliseum-frame coliseum-frame-tall debrief-frame">
          <EvalLayer2 insights={layer2} />
        </div>
      </div>

      <ArtifactFooter meta={artifact} />
    </section>
  );
}
