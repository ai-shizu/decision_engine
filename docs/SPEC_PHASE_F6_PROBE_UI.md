# SPEC Phase F6 PROBE Tab UI

Phase F6 connects the completed D1/D2 deterministic backend to the Tauri + React
desktop UI. This is a UI and IPC wiring phase only. It must not change D1 scoring
logic, D2 candidate/session logic, or any privacy rule.

The PROBE tab is an operational data-entry surface: it shows the next static
question selected by `core.probe_funnel`, accepts one short answer, advances the
FACT -> CONTEXT -> EMOTION -> MEANING funnel, and refreshes deterministic status.

## 1. UI Architecture And Tauri IPC Wiring

### 1.1 Boundaries

React responsibilities:
- Render current PROBE state.
- Show the backend-selected static question.
- Enforce basic input validation (`trim()`, 120 character limit).
- Submit an answer through Tauri IPC.
- Refresh state after successful submit.

React must not:
- Recompute axis priority.
- Infer emotion or sentiment from text.
- Choose questions locally.
- Write to disk directly.
- Call `fetch`, `axios`, remote APIs, CDN resources, localStorage, or Tauri store.
- Display third-party real names.

Backend responsibilities:
- Load daily contexts and probe store through `core.paths`.
- Compute or load `HumanSourceCode`.
- Call `core.probe_funnel.probe_next_question`, `record_probe_answer`,
  `derive_probe_candidates`, and `derive_probe_insights`.
- Persist `probe_store.json` atomically.
- Sanitize answer text and alias third-party names before it reaches UI state.

### 1.2 Required Backend Facade Functions

Composer must add only thin orchestration functions to `core.facade`.

```python
def get_source_code() -> dict:
    """Return source_code.to_dict() for UI display. LLM-free."""

def probe_status(today: str | None = None) -> dict:
    """Return current store/candidate/session summary without starting a session."""

def probe_next(today: str) -> dict:
    """Start or continue the deterministic next session and return the next question."""

def probe_answer(session_id: str, question_id: str, answer: str, today: str) -> dict:
    """Record one answer, persist store, and return updated status plus next question if any."""
```

`today` is supplied by the frontend as a local ISO date string (`YYYY-MM-DD`).
Do not call `date.today()` inside core logic for this phase. If backend receives
an invalid date string, raise `ValueError`; `engine_stdio.py` will convert it to
`{"ok": false, "error": ...}`.

Suggested `probe_status` result:

```json
{
  "schema": "probe_status.v1",
  "today": "2026-07-09",
  "axes": [
    {
      "axis": "decision_threshold",
      "score": 0.71,
      "confidence": 0.5,
      "priority": 0.55,
      "stage": "FACT",
      "node_count": 0,
      "open_session_id": null
    }
  ],
  "active_session": null,
  "insights": [
    {
      "kind": "low_confidence",
      "axis": "decision_threshold",
      "stage": "FACT",
      "priority": 0.55,
      "message_code": "probe.low_confidence"
    }
  ],
  "progress": {
    "completed_stages": 0,
    "total_stages": 20,
    "percent": 0
  }
}
```

Suggested `probe_next` result:

```json
{
  "schema": "probe_question.v1",
  "session_id": "ps-...",
  "question_id": "pq-decision_threshold-FACT-01",
  "axis": "decision_threshold",
  "stage": "FACT",
  "question": "直近で先延ばししたタスクを一つ、事実だけで書いてください。",
  "priority": 0.55
}
```

Suggested `probe_answer` result:

```json
{
  "schema": "probe_answer_result.v1",
  "saved": true,
  "node_id": "hn-...",
  "session_status": "active",
  "next_question": { "...": "probe_question.v1 or null when closed" },
  "status": { "...": "probe_status.v1" }
}
```

### 1.3 `engine_stdio.py` Commands

Add dispatch cases:

```python
if cmd == "profile.source_code":
    return facade.get_source_code()
if cmd == "probe.status":
    return facade.probe_status(params.get("today"))
if cmd == "probe.next":
    return facade.probe_next(params["today"])
if cmd == "probe.answer":
    return facade.probe_answer(
        params["session_id"],
        params["question_id"],
        params["answer"],
        params["today"],
    )
```

No streaming `status` or `chunk` events are needed. PROBE operations are fast
deterministic store updates. If a future implementation adds status events, they
must use the existing `cid` envelope and cleanup rules, but F6 should avoid that
complexity.

### 1.4 `apps/desktop/src/lib/engine.ts`

Add typed wrappers only. Use the existing `pkbInvoke` path.

