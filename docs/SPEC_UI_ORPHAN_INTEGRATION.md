# SPEC UI Orphan Integration

## 0. Purpose And Authority

This document is the implementation authority for wiring backend commands that
already exist but are not yet fully reachable from the React UI.

The implementation must not add new backend commands. It must consume the
existing stdio surface from `src/python/engine_stdio.py` through
`apps/desktop/src/lib/engine.ts`.

## 1. Audited Command Surface

### 1.1 Already Routed And Displayed

These commands are already represented in the current UI:

- `health`
- `settings.get`
- `settings.save_fixed`
- `settings.run_profiler`
- `record.load`
- `record.save`
- `calendar.event_dates`
- `consult`
- `calendar.sync`
- `import.line`
- `import.stats`
- `es.view`
- `import.classify`
- `import.document`
- `probe.status`
- `probe.next`
- `probe.answer`

### 1.2 Orphaned Or Under-Integrated

These commands exist in backend dispatch but lack a visible UI entry point or are
wrapped but unused by components:

| Command | Current State | Integration Decision |
|---|---|---|
| `profile.source_code` | wrapper exists (`sourceCode`) but no component calls it | new `PROFILE` tab, Source Code panel |
| `oracle.payload` | wrapper exists (`oraclePayload`) but no component calls it | new `PROFILE` tab, Echo Metrics panel, auto-load allowed because it is LLM-free |
| `oracle.report` | wrapper exists (`oracleReport`) but no component calls it | new `PROFILE` tab, explicit user button only |
| `twin.forecast` | wrapper exists (`twinForecast`) but no component calls it | new `PROFILE` tab, deterministic scenario form |
| `tensor.rebuild` | wrapper exists (`tensorRebuild`) but no component calls it | new `PROFILE` tab diagnostics, explicit user button only |
| `narrative.compile` | dispatch exists, no frontend wrapper | `INTERVIEW` tab idle-time Narrative panel |
| `knowledge.fetch_pending` | dispatch exists, no frontend wrapper | `IMPORT` tab knowledge queue action |

`shutdown` remains intentionally UI-internal/not user-facing.

## 2. Placement Strategy

### 2.1 New `PROFILE` Tab

Add a main tab:

```ts
export type MainTab =
  | "record"
  | "import"
  | "consult"
  | "interview"
  | "probe"
  | "profile"
  | "settings";
```

Tab order:

```text
RECORD / IMPORT / CONSULT / INTERVIEW / PROBE / PROFILE / SETTINGS
```

Keyboard shortcut range becomes `Alt+1..7`.

`PROFILE` owns read-heavy analytical views:

- `SourceCodePanel`: uses `profile.source_code`
- `EchoMetricsPanel`: uses `oracle.payload`
- `OracleReportPanel`: uses `oracle.report`, but only on explicit button click
- `TwinForecastPanel`: uses `twin.forecast`
- `TensorDiagnosticsPanel`: uses `tensor.rebuild`, explicit button only

### 2.2 Existing `INTERVIEW` Tab

Add `NarrativeDraftPanel` to the idle state. This panel calls
`narrative.compile` through a new frontend wrapper. It is a deliberate LLM action
and must never run on mount.

Output may display:

- `ok`
- `reason`
- `target_domain`
- `draft_path`
- `es_text`
- `recruiters_eye`
- claim count

Do not display raw evidence quotes or hidden source material.

### 2.3 Existing `IMPORT` Tab

Add a small `KnowledgeFetchPanel` near the knowledge source area or import log.
It calls `knowledge.fetch_pending` through a new wrapper.

The button text must make explicit that this is a user-triggered action. The
backend remains offline by default; if `online_allowed` is false, the command
only reports pending queue state and performs no network fetch.

## 3. Frontend Engine Contract

Add wrappers in `apps/desktop/src/lib/engine.ts`:

```ts
export interface NarrativeCompileResult {
  ok: boolean;
  es_text?: string;
  recruiters_eye?: string;
  claims?: unknown[];
  compiled_from?: string;
  target_domain?: string;
  draft_path?: string;
  reason?: string;
}

export async function narrativeCompile(
  targetDomain?: string,
): Promise<NarrativeCompileResult> {
  return pkbInvoke("narrative.compile", { target_domain: targetDomain ?? null });
}

export interface KnowledgeFetchSummary {
  processed: number;
  pending: number;
  online_allowed: boolean;
  index_rebuilt?: boolean;
  [key: string]: unknown;
}

export async function knowledgeFetchPending(): Promise<KnowledgeFetchSummary> {
  return pkbInvoke("knowledge.fetch_pending");
}
```

Existing wrappers for `sourceCode`, `oraclePayload`, `oracleReport`,
`twinForecast`, and `tensorRebuild` must be used by UI components.

`oracleReport` may be extended to accept an optional `cid` only if the component
also listens to `pkb-engine-event`. Do not invent a new command.

## 4. `ProfileTab` Component Requirements

Create `apps/desktop/src/components/ProfileTab.tsx`.

State:

