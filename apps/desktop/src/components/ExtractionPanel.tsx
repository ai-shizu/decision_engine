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
  amount?: boolean;
}): ReactElement {
  const { label, value, unknown, amount } = props;
  return (
    <div className="ledger-extract-row">
      <span className="ledger-extract-key">{label}</span>
      <span
        className={
          amount
            ? "ledger-extract-val ledger-extract-val--amt"
            : unknown
              ? "ledger-extract-val ledger-extract-val--unk"
              : "ledger-extract-val"
        }
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
      settledRef.current = requestId;
      dispatch({ type: "extractionCancelled", requestId });
    } catch {
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
      className={
        friendly
          ? "extraction-panel ledger-extract ledger-extract--friendly"
          : "extraction-panel ledger-extract"
      }
      aria-label={friendly ? "支出データの抽出" : "家計簿抽出"}
    >
      <header className="ledger-extract-head">
        <strong>{friendly ? "支出データの抽出" : "KAKEIBO EXTRACT"}</strong>
        {!friendly ? (
          <span className="ledger-extract-meta">task: {TASK_KAKEIBO_V1}</span>
        ) : null}
      </header>

      <div className="ledger-extract-body">
        {friendly ? (
          <p className="hint ledger-extract-hint">
            買い物メモなどから日付・金額・用途を抜き出し、AI の前提知識に加えます。
          </p>
        ) : (
          <label htmlFor="extraction-input">抽出テキスト</label>
        )}
        <textarea
          id="extraction-input"
          className="ledger-extract-input"
          aria-label={friendly ? "支出メモ" : "家計簿抽出テキスト"}
          value={state.input}
          onChange={(e) => onInputChange(e.target.value)}
          rows={3}
          placeholder="例: 昨日スーパーで牛乳を298円で買った"
        />

        <div className="ledger-extract-actions">
          <button
            type="button"
            aria-label={friendly ? "支出を抽出" : "家計簿を抽出"}
            onClick={() => void onExtract()}
            disabled={!canExtract}
          >
            {extracting ? "抽出中…" : "EXTRACT"}
          </button>
          <button
            type="button"
            aria-label="抽出をキャンセル"
            onClick={() => void onCancel()}
            disabled={!extracting}
          >
            CANCEL
          </button>
          <button
            type="button"
            aria-label="検証済み結果をコピー"
            onClick={() => void onCopy()}
            disabled={state.phase !== "success"}
          >
            COPY
          </button>
        </div>

        {!modelReady && (
          <p role="status" className="hint">
            モデル準備が終わるまでお待ちください
          </p>
        )}

        {extracting && (
          <p role="status" aria-live="polite" className="ledger-extract-stream">
            進捗: {state.streamedText.length > 0 ? state.streamedText : "…"}
          </p>
        )}

        {extracting && state.cancelError != null && (
          <p role="alert" className="error-text">
            {state.cancelError}
          </p>
        )}

        {state.phase === "error" && (
          <p role="alert" className="error-text">
            {state.error}
          </p>
        )}

        {state.phase === "success" && state.result && (
          <article className="ledger-extract-result" aria-label="抽出結果">
            <FieldRow
              label={friendly ? "日付" : "date"}
              value={state.result.date}
              unknown={isUnknownField(state.result.date)}
            />
            <FieldRow
              label={friendly ? "金額" : "amount"}
              value={formatAmountDisplay(state.result.amount)}
              unknown={state.result.amount === null}
              amount
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
          <p role="status" className="hint">
            クリップボードへコピーしました
          </p>
        )}
        {state.phase === "success" && state.copyStatus === "copy_failed" && (
          <p role="alert" className="error-text">
            コピー失敗: {state.copyError}
          </p>
        )}
      </div>
    </section>
  );
}
