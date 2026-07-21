import type { CompanyFacts } from "../../lib/pocketBrain/types";

interface CompanyFactsFormProps {
  facts: CompanyFacts;
  onPatch: (patch: Partial<CompanyFacts>) => void;
  disabled?: boolean;
  /** Ambient net/local enrichment in flight — never disables inputs. */
  researching?: boolean;
  provenanceLabel?: string | null;
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
  const ready = facts.companyName.trim().length > 0;

  return (
    <div
      className={`term-panel company-facts-form interview-section${researching ? " researching-ambient" : ""}${ready ? " is-ready" : " is-missing"}`}
      aria-busy={researching}
    >
      <p className="term-header">
        企業コンテキスト
        {ready ? (
          <span className="term-tag term-tag--ok"> [ 準備完了 ]</span>
        ) : (
          <span className="term-tag term-tag--danger"> [ 企業名が必要 ]</span>
        )}
      </p>
      <p className="hint guide">
        企業名を入れると EDINET コードを自動特定し、必要に応じて情報を補強します。
      </p>
      {researching && (
        <p className="consult-research-ambient" role="status">
          企業情報を補強中…
        </p>
      )}
      {provenanceLabel && !researching && (
        <p className="consult-research-ambient provenance-chip" role="status">
          {provenanceLabel}
        </p>
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
      <div className="term-row config-row">
        <span className="term-source-name">事業概要</span>
        <textarea
          rows={3}
          value={facts.businessSummary}
          disabled={disabled}
          onChange={(e) => onPatch({ businessSummary: e.target.value })}
          placeholder="事業内容の要約（空なら自動補強を試行）"
        />
      </div>
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
