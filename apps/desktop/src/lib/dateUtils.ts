const WEEKDAYS = "月火水木金土日";

export function todayIso(): string {
  return new Date().toISOString().slice(0, 10);
}

export function formatDateLabel(dateStr: string): string {
  const d = new Date(`${dateStr}T12:00:00`);
  const wd = WEEKDAYS[d.getDay() === 0 ? 6 : d.getDay() - 1];
  return `${dateStr} (${wd})`;
}

export function monthLabel(year: number, month: number): string {
  return `${year}年 ${month}月`;
}

export function parseAmount(raw: string): number | null {
  const t = raw.trim();
  if (!/^\d+$/.test(t)) return null;
  const n = parseInt(t, 10);
  return n > 0 ? n : null;
}

export function summarizeDay(transactions: { type: string; amount: number }[]) {
  let income = 0;
  let expense = 0;
  for (const t of transactions) {
    if (t.type === "income") income += t.amount;
    if (t.type === "expense") expense += t.amount;
  }
  return { income, expense, net: income - expense };
}