```ts
export async function sourceCode(): Promise<SourceCodeView> {
  return pkbInvoke("profile.source_code");
}

export async function probeStatus(today: string): Promise<ProbeStatus> {
  return pkbInvoke("probe.status", { today });
}

export async function probeNext(today: string): Promise<ProbeQuestionView> {
  return pkbInvoke("probe.next", { today });
}

export async function probeAnswer(
  sessionId: string,
  questionId: string,
  answer: string,
  today: string,
): Promise<ProbeAnswerResult> {
  return pkbInvoke("probe.answer", {
    session_id: sessionId,
    question_id: questionId,
    answer,
    today,
  });
}
```

Do not add HTTP clients, direct file reads, or local persistence.

### 1.5 TypeScript Types

Add these to `apps/desktop/src/lib/types.ts`.

```ts
export type ProbeAxis =
  | "decision_threshold"
  | "reward_bias"
  | "locus_of_control"
  | "unlearning_rate"
  | "friction_energy_ledger";

export type ProbeStage = "FACT" | "CONTEXT" | "EMOTION" | "MEANING";

export interface ProbeAxisStatus {
  axis: ProbeAxis;
  score: number | null;
  confidence: number;
  priority: number;
  stage: ProbeStage;
  node_count: number;
  open_session_id: string | null;
}

export interface ProbeInsightView {
  kind: "low_confidence" | "under_probed" | "stage_complete";
  axis: ProbeAxis;
  stage: ProbeStage;
  priority: number;
  message_code: "probe.low_confidence" | "probe.under_probed" | "probe.stage_complete";
}

export interface ProbeProgress {
  completed_stages: number;
  total_stages: number;
  percent: number;
}

export interface ProbeStatus {
  schema: "probe_status.v1";
  today: string;
  axes: ProbeAxisStatus[];
  active_session: {
    id: string;
    axis: ProbeAxis;
    stage: ProbeStage;
    status: "active" | "closed";
  } | null;
  insights: ProbeInsightView[];
  progress: ProbeProgress;
}

export interface ProbeQuestionView {
  schema: "probe_question.v1";
  session_id: string;
  question_id: string;
  axis: ProbeAxis;
  stage: ProbeStage;
  question: string;
  priority: number;
}

export interface ProbeAnswerResult {
  schema: "probe_answer_result.v1";
  saved: boolean;
  node_id: string;
  session_status: "active" | "closed";
  next_question: ProbeQuestionView | null;
  status: ProbeStatus;
}
```

Also extend:

```ts
export type MainTab = "record" | "import" | "consult" | "interview" | "probe" | "settings";
```

## 2. PROBE Tab Screen And Component Hierarchy

### 2.1 App Integration

In `App.tsx`, insert PROBE between INTERVIEW and SETTINGS:

```ts
const TABS = [
  { id: "record", label: "RECORD" },
  { id: "import", label: "IMPORT" },
  { id: "consult", label: "CONSULT" },
  { id: "interview", label: "INTERVIEW" },
  { id: "probe", label: "PROBE" },
  { id: "settings", label: "SETTINGS" },
];
```

Keyboard:
- Update Alt tab switching from `Alt+1..5` to `Alt+1..6`.
- Keep ordering aligned with the visible tab order.
- Do not steal `Ctrl+Enter`; it belongs to answer submit inside PROBE.

Render:

```tsx
{tab === "probe" && <ProbeTab />}
```

### 2.2 Component Tree

Create `apps/desktop/src/components/ProbeTab.tsx`.

```text
ProbeTab
├─ ProbeTopline
│  ├─ progress percent
│  ├─ active axis/stage
│  └─ refresh button
├─ ProbeAxisRail
│  └─ ProbeAxisRow × 5
├─ ProbeFunnelStepper
│  └─ FACT / CONTEXT / EMOTION / MEANING
├─ ProbeQuestionPanel
│  ├─ static question text
│  ├─ AnswerEditor
│  └─ submit button
└─ ProbeInsightList
   └─ static message-code mapped rows
```

No nested cards. Use one top-level `<section className="panel probe-panel">`.
Inside it, use restrained blocks (`probe-grid`, `probe-main`, `probe-side`) and
at most one `.term-panel` for the current question. This follows the Foxtrot
`term-` layer: PROBE is one of the allowed terminal-style tabs.

### 2.3 Layout

Desktop:
- Two-column grid:
  - main column: question, answer editor, funnel stepper.
  - side column: axis rail and insights.
- Max width follows existing `.content` (`960px`); do not widen the shell for
  this phase.

