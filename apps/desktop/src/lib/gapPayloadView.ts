//! Pure parsers for gap_analysis.v3 payload (no `any`).

export interface ThemeScoreView {
  theme: string;
  score: number;
  moneySpent?: number;
  eventCount?: number;
}

export interface GapFlagView {
  type: string;
  theme: string | null;
  gap: number | null;
  insight: string | null;
}

export interface GapPayloadView {
  schema: string | null;
  dataSufficiency: number | null;
  subjective: ThemeScoreView[];
  objective: ThemeScoreView[];
  gaps: GapFlagView[];
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  return value as Record<string, unknown>;
}

function asNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function asString(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function parseThemeScores(raw: unknown): ThemeScoreView[] {
  const obj = asRecord(raw);
  if (!obj) return [];
  const out: ThemeScoreView[] = [];
  for (const [theme, entry] of Object.entries(obj)) {
    const row = asRecord(entry);
    if (!row) continue;
    const score = asNumber(row.score);
    if (score === null) continue;
    out.push({
      theme,
      score,
      moneySpent: asNumber(row.money_spent) ?? undefined,
      eventCount: asNumber(row.event_count) ?? undefined,
    });
  }
  return out;
}

function parseGapFlag(raw: unknown): GapFlagView | null {
  const row = asRecord(raw);
  if (!row) return null;
  const type = asString(row.type);
  if (!type) return null;
  return {
    type,
    theme: asString(row.theme),
    gap: asNumber(row.gap),
    insight: asString(row.insight),
  };
}

/** Extract display-ready fields from LatestGapAnalysisResult.payload / calculate payload. */
export function parseGapPayload(payload: Record<string, unknown>): GapPayloadView {
  const gapsRaw = payload.gaps;
  const gaps: GapFlagView[] = Array.isArray(gapsRaw)
    ? gapsRaw
        .map(parseGapFlag)
        .filter((g): g is GapFlagView => g !== null)
    : [];

  return {
    schema: asString(payload.schema),
    dataSufficiency: asNumber(payload.data_sufficiency),
    subjective: parseThemeScores(payload.subjective_scores),
    objective: parseThemeScores(payload.objective_scores),
    gaps,
  };
}

export function sufficiencyLabel(value: number): string {
  if (value >= 0.7) return "十分";
  if (value >= 0.4) return "部分的";
  return "不足";
}
