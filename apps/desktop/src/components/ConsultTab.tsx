import { useEffect, useReducer, useRef, useState } from "react";
import {
  getKnowledgeResearchPolicy,
  knowledgeResearch,
} from "../lib/engine";
import { cancelGeneration } from "../lib/llm";
import {
  calculateInteractionPulse,
  consultWithOracleContext,
} from "../lib/pocketBrain";
import type { RomanceAnalysisV1 } from "../lib/pocketBrain/types";
import {
  deriveProvenance,
  INITIAL_RESEARCH_UI_STATE,
  isResearching,
  provenanceChipText,
  reduceResearchUi,
} from "../lib/researchUiReducer";
import type { ChatMessage } from "../lib/types";
import { uiErrorMessage } from "../lib/uiErrorMessages";
import { useThrottledStream } from "../lib/useThrottledStream";
import { RomanceAnalysisPanel } from "./RomanceAnalysisPanel";

type ConsultMode = "consult" | "romance_analysis";

const CONSULT_MODES = ["consult", "romance_analysis"] as const;

function isConsultMode(value: string): value is ConsultMode {
  return (CONSULT_MODES as readonly string[]).includes(value);
}

const ROMANCE_SUCCESS_MESSAGE = "会話履歴を解析しました";
const ROMANCE_PARSING_STATUS = "交流パルスを解析中…";

// F-11: タブはアンマウントされるが、会話セッションはモジュールに残し
// 再入場時の「白紙＋再ロード感」を消す (ImportTab の importLog と同型)。
let consultSessionMessages: ChatMessage[] = [];
let consultSessionMode: ConsultMode = "consult";

function stripRomanceSuccessMessages(messages: ChatMessage[]): ChatMessage[] {
  return messages.filter(
    (m) => !(m.role === "assistant" && m.text === ROMANCE_SUCCESS_MESSAGE),
  );
}

/** W6: never persist a forever-streaming ghost in the module singleton. */
function freezeStreamingMessages(messages: ChatMessage[]): ChatMessage[] {
  const out: ChatMessage[] = [];
  for (const m of messages) {
    if (!m.streaming) {
      out.push(m);
      continue;
    }
    if (m.role === "assistant" && !m.text.trim()) {
      continue;
    }
    out.push({ ...m, streaming: false });
  }
  return out;
}

function persistConsultSession(messages: ChatMessage[]): void {
  consultSessionMessages = freezeStreamingMessages(messages);
}

