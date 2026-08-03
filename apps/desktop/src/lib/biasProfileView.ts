/**
 * Display transform: CognitiveBiasProfile → radar-ready points (Phase 8).
 *
 * Mirrors `pocketBrainTensorView` / TensorRadarChart expectations without
 * mounting a chart yet — keep GapTensorDashboard non-destructive.
 *
 * CBT grounding: Beck (1976) / Burns (1980) ten cognitive distortions.
 */

import type { CategoryBiasScore, CognitiveBiasProfile } from "./pocketBrain/types";

export type BiasRadarPoint = {
  id: string;
  axisName: string;
  /** Plot value in [0, 1] from profile.score. */
  plot: number;
  count: number;
  meanConfidence: number;
  recentCount30d: number;
};

const AXIS_LABELS: Record<string, string> = {
  all_or_nothing: "全か無か",
  overgeneralization: "過度の一般化",
  mental_filter: "心のフィルター",
  disqualifying_the_positive: "マイナス化",
  jumping_to_conclusions: "結論の飛躍",
  magnification_minimization: "拡大/過小",
  emotional_reasoning: "感情的決めつけ",
  should_statements: "すべき思考",
  labeling: "レッテル",
  personalization: "自己関連づけ",
};

export function categoryAxisName(category: string): string {
  return AXIS_LABELS[category] ?? category;
}

/** Map vault bias profile into radar points (Burns order preserved). */
export function biasProfileToRadarPoints(
  profile: CognitiveBiasProfile,
): BiasRadarPoint[] {
  return profile.categories.map((c: CategoryBiasScore) => ({
    id: c.category,
    axisName: categoryAxisName(c.category),
    plot: Number.isFinite(c.score) ? Math.min(1, Math.max(0, c.score)) : 0,
    count: c.count,
    meanConfidence: c.mean_confidence,
    recentCount30d: c.recent_count_30d,
  }));
}
