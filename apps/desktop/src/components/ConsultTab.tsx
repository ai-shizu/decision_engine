import { useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { consult } from "../lib/engine";
import type { ChatMessage, EngineEvent } from "../lib/types";
import { useThrottledStream } from "../lib/useThrottledStream";

export function ConsultTab() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");
  const [confirmingClear, setConfirmingClear] = useState(false);
  const logRef = useRef<HTMLDivElement>(null);
  // F3 (SPEC_FOXTROT_UI.md §2.3.1 裁定1): pkb-engine-event はコマンド非依存の
  // グローバルバス。自分の consult が in-flight の間だけ status を反映する
  // (W-34 の是正 — import の status 混線を防ぐ)。
  const busyRef = useRef(false);
  // 裁定2: 最下端追従の可否は ref で持つ (state にすると再レンダリングの嵐)。
  const stickRef = useRef(true);

  useEffect(() => {
    busyRef.current = busy;
  }, [busy]);

  function scrollToBottom(smooth = false) {
    requestAnimationFrame(() => {
      logRef.current?.scrollTo({
        top: logRef.current.scrollHeight,
        behavior: smooth ? "smooth" : "auto",
      });
    });
  }

  function handleLogScroll() {
    const el = logRef.current;
    if (!el) return;
    stickRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  }

  // F3.5 (SPEC_FOXTROT_UI.md §7 裁定1): chunk は一定速でキューから放出する。
  // 確定置換 (handleSubmit 側) は必ず flushAndStop() でキューを破棄してから
  // 行う (W-35)。放出コールバック自体は既存の stickRef 追従規律をそのまま踏襲。
  const { push: pushChunk, flushAndStop: flushChunkQueue } = useThrottledStream(
    (piece) => {
      setMessages((prev) => {
        const last = prev[prev.length - 1];
        if (!last || last.role !== "assistant" || !last.streaming) return prev;
        return [...prev.slice(0, -1), { ...last, text: last.text + piece }];
      });
      if (stickRef.current) scrollToBottom(false);
    },
  );

  // Python エンジンの中間イベント (進捗 status / 生成トークン chunk) を受信する。
  // W-22/W-23 の disposed フラグ標準形: cleanup が listen() の resolve より
  // 先に走っても (StrictMode の二重実行等)、購読は確実に解除される。
  useEffect(() => {
    let disposed = false;
    let unlistenFn: UnlistenFn | null = null;

    const handler = ({ payload }: { payload: EngineEvent }) => {
      if (payload.event === "status" && payload.message) {
        if (!busyRef.current) return; // W-34: 他コマンドの status を無視する
        setStatus(payload.message);
        return;
      }
      if (payload.event === "chunk" && payload.text) {
        pushChunk(payload.text); // スロットルキューへ積むだけ (放出はタイマー駆動)
      }
    };

    void listen<EngineEvent>("pkb-engine-event", handler).then((fn) => {
      if (disposed) {
        fn();
        return;
      }
      unlistenFn = fn;
    });

    return () => {
      disposed = true;
      unlistenFn?.();
    };
  }, []);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    const q = input.trim();
    if (!q || busy) return;
    setInput("");
    flushChunkQueue(); // 新規送信時に前回セッションの残留キューを破棄する
    setMessages((prev) => [
      ...prev,
      { role: "user", text: q },
      { role: "assistant", text: "", streaming: true },
    ]);
    setBusy(true);
    busyRef.current = true;
    setStatus("考え中…");
    stickRef.current = true;
    scrollToBottom(true);
    try {
      const res = await consult(q);
      // W-35: 確定置換は必ず「キュー破棄 → 置換」の順で原子的に行う。
      // 順序が逆だと、破棄前に残っていたキューが置換後のメッセージへ
      // 追記され続けてしまう。
      flushChunkQueue();
      setMessages((prev) => {
        const last = prev[prev.length - 1];
        if (last?.role === "assistant" && last.streaming) {
          return [...prev.slice(0, -1), { role: "assistant", text: res.answer }];
        }
        return [...prev, { role: "assistant", text: res.answer }];
      });
      setStatus("");
    } catch (err) {
      flushChunkQueue(); // W-35: エラー経路でも確定 (削除/凍結) 前にキューを破棄する
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
      busyRef.current = false;
      if (stickRef.current) scrollToBottom(true);
    }
  }

  // F-7: セッション内会話は復元不能な破壊対象 — インライン2段クリックで確定する。
  function handleClearClick() {
    if (!confirmingClear) {
      setConfirmingClear(true);
      return;
    }
    setMessages([]);
    setStatus("");
    setConfirmingClear(false);
  }

  return (
    <section className="panel consult-panel">
      <div className="consult-header">
        <h2>AI 相談 (CONSULT)</h2>
        <button
          type="button"
          className="ghost"
          onClick={handleClearClick}
          onBlur={() => setConfirmingClear(false)}
          disabled={busy}
        >
          {confirmingClear ? "本当にクリア" : "履歴クリア"}
        </button>
      </div>
      <p className="hint">記録・プロファイルに基づくオフライン相談。会話はこのセッション内のみ保持されます。</p>

      <div className="chat-log" ref={logRef} onScroll={handleLogScroll}>
        {messages.length === 0 ? (
          <p className="hint chat-empty">質問を入力して送信してください。</p>
        ) : (
          messages.map((m, i) => (
            <div key={i} className={`chat-bubble ${m.role}`}>
              <span className="chat-role">{m.role === "user" ? "あなた" : "PKB"}</span>
              <pre className={`chat-text${m.streaming ? " streaming" : ""}`}>
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
