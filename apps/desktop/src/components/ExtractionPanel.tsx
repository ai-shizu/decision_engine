/**
 * Kakeibo extraction UI (M5 Phase 3).
 *
 * Dumb view + useReducer. Trusts only `TokenEvent.validated` — never JSON.parse
 * of the raw stream. Clipboard sink is the sole persistence preview.
 */

import { useReducer, useRef, type ReactElement } from "react";

import {
  extractionReducer,
  formatAmountDisplay,
  INITIAL_EXTRACTION_STATE,
  isUnknownField,
} from "../lib/extractionReducer";
import {
  createClipboardExtractionSink,
  type ExtractionSink,
} from "../lib/extractionSink";
import {
  cancelGeneration,
  generate,
  TASK_KAKEIBO_V1,
  type TokenEvent,
} from "../lib/llm";

const defaultSink: ExtractionSink = createClipboardExtractionSink();

export interface ExtractionPanelProps {
  /** When false, extract is blocked (model not loaded). Input stays editable. */
  readonly modelReady?: boolean;
  readonly sink?: ExtractionSink;
  /** Soften copy for messenger action sheet (desktop chrome unchanged when false). */
  readonly friendly?: boolean;
}

function FieldRow(props: {
  label: string;
  value: string;
  unknown: boolean;
}): ReactElement {
  const { label, value, unknown } = props;
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "7rem 1fr",
        gap: 8,
        alignItems: "baseline",
        padding: "4px 0",
        borderBottom: "1px solid #333",
      }}
    >
      <span style={{ opacity: 0.7 }}>{label}</span>
      <span
        style={{
          fontStyle: unknown ? "italic" : "normal",
          opacity: unknown ? 0.65 : 1,
          textDecoration: unknown ? "underline dotted" : "none",
        }}
        data-unknown={unknown ? "true" : "false"}
      >
        {unknown ? `不明 (${value})` : value}
      </span>
    </div>
  );
}

