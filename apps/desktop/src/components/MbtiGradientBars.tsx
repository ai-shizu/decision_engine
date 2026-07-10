export type MbtiPair = {
  left: string;
  leftPct: number;
  right: string;
  rightPct: number;
};

const MBTI_PAIRS: MbtiPair[] = [
  { left: "E", leftPct: 42, right: "I", rightPct: 58 },
  { left: "S", leftPct: 55, right: "N", rightPct: 45 },
  { left: "T", leftPct: 61, right: "F", rightPct: 39 },
  { left: "J", leftPct: 47, right: "P", rightPct: 53 },
];

const PREVIEW_LABEL = "PREVIEW / NOT MEASURED";

function clampPct(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(100, Math.round(value)));
}

function pairGradient(leftPct: number): string {
  const left = clampPct(leftPct);
  return `linear-gradient(to right, var(--accent) 0%, var(--accent) ${left}%, var(--ok) ${left}%, var(--ok) 100%)`;
}

export function MbtiGradientBars() {
  const descId = "mbti-preference-preview-desc";

  return (
    <div className="mbti-preview-panel" aria-describedby={descId}>
      <p className="term-header">MBTI_PREFERENCE_PREVIEW</p>
      <p className="mbti-preview-label">{PREVIEW_LABEL}</p>
      <p id={descId} className="hint">
        固定モック表示です。測定値ではなく、型推定・保存・送信は行いません。
      </p>
      <ul className="mbti-preview-list">
        {MBTI_PAIRS.map((pair) => {
          const left = clampPct(pair.leftPct);
          const right = clampPct(pair.rightPct);
          const total = left + right;
          const normLeft = total === 100 ? left : clampPct(left);
          const normRight = total === 100 ? right : 100 - normLeft;
          return (
            <li key={`${pair.left}-${pair.right}`} className="mbti-preview-row">
              <div className="mbti-preview-labels">
                <span>
                  {pair.left} {normLeft}%
                </span>
                <span>
                  {pair.right} {normRight}%
                </span>
              </div>
              <div
                className="mbti-preview-bar"
                style={{ background: pairGradient(normLeft) }}
                role="img"
                aria-label={`${pair.left} ${normLeft} percent, ${pair.right} ${normRight} percent`}
              />
            </li>
          );
        })}
      </ul>
    </div>
  );
}
