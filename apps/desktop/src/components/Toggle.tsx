/** CSS-only iOS スイッチ (SPEC_FOXTROT_UI.md §2.6 / §4.2 — native checkbox + label) */
export interface ToggleProps {
  id: string;
  checked: boolean;
  onChange?: (value: boolean) => void;
  disabled?: boolean;
  /** 行内ラベル (SettingRow 左側で別表示する場合は省略可) */
  label?: string;
}

export function Toggle({ id, checked, onChange, disabled, label }: ToggleProps) {
  const readOnly = disabled || onChange === undefined;

  return (
    <span className={`toggle${readOnly ? " toggle-readonly" : ""}`}>
      {label !== undefined && (
        <span className="toggle-side-label">{label}</span>
      )}
      <span className="toggle-switch">
        <input
          type="checkbox"
          id={id}
          className="toggle-input"
          checked={checked}
          disabled={readOnly}
          onChange={
            onChange
              ? (e) => {
                  onChange(e.target.checked);
                }
              : undefined
          }
        />
        <label htmlFor={id} className="toggle-track">
          <span className="toggle-knob" aria-hidden="true" />
        </label>
      </span>
    </span>
  );
}
