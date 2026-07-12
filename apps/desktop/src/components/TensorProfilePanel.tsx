import { TensorRadarChart } from "./TensorRadarChart";
import {
  tensorProfileDimensionSummaries,
  tensorProfileToRadarData,
} from "../lib/tensorProfileView";
import type { TensorProfileReportV1 } from "../lib/types";

export interface TensorProfilePanelProps {
  tensorProfile: TensorProfileReportV1;
}

/**
 * Audit finding remediation (interview_report.v1 tensor_profile): renders
 * the real measured six-dimensional profile that was reaching the frontend
 * and being silently discarded. Displayed only inside InterviewTab's
 * MISSION_RESULT — no evidence text, speaker alias, turn ID, or indicator ID
 * is rendered (SPEC_ENGINE_TENSOR_PROFILING.md: "Do not display evidence
 * text, quotes, ... or names"). score=null is shown as "N/A / 測定不足",
 * never coerced to 0. confidence is shown as a plain number — no invented
 * high/mid/low classification.
 */
export function TensorProfilePanel({ tensorProfile }: TensorProfilePanelProps) {
  const radarData = tensorProfileToRadarData(tensorProfile);
  const summaries = tensorProfileDimensionSummaries(tensorProfile);
  const allUnmeasured = summaries.every((s) => s.score === null);

  return (
    <div className="term-panel tensor-profile-panel">
      <p className="term-header">TENSOR_PROFILE_6D</p>
      <p className="tensor-profile-session-label">SESSION MEASUREMENT</p>
      <TensorRadarChart data={radarData} title="Six-dimensional tensor profile (measured)" />
      <ul className="tensor-profile-summary-list">
        {summaries.map((s) => (
          <li key={s.id} className="tensor-profile-summary-row">
            <span className="term-source-name">{s.label}</span>
            <span className="term-value">
              {s.score === null ? "N/A / 測定不足" : s.score.toFixed(2)}
            </span>
            <span className="term-value tensor-profile-confidence">
              confidence {s.confidence.toFixed(2)}
            </span>
          </li>
        ))}
      </ul>
      {allUnmeasured && (
        <p className="hint">今回の発言量では測定できませんでした。</p>
      )}
    </div>
  );
}
