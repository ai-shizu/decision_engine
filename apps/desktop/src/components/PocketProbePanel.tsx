import { useEffect, useReducer, useRef, useState } from "react";

import { todayIso } from "../lib/dateUtils";
import {
  getProbeQuestions,
  getProbeStatus,
  probeNextQuestion,
  probeSubmitAnswer,
} from "../lib/pocketBrain";
import { PB_UI_BUSY, PB_UI_FAIL } from "../lib/pocketBrain/uiFailure";
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
const STAGE_LABELS_JA: Record<(typeof STAGE_ORDER)[number], string> = {
  FACT: "事実",
  CONTEXT: "文脈",
  EMOTION: "感情",
  MEANING: "意味",
};

/**
 * M15 PROBE funnel — dense rack, Japanese-first, classified vault strip.
 */
export function PocketProbePanel() {
  const [state, dispatch] = useReducer(
    probePanelReducer,
    undefined,
    initialProbePanelState,
  );
  const [vaultRevealed, setVaultRevealed] = useState(false);
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
    } catch {
      dispatch({ type: "load_failure", message: PB_UI_FAIL.probeStatus });
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
    } catch {
      dispatch({ type: "next_failure", message: PB_UI_FAIL.probeNext });
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
    } catch {
      dispatch({ type: "submit_failure", message: PB_UI_FAIL.probeSubmit });
    }
  }

  const progress = state.status?.progress_percent ?? null;
  const activeStage = state.question?.stage ?? null;
  const charCount = state.answer.length;
  const vaultDump = state.status
    ? `完了 ${state.status.completed_stages}/${state.status.total_stages} · 進捗 ${state.status.progress_percent}% · 設問庫 ${state.bank.length}`
    : `設問庫 ${state.bank.length} · 状態取得待ち`;

  return (
    <div className="pocket-probe-panel magi-rack">
      <div className="magi-mod">
        <div className="magi-mod-head">
          <span>進捗</span>
          <span className="term-tag term-tag--info">
            [ {progress !== null ? `${progress}%` : "—"} ]
          </span>
        </div>
        <div className="magi-mod-body">
          <div className="probe-topline" style={{ border: "none", padding: 0 }}>
            <button
              type="button"
              className="ghost"
              disabled={busy}
              onClick={() => void refreshAll()}
            >
              {state.phase === "loading" ? "更新中…" : "状態を更新"}
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
        </div>
      </div>

      {state.phase === "loading" && (
        <div className="magi-mod">
          <div className="magi-mod-body">
            <p className="sys-log" role="status">
              {`> ${PB_UI_BUSY.probeStatus}`}
            </p>
          </div>
        </div>
      )}

      <div className="magi-mod">
        <div className="magi-mod-head">
          <span>段階</span>
        </div>
        <div className="tactical-array" role="group" aria-label="ファネル段階">
          {STAGE_ORDER.map((stage) => {
            const idx = STAGE_ORDER.indexOf(stage);
            const activeIdx = activeStage
              ? STAGE_ORDER.indexOf(activeStage as (typeof STAGE_ORDER)[number])
              : -1;
            const done = activeIdx > idx;
            const active = stage === activeStage;
            return (
              <button
                key={stage}
                type="button"
                className={`${active ? "active" : ""}${done ? " done" : ""}`}
                disabled
                aria-pressed={active}
              >
                {STAGE_LABELS_JA[stage]}
              </button>
            );
          })}
        </div>
      </div>

      <div className="magi-mod hatch-danger">
        <div className="magi-mod-head">
          <span className="term-tag term-tag--danger">[ 封印データ ]</span>
          <button
            type="button"
            className="ghost"
            onClick={() => setVaultRevealed((v) => !v)}
          >
            {vaultRevealed ? "再封印" : "復号"}
          </button>
        </div>
        <div
          className={`magi-mod-body data-sealed-host${vaultRevealed ? " is-revealed" : ""}`}
        >
          <span className="term-tag term-tag--danger">[ LLM非公開 ]</span>
          <p className="text-redacted data-sealed hint guide">{vaultDump}</p>
        </div>
      </div>

      {state.phase === "complete" && (
        <div className="magi-mod hatch-ok">
          <div className="magi-mod-head">
            <span className="term-tag term-tag--ok">[ 完了 ]</span>
          </div>
          <div className="magi-mod-body">
            <p className="sys-log sys-log--ok">
              {"> この軸の次の質問はありません。"}
            </p>
            <button
              type="button"
              className="primary"
              onClick={() => dispatch({ type: "reset_to_lobby" })}
            >
              ロビーへ戻る
            </button>
          </div>
        </div>
      )}

      {state.phase !== "complete" && (
        <div className="magi-mod">
          <div className="magi-mod-head">
            <span>現在の問い</span>
            {state.question ? (
              <span className="term-tag term-tag--ok">[ 出題中 ]</span>
            ) : (
              <span className="term-tag term-tag--warn">[ 待機 ]</span>
            )}
          </div>
          <div className="magi-mod-body probe-question" aria-live="polite">
            {state.question ? (
              <>
                <p className="term-label">
                  {AXIS_LABELS[state.question.axis] ?? state.question.axis}
                </p>
                <p>{state.question.question}</p>
              </>
            ) : (
              <p className="hint guide">「次の質問」で自己探索を開始します。</p>
            )}

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
                  className={`probe-char-counter${charCount >= 120 ? " sys-log--err" : ""}`}
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
                  {state.phase === "fetching_question" ? "取得中…" : "次の質問"}
                </button>
              </div>
            )}
          </div>
        </div>
      )}

      {state.error && (
        <div className="magi-mod">
          <div className="magi-mod-body">
            <p className="sys-log sys-log--err" role="alert">
              {`> ${state.error}`}
            </p>
          </div>
        </div>
      )}
    </div>
  );
}
