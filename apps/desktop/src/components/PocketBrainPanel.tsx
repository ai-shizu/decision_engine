// [D] Pocket Brain UI (docs/architecture_blueprint.md §3.9).
//
// M20-P: Load/Stop ボタン撤廃。マウント時に自動ロードし、メモリ purge 後も再ウォーム。
// M20-C: variant="messenger" for mobile LINE-like chat (desktop layout intact).

import { useCallback, useEffect, useRef, useState } from "react";

import {
  cancelGeneration,
  loadModel,
  startMemoryMonitor,
  subscribeLlmEvents,
  type MemSample,
} from "../lib/llm";
import { ExtractionPanel } from "./ExtractionPanel";
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

function softLoadError(_raw: string): string {
  // Finding 13: never surface exception text / path / GGUF details.
  return "モデルの準備に失敗しました。バックグラウンドで再試行します。";
}

export function PocketBrainPanel({ variant = "default" }: PocketBrainPanelProps) {
  const [mem, setMem] = useState<MemSample | null>(null);
  const [modelReady, setModelReady] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [phaseNote, setPhaseNote] = useState("モデル準備中…");
  const messenger = variant === "messenger";
  const loadingRef = useRef(false);
  const retryCountRef = useRef(0);

  const ensureModelLoaded = useCallback(async () => {
    if (loadingRef.current) return;
    loadingRef.current = true;
    setBusy(true);
    setError(null);
    setPhaseNote("モデル準備中…");
    try {
      await loadModel({ n_gpu_layers: 999, use_mmap: true });
      setModelReady(true);
      setPhaseNote("準備完了");
      setError(null);
      retryCountRef.current = 0;
      loadingRef.current = false;
    } catch (e) {
      setModelReady(false);
      setError(`load: ${String(e)}`);
      setPhaseNote("再試行待機…");
      if (retryCountRef.current < 3) {
        retryCountRef.current += 1;
        window.setTimeout(() => {
          loadingRef.current = false;
          void ensureModelLoaded();
        }, 4000);
      } else {
        loadingRef.current = false;
        setPhaseNote("準備に失敗");
      }
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    startMemoryMonitor(setMem, SAMPLE_INTERVAL_MS, THRESHOLD_BYTES).catch((e) =>
      setError(`monitor: ${String(e)}`),
    );
  }, []);

  // Auto-load on mount (no manual Load button).
  useEffect(() => {
    void ensureModelLoaded();
  }, [ensureModelLoaded]);

  // Memory purge → auto re-warm (no reload button).
  useEffect(() => {
    let active = true;
    void subscribeLlmEvents((event) => {
      if (!active || event.kind !== "memory_purged") {
        return;
      }
      void cancelGeneration();
      setBusy(false);
      setModelReady(false);
      setError(null);
      setPhaseNote("メモリ保護後に再準備中…");
      void ensureModelLoaded();
    }).catch(() => {
      // No LLM event sink (e.g. desktop without the pocket-brain command).
    });
    return () => {
      active = false;
    };
  }, [ensureModelLoaded]);

  const over = mem?.over_threshold ?? false;

  if (messenger) {
    return (
      <section className="pocket-brain pocket-brain-messenger">
        <header className="pocket-brain-messenger-bar">
          <div className="pocket-brain-messenger-title">
            <strong>CONSULT</strong>
            <span
              className={
                over
                  ? "pocket-brain-mem pocket-brain-mem-over"
                  : "pocket-brain-mem"
              }
            >
              {mem
                ? `${fmtMiB(mem.phys_footprint_bytes)} · ${mem.phase}`
                : phaseNote}
            </span>
          </div>
        </header>

        {error ? (
          <p className="pocket-brain-error">{softLoadError(error)}</p>
        ) : null}

        <RagChatPanel
          variant="messenger"
          modelReady={modelReady}
          onError={setError}
          onBusyChange={setBusy}
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
          color: over ? "var(--err)" : "inherit",
          fontVariantNumeric: "tabular-nums",
        }}
      >
        <strong>Pocket Brain · RAG</strong>
        <span>
          {mem
            ? `${fmtMiB(mem.phys_footprint_bytes)} / ${fmtMiB(mem.threshold_bytes)} · ${mem.phase}`
            : phaseNote}
          {busy && !modelReady ? " · warming" : modelReady ? " · ready" : ""}
        </span>
      </header>

      {error && (
        <p className="pocket-brain-error" style={{ padding: "8px 12px" }}>
          {softLoadError(error)}
        </p>
      )}

      <RagChatPanel
        modelReady={modelReady}
        onError={setError}
        onBusyChange={setBusy}
      />

      <ExtractionPanel modelReady={modelReady} />
    </section>
  );
}
