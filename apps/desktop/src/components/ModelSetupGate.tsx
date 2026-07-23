// Offline model setup gate — blocks main UI until pocket-brain.gguf is local.
// No network download: guidance opens the OS browser; import is FE fs copyFile only.

import { useEffect, useReducer, useRef } from "react";

import {
  checkModelExists,
  importLocalGgufViaFs,
  openRecommendedModelPage,
} from "../lib/modelSetup";
import {
  INITIAL_MODEL_SETUP,
  modelSetupReducer,
  softImportError,
} from "../lib/modelSetupReducer";
import { TitleBar } from "./TitleBar";

export interface ModelSetupGateProps {
  onReady: () => void;
}

export function ModelSetupGate({ onReady }: ModelSetupGateProps) {
  const [state, dispatch] = useReducer(modelSetupReducer, INITIAL_MODEL_SETUP);
  const onReadyRef = useRef(onReady);
  onReadyRef.current = onReady;
  const readyFired = useRef(false);

  useEffect(() => {
    let cancelled = false;
    dispatch({ type: "check_start" });
    void (async () => {
      const status = await checkModelExists();
      if (cancelled) return;
      if (status === null) {
        dispatch({ type: "check_skipped" });
        return;
      }
      dispatch({
        type: "check_result",
        exists: status.exists,
        relativePath: status.relativePath,
        recommendedPageUrl: status.recommendedPageUrl,
      });
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (state.phase !== "ready" && state.phase !== "unavailable") return;
    if (readyFired.current) return;
    readyFired.current = true;
    onReadyRef.current();
  }, [state.phase]);

  async function handleOpenGuide() {
    try {
      await openRecommendedModelPage();
    } catch {
      dispatch({
        type: "import_error",
        message:
          "ブラウザを自動で開けませんでした。下の URL をコピーして開いてください。",
      });
    }
  }

  async function handleImport() {
    if (state.phase === "importing") return;
    try {
      dispatch({ type: "import_start" });
      const ok = await importLocalGgufViaFs({
        onProgress: (percent) =>
          dispatch({ type: "import_progress", percent }),
      });
      if (!ok) {
        // User cancelled the dialog — return to needed without soft error.
        dispatch({
          type: "check_result",
          exists: false,
          relativePath: state.relativePath,
          recommendedPageUrl: state.recommendedPageUrl,
        });
        return;
      }
      dispatch({ type: "import_done" });
    } catch (err) {
      const raw = err instanceof Error ? err.message : String(err);
      dispatch({ type: "import_error", message: softImportError(raw) });
    }
  }

  // Never use `desktop-chrome` here: @media (max-width:768px) sets it to
  // display:none — that paints an empty black WKWebView on iOS (fail-silent).
  if (state.phase === "checking") {
    return (
      <div className="shell">
        <TitleBar />
        <main className="app loading model-setup-gate">
          <h1>Coraxis</h1>
          <p className="status-line">モデル配置を確認しています…</p>
        </main>
      </div>
    );
  }

  if (state.phase === "ready" || state.phase === "unavailable") {
    return null;
  }

  const importing = state.phase === "importing";

  return (
    <div className="shell">
      <TitleBar />
      <main className="app loading model-setup-gate">
        <h1>Coraxis</h1>
        <p className="status-line">初回セットアップ — ローカル LLM モデル</p>
        <p className="hint model-setup-lead">
          完全オフライン運用のため、アプリはモデルを自動ダウンロードしません。
          公式ページから GGUF を取得し、この端末へ取り込んでください。
        </p>

        <section className="model-setup-section" aria-labelledby="model-setup-guide">
          <h2 id="model-setup-guide" className="model-setup-h2">
            1. 推奨モデルの入手先
          </h2>
          <p className="hint">
            Qwen2.5-7B-Instruct（IQ4_XS 推奨）の公式 GGUF 一覧をブラウザで開きます。
            ダウンロード後、ファイル名は取り込み時に{" "}
            <code>pocket-brain.gguf</code> として保存されます。
          </p>
          {state.recommendedPageUrl ? (
            <p className="model-setup-url" title={state.recommendedPageUrl}>
              {state.recommendedPageUrl}
            </p>
          ) : null}
          <button
            type="button"
            className="model-setup-btn"
            onClick={() => void handleOpenGuide()}
            disabled={importing}
          >
            公式ダウンロードページを開く
          </button>
        </section>

        <section className="model-setup-section" aria-labelledby="model-setup-import">
          <h2 id="model-setup-import" className="model-setup-h2">
            2. ローカル取り込み
          </h2>
          <p className="hint">
            配置先（A+1）: Application Support 配下の{" "}
            <code>{state.relativePath}</code>
          </p>
          <button
            type="button"
            className="model-setup-btn model-setup-btn-primary"
            onClick={() => void handleImport()}
            disabled={importing}
          >
            {importing
              ? "取り込み中…"
              : "ローカルの GGUF ファイルを選択して取り込む"}
          </button>

          {(importing || state.progressPercent > 0) && (
            <div
              className="model-setup-progress"
              role="progressbar"
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={state.progressPercent}
              aria-label="モデル取り込み進捗"
            >
              <div
                className="model-setup-progress-bar"
                style={{ width: `${state.progressPercent}%` }}
              />
              <span className="model-setup-progress-label">
                {state.progressPercent}%
              </span>
            </div>
          )}
        </section>

        {state.error ? (
          <p className="status-line error-text" role="alert">
            {state.error}
          </p>
        ) : null}
      </main>
    </div>
  );
}
