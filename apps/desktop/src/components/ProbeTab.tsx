import { useCallback, useEffect, useRef, useState } from "react";
import { probeAnswer, probeNext, probeStatus } from "../lib/engine";
import { todayIso } from "../lib/dateUtils";
import { uiErrorMessage } from "../lib/uiErrorMessages";
import type {
  ProbeAxis,
  ProbeQuestionView,
  ProbeStage,
  ProbeStatus,
} from "../lib/types";
import { PocketProbePanel } from "./PocketProbePanel";
import { PulseRaschDashboard } from "./PulseRaschDashboard";

const AXIS_LABELS: Record<ProbeAxis, string> = {
  decision_threshold: "意思決定閾値",
  reward_bias: "報酬系の偏り",
  locus_of_control: "統制の所在",
  unlearning_rate: "アンラーニング速度",
  friction_energy_ledger: "摩擦収支",
};

const STAGE_LABELS: Record<ProbeStage, string> = {
  FACT: "FACT",
  CONTEXT: "CONTEXT",
  EMOTION: "EMOTION",
  MEANING: "MEANING",
};

const STAGE_ORDER: ProbeStage[] = ["FACT", "CONTEXT", "EMOTION", "MEANING"];

const INSIGHT_LABELS: Record<string, string> = {
  "probe.low_confidence": "観測不足",
  "probe.under_probed": "未探索",
  "probe.stage_complete": "完了",
};

type ProbeSurface = "pb_probe" | "pulse_rasch" | "legacy";

/**
 * PROBE tab: Pocket Brain M15 wiring (primary) + legacy Python engine path.
 */
export function ProbeTab() {
  const [surface, setSurface] = useState<ProbeSurface>("pb_probe");

  return (
    <section className="panel probe-panel">
      <div className="probe-topline">
        <div>
          <h2>PROBE (自己探索)</h2>
          <p className="hint">
            M18-D: Pocket Brain の PROBE ファネルと Romance/Rasch パルスを配線。
            legacy は Python sidecar 経路（非破壊で残置）。
          </p>
        </div>
      </div>

      <div className="sub-tabs">
        <button
          type="button"
          className={surface === "pb_probe" ? "active" : ""}
          onClick={() => setSurface("pb_probe")}
        >
          PROBE (PB)
        </button>
        <button
          type="button"
          className={surface === "pulse_rasch" ? "active" : ""}
          onClick={() => setSurface("pulse_rasch")}
        >
          PULSE / RASCH
        </button>
        <button
          type="button"
          className={surface === "legacy" ? "active" : ""}
          onClick={() => setSurface("legacy")}
        >
          PROBE (legacy)
        </button>
      </div>

      {surface === "pb_probe" && <PocketProbePanel />}
      {surface === "pulse_rasch" && <PulseRaschDashboard />}
      {surface === "legacy" && <LegacyProbePanel />}
    </section>
  );
}

