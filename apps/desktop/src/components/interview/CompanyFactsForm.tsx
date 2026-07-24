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
  const tag = readinessTag(facts);
  const name = facts.companyName.trim();
  const summary = facts.businessSummary.trim()
    ? previewSummary(facts.businessSummary)
    : null;
  const showPreview = name.length > 0;

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
        <p className="consult-research-ambient" role="status">
          企業情報を補強中…
        </p>
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
          {facts.source.trim() === "local_rag" ? (
            <p className="term-tag term-tag--danger" role="status">
              ⚠ 個人 Vault 由来の要約です
            </p>
          ) : null}
          {summary ? (
            <div className="term-row">
              <span className="term-source-name">事業概要</span>
              <details className="term-value" style={{ overflowWrap: "anywhere" }}>
                <summary>
                  {summary.short}（全 {summary.chars} 文字）
                </summary>
                <p style={{ whiteSpace: "pre-wrap", margin: "0.4em 0 0" }}>{summary.full}</p>
              </details>
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
