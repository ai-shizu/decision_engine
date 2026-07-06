export const HOURS = Array.from({ length: 24 }, (_, h) => String(h).padStart(2, "0"));
export const MINUTES_15 = ["00", "15", "30", "45"] as const;

export function defaultTime(): string {
  const now = new Date();
  const h = now.getHours();
  const m = Math.floor(now.getMinutes() / 15) * 15;
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}`;
}

export function snapTime(timeStr: string): string {
  try {
    const [hRaw, mRaw] = timeStr.trim().split(":").slice(0, 2);
    const h = Math.max(0, Math.min(23, parseInt(hRaw, 10)));
    const m = parseInt(mRaw, 10);
    const snapped = MINUTES_15.reduce((best, cur) =>
      Math.abs(parseInt(cur, 10) - m) < Math.abs(parseInt(best, 10) - m) ? cur : best,
    );
    return `${String(h).padStart(2, "0")}:${snapped}`;
  } catch {
    return defaultTime();
  }
}

export function parseTime(timeStr: string): { hourIdx: number; minuteIdx: number } {
  const snapped = snapTime(timeStr);
  const [h, m] = snapped.split(":");
  return {
    hourIdx: HOURS.indexOf(h),
    minuteIdx: MINUTES_15.indexOf(m as (typeof MINUTES_15)[number]),
  };
}

export function formatTime(hourIdx: number, minuteIdx: number): string {
  const h = HOURS[((hourIdx % HOURS.length) + HOURS.length) % HOURS.length];
  const m = MINUTES_15[((minuteIdx % MINUTES_15.length) + MINUTES_15.length) % MINUTES_15.length];
  return `${h}:${m}`;
}
