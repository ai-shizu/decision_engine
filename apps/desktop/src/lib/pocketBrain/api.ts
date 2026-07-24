//! Typed Tauri clients for M11–M17 Coraxis on-device commands.
//!
//! Streaming commands accept an `onToken` callback and open one `Channel`
//! (same M6 contract as `llm_generate`). No `any`.

import { Channel, isTauri } from "@tauri-apps/api/core";
import { BaseDirectory } from "@tauri-apps/api/path";
import { mkdir, remove, writeFile } from "@tauri-apps/plugin-fs";

import { hapticBiasDetected } from "../haptics";
import { PocketBrainInvokeError, pocketInvoke } from "./invoke";
import type {
  AnalyticsDailyDay,
  AxisScoreHint,
  CalculateGapAnalysisResult,
  CalculatePulseResult,
  CognitiveBiasProfile,
  CognitiveMonthView,
  CognitiveDistortionReportV1,
  CompanyFacts,
  ConsultWithOracleResult,
  EvaluateRaschRequest,
  EvaluateRaschResult,
  EvaluateTwinResult,
  FetchAppleCalendarEventsResult,
  GenerateOracleResult,
  IngestKnowledgeResult,
  InterviewSession,
  LatestGapAnalysisResult,
  KnowledgeNamespace,
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
  SyncDailyContextResult,
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

/** Persist EDINET company facts into the Company knowledge namespace (fire-and-forget from FE). */
export function ingestCompanyKnowledge(
  facts: CompanyFacts,
): Promise<IngestKnowledgeResult> {
  return pocketInvoke("ingest_company_knowledge", { facts });
}

/**
 * On-device LINE トーク履歴 (.txt) import (M20 データ連携 Part 1).
 *
 * Stages bytes under `$APPDATA/imports/` via plugin-fs and invokes
 * `ingest_line_history_path` so multi-MB exports never cross IPC as a JSON
 * `number[]` (that path Jetsams / fails on iPhone). Tiny files may still use
 * the legacy `bytes` command as a fallback if staging is unavailable.
 */
export async function ingestLineHistory(file: File): Promise<IngestKnowledgeResult> {
  const buf = new Uint8Array(await file.arrayBuffer());
  if (buf.byteLength === 0) {
    throw new PocketBrainInvokeError("ingest_line_history", "LINE_IMPORT:EMPTY");
  }

  if (isTauri()) {
    const safe =
      file.name.replace(/[^a-zA-Z0-9._-]+/g, "_").slice(0, 80) || "line.txt";
    const relativePath = `imports/line-${Date.now()}-${safe}`;
    try {
      await mkdir("imports", { baseDir: BaseDirectory.AppData, recursive: true });
      await writeFile(relativePath, buf, { baseDir: BaseDirectory.AppData });
      return await pocketInvoke<IngestKnowledgeResult>("ingest_line_history_path", {
        relativePath,
        filename: file.name,
      });
    } catch (err) {
      // If the path command is missing (older binary), fall through to bytes IPC
      // only for small payloads — large Array.from is the known device killer.
      const msg = err instanceof Error ? err.message : String(err);
      const missing =
        msg.includes("not found") ||
        msg.includes("Command") ||
        msg.includes("ingest_line_history_path");
      if (!missing || buf.byteLength > 96 * 1024) {
        throw err instanceof PocketBrainInvokeError
          ? err
          : new PocketBrainInvokeError("ingest_line_history_path", err);
      }
    } finally {
      try {
        await remove(relativePath, { baseDir: BaseDirectory.AppData });
      } catch {
        /* best-effort cleanup */
      }
    }
  }

  return pocketInvoke("ingest_line_history", {
    bytes: Array.from(buf),
    filename: file.name,
  });
}

export function searchKnowledge(
  query: string,
  limit?: number,
  namespace: KnowledgeNamespace = "all",
): Promise<SearchKnowledgeResult> {
  return pocketInvoke("search_knowledge", {
    query,
    limit: limit ?? null,
    namespace,
  });
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
    /** Optional ES body as interview base (empty = zero-base). */
    esText?: string;
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
      esText: args.esText?.trim() ? args.esText : null,
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
    /** SETTINGS fixed attributes (birthday/gender/height/…) for prompt injection. */
    profile?: Record<string, string>;
  },
  onToken: (event: TokenEvent) => void,
): Promise<ConsultWithOracleResult> {
  return pocketInvoke("consult_with_oracle_context", {
    params: {
      message: args.message,
      includeRag: args.includeRag ?? true,
      gen: args.gen ? ragParamsPayload(args.gen) : null,
      profile: args.profile ?? null,
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

/** Phase 12 — daily Twin R(t) / expense / CBT aggregates for one JST month. */
export function getCognitiveMonthView(
  year: number,
  month: number,
): Promise<CognitiveMonthView> {
  return pocketInvoke("get_cognitive_month_view", { year, month });
}

// ─── Calendar (M20 データ連携 Part 2 — EventKit) ────────────────────────────

/**
 * Read iOS/macOS Calendar events in `[startUnix, endUnix)` (Unix seconds,
 * UTC) via the on-device EventKit bridge. Prompts for calendar access at most
 * once (iOS caches the decision); when access is anything other than
 * `full_access`, `events` is empty and `authorized` is `false` rather than
 * throwing — callers should branch on `status`, not treat a rejection as an
 * invoke failure. Read-only: never creates, edits, or deletes events.
 */
export function fetchAppleCalendarEvents(
  startUnix: number,
  endUnix: number,
): Promise<FetchAppleCalendarEventsResult> {
  return pocketInvoke("fetch_apple_calendar_events", {
    params: { startUnix, endUnix },
  });
}

/**
 * Merge one day's calendar events JSON + optional daily log into vault
 * knowledge under `daily-{YYYY-MM-DD}` (M13). Used after EventKit fetch to
 * make schedules searchable by on-device CONSULT/RAG.
 */
export function syncDailyContext(
  dateStr: string,
  eventsJson: string,
  dailyLog: string = "",
): Promise<SyncDailyContextResult> {
  return pocketInvoke("sync_daily_context", {
    dateStr,
    eventsJson,
    dailyLog,
  });
}
