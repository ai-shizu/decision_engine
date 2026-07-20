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
 * M20-C: messenger-mode action sheet — ingest / finance extraction / vault.
 * Mounted only from PocketBrainPanel variant="messenger".
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
          <h2 id={titleId}>Actions</h2>
          <button
            ref={closeRef}
            type="button"
            className="rag-action-close"
            aria-label="閉じる"
            onClick={onClose}
          >
            Close
          </button>
        </div>
        <div className="rag-action-sheet-body">
          <RagIngestPanel modelReady={modelReady} />
          <ExtractionPanel modelReady={modelReady} />
          <VaultPanel />
        </div>
      </div>
    </div>
  );
}
