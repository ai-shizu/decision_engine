# SPEC Feature: GD Thread UI

This document is the implementation authority for Target Echo: the GD
simulation UI/UX overhaul. The goal is to render `gd_sim` responses as a
speaker-separated thread while preserving the existing behavior of
`interview_sim`, `es_review`, PROBE, D1, and D2.

## 0. Authority And Scope

- Target mode: `gd_sim` only.
- Existing protected modes: `interview_sim` and `es_review` must keep their
  current plain assistant rendering.
- No new npm packages.
- No external API, network call, localStorage, random UI behavior, or LLM-side
  postprocessing outside the existing consultation flow.
- The feature may touch only the minimum files needed for the GD prompt,
  GD message rendering, CSS, and tests.

## 1. Architecture Overview

The backend must force a lightweight, line-oriented GD discussion format. The
frontend must parse that text deterministically and render each participant turn
as a visually separated item inside the INTERVIEW chat log.

The format is intentionally plain text so it survives streaming chunks and does
not require JSON parsing from LLM output.

### 1.1 Backend Output Contract

All `gd_sim` discussion-phase assistant responses must use `GD_FORMAT_V1`:

```text
[学生A]: 発言内容
[学生B]: 発言内容
[学生C]: 発言内容
```

For dynamic personas, the speaker names must be the sanitized persona names
already passed from the frontend:

```text
[田中]: 発言内容
[佐藤]: 発言内容
```

Rules:

- A speaker header is valid only at the start of a line.
- The exact header shape is `[speaker]: text`.
- `speaker` is 1-24 visible characters after trimming.
- `speaker` must not contain `[`, `]`, `:`, `\r`, or `\n`.
- One assistant response should contain 2-5 turns.
- Each turn should be one line whenever possible.
- If the model emits a continuation line without a speaker header, the frontend
  appends it to the previous speaker turn.
- No Markdown headings, bullets, code fences, or roleplay prose outside the
  speaker lines during the discussion phase.
- This format applies only to discussion turns. `講評`, `終了`, `debrief`, and
  mentor follow-up responses remain normal text.

### 1.2 Frontend Parse Strategy

Add a pure parser:

```ts
export interface GdSpeakerTurn {
  speaker: string;
  text: string;
}

export function parseGdSpeakerTurns(raw: string): GdSpeakerTurn[];
```

Parser behavior:

- Normalize CRLF to LF.
- Split by line.
- Recognize headers using a line-start regex equivalent to:
  `/^\[([^\][:\n]{1,24})\]:\s*(.*)$/`
- Trim speaker and body.
- Reject empty speaker names.
- Append non-header lines to the previous turn with `\n`.
- Ignore empty leading/trailing lines.
- If no valid header exists, return one fallback turn:
  `{ speaker: "GD", text: raw.trim() }`.
- Do not split on `[speaker]` tokens that appear in the middle of a sentence.

The parser is deterministic and must not call backend, LLM, timers, random, or
browser storage.

### 1.3 Streaming Strategy

The UI should keep one raw AI message for the current GD assistant response and
render it through a thread renderer. Existing chunk events append text to that
raw message. Because the renderer parses `message.text` on each render, partial
chunks remain visible without losing the raw source.

Required message marker:

```ts
renderAs?: "plain" | "gd_thread";
```

Only messages created while `mode === "gd_sim"` and `phase === "active"` may set
`renderAs: "gd_thread"`. Feedback, debrief mentor responses, `interview_sim`,
and `es_review` must not set it.

## 2. UI Component Design

Implementation may keep the logic inside `InterviewTab.tsx` or extract a small
local component in the same file. Do not introduce a new package.

Required frontend elements:

- `parseGdSpeakerTurns(raw)` pure function, exported for static contract tests.
- `GdThreadMessage` renderer that receives raw text and renders parsed turns.
- Each turn renders:
  - stable speaker label
  - deterministic avatar color using existing `avatarColor(name)` or existing
    CSS variables
  - a text block preserving newlines
- Fallback rendering for malformed output: one `GD` turn with raw text.
- Existing user bubbles, feedback bubbles, latency tag, report panel, custom
  theme textarea, GD lobby, and narrative draft panel remain intact.

Suggested DOM/CSS classes:

- `.gd-thread`
- `.gd-turn`
- `.gd-turn-avatar`
- `.gd-turn-body`
- `.gd-turn-speaker`
- `.gd-turn-text`

CSS must reuse existing variables such as `--panel`, `--border`, `--text`,
`--muted`, and existing avatar colors where possible. Avoid adding new hex color
tokens unless strictly necessary.

## 3. Prompt Change Specification

In `src/python/core/consultation_engine.py`, add a single reusable instruction
constant or helper near the GD prompt definitions:

