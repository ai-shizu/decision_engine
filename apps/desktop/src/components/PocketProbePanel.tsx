import { useEffect, useReducer, useRef } from "react";

import { todayIso } from "../lib/dateUtils";
import {
  getProbeQuestions,
  getProbeStatus,
  isPocketBrainInvokeError,
  probeNextQuestion,
  probeSubmitAnswer,
} from "../lib/pocketBrain";
import {
  initialProbePanelState,
  probeBusy,
  probePanelReducer,
} from "../lib/probePanelReducer";

const AXIS_LABELS: Record<string, string> = {
  decision_threshold: "意思決定閾値",
  reward_bias: "報酬系の偏り",
  locus_of_control: "統制の所在",
  unlearning_rate: "アンラーニング速度",
  friction_energy_ledger: "摩擦収支",
};

const STAGE_ORDER = ["FACT", "CONTEXT", "EMOTION", "MEANING"] as const;

/**
 * M15 PROBE funnel via Pocket Brain Tauri commands (useReducer state machine).
 */
export function PocketProbePanel() {
  const [state, dispatch] = useReducer(
    probePanelReducer,
    undefined,
    initialProbePanelState,
  );
  const answerRef = useRef<HTMLTextAreaElement>(null);
  const busy = probeBusy(state.phase);

  async function refreshAll() {
    dispatch({ type: "load_begin" });
    try {
      const today = todayIso();
      const [status, bank] = await Promise.all([
        getProbeStatus(today),
        getProbeQuestions(),
      ]);
      dispatch({ type: "load_success", status, bank });
    } catch (e) {
      const message = isPocketBrainInvokeError(e)
        ? e.message
        : `probe load: ${String(e)}`;
      dispatch({ type: "load_failure", message });
    }
  }

  useEffect(() => {
    void refreshAll();
  }, []);

  async function onNext() {
    if (busy) return;
    dispatch({ type: "next_begin" });
    try {
      const today = todayIso();
      const question = await probeNextQuestion({ today });
      const status = await getProbeStatus(today);
      dispatch({ type: "next_success", question, status });
      requestAnimationFrame(() => answerRef.current?.focus());
    } catch (e) {
      const message = isPocketBrainInvokeError(e)
        ? e.message
        : `probe_next_question: ${String(e)}`;
      dispatch({ type: "next_failure", message });
    }
  }

  async function onSubmit() {
    if (!state.question || busy || !state.answer.trim()) return;
    dispatch({ type: "submit_begin" });
    try {
      const today = todayIso();
      const result = await probeSubmitAnswer({
        sessionId: state.question.session_id,
        questionId: state.question.question_id,
        answer: state.answer.trim(),
        today,
      });
      const status = await getProbeStatus(today);
      dispatch({ type: "submit_success", result, status });
      if (result.next_question) {
        requestAnimationFrame(() => answerRef.current?.focus());
      }
    } catch (e) {
      const message = isPocketBrainInvokeError(e)
        ? e.message
        : `probe_submit_answer: ${String(e)}`;
      dispatch({ type: "submit_failure", message });
    }
  }

  const progress = state.status?.progress_percent ?? null;
  const activeStage = state.question?.stage ?? null;
  const charCount = state.answer.length;

  return (
    <div className="pocket-probe-panel">
      <div className="probe-topline">
        <div>
          <p className="term-header">PROBE_FUNNEL (Pocket Brain / M15)</p>
          <p className="hint">
            進捗{" "}
            <span className="term-metric">
              {progress !== null ? `${progress}%` : "—"}
            </span>
            {state.status && (
              <>
                {" "}
                ({state.status.completed_stages}/{state.status.total_stages}{" "}
                stages)
              </>
            )}
          </p>
        </div>
        <button
          type="button"
          className="ghost"
          disabled={busy}
          onClick={() => void refreshAll()}
        >
          {state.phase === "loading" ? "読込中…" : "状態を更新"}
        </button>
      </div>

      <div
        className="probe-progress-track"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={progress ?? 0}
        aria-label="PROBE 進捗"
      >
        <div
          className="probe-progress-fill"
          style={{ width: `${Math.max(0, Math.min(100, progress ?? 0))}%` }}
        />
      </div>

      <div className="probe-stepper" aria-label="ファネル段階">
        {STAGE_ORDER.map((stage) => {
          const idx = STAGE_ORDER.indexOf(stage);
          const activeIdx = activeStage
            ? STAGE_ORDER.indexOf(activeStage as (typeof STAGE_ORDER)[number])
            : -1;
          const done = activeIdx > idx;
          const active = stage === activeStage;
          return (
            <span
              key={stage}
              className={`probe-step${active ? " active" : ""}${done ? " done" : ""}`}
            >
              {stage}
            </span>
          );
        })}
      </div>

      {state.phase === "complete" && (
        <div className="term-panel probe-complete" role="status">
          <p className="term-header">SESSION_COMPLETE</p>
          <p className="hint">
            この軸の次質問はありません。ロビーから新しい質問を開始できます。
          </p>
          <button
            type="button"
            className="primary"
            onClick={() => dispatch({ type: "reset_to_lobby" })}
          >
            ロビーへ戻る
          </button>
        </div>
      )}

      {state.phase !== "complete" && (
        <>
          <div className="term-panel probe-question" aria-live="polite">
            {state.question ? (
              <>
                <p className="term-label">
                  {AXIS_LABELS[state.question.axis] ?? state.question.axis} /{" "}
                  {state.question.stage}
                  <span className="probe-axis-metric">
                    {" "}
                    pri {state.question.priority.toFixed(3)}
                  </span>
                </p>
                <p>{state.question.question}</p>
              </>
            ) : (
              <p className="hint">
                「次の質問」で `probe_next_question` が選んだ静的バンク質問を表示します。
              </p>
            )}
          </div>

          {state.question && (
            <>
              <textarea
                ref={answerRef}
                className="probe-answer"
                rows={4}
                maxLength={120}
                value={state.answer}
                disabled={busy}
                placeholder="120字以内で回答"
                onChange={(e) =>
                  dispatch({ type: "set_answer", value: e.target.value })
                }
                onKeyDown={(e) => {
                  if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
                    e.preventDefault();
                    void onSubmit();
                  }
                }}
              />
              <div
                className={`probe-char-counter${charCount >= 120 ? " error-text" : ""}`}
              >
                {charCount}/120
              </div>
              <div className="action-row">
                <button
                  type="button"
                  className="primary"
                  disabled={busy || !state.answer.trim()}
                  onClick={() => void onSubmit()}
                >
                  {state.phase === "submitting"
                    ? "送信中…"
                    : "回答を送信 (Ctrl+Enter)"}
                </button>
              </div>
            </>
          )}

          {!state.question && (
            <div className="action-row">
              <button
                type="button"
                className="primary"
                disabled={busy}
                onClick={() => void onNext()}
              >
                {state.phase === "fetching_question"
                  ? "取得中…"
                  : "次の質問"}
              </button>
            </div>
          )}
        </>
      )}

      {state.bank.length > 0 && (
        <p className="hint">
          質問バンク {state.bank.length} 件（`get_probe_questions`）
        </p>
      )}

      {state.error && (
        <p className="status-line error-text" role="alert">
          {state.error}
        </p>
      )}
    </div>
  );
}
