/** Pure parser for company-analysis markdown (no React). */

export interface CompanyAnalysisSection {
  readonly heading: string;
  readonly items: string[];
}

/**
 * Split on `## ` headings; collect `- ` bullet lines only.
 * Never invents missing sections.
 */
export function parseCompanyAnalysis(raw: string): CompanyAnalysisSection[] {
  const text = raw.replace(/\r\n/g, "\n");
  if (!text.trim()) return [];

  const sections: CompanyAnalysisSection[] = [];
  let current: { heading: string; items: string[] } | null = null;

  for (const line of text.split("\n")) {
    const headingMatch = /^##\s+(.+?)\s*$/.exec(line);
    if (headingMatch) {
      if (current) sections.push(current);
      current = { heading: headingMatch[1].trim(), items: [] };
      continue;
    }
    if (!current) continue;
    const bullet = /^-\s+(.+)$/.exec(line);
    if (bullet) {
      current.items.push(bullet[1].trim());
    }
  }
  if (current) sections.push(current);
  return sections;
}
