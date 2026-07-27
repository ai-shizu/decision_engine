/**
 * Command console — one intent per tick. All 12 player intents exposed.
 * Inputs stay enabled while busy (ambient UX); single-flight is the container's job.
 */

import { INTENT_KINDS, type IntentKind } from "../../../lib/blackboxIntent";
import type { BlackboxDraft } from "../../../lib/blackboxArenaReducer";
import type { ArenaLimitsView } from "../../../lib/parseBlackboxArena";

export interface BlackboxCommandConsoleProps {
  draft: BlackboxDraft;
  limits: ArenaLimitsView;
  busy: boolean;
  sealed: boolean;
  onDraftChange: (patch: Partial<BlackboxDraft>) => void;
  onExecute: () => void;
}

function Field({
  label,
  value,
  onChange,
  busy,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  busy: boolean;
}) {
  return (
    <label className="bxs-field">
      <span>{label}</span>
      <input
        type="text"
        inputMode="numeric"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        aria-busy={busy}
      />
    </label>
  );
}

export function BlackboxCommandConsole({
  draft,
  limits,
  busy,
  sealed,
  onDraftChange,
  onExecute,
}: BlackboxCommandConsoleProps) {
  const kind = draft.kind;
  return (
    <section
      className="bxs-console"
      data-busy={busy ? "1" : "0"}
      aria-label="Decision console"
    >
      <header className="bxs-panel-head">
        <span className="bxs-panel-title">COMMAND · 1 INTENT / TICK</span>
        <span className="bxs-ambient" data-busy={busy ? "1" : "0"}>
          {sealed ? "SEALED" : busy ? "BUS BUSY" : "AWAITING"}
        </span>
      </header>
      <label className="bxs-field">
        <span>INTENT</span>
        <select
          value={kind}
          onChange={(e) =>
            onDraftChange({ kind: e.target.value as IntentKind })
          }
          aria-busy={busy}
        >
          {INTENT_KINDS.map((k) => (
            <option key={k} value={k}>
              {k}
            </option>
          ))}
        </select>
      </label>

      <div className="bxs-console-params">
        {(kind === "set_price" || kind === "order_inventory") && (
          <Field
            label={`SKU (0..${limits.maxSkus - 1})`}
            value={draft.sku}
            onChange={(sku) => onDraftChange({ sku })}
            busy={busy}
          />
        )}
        {kind === "set_price" && (
          <Field
            label={`TICK PRICE [${limits.minPriceMinor}..${limits.maxPriceMinor}]`}
            value={draft.tickPrice}
            onChange={(tickPrice) => onDraftChange({ tickPrice })}
            busy={busy}
          />
        )}
        {kind === "order_inventory" && (
          <Field
            label={`UNITS (≤${limits.maxOrderUnits})`}
            value={draft.units}
            onChange={(units) => onDraftChange({ units })}
            busy={busy}
          />
        )}
        {(kind === "accept_offer" || kind === "decline_offer") && (
          <Field
            label="OFFER ID"
            value={draft.offerId}
            onChange={(offerId) => onDraftChange({ offerId })}
            busy={busy}
          />
        )}
        {kind === "close_position" && (
          <Field
            label="POSITION ID"
            value={draft.positionId}
            onChange={(positionId) => onDraftChange({ positionId })}
            busy={busy}
          />
        )}
        {kind === "open_hedge" && (
          <>
            <Field
              label="INSTRUMENT"
              value={draft.instrument}
              onChange={(instrument) => onDraftChange({ instrument })}
              busy={busy}
            />
            <Field
              label="NOTIONAL MINOR"
              value={draft.notionalMinor}
              onChange={(notionalMinor) => onDraftChange({ notionalMinor })}
              busy={busy}
            />
          </>
        )}
        {(kind === "invest" ||
          kind === "continue_project" ||
          kind === "abandon_project") && (
          <Field
            label="PROJECT ID"
            value={draft.projectId}
            onChange={(projectId) => onDraftChange({ projectId })}
            busy={busy}
          />
        )}
        {kind === "invest" && (
          <Field
            label="AMOUNT MINOR"
            value={draft.amountMinor}
            onChange={(amountMinor) => onDraftChange({ amountMinor })}
            busy={busy}
          />
        )}
        {(kind === "borrow" || kind === "repay") && (
          <>
            <Field
              label="FACILITY"
              value={draft.facility}
              onChange={(facility) => onDraftChange({ facility })}
              busy={busy}
            />
            <Field
              label="AMOUNT MINOR"
              value={draft.amountMinor}
              onChange={(amountMinor) => onDraftChange({ amountMinor })}
              busy={busy}
            />
          </>
        )}
        {kind === "forecast_interval" && (
          <>
            <Field
              label="LO MINOR"
              value={draft.loMinor}
              onChange={(loMinor) => onDraftChange({ loMinor })}
              busy={busy}
            />
            <Field
              label="HI MINOR"
              value={draft.hiMinor}
              onChange={(hiMinor) => onDraftChange({ hiMinor })}
              busy={busy}
            />
          </>
        )}
      </div>

      <button
        type="button"
        className="bxs-btn-primary"
        onClick={onExecute}
        aria-busy={busy}
      >
        [ EXECUTE ]
      </button>
    </section>
  );
}