export function ConsultTab() {
  const [messages, setMessages] = useState<ChatMessage[]>(consultSessionMessages);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");
  const [statusKind, setStatusKind] = useState<"info" | "error">("info");
  const [confirmingClear, setConfirmingClear] = useState(false);
  const [mode, setMode] = useState<ConsultMode>(consultSessionMode);
  const [romanceResult, setRomanceResult] = useState<RomanceAnalysisV1 | null>(null);
  const [researchUi, dispatchResearchUi] = useReducer(
    reduceResearchUi,
    INITIAL_RESEARCH_UI_STATE,
  );
  const researchSeqRef = useRef(0);
  const logRef = useRef<HTMLDivElement>(null);
  const stickRef = useRef(true);
  const streamingRef = useRef(false);

  useEffect(() => {
    // Live UI may show streaming; singleton must never keep streaming:true across remount.
    persistConsultSession(messages);
  }, [messages]);

  useEffect(() => {
    consultSessionMode = mode;
  }, [mode]);

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

  // W4/W6: cancel in-flight LLM on unmount; freeze singleton streaming ghosts.
  useEffect(() => {
    return () => {
      flushChunkQueue();
      if (streamingRef.current) {
        void cancelGeneration().catch(() => {
          /* best-effort */
        });
      }
      persistConsultSession(consultSessionMessages);
      streamingRef.current = false;
    };
  }, [flushChunkQueue]);

  function handleModeChange(next: ConsultMode) {
    setMode(next);
    setRomanceResult(null);
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    const q = input.trim();
    if (!q || busy) return;
    setInput("");
    flushChunkQueue();
    stickRef.current = true;

    if (mode === "romance_analysis") {
      setRomanceResult(null);
      setMessages((prev) => stripRomanceSuccessMessages(prev));
      setBusy(true);
      setStatusKind("info");
      setStatus(ROMANCE_PARSING_STATUS);
      try {
        const res = await calculateInteractionPulse(q);
        setRomanceResult(res.analysis);
        setMessages((prev) => [
          ...prev,
          { role: "assistant", text: ROMANCE_SUCCESS_MESSAGE },
        ]);
        setStatusKind("info");
        setStatus("");
      } catch {
        setRomanceResult(null);
        setMessages((prev) => stripRomanceSuccessMessages(prev));
        setStatusKind("error");
        setStatus(uiErrorMessage("ROMANCE_ANALYSIS"));
      } finally {
        setBusy(false);
        if (stickRef.current) scrollToBottom(true);
      }
      return;
    }

    setMessages((prev) => [
      ...prev,
      { role: "user", text: q },
      { role: "assistant", text: "", streaming: true },
    ]);
    scrollToBottom(true);

    let provenanceLabel: string | undefined;
    try {
      const policy = await getKnowledgeResearchPolicy();
      if (policy.enabled) {
        const seq = researchSeqRef.current + 1;
        researchSeqRef.current = seq;
        dispatchResearchUi({ kind: "START", seq });
        try {
          const receipt = await knowledgeResearch(q);
          dispatchResearchUi({ kind: "DONE", seq });
          const provenance = deriveProvenance(receipt);
          if (provenance) {
            provenanceLabel = provenanceChipText(provenance);
          }
        } catch {
          dispatchResearchUi({ kind: "FAIL", seq });
        }
      }
    } catch {
      // Policy read failure: proceed with consult only (no external lane).
    }

    setBusy(true);
    streamingRef.current = true;
    setStatusKind("info");
    setStatus("考え中…");
    const sterile = uiErrorMessage("CONSULT_RESPONSE");
    try {
      await consultWithOracleContext(
        { message: q, includeRag: true },
        (event) => {
          if (event.error) {
            flushChunkQueue();
            setStatusKind("error");
            setStatus(sterile);
            return;
          }
          if (event.done) {
            flushChunkQueue();
            return;
          }
          if (event.text) {
            pushChunk(event.text);
          }
        },
      );
      flushChunkQueue();
      setMessages((prev) => {
        const last = prev[prev.length - 1];
        if (last?.role === "assistant" && last.streaming) {
          return [
            ...prev.slice(0, -1),
            {
              role: "assistant",
              text: last.text,
              streaming: false,
              provenanceLabel,
            },
          ];
        }
        return prev;
      });
      setStatusKind("info");
      setStatus("");
    } catch {
      flushChunkQueue();
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
      setStatusKind("error");
      setStatus(sterile);
    } finally {
      streamingRef.current = false;
      setBusy(false);
      // W6: belt-and-suspenders — freeze singleton even if setState is dropped on unmount.
      persistConsultSession(consultSessionMessages);
      if (stickRef.current) scrollToBottom(true);
    }
  }

  function handleClearClick() {
    if (!confirmingClear) {
      setConfirmingClear(true);
      return;
    }
    setMessages([]);
    consultSessionMessages = [];
    setStatus("");
    setRomanceResult(null);
    setConfirmingClear(false);
  }

  const isRomance = mode === "romance_analysis";

  return (
    <section className="panel consult-panel">
      <div className="consult-header">
        <h2>
          CONSULT <span className="term-tag term-tag--info">[ OFFLINE ]</span>
        </h2>
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
      <div className="ascii-sep ascii-sep--info" role="separator">
        --- SESSION ---
      </div>
      <p className="hint guide">記録・プロファイルに基づくオフライン相談。会話はこのセッション内のみ保持されます。</p>

      <div className="consult-mode-row">
        <label htmlFor="consult-mode-select">モード</label>
        <select
          id="consult-mode-select"
          value={mode}
          onChange={(e) => {
            const next = e.target.value;
            if (isConsultMode(next)) handleModeChange(next);
          }}
          disabled={busy}
        >
          <option value="consult">通常相談</option>
          <option value="romance_analysis">Romance（交流パルス解析）</option>
        </select>
      </div>

      <div className="chat-log" ref={logRef} onScroll={handleLogScroll}>
        {messages.length === 0 ? (
          <p className="hint chat-empty">質問を入力して送信してください。</p>
        ) : (
          messages.map((m, i) => (
            <div key={i} className={`chat-bubble ${m.role}`}>
              <span className="chat-role">{m.role === "user" ? "あなた" : "Coraxis"}</span>
              <pre className={`chat-text${m.streaming ? " streaming" : ""}`}>
                {m.text}
                {m.streaming && <span className="chat-cursor">▌</span>}
              </pre>
              {m.provenanceLabel && (
                <span className="provenance-chip">{m.provenanceLabel}</span>
              )}
            </div>
          ))
        )}
      </div>

      {isRomance && <RomanceAnalysisPanel result={romanceResult} />}

      <form
        className={`consult-form${isResearching(researchUi) ? " researching-ambient" : ""}`}
        onSubmit={(e) => void handleSubmit(e)}
        aria-busy={isResearching(researchUi)}
      >
        {isResearching(researchUi) && (
          <p className="consult-research-ambient" role="status">
            <span className="consult-research-spinner" aria-hidden="true" />
            外部知識を補強中…
          </p>
        )}
        <textarea
          value={input}
          onChange={(e) => setInput(e.target.value)}
          placeholder={
            isRomance
              ? "[self] 自分の発言\n[contact_alias] 相手の発言\n…（最大12,000文字）"
              : "相談内容を入力…"
          }
          rows={isRomance ? 6 : 3}
          maxLength={isRomance ? 12000 : undefined}
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
      {status && (
        <p
          className={`status-line${statusKind === "error" ? " error-text" : ""}`}
          role={statusKind === "error" ? "alert" : "status"}
        >
          {status}
        </p>
      )}
    </section>
  );
}
