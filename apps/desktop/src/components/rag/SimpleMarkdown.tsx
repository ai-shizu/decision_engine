// Lightweight Markdown subset without external deps (bold / code / newlines).

import { memo, type ReactNode } from "react";

function renderInline(text: string, keyPrefix: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  // **bold** then `code` — greedy-safe sequential scan.
  const pattern = /(\*\*[^*]+\*\*|`[^`]+`)/g;
  let last = 0;
  let match: RegExpExecArray | null;
  let part = 0;
  while ((match = pattern.exec(text)) !== null) {
    if (match.index > last) {
      nodes.push(text.slice(last, match.index));
    }
    const token = match[0];
    if (token.startsWith("**") && token.endsWith("**")) {
      nodes.push(
        <strong key={`${keyPrefix}-b-${part}`}>{token.slice(2, -2)}</strong>,
      );
    } else if (token.startsWith("`") && token.endsWith("`")) {
      nodes.push(
        <code key={`${keyPrefix}-c-${part}`}>{token.slice(1, -1)}</code>,
      );
    } else {
      nodes.push(token);
    }
    part += 1;
    last = match.index + token.length;
  }
  if (last < text.length) {
    nodes.push(text.slice(last));
  }
  return nodes;
}

export const SimpleMarkdown = memo(function SimpleMarkdown({
  text,
}: {
  text: string;
}) {
  const lines = text.split("\n");
  return (
    <div className="rag-md">
      {lines.map((line, i) => (
        <p key={i} className="rag-md-line">
          {renderInline(line, `L${i}`)}
        </p>
      ))}
    </div>
  );
});