```python
GD_OUTPUT_FORMAT_INSTRUCTION = """

# GD_FORMAT_V1
議論フェーズの応答は必ず次の行単位フォーマットだけで出力すること:
[話者名]: 発言内容

制約:
- 話者名は現在の参加者名のみを使う。
- 1回の応答には2〜5発言を含める。
- 行頭の [話者名]: 以外で話者を表さない。
- Markdown見出し、箇条書き、コードブロック、司会者の要約文を混ぜない。
- この制約は議論フェーズのみ。講評、終了、感想戦では通常の文章でよい。
"""
```

The exact wording may be adjusted for the existing file style, but the following
tokens are mandatory for testability:

- `GD_FORMAT_V1`
- `[話者名]: 発言内容`
- `議論フェーズ`
- `講評`
- `感想戦`

Backend integration rules:

- `GD_SYSTEM_PROMPT` must include the format instruction.
- `build_gd_system_prompt(personas)` must include the same format instruction
  for dynamic personas.
- `build_gd_system_prompt(None) == GD_SYSTEM_PROMPT` must remain true.
- Custom Theme injection remains a theme clause only. It must not override the
  thread format contract.
- `interview_sim` system prompts must not receive `GD_FORMAT_V1`.
- `es_review` must not receive `GD_FORMAT_V1`.

## 4. Tests

Composer must add or update tests so the feature is locked by contracts.

### 4.1 Backend Contract Tests

Add tests in `tests/test_integration.py` or a focused new pytest file:

1. `test_gd_prompt_requires_thread_format`
   - Assert `GD_SYSTEM_PROMPT` contains `GD_FORMAT_V1`.
   - Assert it contains `[話者名]: 発言内容`.
   - Assert it names the exception for `講評` and `感想戦`.
   - Assert `build_gd_system_prompt(None) == GD_SYSTEM_PROMPT`.

2. `test_dynamic_gd_prompt_requires_thread_format`
   - Pass two personas to `build_gd_system_prompt`.
   - Assert persona names are present.
   - Assert `GD_FORMAT_V1` is present.

3. `test_custom_theme_gd_preserves_thread_format`
   - Start `gd_sim` with `config={"customTheme": "..."}`.
   - Assert backend system prompt contains both sanitized theme and
     `GD_FORMAT_V1`.
   - Assert `interview_sim` and `es_review` prompts do not receive
     `GD_FORMAT_V1`.

### 4.2 Frontend Contract Tests

Add `tests/test_feature_gd_ui_contract.py` as a static UI contract test:

1. Assert `InterviewTab.tsx` exports `parseGdSpeakerTurns`.
2. Assert the parser uses a line-start speaker regex with a colon delimiter.
3. Assert the old broad split shape `\[[...]\]\s*` is not used as the primary
   GD parser.
4. Assert `renderAs: "gd_thread"` or equivalent is gated by
   `mode === "gd_sim"` and not used for `es_review` or `interview_sim`.
5. Assert `phase === "debrief"` remains excluded from GD thread rendering.
6. Assert no `fetch(`, `localStorage`, or new package import is introduced.

### 4.3 Validation Commands

Run these commands after implementation:

```powershell
py -3 -m pytest tests/test_feature_gd_ui_contract.py -q
py -3 -m pytest tests/test_integration.py -k "gd_prompt or custom_theme" -q
py -3 -m pytest tests/test_ui_smoke.py tests/test_probe_ui_contract.py tests/test_probe_ui_ipc.py -q
cd apps\desktop
npx.cmd tsc --noEmit
npm.cmd run build
```

If `py -3` is unavailable, use the project-local or known installed Python
binary exactly as used by the current workspace.

## 5. Stop Conditions

Stop and report without implementing if:

- `docs/SPEC_FEATURE_GD_UI.md` is missing or differs from the requested feature
  after checkout.
- Required files have unrelated user changes that make safe patching ambiguous.
- The change would require a new npm package.
- The change would require touching D1/D2/PROBE core files.
- The implementation would apply GD parsing to `interview_sim`, `es_review`,
  feedback, debrief, or mentor turns.
- Any test requires real network access or real personal data.

## 6. Definition Of Done

The feature is complete only when:

- `gd_sim` discussion responses are prompted with `GD_FORMAT_V1`.
- Dynamic personas and default personas both use the same speaker-line contract.
- Custom Theme and GD thread format coexist.
- Frontend renders GD discussion responses as separated speaker turns.
- Existing `interview_sim`, `es_review`, feedback, and debrief rendering remain
  unchanged.
- All required pytest, TypeScript, and build commands pass.
- `git diff --check` passes.
- No package dependency file changed.
