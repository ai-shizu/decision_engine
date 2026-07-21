interface RagChatInputProps {
  value: string;
  onChange: (value: string) => void;
  onSend: () => void;
  /** Streaming in progress — Send is idle but textarea stays editable (ambient UX). */
  streaming: boolean;
  modelReady: boolean;
  /** M20-C: messenger sticky composer; desktop keeps default when omitted. */
  variant?: "default" | "messenger";
}

export function RagChatInput({
  value,
  onChange,
  onSend,
  streaming,
  modelReady,
  variant = "default",
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
              if (!streaming && modelReady) onSend();
            }
          }}
          placeholder={
            modelReady ? "メッセージ…" : "モデル準備中…すぐ送れます"
          }
          rows={1}
          aria-label="チャット入力"
        />
        <button
          type="button"
          className="rag-composer-send"
          onClick={onSend}
          disabled={streaming || !modelReady || !value.trim()}
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
            if (!streaming && modelReady) onSend();
          }
        }}
        placeholder={
          modelReady
            ? "知識ベースに質問…"
            : "モデル準備中…完了後すぐ送れます"
        }
        rows={2}
        aria-label="RAG chat input"
      />
      <button
        type="button"
        className="rag-chat-input-send"
        onClick={onSend}
        disabled={streaming || !modelReady || !value.trim()}
      >
        {streaming ? "…" : "SEND"}
      </button>
    </div>
  );
}
