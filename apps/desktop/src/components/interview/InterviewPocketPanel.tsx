import { useReducer, useRef, type ReactNode } from "react";

import { CompanyFactsForm } from "./CompanyFactsForm";
import { todayIso } from "../../lib/dateUtils";
import { companyFactsReady } from "../../lib/interviewStage";
import { redactHiddenReasoning } from "../../lib/redactHiddenReasoning";
import { startInterviewSession } from "../../lib/pocketBrain";
import type { CompanyFacts } from "../../lib/pocketBrain/types";
import { uiErrorMessage } from "../../lib/uiErrorMessages";
import { useCompanyFactsEnrichment } from "../../lib/useCompanyFactsEnrichment";
import { useThrottledStream } from "../../lib/useThrottledStream";

type ChatRole = "user" | "assistant";

interface ChatMsg {
  id: string;
  role: ChatRole;
  text: string;
  streaming?: boolean;
}

interface PanelState {
  facts: CompanyFacts;
  input: string;
  messages: ChatMsg[];
  streaming: boolean;
  error: string | null;
  meta: { companyName: string; factsSource: string; contextCount: number } | null;
}

type PanelAction =
  | { type: "patch_facts"; patch: Partial<CompanyFacts> }
  | { type: "set_input"; value: string }
  | { type: "clear_error" }
  | { type: "send_begin"; userId: string; assistantId: string; prompt: string }
  | { type: "token"; assistantId: string; text: string }
  | { type: "token_error"; message: string }
  | {
      type: "send_success";
      assistantId: string;
      companyName: string;
      factsSource: string;
      contextCount: number;
    }
  | { type: "send_failure"; message: string }
  | { type: "send_end" }
  | { type: "reset" };

function emptyFacts(): CompanyFacts {
  return {
    companyName: "",
    edinetCode: "",
    docId: "",
    businessSummary: "",
    businessRisks: "",
    performanceSummary: "",
    source: "",
  };
}

function initialState(): PanelState {
  return {
    facts: emptyFacts(),
    input: "",
    messages: [],
    streaming: false,
    error: null,
    meta: null,
  };
}

function reducer(state: PanelState, action: PanelAction): PanelState {
  switch (action.type) {
    case "patch_facts":
      return { ...state, facts: { ...state.facts, ...action.patch } };
    case "set_input":
      return { ...state, input: action.value };
    case "clear_error":
      return { ...state, error: null };
    case "send_begin":
      return {
        ...state,
        input: "",
        streaming: true,
        error: null,
        messages: [
          ...state.messages,
          { id: action.userId, role: "user", text: action.prompt },
          { id: action.assistantId, role: "assistant", text: "", streaming: true },
        ],
      };
    case "token":
      return {
        ...state,
        messages: state.messages.map((m) =>
          m.id === action.assistantId ? { ...m, text: m.text + action.text } : m,
        ),
      };
    case "token_error":
      return {
        ...state,
        streaming: false,
        error: action.message,
        messages: state.messages.map((m) =>
          m.streaming ? { ...m, streaming: false } : m,
        ),
      };
    case "send_success":
      return {
        ...state,
        messages: state.messages.map((m) =>
          m.id === action.assistantId ? { ...m, streaming: false } : m,
        ),
        meta: {
          companyName: action.companyName,
          factsSource: action.factsSource,
          contextCount: action.contextCount,
        },
      };
    case "send_failure":
      return {
        ...state,
        streaming: false,
        error: action.message,
        messages: state.messages.filter((m) => !(m.streaming && !m.text)),
      };
    case "send_end":
      return { ...state, streaming: false };
    case "reset":
      return { ...initialState(), facts: state.facts };
    default:
      return state;
  }
}

let nextMsgId = 1;
function allocId(prefix: string): string {
  nextMsgId += 1;
  return `${prefix}-${nextMsgId}`;
}

/**
 * Coraxis simple 1:1 interview (`start_interview_session`).
 * Channel streaming via useThrottledStream — same pattern as Multistage / ES.
 */
