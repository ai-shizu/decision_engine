import { memo, useEffect, useRef } from "react";
import type { RagChatMessage } from "../../lib/ragChatReducer";
import { SimpleMarkdown } from "./SimpleMarkdown";

export type { RagChatMessage };

export interface RagMessageListProps {
  messages: RagChatMessage[];
  /** M20-C messenger layout — sticky chat column; desktop layout unchanged when omitted. */
  variant?: "default" | "messenger";
}

function RagBubble({ message }: { message: RagChatMessage }) {
  const isUser = message.role === "user";
  return (
    <li
      className={
        isUser ? "rag-bubble rag-bubble-user" : "rag-bubble rag-bubble-assistant"
      }
    >
      <div className="rag-bubble-meta">
        {isUser ? "あなた" : "AI"}
        {message.streaming ? " · …" : null}
        {message.contextCount != null && message.contextCount > 0
          ? ` · ctx ${message.contextCount}`
          : null}
      </div>
      <div className="rag-bubble-body">
        {isUser ? (
          message.text
        ) : message.error ? (
          <div
            className="sys-log sys-log--err rag-bubble-syserr"
            role="alert"
          >
            {message.error}
          </div>
        ) : message.streaming ? (
          // Parsing the growing document on every token is quadratic. Preserve
          // the exact markdown container/style while streaming plain text, then
          // parse once when `streaming` flips false.
          <div className="rag-md">
            <p className="rag-md-line" style={{ whiteSpace: "pre-wrap" }}>
              {message.text || "…"}
            </p>
          </div>
        ) : (
          <SimpleMarkdown text={message.text || "…"} />
        )}
      </div>
    </li>
  );
}

const MemoRagBubble = memo(
  RagBubble,
  (previous, next) =>
    previous.message.id === next.message.id &&
    previous.message.role === next.message.role &&
    previous.message.text === next.message.text &&
    previous.message.streaming === next.message.streaming &&
    previous.message.contextCount === next.message.contextCount &&
    previous.message.error === next.message.error,
);

export const RagMessageList = memo(function RagMessageList({
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
      {messages.map((message) => (
        <MemoRagBubble key={message.id} message={message} />
      ))}
      {messenger ? (
        <li ref={endRef} className="rag-message-anchor" aria-hidden />
      ) : null}
    </ul>
  );
});
