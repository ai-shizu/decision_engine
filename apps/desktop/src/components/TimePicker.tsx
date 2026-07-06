import { HOURS, MINUTES_15, formatTime, parseTime } from "../lib/timeUtils";
import { RollColumn } from "./RollColumn";

interface TimePickerProps {
  value: string;
  onChange: (value: string) => void;
}

export function TimePicker({ value, onChange }: TimePickerProps) {
  const { hourIdx, minuteIdx } = parseTime(value);

  function setHourIdx(next: number) {
    onChange(formatTime(next, minuteIdx));
  }

  function setMinuteIdx(next: number) {
    onChange(formatTime(hourIdx, next));
  }

  return (
    <div className="time-picker">
      <RollColumn label="時" values={HOURS} index={hourIdx} onIndexChange={setHourIdx} />
      <span className="time-sep">:</span>
      <RollColumn label="分" values={MINUTES_15} index={minuteIdx} onIndexChange={setMinuteIdx} />
    </div>
  );
}
