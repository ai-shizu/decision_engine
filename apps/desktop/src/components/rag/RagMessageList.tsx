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

  return (
    <ul
      className={
        messenger
          ? "rag-message-list rag-message-list-messenger"
          : "rag-message-list rag-message-list-desktop"
      }
      aria-live="polite"
    >
      {messages.length === 0 ? (
        <li className="rag-message-empty">
          {messenger ? "知識ベースに質問してみてください" : "(no messages)"}
        </li>
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
      {messenger ? (
        <li ref={endRef} className="rag-message-anchor" aria-hidden />
      ) : null}
    </ul>
  );
}
