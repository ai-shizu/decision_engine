import { useEffect, useId, useRef } from "react";
import { ExtractionPanel } from "../ExtractionPanel";
import { RagIngestPanel } from "./RagIngestPanel";
import { VaultPanel } from "../VaultPanel";

export interface RagActionSheetProps {
  open: boolean;
  modelReady: boolean;
  onClose: () => void;
}

/**
 * Messenger "+" sheet: AI context provisioning (ingest / spend extract / vault).
 */
export function RagActionSheet({ open, modelReady, onClose }: RagActionSheetProps) {
  const titleId = useId();
  const closeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    closeRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      className="rag-action-backdrop"
      role="presentation"
      onClick={onClose}
    >
      <div
        className="rag-action-sheet"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="rag-action-sheet-head">
          <h2 id={titleId}>AIへのデータ提供</h2>
          <button
            ref={closeRef}
            type="button"
            className="rag-action-close"
            aria-label="閉じる"
            onClick={onClose}
          >
            閉じる
          </button>
        </div>
        <p className="rag-action-lead">
          ここに追加した内容は、チャット回答の前提知識（コンテキスト）として使われます。
        </p>
        <div className="rag-action-sheet-body">
          <section className="rag-action-section" aria-label="メモの学習">
            <RagIngestPanel modelReady={modelReady} friendly />
          </section>
          <section className="rag-action-section" aria-label="支出データの抽出">
            <ExtractionPanel modelReady={modelReady} friendly />
          </section>
          <section className="rag-action-section" aria-label="暗号化保管庫">
            <VaultPanel />
          </section>
        </div>
      </div>
    </div>
  );
}
