//! M17 interview FSM stage helpers (pure; mirror Rust InterviewStage).

import type { CompanyFacts, InterviewStage } from "./pocketBrain/types";

export const INTERVIEW_STAGE_ORDER: readonly InterviewStage[] = [
  "foundation",
  "pressure",
  "debrief",
  "closed",
] as const;

export const INTERVIEW_STAGE_LABELS: Record<InterviewStage, string> = {
  foundation: "Foundation",
  pressure: "Pressure",
  debrief: "Debrief",
  closed: "Closed",
};

export const INTERVIEW_STAGE_HINTS: Record<InterviewStage, string> = {
  foundation: "基礎突撃 — 経歴・動機の確認",
  pressure: "圧迫・深掘り — 矛盾と定量欠落を突く",
  debrief: "最終講評 — Gap/Oracle 注入可",
  closed: "セッション終了",
};

export function isInterviewStage(value: string): value is InterviewStage {
  return (
    value === "foundation" ||
    value === "pressure" ||
    value === "debrief" ||
    value === "closed"
  );
}

/** Coerce Rust `stage: String` into the typed FSM stage. */
export function coerceInterviewStage(raw: string): InterviewStage {
  return isInterviewStage(raw) ? raw : "foundation";
}

export function stageIndex(stage: InterviewStage): number {
  return INTERVIEW_STAGE_ORDER.indexOf(stage);
}

export function emptyCompanyFacts(overrides?: Partial<CompanyFacts>): CompanyFacts {
  return {
    companyName: "",
    edinetCode: "",
    docId: "",
    businessSummary: "",
    businessRisks: "",
    performanceSummary: "",
    source: "injected",
    ...overrides,
  };
}

/** Offline-injectable facts for start_* / review_es_draft (egress not required). */
export function companyFactsReady(facts: CompanyFacts): boolean {
  return facts.companyName.trim().length > 0;
}
