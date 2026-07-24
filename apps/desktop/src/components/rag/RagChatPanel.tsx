import { useEffect, useReducer, useRef } from "react";

import { cancelGeneration } from "../../lib/llm";
import { sendRagChat } from "../../lib/pocketBrain";
import {
  initialRagChatState,
  ragChatReducer,
  type RagChatState,
} from "../../lib/ragChatReducer";
import { uiErrorMessage } from "../../lib/uiErrorMessages";
import { useThrottledStream } from "../../lib/useThrottledStream";
import { createStreamTerminalGate } from "../../lib/streamTerminalGate";
import { RagChatInput } from "./RagChatInput";
import { RagIngestPanel } from "./RagIngestPanel";
import { RagMessageList } from "./RagMessageList";

interface RagChatPanelProps {
  modelReady: boolean;
  onError: (message: string | null) => void;
  onBusyChange: (busy: boolean) => void;
  /** M20-C messenger: sticky composer + bubbles; ingest moved to action sheet. */
  variant?: "default" | "messenger";
}

let nextMsgId = 1;
function allocId(prefix: string): string {
  nextMsgId += 1;
  return `${prefix}-${nextMsgId}`;
}

// N2: tab remount must restore history (ConsultTab / F-11 pattern).
let ragSessionState: RagChatState = initialRagChatState();

/** Controlled sys-log line for a locked vault (matches backend ERR_VAULT_LOCKED). */
const SYS_ERR_VAULT_LOCKED =
  "> SYS_ERR :: [VAULT_LOCKED] 保管庫へのアクセスが拒否されました。ロックを解除してください。";
const STREAM_TERMINAL_TIMEOUT_MS = 180_000;

/**
 * Map a controlled backend error code → sterile sys-log line. Finding 13: never
 * render a raw IPC/embedding error body; only known codes get a specific message,
 * everything else falls back to the operation-keyed sterile string.
 */
function mapRagChatError(code: string): string {
  if (code === "VAULT_LOCKED") return SYS_ERR_VAULT_LOCKED;
  if (code === "MODEL_NOT_LOADED") return uiErrorMessage("RAG_MODEL_NOT_LOADED");
  return uiErrorMessage("RAG_CHAT");
}

/** Never persist a forever-streaming ghost across remount (W6 / N2). */
function freezeRagSession(state: RagChatState): RagChatState {
  const messages = state.messages.flatMap((m) => {
    if (!m.streaming) return [m];
    if (!m.text.trim()) return [];
    return [{ ...m, streaming: false }];
  });
  return {
    ...state,
    streaming: false,
    messages,
  };
}

function persistRagSession(state: RagChatState): void {
  ragSessionState = freezeRagSession(state);
}

/**
 * RAG chat surface: pure-reducer timeline + Channel streaming via pocketBrain API.
 * Cancellation / memory purge remain owned by the parent PocketBrainPanel.
 * Finding 13: never pass raw IPC / embedding errors into UI state.
 */
export function RagChatPanel({
  modelReady,
  onError,
  onBusyChange,
  variant = "default",
}: RagChatPanelProps) {
  const [state, dispatch] = useReducer(
    ragChatReducer,
    undefined,
    () => ragSessionState,
  );
  const stateRef = useRef(state);
  const assistantIdRef = useRef<string | null>(null);
  const streamingRef = useRef(false);
  const messenger = variant === "messenger";

  const { push: pushChunk, drainAndStop, flushAndStop } =
    useThrottledStream((chunk) => {
      const id = assistantIdRef.current;
      if (!id) return;
      dispatch({ type: "token", assistantId: id, text: chunk });
    });

  useEffect(() => {
    stateRef.current = state;
    persistRagSession(state);
  }, [state]);

  useEffect(() => {
    streamingRef.current = state.streaming;
  }, [state.streaming]);

  // W4/N2: unmount cancels LLM; singleton keeps frozen history.
  useEffect(() => {
    return () => {
      flushAndStop();
      if (streamingRef.current) {
        void cancelGeneration().catch(() => {
          /* best-effort */
        });
      }
      persistRagSession(stateRef.current);
    };
  }, [flushAndStop]);

  async function onSend() {
    const prompt = state.input.trim();
    if (!prompt || state.streaming) return;
    // modelReady=false (Jetsam / cold) — still send; Rust auto-loads GGUF.

    onError(null);
    dispatch({ type: "clear_error" });
    onBusyChange(true);

    const userId = allocId("u");
    const assistantId = allocId("a");
    assistantIdRef.current = assistantId;
    dispatch({ type: "send_begin", userId, assistantId, prompt });

    let errored = false;
    const terminal = createStreamTerminalGate(STREAM_TERMINAL_TIMEOUT_MS);

    try {
      const result = await sendRagChat(
        prompt,
        (event) => {
          if (!terminal.isPending()) return;
          if (event.error) {
            // Lessons-learned rule (2026-07-24 context-budget hunt): the RAW
            // stream error code is mapped to a sterile in-bubble sys-log line
            // for the user (Finding 13), but the raw payload MUST be logged
            // first. A backend error as precise as "prompt exceeds context
            // budget: 2394 > 1792" was collapsed to the generic RAG_CHAT
            // message here, hiding the true cause for a long debugging session.
            // eslint-disable-next-line no-console -- intentional diagnostic (see above)
            console.error("[RagChatPanel] stream error event:", event.error);
            errored = true;
            flushAndStop();
            dispatch({
              type: "token_error",
              assistantId,
              message: mapRagChatError(event.error),
            });
            terminal.settle();
            return;
          }
          if (event.done) {
            drainAndStop();
            terminal.settle();
            return;
          }
          if (event.text) {
            pushChunk(event.text);
          }
        },
        { contextLimit: 5 },
      );
      await terminal.promise;

      drainAndStop();
      if (!errored) {
        dispatch({
          type: "send_success",
          assistantId,
          contextCount: result.context_count,
        });
      }
    } catch (err) {
      // Lessons-learned rule: log the raw exception before the sterile UI
      // message, so a silent-failure never again masks the real cause.
      // eslint-disable-next-line no-console -- intentional diagnostic (see above)
      console.error("[RagChatPanel] onSend failed:", err);
      flushAndStop();
      dispatch({
        type: "send_failure",
        assistantId,
        message: uiErrorMessage("RAG_CHAT"),
      });
    } finally {
      terminal.abort();
      assistantIdRef.current = null;
      dispatch({ type: "send_end" });
      onBusyChange(false);
      persistRagSession(stateRef.current);
    }
  }

  return (
    <div
      className={
        messenger ? "rag-chat-panel rag-chat-panel-messenger" : "rag-chat-panel"
      }
    >
      <RagMessageList messages={state.messages} variant={variant} />
      <RagChatInput
        value={state.input}
        onChange={(value) => dispatch({ type: "set_input", value })}
        onSend={() => void onSend()}
        streaming={state.streaming}
        modelReady={modelReady}
        variant={variant}
      />
      {!messenger ? <RagIngestPanel modelReady={modelReady} /> : null}
      {state.error ? (
        <p className="rag-chat-error error-text" role="alert">
          {state.error}
        </p>
      ) : null}
    </div>
  );
}
