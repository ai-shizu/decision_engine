// [D] Coraxis on-device UI (docs/architecture_blueprint.md §3.9).
// Display name: Coraxis. Internal module/path identifiers remain pocket-brain / PKB.
//
// M20-P: Load/Stop ボタン撤廃。マウント時に自動ロードし、purge 後は foreground で再ウォーム。
// M20-C: variant="messenger" for mobile LINE-like chat (desktop layout intact).

import { useCallback, useEffect, useRef, useState } from "react";

import {
  cancelGeneration,
  loadModel,
  memoryMonitorStop,
  startMemoryMonitor,
  subscribeLlmEvents,
  type MemSample,
} from "../lib/llm";
import {
  FOREGROUND_RESTORE_EVENT,
  type ForegroundRestoreDetail,
} from "../lib/foregroundRestore";
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
  const retryTimerRef = useRef<number | null>(null);
  const loadEpochRef = useRef(0);
  const purgedRef = useRef(false);
  const mountedRef = useRef(true);

  const clearRetryTimer = useCallback(() => {
    if (retryTimerRef.current !== null) {
      window.clearTimeout(retryTimerRef.current);
      retryTimerRef.current = null;
    }
  }, []);

  const ensureModelLoaded = useCallback(async () => {
    if (
      loadingRef.current ||
      purgedRef.current ||
      (typeof document !== "undefined" && document.visibilityState !== "visible")
    ) {
      return;
    }
    const loadEpoch = loadEpochRef.current;
    loadingRef.current = true;
    setBusy(true);
    setError(null);
    setPhaseNote("モデル準備中…");
    try {
      await loadModel({ n_gpu_layers: 999, use_mmap: true });
      if (!mountedRef.current || loadEpoch !== loadEpochRef.current) {
        loadingRef.current = false;
        return;
      }
      setModelReady(true);
      setPhaseNote("準備完了");
      setError(null);
      retryCountRef.current = 0;
      loadingRef.current = false;
    } catch (e) {
      if (!mountedRef.current || loadEpoch !== loadEpochRef.current) {
        loadingRef.current = false;
        return;
      }
      setModelReady(false);
      setError(softLoadError(e instanceof Error ? e.message : "load"));
      setPhaseNote("再試行待機…");
      if (
        !purgedRef.current &&
        document.visibilityState === "visible" &&
        retryCountRef.current < 3
      ) {
        retryCountRef.current += 1;
        clearRetryTimer();
        retryTimerRef.current = window.setTimeout(() => {
          retryTimerRef.current = null;
          loadingRef.current = false;
          if (
            mountedRef.current &&
            !purgedRef.current &&
            document.visibilityState === "visible"
          ) {
            void ensureModelLoaded();
          }
        }, 4000);
      } else {
        loadingRef.current = false;
        setPhaseNote("準備に失敗");
      }
    } finally {
      if (mountedRef.current) {
        setBusy(false);
      }
    }
  }, [clearRetryTimer]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      clearRetryTimer();
    };
  }, [clearRetryTimer]);

  useEffect(() => {
    let cancelled = false;
    startMemoryMonitor(setMem, SAMPLE_INTERVAL_MS, THRESHOLD_BYTES).catch((e) => {
      if (!cancelled) setError(`monitor: ${String(e)}`);
    });
    return () => {
      cancelled = true;
      void memoryMonitorStop().catch(() => {
        /* best-effort stop */
      });
    };
  }, []);

  // Memory purge is a sticky cold state. Re-warming here would immediately
  // reverse the native background/Jetsam protection; the foreground restore
  // coordinator is the sole owner of the next load attempt.
  useEffect(() => {
    let active = true;
    void subscribeLlmEvents((event) => {
      if (!active) {
        return;
      }
      if (event.kind === "degradation") {
        // Ambient thermal warn — MemSample also carries the ladder; this is the
        // out-of-band Serious+ rising edge from the worker.
        return;
      }
      if (event.kind !== "memory_purged") {
        return;
      }
      void cancelGeneration();
      purgedRef.current = true;
      loadEpochRef.current += 1;
      clearRetryTimer();
      retryCountRef.current = 0;
      loadingRef.current = false;
      setBusy(false);
      setModelReady(false);
      setError(null);
      setPhaseNote("メモリ保護のため停止中。前景復帰時に再準備します。");
    })
      .then(() => {
        if (active) {
          // Initial load begins only after the lifecycle sink is authoritative.
          void ensureModelLoaded();
        }
      })
      .catch(() => {
        // No LLM event sink (e.g. desktop without the pocket-brain command).
        if (active) {
          void ensureModelLoaded();
        }
      });
    return () => {
      active = false;
    };
  }, [clearRetryTimer, ensureModelLoaded]);

  // Phase 10: sync ready flag after App's ordered foreground restore.
  useEffect(() => {
    function onRestore(ev: Event): void {
      const detail = (ev as CustomEvent<ForegroundRestoreDetail>).detail;
      const report = detail?.report;
      if (!report) return;
      if (report.llmWasLoaded) {
        purgedRef.current = false;
        setModelReady(true);
        setPhaseNote("準備完了");
        setError(null);
        return;
      }
      // Never report a merely attempted warm-up as ready, and do not create a
      // second load owner beside the foreground coordinator.
      setModelReady(false);
      setPhaseNote("前景復帰後の再準備に失敗");
    }
    window.addEventListener(FOREGROUND_RESTORE_EVENT, onRestore);
    return () => {
      window.removeEventListener(FOREGROUND_RESTORE_EVENT, onRestore);
    };
  }, []);

  const over = mem?.over_threshold ?? false;
  const thermalWarn =
    mem?.degradation === "serious" || mem?.degradation === "critical";

  if (messenger) {
    return (
      <section className="pocket-brain pocket-brain-messenger">
        <header className="pocket-brain-messenger-bar">
          <div className="pocket-brain-messenger-title">
            <strong>CONSULT</strong>
            <span
              className={
                over || thermalWarn
                  ? "pocket-brain-mem pocket-brain-mem-over"
                  : "pocket-brain-mem"
              }
            >
              {mem
                ? `${fmtMiB(mem.phys_footprint_bytes)} · ${mem.phase}${
                    thermalWarn ? ` · ${mem.degradation}` : ""
                  }`
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
    <section className="pocket-brain">
      <header
        className={
          over || thermalWarn
            ? "pocket-brain-header pocket-brain-header--alarm"
            : "pocket-brain-header"
        }
      >
        <strong>Coraxis · RAG</strong>
        <span>
          {mem
            ? `${fmtMiB(mem.phys_footprint_bytes)} / ${fmtMiB(mem.threshold_bytes)} · ${mem.phase}${
                thermalWarn ? ` · ${mem.degradation}` : ""
              }`
            : phaseNote}
          {busy && !modelReady ? " · warming" : modelReady ? " · ready" : ""}
        </span>
      </header>

      {error && (
        <p className="pocket-brain-error">{softLoadError(error)}</p>
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
