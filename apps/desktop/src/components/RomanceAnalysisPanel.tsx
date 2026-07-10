import type { RomanceAnalysisResult } from "../lib/engine";

interface Props {
  result: RomanceAnalysisResult | null;
}

function clampScore(value: number | null | undefined): number | null {
  if (value === null || value === undefined) return null;
  if (!Number.isFinite(value)) return null;
  return Math.max(0, Math.min(100, Math.round(value)));
}

export function RomanceAnalysisPanel({ result }: Props) {
  if (!result) return null;

  const score = clampScore(result.affinity_score);
  const meterPct = score === null ? 0 : score;

  return (
    <section className="romance-analysis-panel" aria-label="交流パルス解析結果">
      <h3 className="romance-analysis-heading">ROMANCE / INTERACTION_PULSE</h3>
      <div className="romance-score-block">
        <span className="romance-score-label">交流往復指数</span>
        <span className="romance-score-value">
          {score === null ? "N/A" : String(score)}
        </span>
        <div
          className="romance-pulse-meter"
          role="meter"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={score ?? undefined}
          aria-label="交流往復指数メーター"
        >
          <div
            className="romance-pulse-fill"
            style={{ width: `${meterPct}%` }}
          />
        </div>
      </div>
      <dl className="romance-detail-list">
        <div>
          <dt>傾向</dt>
          <dd>{result.interaction_tendency}</dd>
        </div>
        <div>
          <dt>次の一手</dt>
          <dd>{result.next_best_action}</dd>
        </div>
      </dl>
      <p className="romance-disclaimer">
        この指数は会話の往復量を示すもので、相手の好意や感情を判定するものではありません。
      </p>
    </section>
  );
}
