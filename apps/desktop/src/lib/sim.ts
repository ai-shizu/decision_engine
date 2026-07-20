// M12 interview / ES simulation client stubs (UI wiring prep).
// Streams reuse the same TokenEvent Channel contract as llm_generate / sendRagChat.

import { Channel, invoke } from "@tauri-apps/api/core";

import type { TokenEvent } from "./llm";

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

function genPayload(gen?: SimGenParams) {
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

/** Dual-gated EDINET list fetch → CompanyFacts (fails closed without Live∧egress-live). */
export function fetchEdinetCompanyFacts(args: {
  edinetCode: string;
  edinetDate: string;
  filingText?: string;
}): Promise<CompanyFacts> {
  return invoke("fetch_edinet_company_facts", {
    params: {
      edinetCode: args.edinetCode,
      edinetDate: args.edinetDate,
      filingText: args.filingText ?? null,
    },
  });
}

/** Streaming interview turn (persona + EDINET facts + RAG experience). */
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
  const channel = new Channel<TokenEvent>(onToken);
  return invoke("start_interview_session", {
    params: {
      message: args.message,
      companyFacts: args.companyFacts ?? null,
      edinetCode: args.edinetCode ?? null,
      edinetDate: args.edinetDate ?? null,
      filingText: args.filingText ?? null,
      gen: genPayload(args.gen),
    },
    onToken: channel,
  });
}

/** Streaming ES draft review. */
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
  const channel = new Channel<TokenEvent>(onToken);
  return invoke("review_es_draft", {
    params: {
      esDraft: args.esDraft,
      companyFacts: args.companyFacts ?? null,
      edinetCode: args.edinetCode ?? null,
      edinetDate: args.edinetDate ?? null,
      filingText: args.filingText ?? null,
      experienceQuery: args.experienceQuery ?? null,
      gen: genPayload(args.gen),
    },
    onToken: channel,
  });
}
