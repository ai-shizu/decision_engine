import { useState } from "react";

import type { CompanyFacts } from "../../lib/pocketBrain/types";

interface CompanyFactsFormProps {
  facts: CompanyFacts;
  onPatch: (patch: Partial<CompanyFacts>) => void;
  disabled?: boolean;
  /** Ambient net/local enrichment in flight — never disables inputs. */
  researching?: boolean;
  provenanceLabel?: string | null;
}

function readinessTag(facts: CompanyFacts): { label: string; ok: boolean } {
  const name = facts.companyName.trim();
  if (!name) return { label: " [ 企業名が必要 ]", ok: false };
  if (facts.edinetCode.trim()) return { label: " [ EDINET 特定済 ]", ok: true };
  return { label: " [ 企業名のみ ]", ok: true };
}

function previewSummary(text: string): { short: string; full: string; chars: number } {
  const full = text.trim();
  const chars = full.length;
  if (chars <= 120) return { short: full, full, chars };
  return { short: `${full.slice(0, 120)}…`, full, chars };
}

/**
 * Offline-inject CompanyFacts editor.
 * EDINET code is resolved automatically from company name (no manual field).
 * Dumb view — no invoke. Ambient enrichment is owned by the parent / hook.
 */
export function CompanyFactsForm({
  facts,
  onPatch,
  disabled = false,
  researching = false,
  provenanceLabel = null,
}: CompanyFactsFormProps) {
  const [summaryExpanded, setSummaryExpanded] = useState(false);
  const [editSummary, setEditSummary] = useState(false);
  const [editorExpanded, setEditorExpanded] = useState(false);
  const tag = readinessTag(facts);
  const name = facts.companyName.trim();
  const summary = facts.businessSummary.trim()
    ? previewSummary(facts.businessSummary)
    : null;
  const showPreview = name.length > 0;
  const truncated = summary !== null && summary.chars > 120;

  return (
    <div
      className={`term-panel company-facts-form interview-section${researching ? " researching-ambient" : ""}${tag.ok ? " is-ready" : " is-missing"}`}
      aria-busy={researching}
    >
      <p className="term-header">
        企業コンテキスト
        {tag.ok ? (
          <span className="term-tag term-tag--ok">{tag.label}</span>
        ) : (
          <span className="term-tag term-tag--danger">{tag.label}</span>
        )}
      </p>
      <p className="hint guide">
        企業名を入れると EDINET コードを自動特定し、必要に応じて情報を補強します。
      </p>
      {researching && (
        <div className="consult-research-ambient company-facts-progress" role="status">
          <span>企業情報を補強中…</span>
          <div className="company-facts-progress-track" aria-hidden="true">
            <div className="company-facts-progress-fill" />
          </div>
        </div>
      )}
      {provenanceLabel && !researching && (
        <p className="consult-research-ambient provenance-chip" role="status">
          {provenanceLabel}
        </p>
      )}

      {showPreview && (
        <div className="term-panel company-facts-preview" aria-label="確定済み企業コンテキスト">
          <div className="term-row">
            <span className="term-source-name">正式社名</span>
            <span className="term-value" style={{ overflowWrap: "anywhere" }}>
              {name}
            </span>
          </div>
          <div className="term-row">
            <span className="term-source-name">EDINETコード</span>
            <span
              className="term-value"
              style={
                facts.edinetCode.trim()
                  ? { overflowWrap: "anywhere" }
                  : { opacity: 0.55, overflowWrap: "anywhere" }
              }
            >
              {facts.edinetCode.trim() || "未特定"}
            </span>
          </div>
          {facts.docId.trim() ? (
            <div className="term-row">
              <span className="term-source-name">書類ID</span>
              <span className="term-value" style={{ overflowWrap: "anywhere" }}>
                {facts.docId.trim()}
              </span>
            </div>
          ) : null}
          {facts.source.trim() ? (
            <div className="term-row">
              <span className="term-source-name">出典</span>
              <span className="term-value" style={{ overflowWrap: "anywhere" }}>
                {facts.source.trim()}
              </span>
            </div>
          ) : null}
          {/* Was a red "個人 Vault 由来" alarm keyed on `local_rag`. That claim is
              false: the lane searches the "company" namespace only, and
              `namespace_of` fails closed to Personal for unknown ids, so a
              Personal chunk cannot be returned. Since the Wikipedia lane opened,
              these hits are normally the ingested company article read back from
              the Vault — a red danger chip there sends investigations after a
              breach that cannot happen. Report provenance, do not alarm. */}
          {facts.source.trim() === "vault_company" ? (
            <p className="term-tag term-tag--muted" role="status">
              Vault（企業空間）から復元
            </p>
          ) : null}
          {/* Was a bare <details> carrying `.term-value`, which is `text-align:
              right` + `flex-shrink: 0` — meant for short right-aligned metrics.
              Long prose came out ragged/right-aligned and the collapsed summary
              line stayed duplicated above the expanded body. Prose gets its own
              left-aligned block and an explicit toggle. */}
          {summary ? (
            <div className="company-summary-block">
              <span className="term-source-name">事業概要</span>
              <p className="company-summary-text">
                {summaryExpanded ? summary.full : summary.short}
              </p>
              {truncated ? (
                <button
                  type="button"
                  className="ghost company-summary-toggle"
                  aria-expanded={summaryExpanded}
                  onClick={() => setSummaryExpanded((v) => !v)}
                >
                  {summaryExpanded ? "閉じる" : `もっと見る（全 ${summary.chars} 文字）`}
                </button>
              ) : null}
            </div>
          ) : null}
          <div className="term-row">
            <span className="term-source-name">事業リスク</span>
            <span className="term-tag">
              {facts.businessRisks.trim() ? " [ 取得済 ]" : " [ 未取得 ]"}
            </span>
          </div>
          <div className="term-row">
            <span className="term-source-name">業績サマリ</span>
            <span className="term-tag">
              {facts.performanceSummary.trim() ? " [ 取得済 ]" : " [ 未取得 ]"}
            </span>
          </div>
        </div>
      )}

      <div className="term-row config-row">
        <span className="term-source-name">
          企業名 <span className="field-required">*</span>
        </span>
        <input
          value={facts.companyName}
          disabled={disabled}
          onChange={(e) => onPatch({ companyName: e.target.value })}
          placeholder="例: サンプル株式会社"
          aria-required="true"
        />
      </div>
      {/* Once the preview above shows the fetched summary, repeating the whole
          text in an editable box is pure duplication and pushed the rest of the
          form off screen. Collapse it behind an explicit toggle rather than
          deleting it: this component is the *offline-inject* editor (see the
          docstring), so a user with no egress must still be able to type a
          summary by hand. Empty summary ⇒ the box is shown outright. */}
      {summary && !editSummary ? (
        <div className="term-row config-row">
          <span className="term-source-name">事業概要</span>
          <button
            type="button"
            className="ghost company-summary-toggle"
            onClick={() => setEditSummary(true)}
          >
            手動で編集する
          </button>
        </div>
      ) : (
        <div className="term-row config-row">
          <span className="term-source-name">事業概要</span>
          <div className="company-summary-editor">
            <textarea
              id="company-business-summary-editor"
              className={`company-summary-editor-textarea${editorExpanded ? " is-expanded" : ""}`}
              rows={editorExpanded ? 12 : 3}
              value={facts.businessSummary}
              disabled={disabled}
              onChange={(e) => {
                // If this field started empty, keep the editor open after the
                // first keystroke instead of immediately collapsing to the
                // "手動で編集する" button on the next render.
                setEditSummary(true);
                onPatch({ businessSummary: e.target.value });
              }}
              placeholder="事業内容の要約（空なら自動補強を試行）"
            />
            {summary ? (
              <button
                type="button"
                className="ghost company-summary-toggle"
                aria-controls="company-business-summary-editor"
                aria-expanded={editorExpanded}
                onClick={() => setEditorExpanded((v) => !v)}
              >
                {editorExpanded ? "閉じる" : `もっと見る（全 ${summary.chars} 文字）`}
              </button>
            ) : null}
          </div>
        </div>
      )}
      <div className="term-row config-row">
        <span className="term-source-name">事業リスク</span>
        <textarea
          rows={2}
          value={facts.businessRisks}
          disabled={disabled}
          onChange={(e) => onPatch({ businessRisks: e.target.value })}
          placeholder="有報リスク等（任意）"
        />
      </div>
      <div className="term-row config-row">
        <span className="term-source-name">業績サマリ</span>
        <textarea
          rows={2}
          value={facts.performanceSummary}
          disabled={disabled}
          onChange={(e) => onPatch({ performanceSummary: e.target.value })}
          placeholder="業績ハイライト（任意）"
        />
      </div>
    </div>
  );
}