export function InterviewPocketPanel({
  sharedFacts,
  onSharedFactsPatch,
  hideEmbeddedFactsForm = false,
  esText = "",
  esBaseSlot = null,
}: {
  sharedFacts?: CompanyFacts;
  onSharedFactsPatch?: (patch: Partial<CompanyFacts>) => void;
  hideEmbeddedFactsForm?: boolean;
  /** Optional ES body injected into interviewer prompt. */
  esText?: string;
  /** Desktop: ES picker rendered above company facts. Mobile: null (parent owns). */
  esBaseSlot?: ReactNode;
} = {}) {
  const [state, dispatch] = useReducer(reducer, undefined, initialState);
  const assistantIdRef = useRef<string | null>(null);
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
    const id = assistantIdRef.current;
    if (!id) return;
    dispatch({ type: "token", assistantId: id, text: chunk });
  });

  function scrollToBottom() {
    requestAnimationFrame(() => {
      logRef.current?.scrollTo({ top: logRef.current.scrollHeight });
    });
  }

  async function onSend(e: React.FormEvent) {
    e.preventDefault();
    const prompt = state.input.trim();
    if (!prompt || state.streaming) return;
    if (!companyFactsReady(facts)) {
      dispatch({
        type: "send_failure",
        message: "企業名を入力してから送信してください。",
      });
      return;
    }

    dispatch({ type: "clear_error" });
    const userId = allocId("iv-u");
    const assistantId = allocId("iv-a");
    assistantIdRef.current = assistantId;
    dispatch({ type: "send_begin", userId, assistantId, prompt });
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
      console.error("[InterviewPocketPanel] enrich soft-fail:", err);
      // Ambient enrich soft-fail — proceed with typed facts.
    }

    const sterile = uiErrorMessage("INTERVIEW_RESPONSE");
    try {
      const result = await startInterviewSession(
        {
          message: prompt,
          companyFacts: factsPayload,
          edinetDate: todayIso(),
          esText: esText.trim() || undefined,
        },
        (event) => {
          if (event.error) {
            // eslint-disable-next-line no-console -- intentional diagnostic
            console.error("[InterviewPocketPanel] stream error event:", event.error);
            dispatch({ type: "token_error", message: sterile });
            return;
          }
          if (event.done) {
            throttle.drainAndStop();
            return;
          }
          if (event.text) {
            throttle.push(event.text);
          }
        },
      );

      throttle.drainAndStop();
      dispatch({
        type: "send_success",
        assistantId,
        companyName: result.company_name,
        factsSource: result.facts_source,
        contextCount: result.context_count,
      });
    } catch (err) {
      // eslint-disable-next-line no-console -- intentional diagnostic
      console.error("[InterviewPocketPanel] onSend failed:", err);
      throttle.flushAndStop();
      dispatch({ type: "send_failure", message: sterile });
    } finally {
      assistantIdRef.current = null;
      dispatch({ type: "send_end" });
      scrollToBottom();
    }
  }

  return (
    <div className="interview-pocket-panel interview-section-stack">
      <p className="hint guide mobile-only">
        企業情報を入れたあと、1対1の模擬面接をストリーミングで進めます。
      </p>
      <p className="hint guide dev-noise desktop-only">
        Coraxis 1:1 面接 (`start_interview_session`): 企業ファクト + 任意 ES ベースで面接官が応答します。
      </p>

      {esBaseSlot}

      {!hideEmbeddedFactsForm && (
        <CompanyFactsForm
          facts={facts}
          disabled={state.streaming}
          onPatch={patchFacts}
          researching={researching}
          provenanceLabel={provenanceLabel}
        />
      )}

      <div className="chat-log line-chat" ref={logRef}>
        {state.messages.length === 0 ? (
          <p className="hint guide chat-empty">
            企業ファクトを入力し、最初の発言を送信してください。
          </p>
        ) : (
          state.messages.map((m) => {
            const visible = redactHiddenReasoning(m.text, m.streaming);
            if (m.role === "user") {
              return (
                <div key={m.id} className="line-row user">
                  <div className="line-bubble user">
                    <pre className="chat-text">{visible}</pre>
                  </div>
                </div>
              );
            }
            return (
              <div key={m.id} className="line-row ai">
                <div className="line-bubble ai">
                  <span className="speaker-name">面接官</span>
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

      <form className="consult-form" onSubmit={(e) => void onSend(e)}>
        <textarea
          value={state.input}
          onChange={(e) => dispatch({ type: "set_input", value: e.target.value })}
          placeholder="回答・発言を入力…"
          rows={3}
          disabled={state.streaming}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
              void onSend(e);
            }
          }}
        />
        <div className="action-row">
          <button
            type="submit"
            className="primary"
            disabled={
              state.streaming ||
              !state.input.trim() ||
              !companyFactsReady(facts)
            }
          >
            {state.streaming ? "生成中…" : "送信 (Ctrl+Enter)"}
          </button>
          <button
            type="button"
            className="ghost"
            disabled={state.streaming || state.messages.length === 0}
            onClick={() => dispatch({ type: "reset" })}
          >
            履歴クリア
          </button>
        </div>
      </form>

      {state.meta && (
        <div className="term-panel">
          <p className="term-header">SESSION_META</p>
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
