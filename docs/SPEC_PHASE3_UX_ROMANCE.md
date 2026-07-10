# SPEC: Phase 3 UX (Romance preview boundary)

## Scope

Phase 3-A is **UI-only**. All romance/LINE surfaces use mock data or existing sterile reads. No new IPC, backend routes, or persistence.

Phase 3-B (romance consult, LINE-derived signals) is **deferred**. This spec does not encode Phase 3-B logic.

## Phase 3-B boundary (future, not implemented in 3-A)

- Do **not** infer third-party emotions.
- Observable quantities only: explicit self-language, send/receive intervals, frequency, protocol markers.
- Third parties appear as `contact_alias` only — no real names, no raw LINE body in derived UI.
- Insufficient data → `score=None`; never assert romantic feelings.
- MBTI preview in 3-A is mock; do **not** derive MBTI from D1 five-axis scores.

## Phase 3-A deliverables

### Interview tab

- Custom Theme textarea: `rows={6}`, `className="custom-theme-textarea"`, `resize: vertical` (CSS), `maxLength={240}` unchanged.
- NARRATIVE_DRAFT copy: gap analysis + ES draft explanation; mock interview mount blocks execution.

### Profile tab

- `MbtiGradientBars`: fixed mock E/I, S/N, T/F, J/P pairs summing to 100%; labeled **PREVIEW / NOT MEASURED**.
- `TensorRadarChart` legend: per-axis `[?]` tooltips with Japanese descriptions; keyboard focus + `aria-describedby`.

### Accessibility

- Tooltips: `role="tooltip"`, unique IDs, `tabIndex={0}` on triggers, hover + focus visibility.
- MBTI block: visible and accessible preview / not-measured labeling.

## CSS discipline

- Reuse existing CSS variables (`--accent`, `--ok`, `--text`, spacing, radius).
- No new hex colors, no literal `px` (use `rem` or variables), no `letter-spacing`, no purple glow/shadows/decorative cards/animations.

## Tests

- `tests/test_phase3_ux_contract.py` — static contracts for textarea, CSS, narrative copy, MBTI mock, 6D tooltips, forbidden tokens.

## Stop conditions

- Changes outside allowed files.
- package.json / backend / `data/` diffs.
- Breaking Phase 1/2 tensor radar geometry, redactor, or IPC contracts.
