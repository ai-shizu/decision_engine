// M11 RAG frontend client — thin re-export over Pocket Brain API (M18).

export type {
  IngestKnowledgeResult,
  RagChatParams,
  SearchKnowledgeHit,
  SearchKnowledgeResult,
  SendRagChatResult,
  TokenEvent,
} from "./pocketBrain/types";

export {
  ingestKnowledge,
  searchKnowledge,
  sendRagChat,
  syncDailyContext,
} from "./pocketBrain/api";
