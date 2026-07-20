//! Pocket Brain IPC types (M11–M17). Mirror Rust serde shapes.
//!
//! Responses use snake_case (Rust `rename_all = "snake_case"`).
//! Request payloads follow each command's rename_all (documented per field).

/** Shared streaming token event (same contract as `llm_generate`). */
export interface TokenEvent {
  seq: number;
  text: string;
  done: boolean;
  error: string | null;
  validated: unknown | null;
}

// ─── RAG (M10–M11 / M13) ───────────────────────────────────────────────────

export interface IngestKnowledgeResult {
  source_id: string;
  chunk_count: number;
  inserted: number;
}

export interface SearchKnowledgeHit {
  id: string;
  text_content: string;
  distance: number;
}

export interface SearchKnowledgeResult {
  hits: SearchKnowledgeHit[];
}

export interface RagChatParams {
  nCtx?: number;
  maxTokens?: number;
  temp?: number;
  topK?: number;
  topP?: number;
  seed?: number;
  contextLimit?: number;
}

export interface SendRagChatResult {
  context_ids: string[];
  context_count: number;
}

export interface SyncDailyContextResult {
  date: string;
  source_id: string;
  chunk_count: number;
  inserted: number;
  markdown_bytes: number;
}

// ─── Gap / Tensor (M14) ─────────────────────────────────────────────────────

export interface ConsultationIn {
  query: string;
  isSimulatedPersona?: boolean;
}

export interface TransactionIn {
  type: string;
  category: string;
  amount: number;
}

export interface CalendarEventIn {
  title: string;
}

export interface AnalyticsDailyDay {
  date: string;
  diaryText?: string;
  consultations?: ConsultationIn[];
  transactions?: TransactionIn[];
  calendarEvents?: CalendarEventIn[];
  lineSelfText?: string;
}

export interface CalculateGapRequest {
  days: AnalyticsDailyDay[];
}

export interface CalculateGapAnalysisResult {
  id: string;
  data_sufficiency: number;
  gap_count: number;
  gaps: Record<string, unknown>[];
  languageization_prompt: string;
  payload: Record<string, unknown>;
}

export interface LatestGapAnalysisResult {
  id: string;
  created_at: number;
  data_sufficiency: number;
  payload: Record<string, unknown>;
}

export interface DimensionScore {
  dimension_id: string;
  calculus_axis: string;
  score: number | null;
  confidence: number;
}

export interface TensorProfile {
  schema: string;
  model_hash: string;
  dimensions: DimensionScore[];
  evidence: unknown[];
}

// ─── Psychometrics (M15) ────────────────────────────────────────────────────

export interface RomanceMetrics {
  total: number;
  self_count: number;
  contact_count: number;
  switches: number;
  self_to_contact: number;
  self_turns_with_successor: number;
  balance: number;
  switch_rate: number;
  reply_coverage: number;
}

export interface RomanceAnalysisV1 {
  schema: string;
  affinity_score: number | null;
  interaction_tendency: string;
  next_best_action: string;
}

export interface CalculatePulseResult {
  id: string;
  analysis: RomanceAnalysisV1;
  metrics: RomanceMetrics;
  input_hash: string;
}

export interface EvaluateRaschRequest {
  posterior?: number[] | null;
  item_id: string;
  response: number;
  excluded?: string[] | null;
}

export interface ItemSelection {
  item_id: string;
  eig: number;
  quantized_eig: number;
}

export interface EvaluateRaschResult {
  schema: string;
  artifact_sha256: string;
  posterior: number[];
  excluded: string[];
  next: ItemSelection | null;
}

export interface ProbeQuestionOut {
  id: string;
  axis: string;
  stage: string;
  text: string;
}

export interface AxisScoreHint {
  axis: string;
  score: number | null;
  confidence: number;
}

export interface ProbeQuestionV1 {
  schema: string;
  session_id: string;
  question_id: string;
  axis: string;
  stage: string;
  question: string;
  priority: number;
}

export interface ProbeAnswerResultV1 {
  schema: string;
  saved: boolean;
  node_id: string;
  session_status: string;
  next_question: ProbeQuestionV1 | null;
}

export interface ProbeStatusV1 {
  schema: string;
  today: string;
  progress_percent: number;
  completed_stages: number;
  total_stages: number;
}

// ─── Twin / Oracle (M16) ────────────────────────────────────────────────────

