import { SimpleMarkdown } from "./SimpleMarkdown";

export interface RagChatMessage {
  id: string;
  role: "user" | "assistant";
  text: string;
  contextCount?: number;
}

export function RagMessageList({ messages }: { messages: RagChatMessage[] }) {
  return (
    <ul
      className="rag-message-list"
      style={{
        listStyle: "none",
        margin: 0,
        padding: 12,
        display: "flex",
        flexDirection: "column",
        gap: 10,
        maxHeight: 320,
        overflowY: "auto",
      }}
    >
      {messages.map((m) => {
        const isUser = m.role === "user";
        return (
          <li
            key={m.id}
            style={{
              alignSelf: isUser ? "flex-end" : "flex-start",
              maxWidth: "88%",
              padding: "8px 12px",
              borderRadius: 12,
              background: isUser ? "#2a3340" : "#1a222c",
              border: "1px solid #333",
              whiteSpace: "pre-wrap",
            }}
          >
            <div
              style={{
                fontSize: 11,
                opacity: 0.65,
                marginBottom: 4,
              }}
            >
              {isUser ? "You" : "RAG"}
              {m.contextCount != null && m.contextCount > 0
                ? ` · ctx ${m.contextCount}`
                : null}
            </div>
            {isUser ? m.text : <SimpleMarkdown text={m.text || "…"} />}
          </li>
        );
      })}
    </ul>
  );
}
