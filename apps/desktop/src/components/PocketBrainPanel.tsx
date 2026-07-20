// [D] Pocket Brain UI (docs/architecture_blueprint.md §3.9).
//
// M4 memory header + model load/cancel, plus M11 RAG chat/ingest surface.
// Cancellation and Jetsam purge stay here so M7 governor wiring is unchanged.
// M20-C: variant="messenger" for mobile LINE-like chat (desktop layout intact).

import { useEffect, useState } from "react";

import {
  cancelGeneration,
  loadModel,
  startMemoryMonitor,
  subscribeLlmEvents,
  type MemSample,
} from "../lib/llm";
import { ExtractionPanel } from "./ExtractionPanel";
import { RagActionSheet } from "./rag/RagActionSheet";
import { RagChatPanel } from "./rag/RagChatPanel";

// A17 Pro / 8GB jetsam design budget ≈ 4.8 GB (blueprint §G0-C.1, 60% band).
const THRESHOLD_BYTES = Math.round(4.8 * 1024 * 1024 * 1024);
const SAMPLE_INTERVAL_MS = 500;

function fmtMiB(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(0)} MiB`;
}

export interface PocketBrainPanelProps {
  /** default = desktop / loading chrome; messenger = M20-C mobile chat. */
  variant?: "default" | "messenger";
}

export function PocketBrainPanel({ variant = "default" }: PocketBrainPanelProps) {
  const [mem, setMem] = useState<MemSample | null>(null);
  const [modelReady, setModelReady] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [actionOpen, setActionOpen] = useState(false);
  const messenger = variant === "messenger";

  useEffect(() => {
    startMemoryMonitor(setMem, SAMPLE_INTERVAL_MS, THRESHOLD_BYTES).catch((e) =>
      setError(`monitor: ${String(e)}`),
    );
  }, []);

  // Subscribe once to worker-pushed LLM lifecycle events. On an out-of-band
  // memory purge (iOS dropped the model to survive memory pressure), suspend the
  // UI: stop any stream, mark the model unloaded, and prompt a reload. The
  // native worker already dropped the model — this only re-syncs the UI.
  useEffect(() => {
    let active = true;
    void subscribeLlmEvents((event) => {
      if (!active || event.kind !== "memory_purged") {
        return;
      }
      void cancelGeneration();
      setBusy(false);
      setModelReady(false);
      setError("メモリ保護のためモデルを解放しました。再ロードしてください。");
    }).catch(() => {
      // No LLM event sink (e.g. desktop without the pocket-brain command).
    });
    return () => {
      active = false;
    };
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

  const over = mem?.over_threshold ?? false;

  if (messenger) {
    return (
      <section className="pocket-brain pocket-brain-messenger">
        <header className="pocket-brain-messenger-bar">
          <div className="pocket-brain-messenger-title">
            <strong>RAG</strong>
            <span
              className={
                over
                  ? "pocket-brain-mem pocket-brain-mem-over"
                  : "pocket-brain-mem"
              }
            >
              {mem
                ? `${fmtMiB(mem.phys_footprint_bytes)} · ${mem.phase}`
                : "mem —"}
            </span>
          </div>
          <div className="pocket-brain-messenger-actions">
            <button
              type="button"
              className="pocket-brain-chip-btn"
              onClick={() => void onLoad()}
              disabled={busy || modelReady}
            >
              {modelReady ? "Loaded" : "Load"}
            </button>
            <button
              type="button"
              className="pocket-brain-chip-btn"
              onClick={() => void cancelGeneration()}
              disabled={!busy}
            >
              Cancel
            </button>
          </div>
        </header>

        {error ? <p className="pocket-brain-error">{error}</p> : null}

        <RagChatPanel
          variant="messenger"
          modelReady={modelReady}
          onError={setError}
          onBusyChange={setBusy}
          onActionClick={() => setActionOpen(true)}
        />

        <RagActionSheet
          open={actionOpen}
          modelReady={modelReady}
          onClose={() => setActionOpen(false)}
        />
      </section>
    );
  }

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
        <strong>Pocket Brain · RAG</strong>
        <span>
          {mem
            ? `${fmtMiB(mem.phys_footprint_bytes)} / ${fmtMiB(mem.threshold_bytes)} · ${mem.phase}`
            : "mem —"}
        </span>
      </header>

      <div style={{ padding: 12, display: "flex", gap: 8 }}>
        <button type="button" onClick={() => void onLoad()} disabled={busy || modelReady}>
          {modelReady ? "Model loaded" : "Load model"}
        </button>
        <button type="button" onClick={() => void cancelGeneration()} disabled={!busy}>
          Cancel
        </button>
      </div>

      {error && <p style={{ color: "#ff5555", padding: "0 12px" }}>{error}</p>}

      <RagChatPanel
        modelReady={modelReady}
        onError={setError}
        onBusyChange={setBusy}
      />

      <ExtractionPanel modelReady={modelReady} />
    </section>
  );
}
