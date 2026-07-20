import { useReducer, useRef } from "react";

import { CompanyFactsForm } from "./CompanyFactsForm";
import { InterviewStageRail } from "./InterviewStageRail";
import { companyFactsReady } from "../../lib/interviewStage";
import { redactHiddenReasoning } from "../../lib/redactHiddenReasoning";
import {
  advanceInterviewStage,
  getInterviewSession,
  isPocketBrainInvokeError,
  startMultistageInterview,
} from "../../lib/pocketBrain";
import type { CompanyFacts, MultistageInterviewResult } from "../../lib/pocketBrain/types";
import {
  initialMultistageInterviewState,
  multistageInterviewReducer,
} from "../../lib/multistageInterviewReducer";
import { useThrottledStream } from "../../lib/useThrottledStream";

let nextMsgId = 1;
function allocId(prefix: string): string {
  nextMsgId += 1;
  return `${prefix}-${nextMsgId}`;
}

function handleTokenStream(
  event: { text: string; done: boolean; error: string | null },
  throttle: { push: (t: string) => void; flushAndStop: () => void },
  onError: (message: string) => void,
): void {
  if (event.error) {
    onError(event.error);
    return;
  }
  if (event.done) {
    throttle.flushAndStop();
    return;
  }
  if (event.text) {
    throttle.push(event.text);
  }
}

/**
 * M17 multistage interview surface: Foundation → Pressure → Debrief → Closed.
 * Streams interviewer turns via Channel + useThrottledStream (M18-A pattern).
 */
export function MultistageInterviewPanel({
  sharedFacts,
  onSharedFactsPatch,
  hideEmbeddedFactsForm = false,
}: {
  sharedFacts?: CompanyFacts;
  onSharedFactsPatch?: (patch: Partial<CompanyFacts>) => void;
  hideEmbeddedFactsForm?: boolean;
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
    } catch {
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

    const factsPayload: CompanyFacts = {
      ...facts,
      source: facts.source.trim() || "injected",
    };

    try {
      const result = await startMultistageInterview(
        {
          openingMessage: state.openingMessage.trim() || undefined,
          companyFacts: factsPayload,
        },
        (event) => {
          if (event.error) {
            dispatch({ type: "token_error", message: event.error });
            return;
          }
          handleTokenStream(event, throttle, (message) =>
            dispatch({ type: "token_error", message }),
          );
        },
      );

      throttle.flushAndStop();
      dispatch({ type: "start_success", assistantId, result });
      await hydrateAfter(result);
    } catch (e) {
      throttle.flushAndStop();
      const message = isPocketBrainInvokeError(e)
        ? e.message
        : `multistage start: ${String(e)}`;
      dispatch({ type: "send_failure", message });
    } finally {
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

    try {
      const result = await advanceInterviewStage(
        { sessionId: state.sessionId, candidateAnswer: answer },
        (event) => {
          handleTokenStream(event, throttle, (message) =>
            dispatch({ type: "token_error", message }),
          );
        },
      );

      throttle.flushAndStop();

      if (result.outcome === "closed" || result.stage === "closed") {
        dispatch({ type: "session_closed", result });
      } else {
        dispatch({ type: "advance_success", assistantId, result });
      }
      await hydrateAfter(result);
    } catch (err) {
      throttle.flushAndStop();
      const message = isPocketBrainInvokeError(err)
        ? err.message
        : `multistage advance: ${String(err)}`;
      dispatch({ type: "send_failure", message });
    } finally {
      assistantIdRef.current = null;
      dispatch({ type: "send_end" });
      scrollToBottom();
    }
  }

  const closed = state.stage === "closed" || state.status === "closed";
  const inSession = Boolean(state.sessionId);

  return (
    <div className="multistage-interview-panel">
      <p className="hint mobile-only">
        基礎確認 → 深掘り → 振り返り → 終了の流れで進みます。振り返りで講評が付きます。
      </p>
      <p className="hint dev-noise desktop-only">
        Pocket Brain 多段面接 (M17): Foundation → Pressure → Debrief → Closed。
        議論フェーズに Gap/Oracle は注入されず、Debrief のみ講評に接続されます。
      </p>

      <InterviewStageRail
        stage={state.stage}
        turnInStage={state.turnInStage}
        totalTurns={state.totalTurns}
        status={state.status}
        outcome={state.outcome}
      />

      {!inSession && (
        <>
          {!hideEmbeddedFactsForm && (
            <CompanyFactsForm
              facts={facts}
              disabled={state.streaming}
              onPatch={patchFacts}
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
              disabled={state.streaming || !companyFactsReady(facts)}
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
          {state.companyName && (
            <span className="hint">company={state.companyName}</span>
          )}
        </div>
      )}

      <div className="chat-log line-chat" ref={logRef}>
        {state.messages.length === 0 ? (
          <p className="hint chat-empty">開始後、面接官の発話がここにストリームされます。</p>
        ) : (
          state.messages.map((m) => {
            const visible = redactHiddenReasoning(m.text, m.streaming);
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
              disabled={state.streaming || !state.input.trim()}
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
