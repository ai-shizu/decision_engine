import type { CompanyFacts } from "../../lib/pocketBrain/types";

interface CompanyFactsFormProps {
  facts: CompanyFacts;
  onPatch: (patch: Partial<CompanyFacts>) => void;
  disabled?: boolean;
}

/**
 * Offline-inject CompanyFacts editor (preferred over live EDINET when egress is Off).
 * Dumb view — no invoke.
 */
export function CompanyFactsForm({
  facts,
  onPatch,
  disabled = false,
}: CompanyFactsFormProps) {
  return (
    <div className="term-panel company-facts-form">
      <p className="term-header">COMPANY_FACTS (offline inject)</p>
      <p className="hint">
        外向き EDINET 取得は二要素 egress が必要。オフラインではここに企業ファクトを直接注入する。
      </p>
      <div className="term-row config-row">
        <span className="term-source-name">企業名 *</span>
        <input
          value={facts.companyName}
          disabled={disabled}
          onChange={(e) => onPatch({ companyName: e.target.value })}
          placeholder="例: サンプル株式会社"
        />
      </div>
      <div className="term-row config-row">
        <span className="term-source-name">EDINET コード</span>
        <input
          value={facts.edinetCode}
          disabled={disabled}
          onChange={(e) => onPatch({ edinetCode: e.target.value })}
          placeholder="任意"
        />
      </div>
      <div className="term-row config-row">
        <span className="term-source-name">事業概要</span>
        <textarea
          rows={3}
          value={facts.businessSummary}
          disabled={disabled}
          onChange={(e) => onPatch({ businessSummary: e.target.value })}
          placeholder="事業内容の要約"
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
