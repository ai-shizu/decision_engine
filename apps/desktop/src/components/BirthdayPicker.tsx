import { useMemo } from "react";
import {
  BIRTH_YEARS,
  MONTHS,
  formatBirthday,
  parseBirthday,
} from "../lib/birthdayUtils";
import { RollColumn } from "./RollColumn";

interface BirthdayPickerProps {
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
}

export function BirthdayPicker({ value, onChange, disabled }: BirthdayPickerProps) {
  const parsed = useMemo(() => parseBirthday(value), [value]);

  function setYearIdx(next: number) {
    onChange(formatBirthday(next, parsed.monthIdx, parsed.dayIdx));
  }

  function setMonthIdx(next: number) {
    onChange(formatBirthday(parsed.yearIdx, next, parsed.dayIdx));
  }

  function setDayIdx(next: number) {
    onChange(formatBirthday(parsed.yearIdx, parsed.monthIdx, next));
  }

  return (
    <div className={`birthday-picker${disabled ? " disabled" : ""}`}>
      <RollColumn label="年" values={BIRTH_YEARS} index={parsed.yearIdx} onIndexChange={setYearIdx} wide />
      <RollColumn label="月" values={MONTHS} index={parsed.monthIdx} onIndexChange={setMonthIdx} />
      <RollColumn label="日" values={parsed.days} index={parsed.dayIdx} onIndexChange={setDayIdx} />
    </div>
  );
}
