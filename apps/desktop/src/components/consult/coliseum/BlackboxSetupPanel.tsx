/**
 * BLACKBOX campaign setup — scenario / difficulty / index / date.
 * Dumb view: no IPC.
 */

import type { ArenaDifficulty } from "../../../lib/parseBlackboxArena";

export interface BlackboxSetupValues {
  scenarioId: string;
  difficulty: ArenaDifficulty;
  campaignIndex: string;
  createdDate: string;
}

export interface BlackboxSetupPanelProps {
  values: BlackboxSetupValues;
  onChange: (next: Partial<BlackboxSetupValues>) => void;
  onIgnite: () => void;
  busy: boolean;
  error: string | null;
}

export function BlackboxSetupPanel({
  values,
  onChange,
  onIgnite,
  busy,
  error,
}: BlackboxSetupPanelProps) {
  return (
    <section className="bxs-setup" aria-label="BLACKBOX campaign setup">
      <header className="bxs-panel-head">
        <span className="bxs-panel-title">GENESIS · CAMPAIGN PARAMETERS</span>
        <span className="bxs-ambient" data-busy={busy ? "1" : "0"}>
          {busy ? "BUS BUSY" : "READY"}
        </span>
      </header>
      <p className="bxs-setup-note">
        プロセス内セッションのみ。再起動・タブ離脱で失われます（vault 再開は未結線）。
      </p>
      <div className="bxs-setup-grid">
        <label className="bxs-field">
          <span>SCENARIO ID</span>
          <input
            type="text"
            inputMode="numeric"
            value={values.scenarioId}
            onChange={(e) => onChange({ scenarioId: e.target.value })}
            aria-busy={busy}
          />
        </label>
        <label className="bxs-field">
          <span>DIFFICULTY</span>
          <select
            value={values.difficulty}
            onChange={(e) =>
              onChange({
                difficulty: e.target.value === "hard" ? "hard" : "standard",
              })
            }
            aria-busy={busy}
          >
            <option value="standard">standard</option>
            <option value="hard">hard</option>
          </select>
        </label>
        <label className="bxs-field">
          <span>CAMPAIGN INDEX</span>
          <input
            type="text"
            inputMode="numeric"
            value={values.campaignIndex}
            onChange={(e) => onChange({ campaignIndex: e.target.value })}
            aria-busy={busy}
          />
        </label>
        <label className="bxs-field">
          <span>CREATED DATE (YYYY-MM-DD)</span>
          <input
            type="text"
            value={values.createdDate}
            onChange={(e) => onChange({ createdDate: e.target.value })}
            aria-busy={busy}
          />
        </label>
      </div>
      {error && (
        <p className="error-text" role="alert">
          {error}
        </p>
      )}
      <button
        type="button"
        className="bxs-btn-primary"
        onClick={onIgnite}
        aria-busy={busy}
      >
        [ IGNITE CAMPAIGN ]
      </button>
    </section>
  );
}
