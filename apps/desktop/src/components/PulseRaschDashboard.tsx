import { useEffect, useReducer } from "react";

import {
  calculateInteractionPulse,
  evaluateRaschScale,
  getLatestRaschState,
  getProbeQuestions,
  isPocketBrainInvokeError,
} from "../lib/pocketBrain";
import { hapticSelectionChanged } from "../lib/haptics";
import {
  expectedAbility,
  initialPulseViewState,
  parseRaschStateView,
  pulseViewBusy,
  pulseViewReducer,
  RASCH_ABILITY_GRID,
} from "../lib/pulseViewReducer";

const LIKERT = [
  { value: 0, label: "0 全く違う" },
  { value: 1, label: "1" },
  { value: 2, label: "2 どちらでもない" },
  { value: 3, label: "3" },
  { value: 4, label: "4 非常に当てはまる" },
] as const;

function MeterBar({
  value,
  max = 1,
  label,
  display,
}: {
  value: number | null;
  max?: number;
  label: string;
  display: string;
}) {
  const pct =
    value === null || !Number.isFinite(value) || max <= 0
      ? 0
      : Math.max(0, Math.min(100, Math.round((value / max) * 100)));
  return (
    <div className="pulse-metric-row">
      <span className="term-source-name">{label}</span>
      <div
        className="pulse-metric-meter"
        role="meter"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={pct}
        aria-label={label}
      >
        <div className="pulse-metric-fill" style={{ width: `${pct}%` }} />
      </div>
      <span className="term-value">{display}</span>
    </div>
  );
}

/** Native SVG posterior density over the 17-point ability grid. */
function PosteriorSparkline({ posterior }: { posterior: number[] }) {
  if (posterior.length !== RASCH_ABILITY_GRID.length) {
    return <p className="hint">事後分布の次元が不正です（期待: 17）。</p>;
  }
  const max = Math.max(...posterior, 1e-12);
  const w = 340;
  const h = 80;
  const pad = 4;
  const barW = (w - pad * 2) / posterior.length;
  const peakIdx = posterior.indexOf(max);
  const peakAbility = RASCH_ABILITY_GRID[peakIdx] ?? 0;
  const summary = `Rasch 事後分布。格子 ${posterior.length} 点。ピーク能力値 ${peakAbility.toFixed(2)}、相対密度 ${max.toFixed(4)}。`;
  return (
    <>
      <svg
        className="rasch-posterior-svg"
        viewBox={`0 0 ${w} ${h}`}
        role="img"
        aria-label={summary}
      >
        <title>Dynamic Ordinal Rasch posterior</title>
        {posterior.map((p, i) => {
          const bh = (p / max) * (h - 16);
          const x = pad + i * barW;
          const y = h - 12 - bh;
          return (
            <rect
              key={RASCH_ABILITY_GRID[i]}
              className="rasch-posterior-bar"
              x={x + 1}
              y={y}
              width={Math.max(1, barW - 2)}
              height={Math.max(0.5, bh)}
            />
          );
        })}
        <text className="rasch-posterior-axis" x={pad} y={h - 2} aria-hidden="true">
          −4
        </text>
        <text
          className="rasch-posterior-axis"
          x={w / 2}
          y={h - 2}
          textAnchor="middle"
          aria-hidden="true"
        >
          0
        </text>
        <text
          className="rasch-posterior-axis"
          x={w - pad}
          y={h - 2}
          textAnchor="end"
          aria-hidden="true"
        >
          +4
        </text>
      </svg>
      <p className="sr-only" aria-live="polite" aria-atomic="true">
        {summary}
      </p>
    </>
  );
}

/**
 * Romance Pulse + Rasch evaluation dashboard (native HTML/CSS/SVG only).
 */
