import { useState } from "react";

import { ingestKnowledge } from "../../lib/rag";

interface RagIngestPanelProps {
  modelReady: boolean;
}

/** Ambient ingest UI — never blocks chat input; success via soft status line. */
export function RagIngestPanel({ modelReady }: RagIngestPanelProps) {
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
        `取り込み完了: ${result.source_id} · ${result.inserted}/${result.chunk_count} chunks`,
      );
      setText("");
    } catch (e) {
      setError(`ingest: ${String(e)}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <section
      className="rag-ingest-panel"
      style={{
        padding: 12,
        borderTop: "1px solid #333",
        display: "flex",
        flexDirection: "column",
        gap: 8,
      }}
    >
      <strong style={{ fontSize: 13 }}>記憶の取り込み</strong>
      <input
        value={sourceId}
        onChange={(e) => setSourceId(e.target.value)}
        placeholder="source_id（例: diary-2026）"
        aria-label="source id"
      />
      <textarea
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder="Markdown / テキストを貼り付け…"
        rows={4}
        style={{ resize: "vertical", width: "100%" }}
      />
      <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
        <button
          type="button"
          onClick={() => void onIngest()}
          disabled={!modelReady || !text.trim() || !sourceId.trim()}
        >
          {busy ? "取り込み中…" : "取り込む"}
        </button>
        {busy ? (
          <span style={{ fontSize: 12, opacity: 0.7 }}>embedding…</span>
        ) : null}
      </div>
      {notice ? (
        <p style={{ margin: 0, fontSize: 12, color: "#7dcea0" }}>{notice}</p>
      ) : null}
      {error ? (
        <p style={{ margin: 0, fontSize: 12, color: "#ff5555" }}>{error}</p>
      ) : null}
      {!modelReady ? (
        <p style={{ margin: 0, fontSize: 12, opacity: 0.6 }}>
          取り込みには 384 次元対応モデルのロードが必要です
        </p>
      ) : null}
    </section>
  );
}
