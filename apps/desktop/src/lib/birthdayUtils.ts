const CURRENT_YEAR = new Date().getFullYear();

export const BIRTH_YEARS = Array.from({ length: 101 }, (_, i) =>
  String(CURRENT_YEAR - 100 + i),
);

export const MONTHS = Array.from({ length: 12 }, (_, i) => String(i + 1).padStart(2, "0"));

export function daysInMonth(year: number, month: number): number {
  return new Date(year, month, 0).getDate();
}

export function dayOptions(year: number, month: number): string[] {
  const n = daysInMonth(year, month);
  return Array.from({ length: n }, (_, i) => String(i + 1).padStart(2, "0"));
}

export function defaultBirthday(): string {
  return `${CURRENT_YEAR - 30}-01-01`;
}

export function parseBirthday(value: string): {
  year: number;
  month: number;
  day: number;
  yearIdx: number;
  monthIdx: number;
  dayIdx: number;
  days: string[];
} {
  const fallback = defaultBirthday();
  const raw = /^\d{4}-\d{2}-\d{2}$/.test(value.trim()) ? value.trim() : fallback;
  const [y, m, d] = raw.split("-").map((x) => parseInt(x, 10));
  const yearIdx = Math.max(0, BIRTH_YEARS.indexOf(String(y)));
  const monthIdx = Math.max(0, Math.min(11, m - 1));
  const year = parseInt(BIRTH_YEARS[yearIdx] ?? String(CURRENT_YEAR - 30), 10);
  const month = monthIdx + 1;
  const days = dayOptions(year, month);
  const dayIdx = Math.max(0, Math.min(days.length - 1, days.indexOf(String(d).padStart(2, "0"))));
  return {
    year,
    month,
    day: parseInt(days[dayIdx] ?? "1", 10),
    yearIdx,
    monthIdx,
    dayIdx,
    days,
  };
}

export function formatBirthday(yearIdx: number, monthIdx: number, dayIdx: number): string {
  const y = BIRTH_YEARS[((yearIdx % BIRTH_YEARS.length) + BIRTH_YEARS.length) % BIRTH_YEARS.length];
  const m = MONTHS[((monthIdx % MONTHS.length) + MONTHS.length) % MONTHS.length];
  const year = parseInt(y, 10);
  const month = parseInt(m, 10);
  const days = dayOptions(year, month);
  const d = days[((dayIdx % days.length) + days.length) % days.length];
  return `${y}-${m}-${d}`;
}
