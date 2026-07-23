interface RagChatInputProps {
  value: string;
  onChange: (value: string) => void;
  onSend: () => void;
  /** Streaming in progress — Send is idle but textarea stays editable (ambient UX). */
  streaming: boolean;
  modelReady: boolean;
  /** M20-C: messenger sticky composer; desktop keeps default when omitted. */
  variant?: "default" | "messenger";
  /** Override the default RAG-specific placeholder pair (ready / not-ready). */
  placeholder?: { ready: string; notReady: string };
}

export function RagChatInput({
  value,
  onChange,
  onSend,
  streaming,
  modelReady,
  variant = "default",
  placeholder,
}: RagChatInputProps) {
  if (variant === "messenger") {
    return (
      <div className="rag-composer">
        <textarea
          className="rag-composer-input"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              if (!streaming && value.trim()) onSend();
            }
          }}
          placeholder={
            modelReady
              ? (placeholder?.ready ?? "メッセージ…")
              : (placeholder?.notReady ?? "モデル準備中でも送信可（自動ロード）")
          }
          rows={1}
          aria-label="チャット入力"
        />
        <button
          type="button"
          className="rag-composer-send"
          onClick={onSend}
          disabled={streaming || !value.trim()}
        >
          {streaming ? "…" : "送信"}
        </button>
      </div>
    );
  }

  return (
    <div className="rag-chat-input">
      <textarea
        className="rag-chat-input-field"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            if (!streaming && value.trim()) onSend();
          }
        }}
        placeholder={
          modelReady
            ? (placeholder?.ready ?? "知識ベースに質問…")
            : (placeholder?.notReady ?? "モデル準備中でも送信可（自動ロード）")
        }
        rows={2}
        aria-label="RAG chat input"
      />
      <button
        type="button"
        className="rag-chat-input-send"
        onClick={onSend}
        disabled={streaming || !value.trim()}
      >
        {streaming ? "…" : "SEND"}
      </button>
    </div>
  );
}
