/**
 * Pure display-metadata transform for the validated six-dimensional tensor
 * profile. Converts already-validated TensorProfileReportV1 into the shape
 * TensorRadarChart expects. Structural typing only — TensorRadarChart.tsx is
 * never imported here (keeps this module out of the JSX-free boundary
 * compile and avoids any coupling to component internals).
 *
 * Display metadata (label/description) is defined once here and is the
 * canonical source for measured six-dimensional radar presentation.
 * ProfileTab no longer hosts a fixed preview; this module does not import
 * ProfileTab and does not export anything ProfileTab reads.
 */
import type { TensorDimensionId, TensorProfileReportV1 } from "./types";

interface TensorRadarDatumLike {
  id: string;
  label: string;
  value: number | null;
  axisName?: string;
  description?: string;
}

interface DimensionViewMeta {
  readonly label: string;
  readonly description: string;
}

const DIMENSION_VIEW_META: Record<TensorDimensionId, DimensionViewMeta> = {
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

export interface TensorProfileDimensionSummary {
  readonly id: TensorDimensionId;
  readonly label: string;
  readonly axisName: string;
  readonly description: string;
  readonly score: number | null;
  readonly confidence: number;
}

/** Fixed order preserved (input is already order-validated by the parser).
 * `score=null` is passed through as `null`, never coerced to 0. */
export function tensorProfileToRadarData(
  report: TensorProfileReportV1,
): TensorRadarDatumLike[] {
  return report.dimensions.map((d) => ({
    id: d.dimension_id,
    label: DIMENSION_VIEW_META[d.dimension_id].label,
    value: d.score,
    axisName: d.calculus_axis,
    description: DIMENSION_VIEW_META[d.dimension_id].description,
  }));
}

export function tensorProfileDimensionSummaries(
  report: TensorProfileReportV1,
): TensorProfileDimensionSummary[] {
  return report.dimensions.map((d) => ({
    id: d.dimension_id,
    label: DIMENSION_VIEW_META[d.dimension_id].label,
    axisName: d.calculus_axis,
    description: DIMENSION_VIEW_META[d.dimension_id].description,
    score: d.score,
    confidence: d.confidence,
  }));
}
