//! Typed Tauri clients for M11–M17 Coraxis on-device commands.
//!
//! Streaming commands accept an `onToken` callback and open one `Channel`
//! (same M6 contract as `llm_generate`). No `any`.

import { Channel } from "@tauri-apps/api/core";

import { hapticBiasDetected } from "../haptics";
import { pocketInvoke } from "./invoke";
import type {
  AnalyticsDailyDay,
  AxisScoreHint,
  CalculateGapAnalysisResult,
  CalculatePulseResult,
  CognitiveBiasProfile,
  CognitiveDistortionReportV1,
  CompanyFacts,
  ConsultWithOracleResult,
  EvaluateRaschRequest,
  EvaluateRaschResult,
  EvaluateTwinResult,
  GenerateOracleResult,
  IngestKnowledgeResult,
  InterviewSession,
  LatestGapAnalysisResult,
  MultistageInterviewResult,
  ProbeAnswerResultV1,
  ProbeQuestionOut,
  ProbeQuestionV1,
  ProbeStatusV1,
  RagChatParams,
  RaschStateWire,
  RecordCognitiveDistortionsResult,
  ScenarioModifiers,
  SearchKnowledgeResult,
  SendRagChatResult,
  SimGenParams,
  SimSessionResult,
  TensorProfile,
  TokenEvent,
  TwinIdentifyStatus,
} from "./types";

function genPayload(gen?: SimGenParams): Record<string, unknown> | null {
  if (!gen) return null;
  return {
    nCtx: gen.nCtx ?? null,
    maxTokens: gen.maxTokens ?? null,
    temp: gen.temp ?? null,
    topK: gen.topK ?? null,
    topP: gen.topP ?? null,
    seed: gen.seed ?? null,
    contextLimit: gen.contextLimit ?? null,
  };
}

function ragParamsPayload(params: RagChatParams): Record<string, unknown> {
  return {
    nCtx: params.nCtx ?? null,
    maxTokens: params.maxTokens ?? null,
    temp: params.temp ?? null,
    topK: params.topK ?? null,
    topP: params.topP ?? null,
    seed: params.seed ?? null,
    contextLimit: params.contextLimit ?? null,
  };
}

function tokenChannel(onToken: (event: TokenEvent) => void): Channel<TokenEvent> {
  return new Channel<TokenEvent>((event) => {
    // Phase 10: CBT detections → Impact haptic (metacognitive cue).
    const report = event.validated_distortions;
    if (
      report &&
      Array.isArray(report.detected_distortions) &&
      report.detected_distortions.length > 0
    ) {
      hapticBiasDetected();
    }
    onToken(event);
  });
}

// ─── RAG ────────────────────────────────────────────────────────────────────

export function ingestKnowledge(
  text: string,
  sourceId: string,
): Promise<IngestKnowledgeResult> {
  return pocketInvoke("ingest_knowledge", { text, sourceId });
}

export function searchKnowledge(
  query: string,
  limit?: number,
): Promise<SearchKnowledgeResult> {
  return pocketInvoke("search_knowledge", { query, limit: limit ?? null });
}

export function sendRagChat(
  message: string,
  onToken: (event: TokenEvent) => void,
  params: RagChatParams = {},
): Promise<SendRagChatResult> {
  return pocketInvoke("send_rag_chat", {
    message,
    params: ragParamsPayload(params),
    onToken: tokenChannel(onToken),
  });
}

// ─── Gap / Tensor ───────────────────────────────────────────────────────────

export function calculateGapAnalysis(
  days: AnalyticsDailyDay[],
): Promise<CalculateGapAnalysisResult> {
  return pocketInvoke("calculate_gap_analysis", {
    request: { days },
  });
}

export function getLatestGapAnalysis(): Promise<LatestGapAnalysisResult | null> {
  return pocketInvoke("get_latest_gap_analysis");
}

export function getLatestTensorProfile(): Promise<TensorProfile> {
  return pocketInvoke("get_latest_tensor_profile");
}

export function ensureAuthoritativeTensorProfile(): Promise<TensorProfile> {
  return pocketInvoke("ensure_authoritative_tensor_profile");
}

// ─── Psychometrics ──────────────────────────────────────────────────────────