Mobile/narrow:
- Single column.
- Axis rail appears above insights.
- Answer editor remains visible without horizontal scrolling.

CSS class names:

```css
.probe-panel
.probe-topline
.probe-grid
.probe-main
.probe-side
.probe-axis-list
.probe-axis-row
.probe-axis-row.active
.probe-axis-metric
.probe-stepper
.probe-step
.probe-step.active
.probe-step.done
.probe-question
.probe-answer
.probe-char-counter
.probe-insight-list
.probe-error
```

Use existing CSS variables only (`--bg`, `--bg-raised`, `--bg-deep`, `--border`,
`--text`, `--text-muted`, `--accent`, `--ok`, `--err-soft`, spacing tokens).
No new hex colors, shadows, gradients, animations, or UI libraries.

### 2.4 Visual Content

Axis labels:

```ts
const AXIS_LABELS: Record<ProbeAxis, string> = {
  decision_threshold: "意思決定閾値",
  reward_bias: "報酬系の偏り",
  locus_of_control: "統制の所在",
  unlearning_rate: "アンラーニング速度",
  friction_energy_ledger: "摩擦収支",
};
```

Stage labels:

```ts
const STAGE_LABELS: Record<ProbeStage, string> = {
  FACT: "FACT",
  CONTEXT: "CONTEXT",
  EMOTION: "EMOTION",
  MEANING: "MEANING",
};
```

Message-code mapping:

```ts
const INSIGHT_LABELS = {
  "probe.low_confidence": "観測不足",
  "probe.under_probed": "未探索",
  "probe.stage_complete": "完了",
};
```

Do not show raw D1 evidence quotes in PROBE UI. Question text is enough. Insight
rows display `axis`, `stage`, `priority`, and the static label only.

## 3. Frontend State And Session Requirements

### 3.1 Local State

`ProbeTab` should keep one copy of server state:

```ts
const [status, setStatus] = useState<ProbeStatus | null>(null);
const [question, setQuestion] = useState<ProbeQuestionView | null>(null);
const [answer, setAnswer] = useState("");
const [busy, setBusy] = useState(false);
const [error, setError] = useState("");
```

Do not duplicate axis priority, completed stages, or session state in derived
React state. Derive display values from `status` and `question` during render.

### 3.2 Today String

Add a local helper in `ProbeTab.tsx` or reuse an existing date utility if one is
already present:

```ts
function todayIsoLocal(): string {
  const d = new Date();
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}
```

Never use `new Date().toISOString().slice(0, 10)`; UTC date drift is a known
project bug pattern.

### 3.3 Mount Flow

On mount:

```ts
useEffect(() => {
  let disposed = false;
  async function load() {
    setBusy(true);
    try {
      const today = todayIsoLocal();
      const s = await probeStatus(today);
      if (!disposed) setStatus(s);
    } catch (err) {
      if (!disposed) setError(String(err));
    } finally {
      if (!disposed) setBusy(false);
    }
  }
  void load();
  return () => { disposed = true; };
}, []);
```

This mirrors existing unmount discipline. No event listener is required for F6.

### 3.4 Start Or Continue

Primary button:
- Label when no question: `次の質問`
- Label while submitting: `保存中…`
- Disabled when `busy`.

Click:

```ts
const q = await probeNext(todayIsoLocal());
setQuestion(q);
const s = await probeStatus(todayIsoLocal());
setStatus(s);
setAnswer("");
```

If backend returns an active session, UI continues it. React must not choose a
different axis even if another row has higher priority.

### 3.5 Answer Editor

Use a `<textarea>`:
- `rows={4}`
- `maxLength={120}`
- disabled while `busy`
- value controlled by `answer`
- submit shortcut: `Ctrl+Enter` or `Meta+Enter`

Validation:
- `answer.trim().length > 0`
- `answer.length <= 120` (also enforced by `maxLength`)
- `question !== null`

Counter:
- show mono `N/120`
- use `.probe-char-counter`
- if `N === 120`, use `error-text` or `--err-soft` only; no new color.

Submit:

```ts
const res = await probeAnswer(
  question.session_id,
  question.question_id,
  answer.trim(),
  todayIsoLocal(),
);
setStatus(res.status);
setQuestion(res.next_question);
setAnswer("");
```

If `res.next_question === null`, show a compact closed-session status and a
`次の質問` button. Do not auto-start a new session after closure; make the user
explicitly click.

### 3.6 Error Handling

On error:
- Display a single `<p className="error-text probe-error">`.
- Do not include raw answer text in the error.
- Keep the current answer in the textarea so the user can retry.
- Clear error on next successful call or answer edit.

