import { useReducer, useRef } from "react";

import { CompanyFactsForm } from "./CompanyFactsForm";
import { InterviewStageRail } from "./InterviewStageRail";
import { todayIso } from "../../lib/dateUtils";
import { companyFactsReady } from "../../lib/interviewStage";
import { interviewFallbackFor } from "../../lib/interviewFallback";
import {
  advanceInterviewStage,
  getInterviewSession,
  ingestSessionMemory,
  startMultistageInterview,
} from "../../lib/pocketBrain";
import type { CompanyFacts, MultistageInterviewResult } from "../../lib/pocketBrain/types";
import {
  initialMultistageInterviewState,
  multistageInterviewReducer,
} from "../../lib/multistageInterviewReducer";
import {
  extractHiddenReasoning,
  interviewReasoningMode,
  visibleBody,
} from "../../lib/reasoningVisibility";
import { createStreamTerminalGate } from "../../lib/streamTerminalGate";
import { useCompanyFactsEnrichment } from "../../lib/useCompanyFactsEnrichment";
import { useThrottledStream } from "../../lib/useThrottledStream";

const STREAM_TERMINAL_TIMEOUT_MS = 180_000;

let nextMsgId = 1;
function allocId(prefix: string): string {
  nextMsgId += 1;
  return `${prefix}-${nextMsgId}`;
}

/**
 * M17 multistage interview surface: Foundation → Pressure → Debrief → Closed.
 * Streams interviewer turns via Channel + useThrottledStream (M18-A pattern).
 */
