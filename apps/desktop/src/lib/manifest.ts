/** RetrievalManifestV1 IPC contract (Phase 4-A). Mirrors Python snake_case JSON. */

export type CandidateStatus =
  | "ACCEPTED"
  | "ACCEPTED_TRUNCATED"
  | "REJECTED"
  | "DEDUPLICATED";

export type ContextLane =
  | "CURRENT"
  | "RECENT_TRANSCRIPT"
  | "WORKING_MEMORY"
  | "RETRIEVED_EVIDENCE";

export type SourceType =
  | "CURRENT_QUERY"
  | "TRANSCRIPT_TURN"
  | "MEMORY_ATOM"
  | "KNOWLEDGE_DOCUMENT";

export type ReasonCode =
  | "ACCEPTED_REQUIRED_CURRENT"
  | "ACCEPTED_WITHIN_BUDGET"
  | "ACCEPTED_TRUNCATED_TO_BUDGET"
  | "REJECTED_BUDGET_LIMIT"
  | "REJECTED_TOTAL_BUDGET"
  | "REJECTED_LOW_PRIORITY"
  | "REJECTED_INELIGIBLE_SOURCE"
  | "REJECTED_EMPTY"
  | "REJECTED_SUPERSEDED"
  | "REJECTED_PRIVACY_POLICY"
  | "DEDUPLICATED_DOCUMENT_ID"
  | "DEDUPLICATED_CONTENT_HASH"
  | "DEDUPLICATED_HIGHER_LANE";

export interface RetrievalCandidateV1 {
  candidate_id: string;
  document_id: string;
  content_hash: string;
  source_type: SourceType;
  lane: ContextLane;
  char_count: number;
  included_chars: number;
  status: CandidateStatus;
  reason_code: ReasonCode;
  selection_rank: number | null;
  source_index: number | null;
  speaker_alias: string | null;
  memory_kind: string | null;
}

/** budget_chars = candidate content budget excluding section headers and separators. */
export interface LaneUsageV1 {
  lane: ContextLane;
  budget_chars: number;
  used_chars: number;
  formatting_chars: number;
  accepted_count: number;
  rejected_count: number;
  deduplicated_count: number;
}

export interface RetrievalManifestV1 {
  schema: "retrieval_manifest.v1";
  manifest_id: string;
  session_id: string;
  transcript_version: number;
  query_hash: string;
  context_hash: string;
  policy_version: string;
  prompt_version: string;
  model_hash: string;
  total_budget_chars: number;
  used_chars: number;
  formatting_overhead_chars: number;
  candidates: RetrievalCandidateV1[];
  lane_usage: LaneUsageV1[];
}

export type ContextManifestResponseV1 =
  | {
      manifest: RetrievalManifestV1;
      reason: null;
    }
  | {
      manifest: null;
      reason: "NO_MANIFEST";
    };
