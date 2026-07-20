//! Display transform: Coraxis on-device `TensorProfile` → radar chart data.
//! score=null stays null — never coerced to 0 (AI_SKILLS / FSA-05).
//! No React/component imports (boundary-safe pure module).

import type { DimensionScore, TensorProfile } from "./pocketBrain/types";
import type { TensorDimensionId } from "./types";

export interface PocketTensorRadarDatum {
  id: string;
  label: string;
  value: number | null;
  axisName?: string;
  description?: string;
}

const DIMENSION_ORDER: readonly TensorDimensionId[] = [
  "problem_structuring",
  "quantitative_rigor",
  "hypothesis_evidence",
  "synthesis_judgment",
  "communication",
  "collaboration_adaptability",
] as const;

const DIMENSION_LABELS: Record<TensorDimensionId, { label: string; description: string }> = {
  problem_structuring: {
    label: "構造化",
    description: "複雑な課題を漏れなく分解する力",
  },
  quantitative_rigor: {
    label: "定量精度",
    description: "数量や概算を正確かつ素早く扱う力",
  },
  hypothesis_evidence: {
    label: "仮説検証",
    description: "前提と根拠を結び、筋道立てて検証する力",
  },
  synthesis_judgment: {
    label: "統合判断",
    description: "未知の業界やテーマへ知識を適用する力",
  },
  communication: {
    label: "伝達",
    description: "考えを簡潔かつ明確に伝える力",
  },
  collaboration_adaptability: {
    label: "協働適応",
    description: "反証や相手の意見を受けて考えを更新する力",
  },
};

function scoreForId(
  dimensions: DimensionScore[],
  id: TensorDimensionId,
): DimensionScore | null {
  return dimensions.find((d) => d.dimension_id === id) ?? null;
}

/**
 * Map vault TensorProfile into exactly six radar points (canonical order).
 * Missing dimensions render as N/A (null).
 */
export function pocketTensorToRadarData(
  profile: TensorProfile,
): PocketTensorRadarDatum[] {
  return DIMENSION_ORDER.map((id) => {
    const row = scoreForId(profile.dimensions, id);
    const meta = DIMENSION_LABELS[id];
    return {
      id,
      label: meta.label,
      value: row?.score ?? null,
      axisName: row?.calculus_axis,
      description: meta.description,
    };
  });
}

export function pocketTensorDimensionRows(profile: TensorProfile): Array<{
  id: string;
  label: string;
  axisName: string;
  score: number | null;
  confidence: number;
}> {
  return DIMENSION_ORDER.map((id) => {
    const row = scoreForId(profile.dimensions, id);
    const meta = DIMENSION_LABELS[id];
    return {
      id,
      label: meta.label,
      axisName: row?.calculus_axis ?? "",
      score: row?.score ?? null,
      confidence: row?.confidence ?? 0,
    };
  });
}

export function isPocketTensorProfile(value: unknown): value is TensorProfile {
  if (!value || typeof value !== "object") return false;
  const o = value as Record<string, unknown>;
  return (
    typeof o.schema === "string" &&
    typeof o.model_hash === "string" &&
    Array.isArray(o.dimensions)
  );
}
