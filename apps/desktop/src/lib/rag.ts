// M11 RAG frontend client — thin re-export over Coraxis on-device API (M18).

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
} from "./pocketBrain/api";
