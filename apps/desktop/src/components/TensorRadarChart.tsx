export type TensorRadarDatum = {
  id: string;
  label: string;
  value: number | null;
  axisName?: string;
  description?: string;
};

export type TensorRadarChartProps = {
  data: readonly TensorRadarDatum[];
  title?: string;
};

const N = 6;
const CX = 160;
const CY = 160;
const RADIUS = 104;
const GRID_SCALES = [0.25, 0.5, 0.75, 1] as const; // 0.25, 0.50, 0.75, 1.00

function axisPoint(index: number, scale: number): { x: number; y: number } {
  const angle = -Math.PI / 2 + index * ((2 * Math.PI) / N);
  return {
    x: CX + RADIUS * scale * Math.cos(angle),
    y: CY + RADIUS * scale * Math.sin(angle),
  };
}

function fmtCoord(value: number): string {
  return value.toFixed(2);
}

function clampValue(value: number | null): number | null {
  if (value === null || !Number.isFinite(value)) return null;
  return Math.max(0, Math.min(1, value));
}

function formatDisplayValue(value: number | null): string {
  if (value === null) return "N/A";
  return value.toFixed(2);
}

function labelAnchor(index: number): "start" | "middle" | "end" {
  const angle = -Math.PI / 2 + index * ((2 * Math.PI) / N);
  const x = Math.cos(angle);
  if (x > 0.25) return "start";
  if (x < -0.25) return "end";
  return "middle";
}

function AxisHelp({ id, description }: { id: string; description: string }) {
  const tooltipId = `tensor-radar-tooltip-${id}`;
  return (
    <span className="tensor-radar-help-wrap">
      <span
        className="tensor-radar-help-trigger"
        tabIndex={0}
        aria-describedby={tooltipId}
      >
        [?]
      </span>
      <span id={tooltipId} role="tooltip" className="tensor-radar-tooltip">
        {description}
      </span>
    </span>
  );
}

export function TensorRadarChart({ data, title }: TensorRadarChartProps) {
  if (data.length !== N) {
    return <p className="hint">Tensor radar requires exactly six dimensions.</p>;
  }

  const titleId = "tensor-radar-title";
  const descId = "tensor-radar-desc";
  const normalized = data.map((d) => ({ ...d, plot: clampValue(d.value) }));
  const dataPoints = normalized
    .map((d, i) => {
      const scale = d.plot ?? 0;
      const p = axisPoint(i, scale);
      return `${fmtCoord(p.x)},${fmtCoord(p.y)}`;
    })
    .join(" ");

  const descLines = normalized.map((d) => {
    const name = d.axisName ? `${d.label} (${d.axisName})` : d.label;
    return `${name}: ${formatDisplayValue(d.plot)}`;
  });
  const accessibleDesc = [title ?? "Six-dimensional tensor profile", ...descLines].join("; ");

  return (
    <div className="tensor-radar-panel">
      <svg
        className="tensor-radar-svg"
        viewBox="0 0 320 320"
        role="img"
        aria-labelledby={`${titleId} ${descId}`}
      >
        <title id={titleId}>{title ?? "Six-dimensional tensor profile"}</title>
        <desc id={descId}>{accessibleDesc}</desc>
        {GRID_SCALES.map((scale) => {
          const ring = Array.from({ length: N }, (_, i) => {
            const p = axisPoint(i, scale);
            return `${fmtCoord(p.x)},${fmtCoord(p.y)}`;
          }).join(" ");
          return (
            <polygon
              key={scale}
              className="tensor-radar-grid"
              points={ring}
              fill="none"
            />
          );
        })}
        {Array.from({ length: N }, (_, i) => {
          const p = axisPoint(i, 1);
          return (
            <line
              key={`axis-${i}`}
              className="tensor-radar-axis"
              x1={fmtCoord(CX)}
              y1={fmtCoord(CY)}
              x2={fmtCoord(p.x)}
              y2={fmtCoord(p.y)}
            />
          );
        })}
        <polygon className="tensor-radar-data" points={dataPoints} />
        {normalized.map((d, i) => {
          const lp = axisPoint(i, 1.12);
          return (
            <text
              key={d.id}
              className="tensor-radar-label"
              x={fmtCoord(lp.x)}
              y={fmtCoord(lp.y)}
              textAnchor={labelAnchor(i)}
              dominantBaseline="middle"
            >
              {d.label}
            </text>
          );
        })}
      </svg>
      <ul className="tensor-radar-legend" aria-hidden="false">
        {normalized.map((d) => (
          <li key={d.id} className="tensor-radar-legend-row">
            <span className="tensor-radar-legend-label">
              <span>{d.label}</span>
              {d.axisName ? (
                <span className="tensor-radar-axis-name">{d.axisName}</span>
              ) : null}
              {d.axisName && d.description ? (
                <AxisHelp id={d.id} description={d.description} />
              ) : null}
            </span>
            <span className="tensor-radar-legend-value">{formatDisplayValue(d.plot)}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
