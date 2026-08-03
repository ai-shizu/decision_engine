import {
  INTERVIEW_STAGE_HINTS,
  INTERVIEW_STAGE_LABELS,
  INTERVIEW_STAGE_ORDER,
  stageIndex,
} from "../../lib/interviewStage";
import type { InterviewStage } from "../../lib/pocketBrain/types";

interface InterviewStageRailProps {
  stage: InterviewStage | null;
  turnInStage: number;
  totalTurns: number;
  status: string | null;
  outcome: string | null;
}

/**
 * Visual FSM rail: Foundation → Pressure → Debrief → Closed.
 */
export function InterviewStageRail({
  stage,
  turnInStage,
  totalTurns,
  status,
  outcome,
}: InterviewStageRailProps) {
  const current = stage ? stageIndex(stage) : -1;

  return (
    <div className="interview-stage-rail" aria-label="面接ステージ">
      <ol className="stage-rail-list">
        {INTERVIEW_STAGE_ORDER.map((s, i) => {
          const done = current > i;
          const active = current === i;
          return (
            <li
              key={s}
              className={
                "stage-rail-item" +
                (done ? " is-done" : "") +
                (active ? " is-active" : "")
              }
              title={INTERVIEW_STAGE_HINTS[s]}
            >
              <span className="stage-rail-dot" aria-hidden />
              <span className="stage-rail-label">{INTERVIEW_STAGE_LABELS[s]}</span>
            </li>
          );
        })}
      </ol>
      <div className="stage-rail-meta">
        {stage ? (
          <>
            <span>{INTERVIEW_STAGE_HINTS[stage]}</span>
            <span>
              turn_in_stage={turnInStage} / total={totalTurns}
              {status ? ` / status=${status}` : ""}
              {outcome ? ` / outcome=${outcome}` : ""}
            </span>
          </>
        ) : (
          <span>未開始 — 企業ファクトを注入してセッションを開始</span>
        )}
      </div>
    </div>
  );
}