export interface ScenarioModifiers {
  recover_boost?: number | null;
  switch_cap?: number | null;
  volume_cap?: number | null;
  friction_cap?: number | null;
}

export interface TwinParams {
  rho: number;
  beta1: number;
  beta2: number;
  gamma: number;
  kappa: number;
  theta_r: number | null;
  bss: number;
  n_lapse_test: number;
  gate_passed: boolean;
  fitted_window: string;
}

export interface TwinStateVector {
  r_now: number;
  recovery: number;
  load_switch: number;
  load_volume: number;
  friction: number;
  rasch_ability: number | null;
  pulse_norm: number | null;
  gap_pressure: number;
  tensor_coverage: number;
}

export interface TwinForecast {
  horizon_days: number;
  r_q10: number[];
  r_q50: number[];
  r_q90: number[];
  p_lapse: number[];
  critical_days: string[];
}

export interface TwinScenarioResult {
  schema: string;
  params: TwinParams;
  state: TwinStateVector;
  forecast: TwinForecast;
}

export interface CouplingPair {
  src: number;
  dst: number;
  lag: number;
  rho: number;
  n_eff: number;
  null_q99: number | null;
  sig: boolean;
  reason: string | null;
}

export interface CouplingMatrix {
  pairs: CouplingPair[];
  n_rows: number;
  max_lag: number;
  n_lanes: number;
}

export interface EvaluateTwinResult {
  id: string;
  twin: TwinScenarioResult;
  coupling: CouplingMatrix | null;
}

export interface OracleProvenance {
  gap_run_id: string | null;
  tensor_run_id: string | null;
  pulse_run_id: string | null;
  rasch_run_id: string | null;
  twin_run_id: string | null;
  rag_hit_count: number | null;
  interview_session_id: string | null;
}

export interface GenerateOracleResult {
  id: string;
  payload: Record<string, unknown>;
  provenance: OracleProvenance;
  languageization_prompt: string;
}

// ─── Interview / Consult (M12 / M17) ────────────────────────────────────────

export interface CompanyFacts {
  companyName: string;
  edinetCode: string;
  docId: string;
  businessSummary: string;
  businessRisks: string;
  performanceSummary: string;
  source: string;
}

export interface SimGenParams {
  nCtx?: number;
  maxTokens?: number;
  temp?: number;
  topK?: number;
  topP?: number;
  seed?: number;
  contextLimit?: number;
}

export interface SimSessionResult {
  context_ids: string[];
  context_count: number;
  company_name: string;
  facts_source: string;
}

export interface ConsultWithOracleResult {
  context_ids: string[];
  context_count: number;
  gap_available: boolean;
  oracle_available: boolean;
  gap_run_id: string | null;
  oracle_run_id: string | null;
}

export type InterviewStage =
  | "foundation"
  | "pressure"
  | "debrief"
  | "closed";

export interface InterviewTurn {
  role: string;
  text: string;
  stage: string;
}

export interface InterviewSession {
  schema: string;
  id: string;
  company_name: string;
  facts_json: string;
  stage: InterviewStage;
  turn_in_stage: number;
  total_turns: number;
  transcript: InterviewTurn[];
  status: string;
  evaluation_notes: string;
}

export interface MultistageInterviewResult {
  session_id: string;
  stage: string;
  status: string;
  turn_in_stage: number;
  total_turns: number;
  context_ids: string[];
  company_name: string;
  outcome: string;
}

/** Canonical list of M11–M17 Pocket Brain Tauri commands (invoke names). */
export const POCKET_BRAIN_COMMANDS = [
  "ingest_knowledge",
  "search_knowledge",
  "send_rag_chat",
  "sync_daily_context",
  "calculate_gap_analysis",
  "get_latest_gap_analysis",
  "get_latest_tensor_profile",
  "ensure_authoritative_tensor_profile",
  "calculate_interaction_pulse",
  "evaluate_rasch_scale",
  "rasch_select_next_item",
  "get_probe_questions",
  "probe_next_question",
  "probe_submit_answer",
  "get_probe_status",
  "get_latest_rasch_state",
  "evaluate_digital_twin_scenario",
  "generate_oracle_payload",
  "fetch_edinet_company_facts",
  "start_interview_session",
  "review_es_draft",
  "consult_with_oracle_context",
  "start_multistage_interview",
  "advance_interview_stage",
  "get_interview_session",
] as const;

export type PocketBrainCommand = (typeof POCKET_BRAIN_COMMANDS)[number];