function LegacyProbePanel() {
  const [status, setStatus] = useState<ProbeStatus | null>(null);
  const [question, setQuestion] = useState<ProbeQuestionView | null>(null);
  const [answer, setAnswer] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const answerRef = useRef<HTMLTextAreaElement>(null);
  const nextBtnRef = useRef<HTMLButtonElement>(null);

  const refreshStatus = useCallback(async () => {
    const s = await probeStatus(todayIso());
    setStatus(s);
    return s;
  }, []);

  useEffect(() => {
    let disposed = false;
    async function load() {
      setBusy(true);
      try {
        const s = await probeStatus(todayIso());
        if (!disposed) setStatus(s);
      } catch {
        if (!disposed) setError(uiErrorMessage("PROBE_STATUS_LOAD"));
      } finally {
        if (!disposed) setBusy(false);
      }
    }
    void load();
    return () => {
      disposed = true;
    };
  }, []);

  async function handleRefresh() {
    setBusy(true);
    setError("");
    try {
      await refreshStatus();
    } catch {
      setError(uiErrorMessage("PROBE_STATUS_LOAD"));
    } finally {
      setBusy(false);
    }
  }

  async function handleNextQuestion() {
    setBusy(true);
    setError("");
    try {
      const q = await probeNext(todayIso());
      setQuestion(q);
      setAnswer("");
      await refreshStatus();
      requestAnimationFrame(() => answerRef.current?.focus());
    } catch {
      setError(uiErrorMessage("PROBE_NEXT"));
    } finally {
      setBusy(false);
    }
  }

  async function handleSubmit() {
    if (!question || answer.trim().length === 0 || busy) return;
    setBusy(true);
    setError("");
    try {
      const res = await probeAnswer(
        question.session_id,
        question.question_id,
        answer.trim(),
        todayIso(),
      );
      setStatus(res.status);
      setQuestion(res.next_question);
      setAnswer("");
      if (res.next_question) {
        requestAnimationFrame(() => answerRef.current?.focus());
      } else {
        requestAnimationFrame(() => nextBtnRef.current?.focus());
      }
    } catch {
      setError(uiErrorMessage("PROBE_ANSWER"));
    } finally {
      setBusy(false);
    }
  }

  function onAnswerKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
      e.preventDefault();
      void handleSubmit();
    }
  }

  const activeAxis = question?.axis ?? status?.active_session?.axis ?? null;
  const activeStage = question?.stage ?? status?.active_session?.stage ?? null;
  const progress = status?.progress;
  const charCount = answer.length;

  return (
    <div className="legacy-probe-panel">
      <div className="probe-topline">
        <div>
          <p className="hint">
            進捗{" "}
            <span className="term-metric">
              {progress ? `${progress.percent}%` : "—"}
            </span>
            {activeAxis && activeStage && (
              <>
                {" "}
                / 現在{" "}
                <span className="term-metric">
                  {AXIS_LABELS[activeAxis]} · {STAGE_LABELS[activeStage]}
                </span>
              </>
            )}
          </p>
        </div>
        <button
          type="button"
          className="secondary"
          disabled={busy}
          onClick={() => void handleRefresh()}
        >
          更新
        </button>
      </div>

      {error && (
        <p className="error-text probe-error" role="alert">
          {error}
        </p>
      )}

      <div className="probe-grid">
        <div className="probe-main">
          <div className="probe-stepper" aria-label="ファネル段階">
            {STAGE_ORDER.map((stage) => {
              const done =
                activeStage !== null &&
                STAGE_ORDER.indexOf(stage) < STAGE_ORDER.indexOf(activeStage);
              const active = stage === activeStage;
              return (
                <span
                  key={stage}
                  className={`probe-step${active ? " active" : ""}${done ? " done" : ""}`}
                >
                  {STAGE_LABELS[stage]}
                </span>
              );
            })}
          </div>

          <div className="term-panel probe-question" aria-live="polite">
            {question ? (
              <>
                <p className="term-label">
                  {AXIS_LABELS[question.axis]} / {STAGE_LABELS[question.stage]}
                </p>
                <p>{question.question}</p>
              </>
            ) : (
              <p className="hint">
                「次の質問」でバックエンドが選んだ静的質問を表示します。
              </p>
            )}
          </div>

          {question && (
            <>
              <textarea
                ref={answerRef}
                className="probe-answer"
                rows={4}
                maxLength={120}
                value={answer}
                disabled={busy}
                placeholder="120字以内で回答"
                onChange={(e) => {
                  setAnswer(e.target.value);
                  if (error) setError("");
                }}
                onKeyDown={onAnswerKeyDown}
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
                  disabled={busy || answer.trim().length === 0}
                  onClick={() => void handleSubmit()}
                >
                  {busy ? "保存中…" : "回答を送信 (Ctrl+Enter)"}
                </button>
              </div>
            </>
          )}

          {!question && (
            <div className="action-row">
              <button
                ref={nextBtnRef}
                type="button"
                className="primary"
                disabled={busy}
                onClick={() => void handleNextQuestion()}
              >
                {busy ? "保存中…" : "次の質問"}
              </button>
              {status?.active_session?.status === "closed" && (
                <p className="hint">
                  直近セッションは完了しました。新しい質問を開始できます。
                </p>
              )}
            </div>
          )}
        </div>

        <div className="probe-side">
          <div className="probe-axis-list">
            {(status?.axes ?? []).map((row) => (
              <div
                key={row.axis}
                className={`probe-axis-row${row.axis === activeAxis ? " active" : ""}`}
              >
                <span className="probe-axis-label">{AXIS_LABELS[row.axis]}</span>
                <span className="probe-axis-metric">
                  conf {row.confidence.toFixed(2)} / pri {row.priority.toFixed(2)}
                </span>
                <span className="probe-axis-metric">
                  {STAGE_LABELS[row.stage]} · nodes {row.node_count}
                </span>
              </div>
            ))}
            {!status && <p className="hint">軸状態を読み込み中…</p>}
          </div>

          <ul className="probe-insight-list">
            {(status?.insights ?? []).map((ins) => (
              <li key={`${ins.kind}-${ins.axis}-${ins.stage}`}>
                <span className="term-metric">
                  {INSIGHT_LABELS[ins.message_code] ?? ins.message_code}
                </span>
                {" — "}
                {AXIS_LABELS[ins.axis]} / {STAGE_LABELS[ins.stage]}{" "}
                <span className="probe-axis-metric">
                  ({ins.priority.toFixed(2)})
                </span>
              </li>
            ))}
            {status && status.insights.length === 0 && (
              <li className="hint">インサイトなし</li>
            )}
          </ul>
        </div>
      </div>
    </div>
  );
}
