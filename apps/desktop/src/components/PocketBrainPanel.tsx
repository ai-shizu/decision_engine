// [D] Pocket Brain UI (docs/architecture_blueprint.md §3.9).
//
// Minimal chat panel for the M4 on-device LLM, with a real-time phys_footprint
// indicator in the header. Deliberately unstyled beyond the essentials — the
// polished mobile UI (fonts, ceremony) is M5, not this phase.

import { useEffect, useState } from "react";

import {
  cancelGeneration,
  generate,
  loadModel,
  startMemoryMonitor,
  type MemSample,
} from "../lib/llm";

interface ChatMessage {
  role: "user" | "assistant";
  text: string;
}

// A17 Pro / 8GB jetsam design budget ≈ 4.8 GB (blueprint §G0-C.1, 60% band).
const THRESHOLD_BYTES = Math.round(4.8 * 1024 * 1024 * 1024);
const SAMPLE_INTERVAL_MS = 500;

function fmtMiB(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(0)} MiB`;
}

export function PocketBrainPanel() {
  const [mem, setMem] = useState<MemSample | null>(null);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [modelReady, setModelReady] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    startMemoryMonitor(setMem, SAMPLE_INTERVAL_MS, THRESHOLD_BYTES).catch((e) =>
      setError(`monitor: ${String(e)}`),
    );
  }, []);

  async function onLoad() {
    setError(null);
    setBusy(true);
    try {
      await loadModel({ n_gpu_layers: 999, use_mmap: true });
      setModelReady(true);
    } catch (e) {
      setError(`load: ${String(e)}`);
    } finally {
      setBusy(false);
    }
  }

  async function onSend() {
    const prompt = input.trim();
    if (!prompt || busy) return;
    setInput("");
    setError(null);
    setBusy(true);
    setMessages((m) => [
      ...m,
      { role: "user", text: prompt },
      { role: "assistant", text: "" },
    ]);
    try {
      await generate(
        {
          prompt,
          n_ctx: 2048,
          max_tokens: 256,
          temp: 0.7,
          top_k: 40,
          top_p: 0.95,
          seed: 0,
        },
        (event) => {
          if (event.error) {
            setError(event.error);
            return;
          }
          if (event.done) return;
          setMessages((msgs) => {
            const copy = msgs.slice();
            const last = copy[copy.length - 1];
            if (last && last.role === "assistant") {
              copy[copy.length - 1] = {
                role: "assistant",
                text: last.text + event.text,
              };
            }
            return copy;
          });
        },
      );
    } catch (e) {
      setError(`generate: ${String(e)}`);
    } finally {
      setBusy(false);
    }
  }

  const over = mem?.over_threshold ?? false;

  return (
    <section className="pocket-brain" style={{ textAlign: "left", width: "100%" }}>
      <header
        style={{
          display: "flex",
          justifyContent: "space-between",
          padding: "8px 12px",
          borderBottom: "1px solid #333",
          color: over ? "#ff5555" : "inherit",
          fontVariantNumeric: "tabular-nums",
        }}
      >
        <strong>Pocket Brain</strong>
        <span>
          {mem
            ? `${fmtMiB(mem.phys_footprint_bytes)} / ${fmtMiB(mem.threshold_bytes)} · ${mem.phase}`
            : "mem —"}
        </span>
      </header>

      <div style={{ padding: 12, display: "flex", gap: 8 }}>
        <button type="button" onClick={onLoad} disabled={busy || modelReady}>
          {modelReady ? "Model loaded" : "Load model"}
        </button>
        <button type="button" onClick={() => void cancelGeneration()} disabled={!busy}>
          Cancel
        </button>
      </div>

      <ul
        style={{
          listStyle: "none",
          margin: 0,
          padding: 12,
          display: "flex",
          flexDirection: "column",
          gap: 8,
        }}
      >
        {messages.map((m, i) => (
          <li key={i} style={{ whiteSpace: "pre-wrap", opacity: m.role === "user" ? 0.75 : 1 }}>
            <b>{m.role === "user" ? "You" : "AI"}:</b> {m.text}
          </li>
        ))}
      </ul>

      {error && <p style={{ color: "#ff5555", padding: "0 12px" }}>{error}</p>}

      <div style={{ display: "flex", gap: 8, padding: 12 }}>
        <input
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void onSend();
          }}
          placeholder="メッセージを入力…"
          style={{ flex: 1 }}
        />
        <button type="button" onClick={() => void onSend()} disabled={busy || !modelReady}>
          Send
        </button>
      </div>
    </section>
  );
}