- `sourceCode: SourceCodeView | null`
- `oracle: OraclePayload | null`
- `oracleAnalysis: string`
- `forecast: TwinForecast | null`
- `tensorResult: { rebuilt: boolean; rows: number } | null`
- `busy: string | null`
- `error: string`

On mount:

- call `sourceCode()`
- call `oraclePayload("global")`
- do not call `oracleReport()`
- do not call `twinForecast()`
- do not call `tensorRebuild()`

Display constraints:

- Source Code axes: show axis id, `score`, `confidence`, evidence count only.
- Do not display `EvidenceRef.quote`, `fact_text`, `line_text`, or raw LINE text.
- Oracle payload: display sterile IDs and numbers only: sufficiency, state,
  significant couplings, forecast critical days, finding rule IDs,
  intervention bank IDs.
- Do not display contact real names. If dyad support is added later, only
  `contact_alias` may be displayed.
- Oracle report text may be displayed only after explicit click.

Twin scenario form:

- `horizon_days`: number input, integer 1..60, default 14
- `mode`: select `"daily"` or `"interview"`
- `interview_turns`: integer 0..20, enabled for interview mode
- `calendar`: empty array in v1; do not auto-read UI calendar state

No `Date.now`, `Math.random`, `fetch`, `axios`, `localStorage`, or
`sessionStorage`.

## 5. Styling

Use existing tokens in `apps/desktop/src/App.css`.

Allowed new classes:

- `.profile-panel`
- `.profile-grid`
- `.profile-section`
- `.profile-metric-row`
- `.profile-axis-list`
- `.profile-axis-row`
- `.profile-report`
- `.profile-form-row`
- `.knowledge-fetch-panel`
- `.narrative-panel`

No new hex colors. No decorative animation. No nested cards.

## 6. Constitutional Guards

- Offline-first remains intact. The only network-capable path is
  `knowledge.fetch_pending`, and it is already guarded by
  `PKB_ALLOW_ONLINE_FETCH=1`; UI must expose it only as an explicit button.
- UI must not infer emotion or compute psychological scores. It displays backend
  numbers and IDs only.
- `oracle.payload` is the default display path because it is sterile and LLM-free.
  `oracle.report` is optional language output and must not drive metrics.
- D1/D2/PROBE core files are forbidden.
- No backend command additions.

## 7. Required Tests

Add `tests/test_ui_orphan_integration_contract.py`.

Required static tests:

1. `test_profile_tab_registered`
   - `MainTab` includes `"profile"`
   - `App.tsx` contains `{ id: "profile", label: "PROFILE" }`
   - `App.tsx` renders `<ProfileTab />`
   - shortcut regex accepts `[1-7]`

2. `test_orphan_engine_wrappers`
   - `engine.ts` exports `narrativeCompile`
   - `engine.ts` exports `knowledgeFetchPending`
   - command strings `"narrative.compile"` and `"knowledge.fetch_pending"` exist
   - existing command strings for source/oracle/twin/tensor remain present

3. `test_profile_tab_contract`
   - `ProfileTab.tsx` imports `sourceCode`, `oraclePayload`, `oracleReport`,
     `twinForecast`, `tensorRebuild`
   - `oraclePayload` is called on mount or refresh
   - `oracleReport` is not called inside the initial load effect
   - forbidden tokens are absent: `fetch(`, `axios`, `localStorage`,
     `sessionStorage`, `Math.random`, `Date.now`
   - forbidden display-source tokens are absent: `.quote`, `fact_text`,
     `line_text`

4. `test_existing_tabs_receive_orphan_actions`
   - `ImportTab.tsx` imports and calls `knowledgeFetchPending`
   - `InterviewTab.tsx` imports and calls `narrativeCompile`
   - neither panel uses `fetch(` or `localStorage`

Verification commands:

```powershell
python -m pytest tests/test_ui_orphan_integration_contract.py -q
python -m pytest tests/test_probe_ui_contract.py tests/test_probe_ui_ipc.py -q
python -m pytest tests/test_ui_smoke.py -q
cd apps\desktop
npx.cmd tsc --noEmit
npm.cmd run build
cd ..\..
git diff --check
git diff -- src/python/core/source_code.py src/python/core/probe_engine.py src/python/core/probe_funnel.py src/python/engine_stdio.py src/python/core/facade.py
git status --short data/
```

## 8. Allowed Implementation Files

- `apps/desktop/src/App.tsx`
- `apps/desktop/src/App.css`
- `apps/desktop/src/lib/types.ts`
- `apps/desktop/src/lib/engine.ts`
- `apps/desktop/src/components/ProfileTab.tsx`
- `apps/desktop/src/components/ImportTab.tsx`
- `apps/desktop/src/components/InterviewTab.tsx`
- `tests/test_ui_orphan_integration_contract.py`

No other file may be edited during implementation.

## 9. Stop Conditions

Stop before editing if:

- this spec is missing
- a backend command addition appears necessary
- `engine_stdio.py` or `facade.py` appears necessary
- a new dependency appears necessary
- displaying raw evidence text appears necessary
- D1/D2/PROBE core files appear necessary
