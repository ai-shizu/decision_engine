import { useEffect, useRef } from "react";
import type { RagChatMessage } from "../../lib/ragChatReducer";
import { SimpleMarkdown } from "./SimpleMarkdown";

export type { RagChatMessage };

export interface RagMessageListProps {
  messages: RagChatMessage[];
  /** M20-C messenger layout — sticky chat column; desktop layout unchanged when omitted. */
  variant?: "default" | "messenger";
}

export function RagMessageList({
  messages,
  variant = "default",
}: RagMessageListProps) {
  const endRef = useRef<HTMLLIElement>(null);
  const messenger = variant === "messenger";

  useEffect(() => {
    if (!messenger) return;
    endRef.current?.scrollIntoView({ block: "end", behavior: "smooth" });
  }, [messages, messenger]);

  if (!messenger) {
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
                {m.streaming ? " · …" : null}
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

  return (
    <ul className="rag-message-list rag-message-list-messenger" aria-live="polite">
      {messages.length === 0 ? (
        <li className="rag-message-empty">知識ベースに質問してみてください</li>
      ) : null}
      {messages.map((m) => {
        const isUser = m.role === "user";
        return (
          <li
            key={m.id}
            className={
              isUser ? "rag-bubble rag-bubble-user" : "rag-bubble rag-bubble-assistant"
            }
          >
            <div className="rag-bubble-meta">
              {isUser ? "You" : "RAG"}
              {m.streaming ? " · …" : null}
              {m.contextCount != null && m.contextCount > 0
                ? ` · ctx ${m.contextCount}`
                : null}
            </div>
            <div className="rag-bubble-body">
              {isUser ? m.text : <SimpleMarkdown text={m.text || "…"} />}
            </div>
          </li>
        );
      })}
      <li ref={endRef} className="rag-message-anchor" aria-hidden />
    </ul>
  );
}