export function PulseRaschDashboard() {
  const [state, dispatch] = useReducer(
    pulseViewReducer,
    undefined,
    initialPulseViewState,
  );
  const busy = pulseViewBusy(state.phase);

  async function loadRasch() {
    dispatch({ type: "load_rasch_begin" });
    try {
      const [raw, bank] = await Promise.all([
        getLatestRaschState(),
        getProbeQuestions(),
      ]);
      dispatch({
        type: "load_rasch_success",
        persisted: parseRaschStateView(raw),
        bank,
      });
    } catch (e) {
      const message = isPocketBrainInvokeError(e)
        ? e.message
        : `rasch load: ${String(e)}`;
      dispatch({ type: "load_rasch_failure", message });
    }
  }

  useEffect(() => {
    void loadRasch();
  }, []);

  async function onPulse() {
    const transcript = state.transcript.trim();
    if (!transcript || busy) return;
    dispatch({ type: "pulse_begin" });
    try {
      const result = await calculateInteractionPulse(transcript);
      dispatch({ type: "pulse_success", result });
    } catch (e) {
      const message = isPocketBrainInvokeError(e)
        ? e.message
        : `calculate_interaction_pulse: ${String(e)}`;
      dispatch({ type: "pulse_failure", message });
    }
  }

  async function onRasch() {
    if (!state.selectedItemId || busy) return;
    dispatch({ type: "rasch_begin" });
    try {
      const prior = state.rasch?.posterior ?? state.raschPersisted?.posterior;
      const excluded =
        state.rasch?.excluded ?? state.raschPersisted?.excluded ?? [];
      const result = await evaluateRaschScale({
        posterior: prior ?? null,
        item_id: state.selectedItemId,
        response: state.response,
        excluded,
      });
      dispatch({ type: "rasch_success", result });
      // Phase 10: Rasch response commit → Selection Changed.
      hapticSelectionChanged();
    } catch (e) {
      const message = isPocketBrainInvokeError(e)
        ? e.message
        : `evaluate_rasch_scale: ${String(e)}`;
      dispatch({ type: "rasch_failure", message });
    }
  }

  const affinity = state.pulse?.analysis.affinity_score ?? null;
  const metrics = state.pulse?.metrics ?? null;
  const posterior =
    state.rasch?.posterior ?? state.raschPersisted?.posterior ?? null;
  const theta = posterior ? expectedAbility(posterior) : null;

  return (
    <div className="pulse-rasch-dashboard">
      <p className="term-header">
        <span className="desktop-only">PULSE_VIEW (Romance + Rasch)</span>
        <span className="mobile-only">Pulse</span>
      </p>
      <p className="hint dev-noise">
        対人パルスは `[self]` / `[contact_alias]` 行のみ。生トランスクリプトは Vault に保存されません。
        Rasch は PCM・discrimination≡1.0・グリッド17点。外部チャートライブラリ不使用。
      </p>

      <div className="gap-tensor-grid">
        <div className="term-panel">
          <p className="term-header">
            <span className="desktop-only">INTERACTION_PULSE</span>
            <span className="mobile-only">対話</span>
          </p>
          <textarea
            className="pulse-transcript"
            rows={8}
            value={state.transcript}
            disabled={busy}
            placeholder={
              "[self] こんにちは\n[contact_alias] どうも\n[self] 近況どう？\n[contact_alias] 元気だよ"
            }
            onChange={(e) =>
              dispatch({ type: "set_transcript", value: e.target.value })
            }
          />
          <div className="action-row">
            <button
              type="button"
              className="primary"
              disabled={busy || !state.transcript.trim()}
              onClick={() => void onPulse()}
            >
              {state.phase === "computing_pulse"
                ? "計算中…"
                : "パルスを計算"}
            </button>
          </div>

          {state.pulse && (
            <div className="romance-score-block pulse-result-block">
              <span className="romance-score-label">親和度 (affinity)</span>
              <span className="romance-score-value">
                {affinity === null ? "N/A" : String(affinity)}
              </span>
              <div
                className="romance-pulse-meter"
                role="meter"
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={affinity ?? undefined}
                aria-label="親和度メーター"
              >
                <div
                  className="romance-pulse-fill"
                  style={{
                    width: `${affinity === null ? 0 : Math.max(0, Math.min(100, affinity))}%`,
                  }}
                />
              </div>
              <dl className="romance-detail-list">
                <div>
                  <dt>傾向</dt>
                  <dd>{state.pulse.analysis.interaction_tendency}</dd>
                </div>
                <div>
                  <dt>次の一手</dt>
                  <dd>{state.pulse.analysis.next_best_action}</dd>
                </div>
              </dl>
              {metrics && (
                <div className="pulse-metrics">
                  <MeterBar
                    label="balance"
                    value={metrics.balance}
                    display={metrics.balance.toFixed(3)}
                  />
                  <MeterBar
                    label="switch_rate"
                    value={metrics.switch_rate}
                    display={metrics.switch_rate.toFixed(3)}
                  />
                  <MeterBar
                    label="reply_coverage"
                    value={metrics.reply_coverage}
                    display={metrics.reply_coverage.toFixed(3)}
                  />
                  <div className="term-row">
                    <span className="term-source-name">turns</span>
                    <span className="term-value">
                      total={metrics.total} self={metrics.self_count} contact=
                      {metrics.contact_count}
                    </span>
                  </div>
                </div>
              )}
              <p className="romance-disclaimer">
                この指数は会話の往復量を示すもので、相手の好意や感情を判定するものではありません。
              </p>
            </div>
          )}
        </div>

        <div className="term-panel">
          <p className="term-header">RASCH_SCALE</p>
          <div className="action-row">
            <button
              type="button"
              className="ghost"
              disabled={busy}
              onClick={() => void loadRasch()}
            >
              {state.phase === "loading_rasch"
                ? "読込中…"
                : "最新 Rasch を再読込"}
            </button>
          </div>

          {theta !== null && (
            <div className="term-row">
              <span className="term-source-name">EAP θ̂</span>
              <span className="term-value">{theta.toFixed(3)}</span>
            </div>
          )}

          {posterior && <PosteriorSparkline posterior={posterior} />}

          {(state.rasch?.excluded.length ??
            state.raschPersisted?.excluded.length ??
            0) > 0 && (
            <p className="hint">
              excluded:{" "}
              {(state.rasch?.excluded ?? state.raschPersisted?.excluded ?? [])
                .length}
              件
            </p>
          )}

          <div className="term-row config-row">
            <span className="term-source-name">item_id</span>
            <select
              value={state.selectedItemId}
              disabled={busy}
              onChange={(e) =>
                dispatch({ type: "set_item", itemId: e.target.value })
              }
            >
              {state.bank.length === 0 && (
                <option value="">（バンク未読込）</option>
              )}
              {state.bank.map((q) => (
                <option key={q.id} value={q.id}>
                  {q.id}
                </option>
              ))}
            </select>
          </div>

          <div className="rasch-likert" role="radiogroup" aria-label="応答 0–4">
            {LIKERT.map((opt) => (
              <label key={opt.value} className="rasch-likert-option">
                <input
                  type="radio"
                  name="rasch-response"
                  value={opt.value}
                  checked={state.response === opt.value}
                  disabled={busy}
                  onChange={() => {
                    dispatch({ type: "set_response", value: opt.value });
                    hapticSelectionChanged();
                  }}
                />
                {opt.label}
              </label>
            ))}
          </div>

          <div className="action-row">
            <button
              type="button"
              className="primary"
              disabled={busy || !state.selectedItemId}
              onClick={() => void onRasch()}
            >
              {state.phase === "updating_rasch"
                ? "更新中…"
                : "Rasch を更新"}
            </button>
          </div>

          {state.rasch?.next && (
            <div className="term-row">
              <span className="term-source-name">next (EIG)</span>
              <span className="term-value">
                {state.rasch.next.item_id} · eig=
                {state.rasch.next.eig.toFixed(6)}
              </span>
            </div>
          )}
        </div>
      </div>

      {state.error && (
        <p className="status-line error-text" role="alert">
          {state.error}
        </p>
      )}
    </div>
  );
}