export function calculateInteractionPulse(
  transcript: string,
): Promise<CalculatePulseResult> {
  return pocketInvoke("calculate_interaction_pulse", {
    request: { transcript },
  });
}

export function evaluateRaschScale(
  request: EvaluateRaschRequest,
): Promise<EvaluateRaschResult> {
  return pocketInvoke("evaluate_rasch_scale", {
    request: {
      posterior: request.posterior ?? null,
      item_id: request.item_id,
      response: request.response,
      excluded: request.excluded ?? null,
    },
  });
}

export function getProbeQuestions(): Promise<ProbeQuestionOut[]> {
  return pocketInvoke("get_probe_questions");
}

export function probeNextQuestion(args: {
  today: string;
  axisHints?: AxisScoreHint[] | null;
}): Promise<ProbeQuestionV1> {
  return pocketInvoke("probe_next_question", {
    request: {
      today: args.today,
      axis_hints: args.axisHints ?? null,
    },
  });
}

export function probeSubmitAnswer(args: {
  sessionId: string;
  questionId: string;
  answer: string;
  today: string;
  axisHints?: AxisScoreHint[] | null;
}): Promise<ProbeAnswerResultV1> {
  return pocketInvoke("probe_submit_answer", {
    request: {
      session_id: args.sessionId,
      question_id: args.questionId,
      answer: args.answer,
      today: args.today,
      axis_hints: args.axisHints ?? null,
    },
  });
}

export function getProbeStatus(today: string): Promise<ProbeStatusV1> {
  return pocketInvoke("get_probe_status", {
    request: { today },
  });
}

export function getLatestRaschState(): Promise<RaschStateWire | null> {
  return pocketInvoke("get_latest_rasch_state");
}

// ─── Twin / Oracle ──────────────────────────────────────────────────────────

export function evaluateDigitalTwinScenario(args: {
  today: string;
  horizonDays?: number;
  scenario?: ScenarioModifiers;
  pulseAffinity?: number | null;
  raschPosterior?: number[] | null;
  gapDataSufficiency?: number | null;
  gapCount?: number | null;
}): Promise<EvaluateTwinResult> {
  return pocketInvoke("evaluate_digital_twin_scenario", {
    request: {
      today: args.today,
      horizon_days: args.horizonDays ?? null,
      scenario: args.scenario ?? null,
      tensor: null,
      pulse_affinity: args.pulseAffinity ?? null,
      rasch_posterior: args.raschPosterior ?? null,
      gap_data_sufficiency: args.gapDataSufficiency ?? null,
      gap_count: args.gapCount ?? null,
      lane_values: null,
      lane_mask: null,
      n_rows: null,
      n_lanes: null,
    },
  });
}

export function getTwinIdentifyStatus(): Promise<TwinIdentifyStatus> {
  return pocketInvoke("get_twin_identify_status");
}

export function generateOraclePayload(args: {
  today: string;
  horizonDays?: number;
  scenario?: ScenarioModifiers;
  ragHitCount?: number | null;
  interviewSessionId?: string | null;
}): Promise<GenerateOracleResult> {
  return pocketInvoke("generate_oracle_payload", {
    request: {
      today: args.today,
      horizon_days: args.horizonDays ?? null,
      scenario: args.scenario ?? null,
      tensor: null,
      pulse_affinity: null,
      rasch_posterior: null,
      gap_data_sufficiency: null,
      gap_count: null,
      lane_values: null,
      lane_mask: null,
      n_rows: null,
      n_lanes: null,
      rag_hit_count: args.ragHitCount ?? null,
      interview_session_id: args.interviewSessionId ?? null,
    },
  });
}

// ─── Interview / Consult ────────────────────────────────────────────────────

export function fetchEdinetCompanyFacts(args: {
  companyName?: string;
  edinetCode?: string;
  edinetDate?: string;
  filingText?: string;
}): Promise<CompanyFacts> {
  return pocketInvoke("fetch_edinet_company_facts", {
    params: {
      companyName: args.companyName ?? null,
      edinetCode: args.edinetCode ?? null,
      edinetDate: args.edinetDate ?? null,
      filingText: args.filingText ?? null,
    },
  });
}

