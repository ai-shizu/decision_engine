import { useReducer, useRef, type CSSProperties } from "react";
import { ContextObservatory } from "./ContextObservatory";
import { latestContextManifest } from "../lib/engine";
import {
  INITIAL_MANIFEST_FETCH_STATE,
  reduceManifestFetch,
} from "../lib/manifestFetchState";

const rootStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--s3)",
  width: "100%",
  padding: "var(--s3)",
  backgroundColor: "var(--bg-raised)",
  border: "thin solid var(--border)",
  borderRadius: "var(--radius-s)",
  overflowWrap: "anywhere",
};

const statusStyle: CSSProperties = {
  margin: 0,
  fontSize: "0.875rem",
  fontFamily: "var(--font-mono)",
  color: "var(--text-muted)",
  letterSpacing: 0,
};

const errorStyle: CSSProperties = {
  margin: 0,
  fontSize: "0.875rem",
  fontFamily: "var(--font-mono)",
  color: "var(--err)",
  letterSpacing: 0,
};

const buttonStyle: CSSProperties = {
  alignSelf: "flex-start",
  padding: "var(--s1) var(--s2)",
  fontSize: "0.75rem",
  fontFamily: "var(--font-mono)",
  color: "var(--text)",
  backgroundColor: "var(--bg-selected)",
  border: "thin solid var(--accent)",
  borderRadius: "var(--radius-s)",
  cursor: "pointer",
  letterSpacing: 0,
};

export function ContextObservatoryContainer() {
  const [state, dispatch] = useReducer(
    reduceManifestFetch,
    INITIAL_MANIFEST_FETCH_STATE,
  );
  const seqRef = useRef(0);

  async function loadManifest() {
    seqRef.current += 1;
    const seq = seqRef.current;
    dispatch({ kind: "REQUEST_START", seq });
    try {
      const response = await latestContextManifest();
      dispatch({ kind: "REQUEST_SUCCESS", seq, response });
    } catch {
      dispatch({ kind: "REQUEST_FAILURE", seq });
    }
  }

  const buttonLabel =
    state.phase === "empty" || state.phase === "error"
      ? "読み込み"
      : "再読み込み";

  return (
    <div style={rootStyle}>
      <button type="button" style={buttonStyle} onClick={() => void loadManifest()}>
        {buttonLabel}
      </button>
      {state.phase === "loading" ? (
        <p style={statusStyle}>読み込み中…</p>
      ) : null}
      {state.phase === "empty" ? (
        <p style={statusStyle}>マニフェスト未生成</p>
      ) : null}
      {state.phase === "error" ? (
        <p style={errorStyle}>{state.errorMessage}</p>
      ) : null}
      {state.phase === "ready" && state.manifest !== null ? (
        <ContextObservatory manifest={state.manifest} />
      ) : null}
    </div>
  );
}
