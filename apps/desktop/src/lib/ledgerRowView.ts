/**
 * Purchase ledger row presentation (pure). No React / DOM.
 * Risk flags are derived only from explicit category markers — never invent schema.
 */

export type LedgerRiskLevel = "none" | "warn" | "danger";

const WARN_MARKERS: readonly RegExp[] = [
  /非計画/,
  /衝動/,
  /impulse/i,
  /unplanned/i,
  /深夜/,
  /late[\s_-]?night/i,
];

const DANGER_MARKERS: readonly RegExp[] = [
  /破局/,
  /catastroph/i,
  /全か無か/,
  /all[\s_-]?or[\s_-]?nothing/i,
  /読心/,
  /mind[\s_-]?reading/i,
  /歪み/,
  /distortion/i,
];

/** Right-aligned yen display string (no currency suffix — CSS adds 円). */
export function formatLedgerYen(amount: number): string {
  if (!Number.isFinite(amount)) return "—";
  return Math.trunc(amount).toLocaleString("ja-JP");
}

export function ledgerTypeCode(type: "expense" | "income"): "EXP" | "INC" {
  return type === "expense" ? "EXP" : "INC";
}

/**
 * Point indicators for hazardous rows.
 * warn  = unplanned / impulse markers → `--err-soft`
 * danger = CBT distortion markers → `--err`
 */
export function ledgerRiskLevel(category: string): LedgerRiskLevel {
  const text = category.trim();
  if (!text) return "none";
  for (const re of DANGER_MARKERS) {
    if (re.test(text)) return "danger";
  }
  for (const re of WARN_MARKERS) {
    if (re.test(text)) return "warn";
  }
  return "none";
}

export function ledgerRiskLabel(level: LedgerRiskLevel): string | null {
  if (level === "danger") return "DISTORTION";
  if (level === "warn") return "UNPLANNED";
  return null;
}

export function ledgerRowClassName(level: LedgerRiskLevel): string {
  if (level === "danger") return "ledger-row ledger-row--danger";
  if (level === "warn") return "ledger-row ledger-row--warn";
  return "ledger-row";
}
