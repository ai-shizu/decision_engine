/**
 * Session ledger of successful on-device Vault imports (ImportTab visibility).
 * Pure helpers — no React. Survives tab remount within the same app session.
 */

export interface VaultImportEntry {
  kind: "line" | "calendar" | "knowledge";
  label: string;
  detail: string;
  atIso: string;
}

const LEDGER_MAX = 40;
let ledger: VaultImportEntry[] = [];

export function pushVaultImport(entry: Omit<VaultImportEntry, "atIso">): VaultImportEntry[] {
  const row: VaultImportEntry = {
    ...entry,
    atIso: new Date().toISOString(),
  };
  ledger = [...ledger, row].slice(-LEDGER_MAX);
  return [...ledger];
}

export function listVaultImports(): VaultImportEntry[] {
  return [...ledger];
}

export function formatVaultImportLine(e: VaultImportEntry): string {
  const t = e.atIso.slice(11, 19);
  return `[${t}] ${e.kind.toUpperCase()} · ${e.label} — ${e.detail}`;
}
