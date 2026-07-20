import { useReducer, useRef } from "react";

import {
  isPocketBrainInvokeError,
  sendRagChat,
} from "../../lib/pocketBrain";
import {
  initialRagChatState,
  ragChatReducer,
} from "../../lib/ragChatReducer";
import { useThrottledStream } from "../../lib/useThrottledStream";
import { RagChatInput } from "./RagChatInput";
import { RagIngestPanel } from "./RagIngestPanel";
import { RagMessageList } from "./RagMessageList";

interface RagChatPanelProps {
  modelReady: boolean;
  onError: (message: string | null) => void;
  onBusyChange: (busy: boolean) => void;
}

let nextMsgId = 1;
function allocId(prefix: string): string {
  nextMsgId += 1;
  return `${prefix}-${nextMsgId}`;
}

/**
 * RAG chat surface: pure-reducer timeline + Channel streaming via pocketBrain API.
 * Cancellation / memory purge remain owned by the parent PocketBrainPanel.
 */
export function RagChatPanel({
  modelReady,
  onError,
  onBusyChange,
}: RagChatPanelProps) {
  const [state, dispatch] = useReducer(ragChatReducer, undefined, initialRagChatState);
  const assistantIdRef = useRef<string | null>(null);

  const throttle = useThrottledStream((chunk) => {
    const id = assistantIdRef.current;
    if (!id) return;
    dispatch({ type: "token", assistantId: id, text: chunk });
  });

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

    try {
      const result = await sendRagChat(
        prompt,
        (event) => {
          if (event.error) {
            dispatch({ type: "token_error", message: event.error });
            onError(event.error);
            return;
          }
          if (event.done) {
            throttle.flushAndStop();
            return;
          }
          if (event.text) {
            throttle.push(event.text);
          }
        },
        { contextLimit: 5 },
      );

      throttle.flushAndStop();
      dispatch({
        type: "send_success",
        assistantId,
        contextCount: result.context_count,
      });
    } catch (e) {
      throttle.flushAndStop();
      const message = isPocketBrainInvokeError(e)
        ? e.message
        : `rag: ${String(e)}`;
      dispatch({ type: "send_failure", message });
      onError(message);
    } finally {
      assistantIdRef.current = null;
      dispatch({ type: "send_end" });
      onBusyChange(false);
    }
  }

  return (
    <div className="rag-chat-panel">
      <RagMessageList messages={state.messages} />
      <RagChatInput
        value={state.input}
        onChange={(value) => dispatch({ type: "set_input", value })}
        onSend={() => void onSend()}
        streaming={state.streaming}
        modelReady={modelReady}
      />
      <RagIngestPanel modelReady={modelReady} />
      {state.error ? (
        <p style={{ margin: "0 12px 8px", fontSize: 12, color: "#ff5555" }}>
          {state.error}
        </p>
      ) : null}
    </div>
  );
}
