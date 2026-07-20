// M11 RAG frontend client — ingest / search / streaming RAG chat.

import { Channel, invoke } from "@tauri-apps/api/core";

import type { TokenEvent } from "./llm";

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

/** Chunk → embed → vault replace for a source document. */
export function ingestKnowledge(
  text: string,
  sourceId: string,
): Promise<IngestKnowledgeResult> {
  return invoke("ingest_knowledge", { text, sourceId });
}

/** Embed query and run sqlite-vec KNN. */
export function searchKnowledge(
  query: string,
  limit?: number,
): Promise<SearchKnowledgeResult> {
  return invoke("search_knowledge", { query, limit: limit ?? null });
}

/**
 * Retrieve context, inject into the RAG prompt on the Rust side, then stream
 * tokens over the same Channel pipeline as `llm_generate`.
 */
export function sendRagChat(
  message: string,
  onToken: (event: TokenEvent) => void,
  params: RagChatParams = {},
): Promise<SendRagChatResult> {
  const channel = new Channel<TokenEvent>(onToken);
  return invoke("send_rag_chat", {
    message,
    params: {
      nCtx: params.nCtx ?? null,
      maxTokens: params.maxTokens ?? null,
      temp: params.temp ?? null,
      topK: params.topK ?? null,
      topP: params.topP ?? null,
      seed: params.seed ?? null,
      contextLimit: params.contextLimit ?? null,
    },
    onToken: channel,
  });
}