export function ExtractionPanel(props: ExtractionPanelProps): ReactElement {
  const modelReady = props.modelReady ?? true;
  const sink = props.sink ?? defaultSink;
  const friendly = props.friendly ?? false;
  const [state, dispatch] = useReducer(extractionReducer, INITIAL_EXTRACTION_STATE);
  const nextRequestId = useRef(1);
  /** Guards against double completion handling for the same request. */
  const settledRef = useRef<number | null>(null);

  const extracting = state.phase === "extracting";
  const canExtract =
    modelReady && !extracting && state.input.trim().length > 0;

  function onInputChange(value: string): void {
    dispatch({ type: "inputChanged", input: value });
  }

  function handleTokenEvent(requestId: number, event: TokenEvent): void {
    if (settledRef.current === requestId) {
      return;
    }

    if (event.error) {
      settledRef.current = requestId;
      const cancelled =
        event.error.toLowerCase().includes("cancel") ||
        event.error.includes("キャンセル");
      if (cancelled) {
        dispatch({ type: "extractionCancelled", requestId });
      } else {
        dispatch({
          type: "extractionFailed",
          requestId,
          error: event.error,
        });
      }
      return;
    }

    if (!event.done) {
      if (event.text.length > 0) {
        dispatch({ type: "tokenReceived", requestId, text: event.text });
      }
      return;
    }

    // done === true
    settledRef.current = requestId;
    if (event.validated == null) {
      dispatch({
        type: "extractionFailed",
        requestId,
        error: "extraction completed without validated result",
      });
      return;
    }
    dispatch({
      type: "extractionSucceeded",
      requestId,
      result: event.validated,
    });
  }

  async function onExtract(): Promise<void> {
    const prompt = state.input.trim();
    if (!prompt || extracting || !modelReady) {
      return;
    }

    const requestId = nextRequestId.current;
    nextRequestId.current += 1;
    settledRef.current = null;
    dispatch({ type: "extractionStarted", requestId, input: prompt });

    try {
      await generate(
        {
          prompt,
          n_ctx: 2048,
          max_tokens: 256,
          temp: 0.0,
          top_k: 40,
          top_p: 0.95,
          seed: 0,
        },
        (event) => {
          handleTokenEvent(requestId, event);
        },
        TASK_KAKEIBO_V1,
      );
      // llm_generate is fire-and-forget: invoke resolves when queued, not when
      // streaming finishes. Completion is solely via the Channel callback.
    } catch (e) {
      if (settledRef.current !== requestId) {
        settledRef.current = requestId;
        dispatch({
          type: "extractionFailed",
          requestId,
          error: `generate: ${String(e)}`,
        });
      }
    }
  }

  async function onCancel(): Promise<void> {
    if (!extracting || state.requestId == null) {
      return;
    }
    const requestId = state.requestId;
    try {
      await cancelGeneration();
      // Only leave extracting after the cancel IPC succeeds.
      settledRef.current = requestId;
      dispatch({ type: "extractionCancelled", requestId });
    } catch {
      // Keep extracting + active requestId; surface IPC failure (do not force idle).
      dispatch({
        type: "cancelFailed",
        requestId,
        error: "キャンセルの送信に失敗しました",
      });
    }
  }

  async function onCopy(): Promise<void> {
    if (state.phase !== "success") {
      return;
    }
    dispatch({ type: "copyStarted" });
    try {
      await sink.persist(state.result);
      dispatch({ type: "copySucceeded" });
    } catch (e) {
      dispatch({
        type: "copyFailed",
        error: e instanceof Error ? e.message : String(e),
      });
    }
  }

  return (
    <section
      className="extraction-panel"
      aria-label={friendly ? "支出データの抽出" : "家計簿抽出"}
      style={{
        textAlign: "left",
        width: "100%",
        marginTop: friendly ? 0 : 16,
        borderTop: friendly ? "none" : "1px solid #444",
        paddingTop: friendly ? 0 : 12,
      }}
    >
      <header style={{ padding: "0 12px 8px" }}>
        <strong>{friendly ? "支出データの抽出" : "家計簿抽出"}</strong>
        {!friendly ? (
          <span style={{ marginLeft: 8, opacity: 0.7, fontSize: "0.9em" }}>
            task: {TASK_KAKEIBO_V1}
          </span>
        ) : null}
      </header>

      <div style={{ padding: "0 12px", display: "flex", flexDirection: "column", gap: 8 }}>
        {friendly ? (
          <p style={{ margin: 0, fontSize: 12, opacity: 0.75 }}>
            買い物メモなどから日付・金額・用途を抜き出し、AI の前提知識に加えます。
          </p>
        ) : (
          <label htmlFor="extraction-input">抽出テキスト</label>
        )}
        <textarea
          id="extraction-input"
          aria-label={friendly ? "支出メモ" : "家計簿抽出テキスト"}
          value={state.input}
          onChange={(e) => onInputChange(e.target.value)}
          rows={3}
          placeholder="例: 昨日スーパーで牛乳を298円で買った"
          style={{ width: "100%", resize: "vertical" }}
        />

        <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
          <button
            type="button"
            aria-label={friendly ? "支出を抽出" : "家計簿を抽出"}
            onClick={() => void onExtract()}
            disabled={!canExtract}
          >
            {extracting ? "抽出中…" : "抽出"}
          </button>
          <button
            type="button"
            aria-label="抽出をキャンセル"
            onClick={() => void onCancel()}
            disabled={!extracting}
          >
            キャンセル
          </button>
          <button
            type="button"
            aria-label="検証済み結果をコピー"
            onClick={() => void onCopy()}
            disabled={state.phase !== "success"}
          >
            コピー
          </button>
        </div>

        {!modelReady && (
          <p role="status" style={{ opacity: 0.7, margin: 0 }}>
            モデル準備が終わるまでお待ちください
          </p>
        )}

        {extracting && (
          <p role="status" aria-live="polite" style={{ margin: 0, opacity: 0.8 }}>
            進捗: {state.streamedText.length > 0 ? state.streamedText : "…"}
          </p>
        )}

        {extracting && state.cancelError != null && (
          <p role="alert" style={{ color: "#ff5555", margin: 0 }}>
            {state.cancelError}
          </p>
        )}

        {state.phase === "error" && (
          <p role="alert" style={{ color: "#ff5555", margin: 0 }}>
            {state.error}
          </p>
        )}

        {state.phase === "success" && state.result && (
          <article
            aria-label="抽出結果"
            style={{
              marginTop: 4,
              padding: 8,
              border: "1px solid #555",
              background: "#1a1a1a",
            }}
          >
            <FieldRow
              label={friendly ? "日付" : "date"}
              value={state.result.date}
              unknown={isUnknownField(state.result.date)}
            />
            <FieldRow
              label={friendly ? "金額" : "amount"}
              value={formatAmountDisplay(state.result.amount)}
              unknown={state.result.amount === null}
            />
            <FieldRow
              label={friendly ? "カテゴリ" : "category"}
              value={state.result.category}
              unknown={isUnknownField(state.result.category)}
            />
            <FieldRow
              label={friendly ? "支払先" : "payee"}
              value={state.result.payee}
              unknown={isUnknownField(state.result.payee)}
            />
            <FieldRow
              label={friendly ? "メモ" : "memo"}
              value={state.result.memo}
              unknown={isUnknownField(state.result.memo)}
            />
          </article>
        )}

        {state.phase === "success" && state.copyStatus === "copied" && (
          <p role="status" style={{ margin: 0, opacity: 0.8 }}>
            クリップボードへコピーしました
          </p>
        )}
        {state.phase === "success" && state.copyStatus === "copy_failed" && (
          <p role="alert" style={{ color: "#ff5555", margin: 0 }}>
            コピー失敗: {state.copyError}
          </p>
        )}
      </div>
    </section>
  );
}
