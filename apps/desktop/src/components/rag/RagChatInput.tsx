interface RagChatInputProps {
  value: string;
  onChange: (value: string) => void;
  onSend: () => void;
  /** Streaming in progress — Send is idle but textarea stays editable (ambient UX). */
  streaming: boolean;
  modelReady: boolean;
}

export function RagChatInput({
  value,
  onChange,
  onSend,
  streaming,
  modelReady,
}: RagChatInputProps) {
  return (
    <div
      className="rag-chat-input"
      style={{ display: "flex", gap: 8, padding: 12, alignItems: "flex-end" }}
    >
      <textarea
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
            : "先にモデルをロードしてください"
        }
        rows={2}
        style={{ flex: 1, resize: "vertical", minHeight: 48 }}
      />
      <button
        type="button"
        onClick={onSend}
        disabled={streaming || !modelReady || !value.trim()}
      >
        {streaming ? "…" : "Send"}
      </button>
    </div>
  );
}
