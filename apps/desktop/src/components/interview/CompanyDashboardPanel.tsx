import { useReducer, useRef } from "react";

import { parseCompanyAnalysis } from "../../lib/companyAnalysisParse";
import { analyzeCompanyKnowledge } from "../../lib/pocketBrain";
import { redactHiddenReasoning } from "../../lib/redactHiddenReasoning";
import { createStreamTerminalGate } from "../../lib/streamTerminalGate";
import { uiErrorMessage } from "../../lib/uiErrorMessages";
import { useThrottledStream } from "../../lib/useThrottledStream";

const STREAM_TERMINAL_TIMEOUT_MS = 180_000;

interface DashState {
  streaming: boolean;
  raw: string;
  error: string | null;
  emptyVault: boolean;
  done: boolean;
}

type DashAction =
  | { type: "start" }
  | { type: "token"; text: string }
  | { type: "empty_vault" }
  | { type: "success" }
  | { type: "failure"; message: string }
  | { type: "end" };

function initialState(): DashState {
  return {
    streaming: false,
    raw: "",
    error: null,
    emptyVault: false,
    done: false,
  };
}

function reducer(state: DashState, action: DashAction): DashState {
  switch (action.type) {
    case "start":
      return {
        streaming: true,
        raw: "",
        error: null,
        emptyVault: false,
        done: false,
      };
    case "token":
      return { ...state, raw: state.raw + action.text };
    case "empty_vault":
      return { ...state, emptyVault: true, done: true };
    case "success":
      return { ...state, done: true };
    case "failure":
      return { ...state, error: action.message, done: true };
    case "end":
      return { ...state, streaming: false };
    default:
      return state;
  }
}

/**
 * Delayed company-knowledge dashboard (Company namespace only).
 * Never auto-runs on mount.
 */
export function CompanyDashboardPanel({ companyName }: { companyName: string }) {
  const [state, dispatch] = useReducer(reducer, undefined, initialState);
  const name = companyName.trim();
  const throttle = useThrottledStream((chunk) => {
    dispatch({ type: "token", text: chunk });
  });
  const runningRef = useRef(false);

  async function onGenerate() {
    if (!name || state.streaming || runningRef.current) return;
    runningRef.current = true;
    dispatch({ type: "start" });
    const terminal = createStreamTerminalGate(STREAM_TERMINAL_TIMEOUT_MS);
    let errored = false;
    try {
      const result = await analyzeCompanyKnowledge(name, (event) => {
        if (!terminal.isPending()) return;
        if (event.error) {
          // eslint-disable-next-line no-console -- intentional diagnostic
          console.error("[CompanyDashboardPanel] stream error:", event.error);
          errored = true;
          throttle.flushAndStop();
          dispatch({
            type: "failure",
            message: uiErrorMessage("INTERVIEW_RESPONSE"),
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
          throttle.push(event.text);
        }
      });

      if (result.context_count === 0) {
        terminal.settle();
        dispatch({ type: "empty_vault" });
      } else {
        await terminal.promise;
        throttle.drainAndStop();
        if (!errored) {
          dispatch({ type: "success" });
        }
      }
    } catch (err) {
      // eslint-disable-next-line no-console -- intentional diagnostic
      console.error("[CompanyDashboardPanel] analyze failed:", err);
      throttle.flushAndStop();
      dispatch({
        type: "failure",
        message: uiErrorMessage("INTERVIEW_RESPONSE"),
      });
    } finally {
      terminal.abort();
      dispatch({ type: "end" });
      runningRef.current = false;
    }
  }

  const sections =
    state.done && !state.emptyVault && !state.error
      ? parseCompanyAnalysis(redactHiddenReasoning(state.raw, false))
      : [];

  return (
    <div className="company-dashboard-panel interview-section-stack">
      <p className="hint guide">
        取得済みの企業データを面接対策ダッシュボードに構造化します（遅延評価）。
      </p>
      {!name ? (
        <p className="status-line" role="status">
          企業名を入力してください
        </p>
      ) : null}
      <div className="action-row">
        <button
          type="button"
          className="primary"
          disabled={!name || state.streaming}
          onClick={() => void onGenerate()}
        >
          {state.streaming ? "分析中…" : "最新の企業分析を生成"}
        </button>
      </div>

      {state.emptyVault ? (
        <p className="status-line" role="status">
          この企業の取得済みデータがありません
        </p>
      ) : null}

      {state.error ? (
        <p className="status-line error-text" role="alert">
          {state.error}
        </p>
      ) : null}

      {state.streaming && state.raw ? (
        <div className="term-panel">
          <p className="term-header">生成中</p>
          <pre className="chat-text">
            {redactHiddenReasoning(state.raw, true)}
            <span className="chat-cursor">▌</span>
          </pre>
        </div>
      ) : null}

      {sections.map((sec) => (
        <div key={sec.heading} className="term-panel">
          <p className="term-header">{sec.heading}</p>
          {sec.items.length === 0 ? (
            <p className="hint">（箇条書きなし）</p>
          ) : (
            sec.items.map((item) => (
              <div key={item} className="term-row">
                <span className="term-value">- {item}</span>
              </div>
            ))
          )}
        </div>
      ))}
    </div>
  );
}
