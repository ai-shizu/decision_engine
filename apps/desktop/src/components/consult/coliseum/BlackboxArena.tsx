/**
 * BLACKBOX Arena container — sole side-effect holder.
 * Dumb views below; IPC via blackboxArena.ts; state via pure reducer.
 */

import { useEffect, useReducer, useRef, useState } from "react";

import {
  BlackboxArenaIpcError,
  bxsAbort,
  bxsAdvance,
  bxsStartCampaign,
  bxsSubmitDecision,
  bxsTakeFlavor,
} from "../../../lib/blackboxArena";
import {
  blackboxArenaReducer,
  initialBlackboxArenaState,
} from "../../../lib/blackboxArenaReducer";
import { buildIntentFromDraft } from "../../../lib/blackboxDraftIntent";
import { pollForFlavor } from "../../../lib/flavorPoll";
import { BlackboxIntentError } from "../../../lib/blackboxIntent";
import { blackboxUiErrorMessage } from "../../../lib/blackboxUiError";
import { todayIso } from "../../../lib/dateUtils";
import { uiErrorMessage } from "../../../lib/uiErrorMessages";
import type { ArenaDifficulty } from "../../../lib/parseBlackboxArena";
import { BlackboxBooksPanel } from "./BlackboxBooksPanel";
import { BlackboxCommandConsole } from "./BlackboxCommandConsole";
import { BlackboxFlavorSlot } from "./BlackboxFlavorSlot";
import { BlackboxMarketRail } from "./BlackboxMarketRail";
import {
  BlackboxSetupPanel,
  type BlackboxSetupValues,
} from "./BlackboxSetupPanel";
import { BlackboxStimulusDeck } from "./BlackboxStimulusDeck";
import { BlackboxTurnLog } from "./BlackboxTurnLog";

export interface BlackboxArenaProps {
  /** When true, abort the live campaign (SovereignBar halt). */
  haltRequest?: number;
  onCampaignIdChange?: (campaignId: string | null) => void;
}