export function MultistageInterviewPanel({
  sharedFacts,
  onSharedFactsPatch,
  hideEmbeddedFactsForm = false,
  preparingOverride,
}: {
  sharedFacts?: CompanyFacts;
  onSharedFactsPatch?: (patch: Partial<CompanyFacts>) => void;
  hideEmbeddedFactsForm?: boolean;
  preparingOverride?: boolean;
} = {}) {
  const [state, dispatch] = useReducer(
    multistageInterviewReducer,
    undefined,
    initialMultistageInterviewState,
  );
  const assistantIdRef = useRef<string | null>(null);
  const logRef = useRef<HTMLDivElement>(null);

  const facts = sharedFacts ?? state.facts;
  function patchFacts(patch: Partial<CompanyFacts>) {
    if (onSharedFactsPatch) onSharedFactsPatch(patch);
    else dispatch({ type: "patch_facts", patch });
  }

  // Shared form on narrow InterviewTab owns debounce; embedded form enriches here.
  const { researching, preparing: localPreparing, provenanceLabel, enrichNow } =
    useCompanyFactsEnrichment(
      facts,
      patchFacts,
      !hideEmbeddedFactsForm && !state.sessionId,
    );
  const preparing = preparingOverride ?? localPreparing;

  const throttle = useThrottledStream((chunk) => {
    const id = assistantIdRef.current;
    if (!id) return;
    dispatch({ type: "token", assistantId: id, text: chunk });
  });

  function scrollToBottom() {
    requestAnimationFrame(() => {
      logRef.current?.scrollTo({ top: logRef.current.scrollHeight });
    });
  }

  async function hydrateAfter(result: MultistageInterviewResult) {
    try {
      const session = await getInterviewSession(result.session_id);
      dispatch({ type: "hydrate_session", session });
    } catch (err) {
      // eslint-disable-next-line no-console -- intentional diagnostic
      console.error("[MultistageInterviewPanel] hydrateAfter failed:", err);
      // Best-effort sync; result meta already applied.
    }
  }

  async function onStart() {
    if (state.streaming || state.sessionId) return;
    if (!companyFactsReady(facts)) {
      dispatch({
        type: "send_failure",
        message: "企業名を入力してから開始してください。",
      });
      return;
    }

    dispatch({ type: "clear_error" });
    const assistantId = allocId("iv");
    assistantIdRef.current = assistantId;
    dispatch({ type: "start_begin", assistantId });
    scrollToBottom();

    let factsPayload: CompanyFacts = {
      ...facts,
      source: facts.source.trim() || "injected",
    };
    try {
      const enriched = await enrichNow();
      factsPayload = {
        ...enriched.facts,
        source: enriched.facts.source.trim() || "injected",
      };
    } catch (err) {
      // eslint-disable-next-line no-console -- intentional diagnostic
      console.error("[MultistageInterviewPanel] enrich soft-fail:", err);
      // Ambient enrich soft-fail — proceed with typed facts.
    }

    const edinetDate = todayIso();
    const terminal = createStreamTerminalGate(STREAM_TERMINAL_TIMEOUT_MS);
    let errored = false;
    try {
      const result = await startMultistageInterview(
        {
          openingMessage: state.openingMessage.trim() || undefined,
          companyFacts: factsPayload,
          edinetDate,
        },
        (event) => {
          if (!terminal.isPending()) return;
          if (event.error) {
            // eslint-disable-next-line no-console -- intentional diagnostic
            console.error("[MultistageInterviewPanel] onStart stream error:", event.error);
            const fb = interviewFallbackFor(event.error);
            errored = true;
            throttle.flushAndStop();
            dispatch({ type: "token_error", message: fb.message });
            terminal.settle();
            return;
          }
          if (event.done) {
            throttle.drainAndStop();
            terminal.settle();
            return;
          }
          if (event.text) {
            throttle.push(event.text);
          }
        },
      );
      await terminal.promise;

      throttle.drainAndStop();
      if (!errored) {
        dispatch({ type: "start_success", assistantId, result });
        await hydrateAfter(result);
      }
    } catch (err) {
      // eslint-disable-next-line no-console -- intentional diagnostic
      console.error("[MultistageInterviewPanel] onStart failed:", err);
      throttle.flushAndStop();
      const fb = interviewFallbackFor(err);
      dispatch({
        type: "send_failure",
        message: fb.message,
      });
    } finally {
      terminal.abort();
      assistantIdRef.current = null;
      dispatch({ type: "send_end" });
      scrollToBottom();
    }
  }

  async function onAdvance(e: React.FormEvent) {
    e.preventDefault();
    const answer = state.input.trim();
    if (!answer || state.streaming || !state.sessionId) return;
    if (state.stage === "closed" || state.status === "closed") return;

    dispatch({ type: "clear_error" });
    const userId = allocId("cand");
    const assistantId = allocId("iv");
    assistantIdRef.current = assistantId;
    dispatch({ type: "advance_begin", userId, assistantId, answer });
    scrollToBottom();

    const terminal = createStreamTerminalGate(STREAM_TERMINAL_TIMEOUT_MS);
    let errored = false;
    let streamedAssistant = "";
    try {
      const result = await advanceInterviewStage(
        { sessionId: state.sessionId, candidateAnswer: answer },
        (event) => {
          if (!terminal.isPending()) return;
          if (event.error) {
            // eslint-disable-next-line no-console -- intentional diagnostic
            console.error(
              "[MultistageInterviewPanel] onAdvance stream error:",
              event.error,
            );
            const fb = interviewFallbackFor(event.error);
            errored = true;
            throttle.flushAndStop();
            dispatch({
              type: "token_error",
              message: fb.message,
              restoreInput: fb.resendable ? answer : undefined,
            });
            terminal.settle();
            return;
          }
          if (event.done) {
            throttle.drainAndStop();
            terminal.settle();
            return;
          }
          if (event.text) {
            streamedAssistant += event.text;
            throttle.push(event.text);
          }
        },
      );
      await terminal.promise;

      throttle.drainAndStop();
      if (!errored) {
        if (result.outcome === "closed" || result.stage === "closed") {
          dispatch({ type: "session_closed", result });
          // Closure `state.messages` is pre-advance; append this turn's answer + stream.
          const transcript = state.messages
            .filter((m) => m.role === "candidate" || m.role === "interviewer")
            .map((m) => `${m.role}: ${m.text}`)
            .concat([`candidate: ${answer}`])
            .concat(
              streamedAssistant.trim()
                ? [`interviewer: ${streamedAssistant}`]
                : [],
            )
            .filter((line) => line.trim().length > 0)
            .join("\n");
          if (transcript.trim()) {
            void ingestSessionMemory(transcript, "interview").catch((err) => {
              // eslint-disable-next-line no-console -- intentional diagnostic
              console.error(
                "[MultistageInterviewPanel] session memory ingest failed:",
                err,
              );
            });
          }
        } else {
          dispatch({ type: "advance_success", assistantId, result });
        }
        await hydrateAfter(result);
      }
    } catch (err) {
      // eslint-disable-next-line no-console -- intentional diagnostic
      console.error("[MultistageInterviewPanel] onAdvance failed:", err);
      throttle.flushAndStop();
      const fb = interviewFallbackFor(err);
      dispatch({
        type: "send_failure",
        message: fb.message,
        restoreInput: fb.resendable ? answer : undefined,
      });
    } finally {
      terminal.abort();
      assistantIdRef.current = null;
      dispatch({ type: "send_end" });
      scrollToBottom();
    }
  }

  const closed = state.stage === "closed" || state.status === "closed";
  const inSession = Boolean(state.sessionId);
  const mode = interviewReasoningMode(state.stage, state.outcome);
  const showSessionMeta = mode === "revealed";

  return (
    <div className="multistage-interview-panel">
      <p className="hint mobile-only">
        基礎確認 → 深掘り → 振り返り → 終了の流れで進みます。振り返りで講評が付きます。
      </p>
      <p className="hint dev-noise desktop-only">
        Coraxis 多段面接 (M17): Foundation → Pressure → Debrief → Closed。
        議論フェーズに Gap/Oracle は注入されず、Debrief のみ講評に接続されます。
      </p>

      {showSessionMeta && (
        <InterviewStageRail
          stage={state.stage}
          turnInStage={state.turnInStage}
          totalTurns={state.totalTurns}
          status={state.status}
          outcome={state.outcome}
        />
      )}

      {!inSession && (
        <>
          {!hideEmbeddedFactsForm && (
            <CompanyFactsForm
              facts={facts}
              disabled={state.streaming}
              onPatch={patchFacts}
              researching={researching}
              provenanceLabel={provenanceLabel}
            />
          )}
          <div className="term-row config-row">
            <span className="term-source-name">開始プロンプト (任意)</span>
            <input
              value={state.openingMessage}
              disabled={state.streaming}
              onChange={(e) =>
                dispatch({ type: "set_opening", value: e.target.value })
              }
              placeholder="空欄なら既定の自己紹介・志望動機プロンプト"
            />
          </div>
          <div className="action-row">
            <button
              type="button"
              className="primary"
              disabled={state.streaming || preparing || !companyFactsReady(facts)}
              onClick={() => void onStart()}
            >
              {state.streaming ? "開始中…" : "多段面接を開始"}
            </button>
          </div>
        </>
      )}

      {inSession && (
        <div className="action-row">
          <button
            type="button"
            className="ghost"
            disabled={state.streaming}
            onClick={() => dispatch({ type: "reset" })}
          >
            セッションをリセット
          </button>
          {showSessionMeta && state.companyName && (
            <span className="hint">company={state.companyName}</span>
          )}
        </div>
      )}

      <div className="chat-log line-chat" ref={logRef}>
        {state.messages.length === 0 ? (
          <p className="hint chat-empty">開始後、面接官の発話がここにストリームされます。</p>
        ) : (
          state.messages.map((m) => {
            const visible = visibleBody(m.text, mode, !!m.streaming);
            if (m.role === "candidate") {
              return (
                <div key={m.id} className="line-row user">
                  <div className="line-bubble user">
                    <pre className="chat-text">{visible}</pre>
                  </div>
                </div>
              );
            }
            if (m.role === "system") {
              return (
                <div key={m.id} className="line-row feedback">
                  <div className="line-bubble feedback">
                    <span className="feedback-label">■ セッション</span>
                    <pre className="chat-text">{visible}</pre>
                  </div>
                </div>
              );
            }
            return (
              <div key={m.id} className="line-row ai">
                <div className="line-bubble ai">
                  <span className="speaker-name">
                    面接官 · {m.stage}
                  </span>
                  <pre className="chat-text">
                    {visible}
                    {m.streaming && <span className="chat-cursor">▌</span>}
                  </pre>
                </div>
                {mode === "revealed" && (() => {
                  const thoughts = extractHiddenReasoning(m.text);
                  if (thoughts.length === 0) return null;
                  return (
                    <details className="term-panel interview-reasoning-reveal">
                      <summary>この質問の意図</summary>
                      <pre className="chat-text">{thoughts.join("\n\n")}</pre>
                    </details>
                  );
                })()}
              </div>
            );
          })
        )}
      </div>

      {inSession && !closed && (
        <form className="consult-form" onSubmit={(e) => void onAdvance(e)}>
          <textarea
            value={state.input}
            onChange={(e) =>
              dispatch({ type: "set_input", value: e.target.value })
            }
            placeholder={
              state.stage === "debrief"
                ? "講評への応答（送信でセッションを閉じます）…"
                : "回答を入力…"
            }
            rows={3}
            disabled={state.streaming}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
                void onAdvance(e);
              }
            }}
          />
          <div className="action-row">
            <button
              type="submit"
              className="primary"
              disabled={state.streaming || preparing || !state.input.trim()}
            >
              {state.streaming
                ? "生成中…"
                : state.stage === "debrief"
                  ? "講評を閉じる (Ctrl+Enter)"
                  : "回答を送信 (Ctrl+Enter)"}
            </button>
          </div>
        </form>
      )}

      {closed && (
        <p className="hint">セッションは Closed です。「セッションをリセット」で再開できます。</p>
      )}

      {state.error && (
        <p className="status-line error-text" role="alert">
          {state.error}
        </p>
      )}
    </div>
  );
}
