import { useState } from "react";

import {
  ingestKnowledge,
  isPocketBrainInvokeError,
} from "../../lib/pocketBrain";

interface RagIngestPanelProps {
  modelReady: boolean;
  /** Soften copy for messenger action sheet (desktop ingest chrome unchanged when false). */
  friendly?: boolean;
}

/** Ambient ingest UI — never blocks chat input; success via soft status line. */
export function RagIngestPanel({ modelReady, friendly = false }: RagIngestPanelProps) {
  const [sourceId, setSourceId] = useState("memo");
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function onIngest() {
    const body = text.trim();
    const sid = sourceId.trim();
    if (!body || !sid || busy) return;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const result = await ingestKnowledge(body, sid);
      setNotice(
        friendly
          ? `学習しました（${result.inserted} 件）`
          : `取り込み完了: ${result.source_id} · ${result.inserted}/${result.chunk_count} chunks`,
      );
      setText("");
    } catch (e) {
      setError(
        isPocketBrainInvokeError(e) ? e.message : `ingest: ${String(e)}`,
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <section
      className={
        friendly
          ? "rag-ingest-panel rag-ingest-panel--friendly"
          : "rag-ingest-panel"
      }
    >
      <strong className="rag-ingest-title">
        {friendly ? "メモの学習" : "INGEST · KNOWLEDGE"}
      </strong>
      {friendly ? (
        <p className="hint rag-ingest-hint">
          日記・メモ・Markdown を貼ると、AI が参照できる知識として保存します。
        </p>
      ) : null}
      <input
        value={sourceId}
        onChange={(e) => setSourceId(e.target.value)}
        placeholder={friendly ? "ラベル（例: 日記）" : "source_id（例: diary-2026）"}
        aria-label={friendly ? "メモのラベル" : "source id"}
      />
      <textarea
        className="rag-ingest-input"
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder={
          friendly
            ? "学習させたいテキストを貼り付け…"
            : "Markdown / テキストを貼り付け…"
        }
        rows={4}
      />
      <div className="rag-ingest-actions">
        <button
          type="button"
          onClick={() => void onIngest()}
          disabled={!modelReady || !text.trim() || !sourceId.trim()}
        >
          {busy ? (friendly ? "学習中…" : "取り込み中…") : friendly ? "学習させる" : "INGEST"}
        </button>
        {busy && !friendly ? (
          <span className="rag-ingest-busy">embedding…</span>
        ) : null}
      </div>
      {notice ? <p className="rag-ingest-notice">{notice}</p> : null}
      {error ? <p className="error-text rag-ingest-error">{error}</p> : null}
      {!modelReady ? (
        <p className="hint">モデル準備が終わるまでお待ちください</p>
      ) : null}
    </section>
  );
}