### 3.7 Privacy Guarantees

UI must assume backend has sanitized, but still must not introduce leaks:
- Never render `EvidenceRef.quote` in PROBE tab.
- Never render `HistoricalNode.fact_text` except the just-submitted sanitized
  `text_quote` if backend returns it in future; F6 v1 does not need history text.
- Never show LINE raw text.
- If a string matching a `contact_alias` appears, render it as-is.
- Do not display third-party real names from client-side fixtures or examples.

### 3.8 Accessibility And Keyboard

- Current question panel has `aria-live="polite"` so new static questions are
  announced without stealing focus.
- After `probeNext`, focus the textarea.
- After successful submit with `next_question`, keep focus in textarea.
- If session closes, focus the `次の質問` button.
- Buttons use real `<button>` elements; no clickable `<div>`.

## 4. Mandatory Tests For Composer

No new JS test framework is allowed in F6. Verification uses existing Python
pytest for backend/UI contract checks plus TypeScript compilation.

### 4.1 `tests/test_probe_ui_ipc.py::test_probe_stdio_contract`

Use `tests/conftest.py` sandbox. Call `engine_stdio.dispatch` directly.

Seed minimal D1/D2 data through public core functions or monkeypatch facade
loaders. Assert:
- `dispatch("probe.status", {"today": "2026-07-09"})` returns
  `schema == "probe_status.v1"`.
- `dispatch("probe.next", {"today": "2026-07-09"})` returns
  `schema == "probe_question.v1"` and a `question_id` from
  `PROBE_QUESTION_BANK`.
- `dispatch("probe.answer", {...})` returns
  `schema == "probe_answer_result.v1"` and `saved is True`.
- No result contains `山田太郎`; sanitized alias `C-...` is allowed.

### 4.2 `tests/test_probe_ui_ipc.py::test_probe_ipc_does_not_call_llm`

Patch a backend `.generate()` that raises. Exercise `profile.source_code`,
`probe.status`, `probe.next`, and `probe.answer`.

Assert:
- `.generate()` is never called.
- No serialized result contains fields named `sentiment`, `valence`,
  `emotion_score`, or `inferred_emotion`.

### 4.3 `tests/test_probe_ui_contract.py::test_probe_tab_registered`

Static-read `apps/desktop/src/App.tsx` and `apps/desktop/src/lib/types.ts`.

Assert:
- `MainTab` union contains `"probe"`.
- `TABS` contains `{ id: "probe", label: "PROBE" }`.
- `App.tsx` renders `<ProbeTab />`.
- Alt shortcut accepts `1-6`, not only `1-5`.

### 4.4 `tests/test_probe_ui_contract.py::test_probe_engine_wrappers`

Static-read `apps/desktop/src/lib/engine.ts`.

Assert:
- Exports `sourceCode`, `probeStatus`, `probeNext`, `probeAnswer`.
- Wrapper command strings are exactly:
  - `"profile.source_code"`
  - `"probe.status"`
  - `"probe.next"`
  - `"probe.answer"`
- `probeAnswer` sends snake_case keys: `session_id`, `question_id`, `answer`,
  `today`.

### 4.5 `tests/test_probe_ui_contract.py::test_probe_tab_privacy_and_validation`

Static-read `apps/desktop/src/components/ProbeTab.tsx`.

Assert:
- Contains `maxLength={120}`.
- Contains a `Ctrl+Enter` or `Meta+Enter` submit path.
- Does not contain `fetch(`, `axios`, `localStorage`, `sessionStorage`,
  `Math.random`, or `toISOString().slice(0, 10)`.
- Does not render strings named `EvidenceRef`, `fact_text`, or `line_text`.
- Imports only React, local engine/types/helpers, and no new UI libraries.

### 4.6 TypeScript Gate

Run from `apps/desktop`:

```powershell
npx tsc --noEmit
```

Expected:
- No type errors.
- No new dependencies in `package.json`.

### 4.7 Manual Smoke Checklist

Composer must report these observations after running the app:
- PROBE tab appears between INTERVIEW and SETTINGS.
- `Alt+5` selects PROBE and `Alt+6` selects SETTINGS.
- Initial load shows axis rows and progress without starting a session.
- `次の質問` shows a static question.
- Empty answer cannot submit.
- 120 character counter stops input at 120.
- `Ctrl+Enter` submits a valid answer.
- FACT advances to CONTEXT after submit.
- No raw D1 quote or third-party real name appears on screen.

F6 GREEN requires 4.1 through 4.6. The manual smoke checklist is required in the
implementation report because this repository currently has no React test
runner.
