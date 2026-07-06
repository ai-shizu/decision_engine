import { useRef, useState } from "react";
import { consult } from "../lib/engine";
import type { ChatMessage } from "../lib/types";

export function ConsultTab() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");
  const logRef = useRef<HTMLDivElement>(null);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    const q = input.trim();
    if (!q || busy) return;
    setInput("");
    setMessages((prev) => [...prev, { role: "user", text: q }]);
    setBusy(true);
    setStatus("考え中…");
    try {
      const res = await consult(q);
      setMessages((prev) => [...prev, { role: "assistant", text: res.answer }]);
      setStatus("");
    } catch (err) {
      setStatus(String(err));
    } finally {
      setBusy(false);
      requestAnimationFrame(() => {
        logRef.current?.scrollTo({ top: logRef.current.scrollHeight, behavior: "smooth" });
      });
    }
  }

  function clearChat() {
    setMessages([]);
    setStatus("");
  }

  return (
    <section className="panel consult-panel">
      <div className="consult-header">
        <h2>AI 相談 (CONSULT)</h2>
        <button type="button" className="ghost" onClick={clearChat} disabled={busy}>
          履歴クリア
        </button>
      </div>
      <p className="hint">記録・プロファイルに基づくオフライン相談。会話はこのセッション内のみ保持されます。</p>

      <div className="chat-log" ref={logRef}>
        {messages.length === 0 ? (
          <p className="hint chat-empty">質問を入力して送信してください。</p>
        ) : (
          messages.map((m, i) => (
            <div key={i} className={`chat-bubble ${m.role}`}>
              <span className="chat-role">{m.role === "user" ? "あなた" : "PKB"}</span>
              <pre className="chat-text">{m.text}</pre>
            </div>
          ))
        )}
      </div>

      <form className="consult-form" onSubmit={(e) => void handleSubmit(e)}>
        <textarea
          value={input}
          onChange={(e) => setInput(e.target.value)}
          placeholder="相談内容を入力…"
          rows={3}
          disabled={busy}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
              void handleSubmit(e);
            }
          }}
        />
        <div className="action-row">
          <button type="submit" className="primary" disabled={busy || !input.trim()}>
            {busy ? "送信中…" : "送信 (Ctrl+Enter)"}
          </button>
        </div>
      </form>
      {status && <p className="status-line">{status}</p>}
    </section>
  );
}