export function BlackboxArena({
  haltRequest = 0,
  onCampaignIdChange,
}: BlackboxArenaProps) {
  const [state, dispatch] = useReducer(
    blackboxArenaReducer,
    undefined,
    initialBlackboxArenaState,
  );
  const [setup, setSetup] = useState<BlackboxSetupValues>(() => ({
    scenarioId: "0",
    difficulty: "standard" as ArenaDifficulty,
    campaignIndex: "0",
    createdDate: todayIso(),
  }));
  /** Ambient flavor prose; null is the default (draw nothing). */
  const [flavorText, setFlavorText] = useState<string | null>(null);
  const inFlight = useRef(false);
  const observeShownAt = useRef<number | null>(null);
  const lastHalt = useRef(0);
  /**
   * Generation counter for the ambient flavor poll. Bumping it supersedes any
   * poll still running, which is what keeps a late arrival from painting prose
   * onto a turn it does not belong to.
   */
  const flavorPollId = useRef(0);

  // Supersede any in-flight poll on unmount so it cannot setState afterwards.
  useEffect(
    () => () => {
      flavorPollId.current += 1;
    },
    [],
  );

  /**
   * Fire-and-forget: never awaited on the turn path, so the arena stays
   * responsive and A-4's "never waits on the model" still holds. The value
   * lands ~267ms after `advance` returns, which a single pull always missed.
   */
  function pullFlavorForCurrentTick(campaignId: string): void {
    flavorPollId.current += 1;
    const myId = flavorPollId.current;
    void pollForFlavor({
      take: () => bxsTakeFlavor(campaignId),
      sleep: (ms) =>
        new Promise<void>((resolve) => {
          setTimeout(resolve, ms);
        }),
      cancelled: () => flavorPollId.current !== myId,
    })
      .then((prose) => {
        if (flavorPollId.current !== myId) return;
        if (prose !== null) setFlavorText(prose);
      })
      .catch((flavorErr: unknown) => {
        console.error("[BlackboxArena] take_flavor failed:", flavorErr);
      });
  }

  useEffect(() => {
    onCampaignIdChange?.(state.campaignId);
  }, [state.campaignId, onCampaignIdChange]);

  useEffect(() => {
    if (state.observation) {
      observeShownAt.current = performance.now();
    }
  }, [state.observation]);

  useEffect(() => {
    if (haltRequest === lastHalt.current) return;
    lastHalt.current = haltRequest;
    const id = state.campaignId;
    if (!id || state.sealed) return;
    void (async () => {
      try {
        await bxsAbort(id);
      } catch (err) {
        console.error("[BlackboxArena] abort failed:", err);
      } finally {
        dispatch({ type: "sealed" });
        // Supersede first: a poll landing after the halt must not repaint prose
        // onto a sealed campaign.
        flavorPollId.current += 1;
        setFlavorText(null);
      }
    })();
  }, [haltRequest, state.campaignId, state.sealed]);

  async function handleIgnite() {
    if (inFlight.current) return;
    const scenarioId = Number(setup.scenarioId);
    const campaignIndex = Number(setup.campaignIndex);
    if (!Number.isSafeInteger(scenarioId) || scenarioId < 0) {
      dispatch({
        type: "command_failed",
        message: uiErrorMessage("BXS_ARENA_STATE"),
      });
      return;
    }
    if (!Number.isSafeInteger(campaignIndex) || campaignIndex < 0) {
      dispatch({
        type: "command_failed",
        message: uiErrorMessage("BXS_ARENA_STATE"),
      });
      return;
    }
    inFlight.current = true;
    dispatch({ type: "submit_begin" });
    try {
      const observation = await bxsStartCampaign({
        scenarioId,
        difficulty: setup.difficulty,
        campaignIndex,
        createdDate: setup.createdDate.trim(),
      });
      dispatch({ type: "campaign_started", observation });
      // A poll from a previous campaign must not paint into the new one.
      flavorPollId.current += 1;
      setFlavorText(null);
    } catch (err) {
      console.error("[BlackboxArena] start failed:", err);
      const code =
        err instanceof BlackboxArenaIpcError ? err.code : "unknown";
      dispatch({
        type: "command_failed",
        message: blackboxUiErrorMessage(code),
      });
    } finally {
      inFlight.current = false;
    }
  }

  async function handleExecute() {
    if (inFlight.current || !state.campaignId || !state.limits || state.sealed) {
      return;
    }
    let intent;
    try {
      intent = buildIntentFromDraft(state.draft, state.limits);
    } catch (err) {
      console.error("[BlackboxArena] intent build failed:", err);
      dispatch({
        type: "command_failed",
        message:
          err instanceof BlackboxIntentError
            ? uiErrorMessage("BXS_ARENA_REJECTED")
            : uiErrorMessage("BXS_ARENA_FAULT"),
      });
      return;
    }

    const started = observeShownAt.current;
    const latencyMs =
      started === null
        ? null
        : Math.max(0, Math.round(performance.now() - started));

    inFlight.current = true;
    dispatch({ type: "submit_begin" });
    try {
      const outcome = await bxsSubmitDecision(
        state.campaignId,
        intent,
        latencyMs,
      );
      dispatch({ type: "submit_ok", outcome });
      const advance = await bxsAdvance(state.campaignId);
      dispatch({ type: "advance_ok", advance });
      // Prose belongs to the tick that produced it. Clear the previous line
      // before polling so a stale one cannot linger over the new turn.
      setFlavorText(null);
      // Non-blocking pull after advance returns — never waits on model (A-4).
      pullFlavorForCurrentTick(state.campaignId);
    } catch (err) {
      console.error("[BlackboxArena] execute/advance failed:", err);
      const code =
        err instanceof BlackboxArenaIpcError ? err.code : "unknown";
      dispatch({
        type: "command_failed",
        message: blackboxUiErrorMessage(code),
      });
    } finally {
      inFlight.current = false;
    }
  }

  const live = state.observation !== null && !state.sealed;

  return (
    <div className="bxs-root" data-busy={state.busy ? "1" : "0"}>
      {!live && (
        <BlackboxSetupPanel
          values={setup}
          onChange={(patch) => setSetup((prev) => ({ ...prev, ...patch }))}
          onIgnite={() => void handleIgnite()}
          busy={state.busy}
          error={state.error}
        />
      )}

      {live && state.observation && state.limits && (
        <>
          <BlackboxMarketRail
            market={state.observation.market}
            turnsCompleted={state.observation.turnsCompleted}
            limits={state.limits}
          />
          <div className="bxs-main-grid">
            <BlackboxBooksPanel books={state.observation.books} />
            <div className="bxs-main-side">
              <BlackboxStimulusDeck stimuli={state.observation.stimuli} />
              <BlackboxCommandConsole
                draft={state.draft}
                limits={state.limits}
                busy={state.busy}
                sealed={state.sealed}
                onDraftChange={(patch) =>
                  dispatch({ type: "draft_changed", draft: patch })
                }
                onExecute={() => void handleExecute()}
              />
            </div>
          </div>
          <BlackboxFlavorSlot text={flavorText} />
          <BlackboxTurnLog entries={state.turnLog} />
          {state.error && (
            <p className="error-text" role="alert">
              {state.error}
            </p>
          )}
          <p className="bxs-session-note">
            IN-PROCESS SESSION · campaign{" "}
            {state.campaignId?.slice(0, 12) ?? "—"}… · abort via SOVEREIGN BAR
          </p>
        </>
      )}

      {state.sealed && (
        <section className="bxs-sealed" role="status">
          <header className="bxs-panel-head">
            <span className="bxs-panel-title">CAMPAIGN SEALED</span>
          </header>
          <BlackboxTurnLog entries={state.turnLog} />
          <button
            type="button"
            className="bxs-btn-primary"
            onClick={() => dispatch({ type: "reset" })}
          >
            [ NEW CAMPAIGN ]
          </button>
        </section>
      )}
    </div>
  );
}