export function startInterviewSession(
  args: {
    message: string;
    companyFacts?: CompanyFacts;
    edinetCode?: string;
    edinetDate?: string;
    filingText?: string;
    gen?: SimGenParams;
  },
  onToken: (event: TokenEvent) => void,
): Promise<SimSessionResult> {
  return pocketInvoke("start_interview_session", {
    params: {
      message: args.message,
      companyFacts: args.companyFacts ?? null,
      edinetCode: args.edinetCode ?? null,
      edinetDate: args.edinetDate ?? null,
      filingText: args.filingText ?? null,
      gen: genPayload(args.gen),
    },
    onToken: tokenChannel(onToken),
  });
}

export function reviewEsDraft(
  args: {
    esDraft: string;
    companyFacts?: CompanyFacts;
    edinetCode?: string;
    edinetDate?: string;
    filingText?: string;
    experienceQuery?: string;
    gen?: SimGenParams;
  },
  onToken: (event: TokenEvent) => void,
): Promise<SimSessionResult> {
  return pocketInvoke("review_es_draft", {
    params: {
      esDraft: args.esDraft,
      companyFacts: args.companyFacts ?? null,
      edinetCode: args.edinetCode ?? null,
      edinetDate: args.edinetDate ?? null,
      filingText: args.filingText ?? null,
      experienceQuery: args.experienceQuery ?? null,
      gen: genPayload(args.gen),
    },
    onToken: tokenChannel(onToken),
  });
}

export function consultWithOracleContext(
  args: {
    message: string;
    includeRag?: boolean;
    gen?: RagChatParams;
  },
  onToken: (event: TokenEvent) => void,
): Promise<ConsultWithOracleResult> {
  return pocketInvoke("consult_with_oracle_context", {
    params: {
      message: args.message,
      includeRag: args.includeRag ?? true,
      gen: args.gen ? ragParamsPayload(args.gen) : null,
    },
    onToken: tokenChannel(onToken),
  });
}

export function startMultistageInterview(
  args: {
    openingMessage?: string;
    companyFacts?: CompanyFacts;
    edinetCode?: string;
    edinetDate?: string;
    filingText?: string;
    gen?: SimGenParams;
  },
  onToken: (event: TokenEvent) => void,
): Promise<MultistageInterviewResult> {
  return pocketInvoke("start_multistage_interview", {
    params: {
      openingMessage: args.openingMessage ?? null,
      companyFacts: args.companyFacts ?? null,
      edinetCode: args.edinetCode ?? null,
      edinetDate: args.edinetDate ?? null,
      filingText: args.filingText ?? null,
      gen: genPayload(args.gen),
    },
    onToken: tokenChannel(onToken),
  });
}

export function advanceInterviewStage(
  args: {
    sessionId: string;
    candidateAnswer: string;
    gen?: SimGenParams;
  },
  onToken: (event: TokenEvent) => void,
): Promise<MultistageInterviewResult> {
  return pocketInvoke("advance_interview_stage", {
    params: {
      sessionId: args.sessionId,
      candidateAnswer: args.candidateAnswer,
      gen: genPayload(args.gen),
    },
    onToken: tokenChannel(onToken),
  });
}

export function getInterviewSession(
  sessionId: string,
): Promise<InterviewSession> {
  return pocketInvoke("get_interview_session", { sessionId });
}

/** Persist a GBNF-validated CBT extraction report into vault `distortion_tags`. */
export async function recordCognitiveDistortions(args: {
  report: CognitiveDistortionReportV1;
  sourceKind: string;
  sourceId: string;
}): Promise<RecordCognitiveDistortionsResult> {
  const result = await pocketInvoke<RecordCognitiveDistortionsResult>(
    "record_cognitive_distortions",
    {
      report: args.report,
      sourceKind: args.sourceKind,
      sourceId: args.sourceId,
    },
  );
  if (
    result.inserted > 0 ||
    (args.report.detected_distortions?.length ?? 0) > 0
  ) {
    hapticBiasDetected();
  }
  return result;
}

/** Deterministic Burns-category fingerprint over accumulated distortion tags. */
export function getCognitiveBiasProfile(
  limit?: number,
): Promise<CognitiveBiasProfile> {
  return pocketInvoke("get_cognitive_bias_profile", {
    limit: limit ?? null,
  });
}
