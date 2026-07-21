import { useEffect, useReducer, useRef } from "react";

import { cancelGeneration } from "../../lib/llm";
import { sendRagChat } from "../../lib/pocketBrain";
import {
  initialRagChatState,
  ragChatReducer,
} from "../../lib/ragChatReducer";
import { uiErrorMessage } from "../../lib/uiErrorMessages";
import { useThrottledStream } from "../../lib/useThrottledStream";
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
  const [state, dispatch] = useReducer(ragChatReducer, undefined, initialRagChatState);
  const assistantIdRef = useRef<string | null>(null);
  const streamingRef = useRef(false);
  const messenger = variant === "messenger";

  const { push: pushChunk, flushAndStop } = useThrottledStream((chunk) => {
    const id = assistantIdRef.current;
    if (!id) return;
    dispatch({ type: "token", assistantId: id, text: chunk });
  });

  useEffect(() => {
    streamingRef.current = state.streaming;
  }, [state.streaming]);

  // W4: unmount while generating must cancel the LLM worker.
  useEffect(() => {
    return () => {
      flushAndStop();
      if (streamingRef.current) {
        void cancelGeneration().catch(() => {
          /* best-effort */
        });
      }
    };
  }, [flushAndStop]);

  async function onSend() {
    const prompt = state.input.trim();
    if (!prompt || state.streaming || !modelReady) return;

    onError(null);
    dispatch({ type: "clear_error" });
    onBusyChange(true);

    const userId = allocId("u");
    const assistantId = allocId("a");
    assistantIdRef.current = assistantId;
    dispatch({ type: "send_begin", userId, assistantId, prompt });

    const sterile = uiErrorMessage("RAG_CHAT");

    try {
      const result = await sendRagChat(
        prompt,
        (event) => {
          if (event.error) {
            // Ignore event.error body — operation-keyed sterile message only.
            dispatch({ type: "token_error", message: sterile });
            onError(sterile);
            return;
          }
          if (event.done) {
            flushAndStop();
            return;
          }
          if (event.text) {
            pushChunk(event.text);
          }
        },
        { contextLimit: 5 },
      );

      flushAndStop();
      dispatch({
        type: "send_success",
        assistantId,
        contextCount: result.context_count,
      });
    } catch {
      flushAndStop();
      dispatch({ type: "send_failure", message: sterile });
      onError(sterile);
    } finally {
      assistantIdRef.current = null;
      dispatch({ type: "send_end" });
      onBusyChange(false);
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
