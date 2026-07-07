import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { consult } from "../lib/engine";
import type { ChatMessage, EngineEvent } from "../lib/types";

export function ConsultTab() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");
  const logRef = useRef<HTMLDivElement>(null);

  function scrollToBottom(smooth = false) {
    requestAnimationFrame(() => {
      logRef.current?.scrollTo({
        top: logRef.current.scrollHeight,
        behavior: smooth ? "smooth" : "auto",
      });
    });
  }

  // Python エンジンの中間イベント (進捗 status / 生成トークン chunk) を受信する
  useEffect(() => {
    const unlisten = listen<EngineEvent>("pkb-engine-event", ({ payload }) => {
      if (payload.event === "status" && payload.message) {
        setStatus(payload.message);
        return;
      }
      if (payload.event === "chunk" && payload.text) {
        setMessages((prev) => {
          const last = prev[prev.length - 1];
          if (!last || last.role !== "assistant" || !last.streaming) return prev;
          return [...prev.slice(0, -1), { ...last, text: last.text + payload.text }];
        });
        scrollToBottom();
      }
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    const q = input.trim();
    if (!q || busy) return;
    setInput("");
    setMessages((prev) => [
      ...prev,
      { role: "user", text: q },
      { role: "assistant", text: "", streaming: true },
    ]);
    setBusy(true);
    setStatus("考え中…");
    try {
      const res = await consult(q);
      // ストリーミング中の一時テキストを最終回答で確定置換する
      setMessages((prev) => {
        const last = prev[prev.length - 1];
        if (last?.role === "assistant" && last.streaming) {
          return [...prev.slice(0, -1), { role: "assistant", text: res.answer }];
        }
        return [...prev, { role: "assistant", text: res.answer }];
      });
      setStatus("");
    } catch (err) {
      // 空のプレースホルダーは取り除き、エラーは status 行に出す
      setMessages((prev) => {
        const last = prev[prev.length - 1];
        if (last?.role === "assistant" && last.streaming && !last.text) {
          return prev.slice(0, -1);
        }
        if (last?.role === "assistant" && last.streaming) {
          return [...prev.slice(0, -1), { ...last, streaming: false }];
        }
        return prev;
      });
      setStatus(String(err));
    } finally {
      setBusy(false);
      scrollToBottom(true);
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
              <pre className="chat-text">
                {m.text}
                {m.streaming && <span className="chat-cursor">▌</span>}
              </pre>
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
