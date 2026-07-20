import { useState } from "react";

import { sendRagChat } from "../../lib/rag";
import { RagChatInput } from "./RagChatInput";
import { RagIngestPanel } from "./RagIngestPanel";
import { RagMessageList, type RagChatMessage } from "./RagMessageList";

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
 * RAG chat surface: timeline + streaming send_rag_chat + ingest panel.
 * Cancellation / memory purge remain owned by the parent PocketBrainPanel.
 */
export function RagChatPanel({
  modelReady,
  onError,
  onBusyChange,
}: RagChatPanelProps) {
  const [messages, setMessages] = useState<RagChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [streaming, setStreaming] = useState(false);

  async function onSend() {
    const prompt = input.trim();
    if (!prompt || streaming || !modelReady) return;
    setInput("");
    onError(null);
    setStreaming(true);
    onBusyChange(true);

    const userId = allocId("u");
    const assistantId = allocId("a");
    setMessages((m) => [
      ...m,
      { id: userId, role: "user", text: prompt },
      { id: assistantId, role: "assistant", text: "" },
    ]);

    try {
      const result = await sendRagChat(
        prompt,
        (event) => {
          if (event.error) {
            onError(event.error);
            return;
          }
          if (event.done) return;
          setMessages((msgs) => {
            const copy = msgs.slice();
            const last = copy[copy.length - 1];
            if (last && last.id === assistantId && last.role === "assistant") {
              copy[copy.length - 1] = {
                ...last,
                text: last.text + event.text,
              };
            }
            return copy;
          });
        },
        { contextLimit: 5 },
      );

      setMessages((msgs) => {
        const copy = msgs.slice();
        const last = copy[copy.length - 1];
        if (last && last.id === assistantId) {
          copy[copy.length - 1] = {
            ...last,
            contextCount: result.context_count,
          };
        }
        return copy;
      });
    } catch (e) {
      onError(`rag: ${String(e)}`);
    } finally {
      setStreaming(false);
      onBusyChange(false);
    }
  }

  return (
    <div className="rag-chat-panel">
      <RagMessageList messages={messages} />
      <RagChatInput
        value={input}
        onChange={setInput}
        onSend={() => void onSend()}
        streaming={streaming}
        modelReady={modelReady}
      />
      <RagIngestPanel modelReady={modelReady} />
    </div>
  );
}
