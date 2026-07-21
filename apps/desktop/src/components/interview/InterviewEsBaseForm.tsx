import type { EsListItem } from "../../lib/types";

export interface InterviewEsBaseFormProps {
  esId: string;
  esText: string;
  items: EsListItem[];
  disabled?: boolean;
  onEsIdChange: (id: string) => void;
  onEsTextChange: (text: string) => void;
}

/**
 * Base ES selector for 1:1 interview (M20-N restore).
 * Dumb view — parent owns list load + es.view body fetch.
 */
export function InterviewEsBaseForm({
  esId,
  esText,
  items,
  disabled = false,
  onEsIdChange,
  onEsTextChange,
}: InterviewEsBaseFormProps) {
  return (
    <div className="term-panel interview-es-base-form interview-section">
      <p className="term-header">
        <span className="desktop-only">ES_BASE (面接の前提)</span>
        <span className="mobile-only">ベースとなるES</span>
      </p>
      <p className="hint guide">
        登録済み ES を選ぶか、下に本文を直接入力します。未指定ならゼロベース面接です。
      </p>
      <div className="term-row config-row">
        <span className="term-source-name">対象企業のES</span>
        <select
          value={esId}
          disabled={disabled}
          onChange={(e) => onEsIdChange(e.target.value)}
          aria-label="ベースとなるES"
        >
          <option value="">ゼロベース（ESなし）</option>
          {items.map((item) => (
            <option key={item.id} value={item.id}>
              {item.company_name}のES
              {item.char_count > 0 ? ` (${item.char_count}字)` : ""}
            </option>
          ))}
        </select>
      </div>
      <div className="term-row config-row">
        <span className="term-source-name">ES本文</span>
        <textarea
          rows={5}
          value={esText}
          disabled={disabled}
          onChange={(e) => onEsTextChange(e.target.value)}
          placeholder="選択した ES の本文、またはここに貼り付け / 直接入力"
        />
      </div>
      <p className="hint guide">
        {esId
          ? "選択した企業の ES を面接官 AI の前提として使います。"
          : esText.trim()
            ? "貼り付けた ES 本文を前提に面接します。"
            : "ES を使わず、企業コンテキストのみでゼロベースの面接にします。"}
      </p>
    </div>
  );
}
