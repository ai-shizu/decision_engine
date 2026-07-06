interface RollColumnProps {
  label: string;
  values: readonly string[];
  index: number;
  onIndexChange: (index: number) => void;
  wide?: boolean;
}

export function RollColumn({ label, values, index, onIndexChange, wide }: RollColumnProps) {
  const safeIndex = ((index % values.length) + values.length) % values.length;

  function roll(delta: number) {
    onIndexChange(safeIndex + delta);
  }

  return (
    <div className={wide ? "roll-column roll-column-wide" : "roll-column"}>
      <span className="roll-label">{label}</span>
      <button type="button" className="roll-btn" aria-label={`${label} 前へ`} onClick={() => roll(-1)}>
        ▲
      </button>
      <span className="roll-display">{values[safeIndex]}</span>
      <button type="button" className="roll-btn" aria-label={`${label} 次へ`} onClick={() => roll(1)}>
        ▼
      </button>
    </div>
  );
}
