import { useReducer, useRef } from "react";

import { CompanyFactsForm } from "./CompanyFactsForm";
import { todayIso } from "../../lib/dateUtils";
import { companyFactsReady } from "../../lib/interviewStage";
import { redactHiddenReasoning } from "../../lib/redactHiddenReasoning";
import {
  isPocketBrainInvokeError,
  reviewEsDraft,
} from "../../lib/pocketBrain";
import type { CompanyFacts } from "../../lib/pocketBrain/types";
import {
  esReviewReducer,
  initialEsReviewState,
} from "../../lib/esReviewReducer";
import { useCompanyFactsEnrichment } from "../../lib/useCompanyFactsEnrichment";
import { useThrottledStream } from "../../lib/useThrottledStream";

let nextMsgId = 1;
function allocId(prefix: string): string {
  nextMsgId += 1;
  return `${prefix}-${nextMsgId}`;
}

/**
 * Pocket Brain ES review: review_es_draft + offline CompanyFacts + streaming feedback.
 */
export function EsReviewPanel({
  sharedFacts,
  onSharedFactsPatch,
  hideEmbeddedFactsForm = false,
}: {
  sharedFacts?: CompanyFacts;
  onSharedFactsPatch?: (patch: Partial<CompanyFacts>) => void;
  hideEmbeddedFactsForm?: boolean;
} = {}) {
  const [state, dispatch] = useReducer(
    esReviewReducer,
    undefined,
    initialEsReviewState,
  );
  const reviewerIdRef = useRef<string | null>(null);
  const logRef = useRef<HTMLDivElement>(null);

  const facts = sharedFacts ?? state.facts;
  function patchFacts(patch: Partial<CompanyFacts>) {
    if (onSharedFactsPatch) onSharedFactsPatch(patch);
    else dispatch({ type: "patch_facts", patch });
  }

  const { researching, provenanceLabel, enrichNow } = useCompanyFactsEnrichment(
    facts,
    patchFacts,
    !hideEmbeddedFactsForm,
  );

  const throttle = useThrottledStream((chunk) => {
    const id = reviewerIdRef.current;
    if (!id) return;
    dispatch({ type: "token", reviewerId: id, text: chunk });
  });

  function scrollToBottom() {
    requestAnimationFrame(() => {
      logRef.current?.scrollTo({ top: logRef.current.scrollHeight });
    });
  }

  async function onReview(e: React.FormEvent) {
    e.preventDefault();
    const draft = state.esDraft.trim();
    if (!draft || state.streaming) return;
    if (!companyFactsReady(facts)) {
      dispatch({
        type: "review_failure",
        message: "企業名を入力してから添削してください。",
      });
      return;
    }

    dispatch({ type: "clear_error" });
    const userId = allocId("es-u");
    const reviewerId = allocId("es-r");
    reviewerIdRef.current = reviewerId;
    dispatch({ type: "review_begin", userId, reviewerId, draft });
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
    } catch {
      // proceed with typed facts
    }

    const edinetDate = todayIso();
    try {
      const result = await reviewEsDraft(
        {
          esDraft: draft,
          companyFacts: factsPayload,
          edinetDate,
          experienceQuery: state.experienceQuery.trim() || undefined,
        },
        (event) => {
          if (event.error) {
            dispatch({ type: "token_error", message: event.error });
            return;
          }
          if (event.done) {
            throttle.flushAndStop();
            return;
          }
          if (event.text) {
            throttle.push(event.text);
          }
        },
      );

      throttle.flushAndStop();
      dispatch({ type: "review_success", reviewerId, result });
    } catch (err) {
      throttle.flushAndStop();
      const message = isPocketBrainInvokeError(err)
        ? err.message
        : `es review: ${String(err)}`;
      dispatch({ type: "review_failure", message });
    } finally {
      reviewerIdRef.current = null;
      dispatch({ type: "review_end" });
      scrollToBottom();
    }
  }

  return (
    <div className="es-review-panel">
      <p className="hint dev-noise">
        Pocket Brain ES 添削 (`review_es_draft`): 企業ファクト + RAG 経験チャンクを根拠に採用責任者ペルソナが添削します。
      </p>

      {!hideEmbeddedFactsForm && (
        <CompanyFactsForm
          facts={facts}
          disabled={state.streaming}
          onPatch={patchFacts}
          researching={researching}
          provenanceLabel={provenanceLabel}
        />
      )}

      {facts.companyName.trim() && (
        <div className="term-panel company-facts-preview">
          <p className="term-header">FACTS_PREVIEW</p>
          <div className="term-row">
            <span className="term-source-name">company</span>
            <span className="term-value">{facts.companyName}</span>
          </div>
          {facts.businessSummary.trim() && (
            <pre className="term-es-body">{facts.businessSummary}</pre>
          )}
          {state.streaming && (
            <p className="hint ambient-spinner" role="status">
              添削生成中…
            </p>
          )}
        </div>
      )}

      <form className="consult-form" onSubmit={(e) => void onReview(e)}>
        <div className="term-row config-row">
          <span className="term-source-name">経験 RAG クエリ (任意)</span>
          <input
            value={state.experienceQuery}
            disabled={state.streaming}
            onChange={(e) =>
              dispatch({
                type: "set_experience_query",
                value: e.target.value,
              })
            }
            placeholder="空欄なら ES 本文を検索クエリに使用"
          />
        </div>
        <div className="term-row config-row">
          <span className="term-source-name">ES 本文 *</span>
          <textarea
            className="custom-theme-textarea"
            rows={8}
            value={state.esDraft}
            onChange={(e) =>
              dispatch({ type: "set_draft", value: e.target.value })
            }
            placeholder="エントリーシート本文を貼り付け…"
          />
        </div>
        <div className="action-row">
          <button
            type="submit"
            className="primary"
            disabled={
              state.streaming ||
              !state.esDraft.trim() ||
              !companyFactsReady(facts)
            }
          >
            {state.streaming ? "添削中…" : "ES を添削"}
          </button>
          <button
            type="button"
            className="ghost"
            disabled={state.streaming || state.messages.length === 0}
            onClick={() => dispatch({ type: "reset_feedback" })}
          >
            フィードバックをクリア
          </button>
        </div>
      </form>

      <div className="chat-log line-chat" ref={logRef}>
        {state.messages.length === 0 ? (
          <p className="hint chat-empty">
            ES と企業ファクトを入力し、「ES を添削」でストリーミング講評を開始します。
          </p>
        ) : (
          state.messages.map((m) => {
            const visible = redactHiddenReasoning(m.text, m.streaming);
            if (m.role === "user") {
              return (
                <div key={m.id} className="line-row user">
                  <div className="line-bubble user">
                    <span className="speaker-name">提出 ES</span>
                    <pre className="chat-text">{visible}</pre>
                  </div>
                </div>
              );
            }
            return (
              <div key={m.id} className="line-row feedback">
                <div className="line-bubble feedback">
                  <span className="feedback-label">■ 採用責任者 — ES 添削</span>
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

      {state.meta && (
        <div className="term-panel">
          <p className="term-header">REVIEW_META</p>
          <div className="term-row">
            <span className="term-source-name">company_name</span>
            <span className="term-value">{state.meta.companyName}</span>
          </div>
          <div className="term-row">
            <span className="term-source-name">facts_source</span>
            <span className="term-value">{state.meta.factsSource}</span>
          </div>
          <div className="term-row">
            <span className="term-source-name">context_count</span>
            <span className="term-value">{state.meta.contextCount}</span>
          </div>
        </div>
      )}

      {state.error && (
        <p className="status-line error-text" role="alert">
          {state.error}
        </p>
      )}
    </div>
  );
}
