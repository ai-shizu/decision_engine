# Project Calculus: Tensor Profiling Advanced Edition

> Status: DESIGN FROZEN for Phase 1 (history) / Phase 2 measured wiring AS-BUILT
> Scope: long-session context compression, private-reasoning redaction, evidence-backed six-dimensional profiling, and zero-dependency SVG visualization
> Phase 1: frontend defenses and preview visualization only (historical)
> Phase 2: Python backend, memory compiler, prompt policy, validated scoring, and real data wiring

## 現行優先裁定 (Finding 9 / 2026-07-12)

Phase 1 の PROFILE 固定 preview（`TENSOR_RADAR_PREVIEW` / `preview` prop /
`PHASE 1 PREVIEW / NOT MEASURED`）は **退役済み**。Phase 2 で Interview/GD の
`MISSION_RESULT` → `TensorProfilePanel` → 検証済み `report.tensor_profile` が
実測 6D の唯一の UI 出口となったため、PROFILE 上の幾何検証用モックは撤去した。
`TensorRadarChart` の `preview` prop も削除済み。復元・再導入は禁止。
PROFILE 用の genre 横断「最新成績表」IPC / 永続化 / latest 選択は新設しない。
下記 §7 等の Phase 1 preview 記述は **履歴** として保持する（現行要件ではない）。

## 0. Authority and architectural rulings

This specification is subordinate to `docs/AI_SKILLS.md` and the privacy and
data-isolation invariants already frozen for D1/D2/PROBE. If an illustrative
example conflicts with those invariants, the invariant wins.

The following rulings are normative:

1. **No forced disclosure of chain-of-thought.** The draft requirement to force
   every model response to begin with `<think>...</think>` is rejected as a
   runtime contract. `AI_SKILLS.md` requires model-independent internal
   verification instructions, and forcing verbose private reasoning increases
   output tokens and latency while intrinsic self-correction is not reliably
   beneficial without external feedback. The system may receive `<think>` from
   reasoning models, but must treat it as untrusted private material and remove
   it at every output boundary.
2. **Defense in depth.** Phase 1 removes hidden-reasoning blocks before React
   rendering, including while tags are split across streaming chunks. Phase 2
   must also redact them in Python before IPC, persistence, reports, or indexing.
   CSS hiding alone is forbidden because the hidden content would remain in the
   DOM.
3. **No summary-as-truth.** Semantic compression produces evidence-bearing
   `MemoryAtom` records linked to immutable transcript turns. A generated
   paraphrase is never accepted as a fact without an exact source quote.
4. **No fake profile.** Phase 1 uses deterministic preview values only to prove
   chart geometry. The UI must visibly label them `PREVIEW / NOT MEASURED` and
   must not describe them as the user's score. Real scores are a Phase 2 input.
5. **No proprietary-rubric claim.** The six dimensions are a PKB rubric derived
   from observable skills in public McKinsey, BCG, and Bain candidate guidance.
   They are not represented as any firm's confidential scoring system.
6. **Evidence first.** A real dimension with insufficient evidence has
   `score=null`; it is never converted to `0.00`. Confidence measures evidence
   coverage, not model certainty.

## 1. Goals and non-goals

### 1.1 Goals

- Bound the dynamic context of long interview and GD sessions so prompt growth
  does not scale linearly with transcript length.
- Preserve the raw transcript as the audit source while selecting a compact,
  relevant working set for each inference.
- ensure private reasoning never reaches the visible React tree, even when an
  opening tag is split across streaming chunks.
- Represent interview performance as six continuous dimensions in `[0.00, 1.00]`
  with transcript-verifiable evidence and evidence-derived confidence.
- Render the six-dimensional vector with React and native SVG only.
- Preserve `gd_sim` thread rendering, `interview_sim`, `es_review`, feedback,
  debrief, and mentor behavior.

### 1.2 Non-goals

- Phase 1 does not modify prompts, Python, IPC schemas, stores, or real scores.
- This feature does not alter D1 `HumanSourceCode`, D2 PROBE, HistoricalNode,
  Oracle, Digital Twin, or existing tensor binaries.
- The chart is not a personality diagnosis, emotion inference, hiring result, or
  prediction of acceptance.
- Hidden reasoning is not stored, indexed, displayed on demand, or exposed by a
  debug toggle.
- No external API, CDN, telemetry service, npm package, font, or chart library is
  introduced.

## 2. Research and practice basis

The design adopts the following findings conservatively:

- Hierarchical memory can separate a small active context from a larger cold
  store, analogous to virtual memory. PKB applies this as immutable transcript,
  bounded working memory, and deterministic retrieval tiers.
- Prompt-compression evaluations show that extractive methods can outperform
  more elaborate token pruning in some long-context tasks. Therefore PKB keeps
  exact source spans and removes whole low-priority units rather than deleting
  arbitrary tokens inside evidence.
- Learned compression can reduce latency, but semantic similarity alone is not a
  sufficient integrity guarantee. PKB requires source-turn and quote validation.
- Intrinsic self-correction can degrade reasoning without external feedback.
  PKB's verification loop therefore checks observable constraints, arithmetic,
  quote existence, and contradictions instead of trusting a model's statement
  that it reviewed itself.
- Public case-interview guidance consistently emphasizes structuring, thoughtful
  questions, data analysis and calculations, logical reasoning, practical
  synthesis, clear communication, creativity, and constructive collaboration.
  The six PKB dimensions group those observable behaviors without claiming to
  reproduce a firm's internal rubric.

## 3. End-state architecture and phase split

```text
Immutable session transcript
        |
        +--> Phase 2 Memory Compiler --> validated MemoryAtom ledger
        |                                  |
        |                                  +--> bounded WorkingMemoryV1
        |                                  +--> deterministic retrieval
        |
        +--> Phase 2 Profile Evaluator --> evidence proposals
                                           |
                                           +--> quote/range validator
                                           +--> deterministic aggregator
                                           +--> TensorProfile6D

Local LLM stream
        |
        +--> Phase 2 Python redactor --> stdio IPC
                                          |
                                          +--> Phase 1 React redactor
                                                    |
                                                    +--> plain renderer
                                                    +--> GD thread renderer

TensorProfile6D --(Phase 2 IPC)--> TensorRadarChart
Phase 1 uses explicit preview data at this final edge only.
```

### 3.1 Phase 1, authorized now

- Add a pure, deterministic hidden-reasoning redactor in the frontend.
- Apply the redactor to AI and feedback text before either plain or GD-thread
  rendering.
- Add `TensorRadarChart.tsx`, implemented with `<svg>` and trigonometry.
- Mount a clearly labeled preview in `ProfileTab.tsx`.
- Add only the CSS and frontend contract tests required for these two features.

### 3.2 Phase 2, explicitly deferred

- Python streaming redaction before IPC.
- Internal-verification prompt revision without forcing chain-of-thought output.
- `MemoryAtom` extraction, validation, bounded working memory, and retrieval.
- Six-dimensional profile generation, validation, persistence, and IPC.
- Replacement of Phase 1 preview data with a backend `TensorProfile6D` payload.
- Backend, performance, privacy, and end-to-end tests.

## 4. Semantic compression and working memory, Phase 2 contract

### 4.1 Memory tiers

| Tier | Contents | Prompt policy |
|---|---|---|
| L0 Current | current user turn and scenario control state | always included |
| L1 Exact tail | most recent complete turns | always included within budget |
| L2 Working memory | validated active atoms | included by fixed priority |
| L3 Retrieved evidence | older exact source spans | top-k deterministic retrieval |
| L4 Cold transcript | full immutable transcript | never injected wholesale |

The transcript remains the source of truth. Compression changes prompt
selection, not stored history.

### 4.2 Data structures

Phase 2 should introduce structures equivalent to the following. Names may be
adjusted only to match an established local module pattern; field semantics may
not change.

```python
from dataclasses import dataclass, field
from typing import Literal

MemoryKind = Literal[
    "goal", "claim", "datum", "constraint", "assumption",
    "decision", "open_question", "contradiction", "recommendation",
]

@dataclass(frozen=True)
class TranscriptRef:
    turn_id: str
    turn_index: int
    speaker_alias: str
    quote: str                 # exact substring, 120 characters maximum

@dataclass(frozen=True)
class MemoryAtom:
    atom_id: str               # blake2b of normalized fields and source ref
    kind: MemoryKind
    canonical_text: str        # 240 characters maximum
    source: TranscriptRef
    subject_key: str
    status: Literal["active", "superseded"] = "active"
    superseded_by: str | None = None

@dataclass(frozen=True)
class WorkingMemoryV1:
    schema: Literal["working_memory.v1"]
    session_id: str
    transcript_version: int
    active_goal_ids: tuple[str, ...]
    constraint_ids: tuple[str, ...]
    decision_ids: tuple[str, ...]
    open_question_ids: tuple[str, ...]
    contradiction_ids: tuple[str, ...]
    supporting_atom_ids: tuple[str, ...]
    context_chars: int
```

Third-party names must be converted to existing `contact_alias` values before a
`TranscriptRef` or `MemoryAtom` is persisted. The candidate uses the fixed alias
`candidate`; simulated participants use their scenario aliases.

### 4.3 Evidence-preserving extraction

1. Append the new raw turn to the immutable transcript.
2. Build sentence candidates without deleting text inside a sentence.
3. A local extractor may propose typed atoms using temperature `0`, a fixed seed,
   fixed prompt version, and a fixed model hash. Its output is untrusted.
4. Accept a proposal only if `turn_id` exists and `quote` is an exact substring of
   that turn after the same Unicode normalization used by the validator.
5. Reject missing quotes, quotes over 120 characters, unknown kinds, overlong
   canonical text, real names, invalid aliases, and unsupported control fields.
6. Assign `atom_id` deterministically. Sort accepted atoms by
   `(turn_index, kind, atom_id)` before persistence.
7. Never edit or delete an atom. A correction appends another atom and marks the
   old atom as superseded through `superseded_by`.
8. Extractor failure does not block the interview. Continue with L0 and L1 exact
   context and retain the last valid working-memory version.

An atom records what was said, assumed, decided, or left unresolved. It does not
promote a user's claim to objective truth.

### 4.4 Fixed prompt budgets

All limits count Unicode code points after normalization; no new tokenizer
dependency is permitted.

```text
DYNAMIC_CONTEXT_CHAR_BUDGET = 12_000
CURRENT_TURN_CHAR_BUDGET     =  2_400
EXACT_TAIL_CHAR_BUDGET       =  3_600
WORKING_MEMORY_CHAR_BUDGET   =  4_000
RETRIEVED_EVIDENCE_BUDGET    =  2_000
MAX_ACTIVE_MEMORY_ATOMS      =     24
MAX_RETRIEVED_ATOMS          =      8
COMPILE_AFTER_TURNS          =      4
COMPILE_TRIGGER_CHARS        =  8_000
```

The static system/profile prefix is separately capped and remains stable so the
existing KV-prefix cache can be reused. The current turn is never truncated
silently: UI/backend input validation must reject over-limit input before this
selector runs.

### 4.5 Deterministic retrieval

Tokenization uses normalized Unicode character trigrams so Japanese text needs no
new morphological package. For each active atom `a`:

```text
overlap(a)  = Jaccard(trigrams(query + active_goal), trigrams(a.text + a.quote))
recency(a)  = 1 / (1 + max(0, current_turn_index - a.turn_index))
kind(a)     = fixed lookup:
              contradiction=1.00, open_question=0.95, constraint=0.90,
              decision=0.85, goal=0.80, assumption=0.70,
              datum=0.65, claim=0.55, recommendation=0.50
priority(a) = 0.55*overlap(a) + 0.25*recency(a) + 0.20*kind(a)
```

Select by `(-priority, -turn_index, atom_id)` until either the item or character
budget is reached. No randomness and no wall-clock time enter selection.

### 4.6 Compression observability

Only sterile numeric metadata may be logged or shown:

```json
{
  "schema": "context_budget.v1",
  "transcript_turns": 84,
  "dynamic_context_chars": 11620,
  "exact_tail_turns": 4,
  "working_memory_atoms": 20,
  "retrieved_atoms": 6,
  "compression_ratio": 0.18,
  "memory_version": 21
}
```

No quotes, prompts, hidden reasoning, names, or transcript text belong in this
telemetry.

## 5. Hidden-reasoning redaction protocol

### 5.1 Security property

For any accumulated model output `raw`, no character between an opening
`<think>` and its matching closing `</think>` may appear in the rendered DOM.
During streaming, a trailing partial opener such as `<`, `<th`, or `<think` must
also remain withheld until it is known not to be the tag. This is a fail-closed
privacy boundary.

### 5.2 Phase 1 frontend function

`InterviewTab.tsx` must export a pure function with this contract:

```ts
export function redactHiddenReasoning(raw: string, streaming = false): string
```

Required behavior:

- Scan once from left to right; runtime is `O(n)` and output storage is `O(n)`.
- Recognize exact case-insensitive `<think>` and `</think>` markers.
- Support multiple blocks and nested open markers by maintaining depth.
- Remove the markers themselves.
- Ignore unmatched closing markers rather than rendering them.
- If an opening marker is never closed, hide from that opener to end-of-input in
  both streaming and final states.
- In streaming state, withhold the longest trailing suffix that is a prefix of
  `<think>` so split chunks cannot flash private text or marker fragments.
- Preserve visible text byte-for-byte except for marker-adjacent whitespace that
  belongs wholly to the hidden block. Do not call `trim()` on the whole response.
- Return the same result for the same accumulated string. Do not use time,
  randomness, browser storage, or network calls.

React already stores accumulated chunk text in the streaming placeholder. The
renderer must call the function on that accumulated text; it must not parse each
chunk in isolation.

### 5.3 Renderer ordering

The mandatory order is:

```text
AI/feedback raw accumulated text
  -> redactHiddenReasoning
  -> if renderAs == gd_thread: parseGdSpeakerTurns
  -> React text nodes
```

This order preserves the existing GD parser while ensuring hidden content cannot
create fake speaker headers. User-authored messages are not passed through this
redactor. `interview_sim`, `es_review`, feedback, debrief, and mentor output keep
their current renderer; only private blocks disappear.

The raw hidden text must not be inserted into the DOM with `display:none`,
`visibility:hidden`, an `aria-hidden` element, a tooltip, `data-*`, or a debug
panel.

### 5.4 Phase 2 backend boundary

Python must use an incremental equivalent before invoking the stdio `chunk`
callback. It must carry partial-marker state across token callbacks and send only
visible deltas. The final response must be redacted before logging, report
generation, indexing, or return. Frontend redaction remains as a second boundary.

Prompt policy should request concise internal verification of assumptions,
counterevidence, arithmetic, and unanswered constraints, followed by only the
answer. It must not require the model to reveal a scratchpad or to emit a
particular hidden tag.

## 6. Six-dimensional tensor profile

### 6.1 Dimensions

| ID | Label | Observable signals |
|---|---|---|
| `problem_structuring` | Problem Structuring | clarifies the objective, decomposes the problem, prioritizes key branches |
| `quantitative_rigor` | Quantitative Rigor | states units and assumptions, performs correct calculations, interprets data |
| `hypothesis_evidence` | Hypothesis & Evidence | forms testable hypotheses, seeks disconfirming evidence, updates from facts |
| `synthesis_judgment` | Synthesis & Judgment | identifies implications, weighs trade-offs, gives practical recommendations |
| `communication` | Communication | concise signposting, clear conclusions, persuasive and comprehensible delivery |
| `collaboration_adaptability` | Collaboration & Adaptability | listens, builds on others, handles prompts or challenge, redirects constructively |

These dimensions synthesize publicly described behaviors. They are PKB's own
observable rubric.

### 6.2 Data structures

```python
DimensionId = Literal[
    "problem_structuring", "quantitative_rigor", "hypothesis_evidence",
    "synthesis_judgment", "communication", "collaboration_adaptability",
]

@dataclass(frozen=True)
class TensorEvidence:
    evidence_id: str
    dimension_id: DimensionId
    indicator_id: str
    level: int                 # integer 0..4 from fixed rubric
    turn_id: str
    turn_index: int
    speaker_alias: str
    quote: str                 # exact substring, <=120 characters

@dataclass(frozen=True)
class TensorDimension:
    dimension_id: DimensionId
    score: float | None        # rounded to 2 decimals
    confidence: float          # rounded to 2 decimals
    evidence: tuple[TensorEvidence, ...]

@dataclass(frozen=True)
class TensorProfile6D:
    schema: Literal["tensor_profile.6d.v1"]
    session_id: str
    transcript_hash: str
    model_hash: str
    prompt_version: str
    dimensions: tuple[TensorDimension, ...]  # fixed table order
```

`generated_at` is intentionally absent from scoring identity. If the UI needs a
date, it must be supplied as an explicit session date and must not affect scores.

### 6.3 Fixed rubric levels

```text
0 = observed behavior materially contradicts the indicator
1 = attempted but incorrect, unsupported, or abandoned
2 = partially demonstrated; important gap remains
3 = clearly demonstrated with minor omission
4 = clearly and consistently demonstrated under challenge

level_score = level / 4
```

Each dimension has exactly three indicator IDs corresponding to the three signal
phrases in the table. Repeated verbosity must not inflate a score. Aggregate all
accepted evidence for one indicator by the median level, then take the arithmetic
mean across observed indicators.

```text
indicator_score_i = median(level_e / 4 for evidence e in indicator i)
dimension_score   = mean(indicator_score_i for observed indicators i)
```

A score is valid only when evidence covers at least two distinct indicators and
at least two distinct candidate turns. Otherwise `score=null`.

Confidence is deterministic and independent of model self-confidence:

```text
indicator_coverage = observed_indicator_count / 3
turn_coverage      = min(distinct_candidate_turns, 4) / 4
evidence_coverage  = min(accepted_evidence_count, 6) / 6

confidence = min(1,
    0.40*indicator_coverage +
    0.35*turn_coverage +
    0.25*evidence_coverage)
```

Round only final score and confidence with decimal half-up to two places. An
LLM may propose evidence and levels, but the validator performs quote matching,
minimum-evidence checks, aggregation, ordering, and rounding.

### 6.4 Validation and privacy

- Every non-null score has at least two accepted evidence entries.
- Every quote is an exact substring of the referenced turn and at most 120
  characters.
- Only candidate utterances score the first five dimensions. Simulated GD
  participant utterances may be context for `collaboration_adaptability` but may
  not themselves count as candidate evidence.
- Third-party real names are rejected; aliases only.
- Hidden reasoning, system prompts, mentor output, and LLM evaluator prose are
  never evidence.
- `score=0.00` means observed contradictory behavior satisfied the evidence
  threshold. Missing data remains `null`.

## 7. Zero-dependency SVG radar chart

### 7.1 Component API

Phase 1 adds:

```ts
export type TensorRadarDatum = {
  id: string;
  label: string;
  value: number | null;
};

export type TensorRadarChartProps = {
  data: readonly TensorRadarDatum[]; // exactly six entries in canonical order
  preview?: boolean;
  title?: string;
};

export function TensorRadarChart(props: TensorRadarChartProps): JSX.Element
```

The component validates length and clamps finite numeric values to `[0, 1]`.
`null`, `NaN`, and infinite values plot at the center but display as `N/A` in the
accessible value list; they are not silently converted to a real zero score.

### 7.2 Geometry

Use a square `viewBox` and derive all geometry without layout measurement:

```text
N      = 6
center = (160, 160)
radius = 104
angle_i = -PI/2 + i * 2*PI/N

x(i, scale) = center.x + radius * scale * cos(angle_i)
y(i, scale) = center.y + radius * scale * sin(angle_i)
```

Render grid polygons at `scale = 0.25, 0.50, 0.75, 1.00`, six radial axes, and one
data polygon. Serialize coordinates with `toFixed(2)` for stable output. Labels
sit beyond the outer radius using the same angle and deterministic text-anchor
selection. No DOM measurement, random jitter, animation, canvas, D3, Chart.js,
Recharts, CSS-generated chart, or inline SVG path copied from an external asset
is allowed.

### 7.3 Accessibility and visual rules

- `<svg role="img" aria-labelledby="...">` contains `<title>` and `<desc>`.
- A text list adjacent to the SVG exposes all six labels and values; color is not
  the only carrier of information.
- `preview=true` renders a visible `PREVIEW / NOT MEASURED` label in both the
  panel and accessible description.
- Use existing `App.css` variables only. No new hex/rgb/hsl literals.
- The SVG is responsive through `width: 100%` and a square aspect ratio.
- No autonomous motion. Existing reduced-motion rules remain untouched.

### 7.4 Phase 1 preview values

The preview dataset is fixed and may exist only in `ProfileTab.tsx`:

```ts
const TENSOR_RADAR_PREVIEW = [
  { id: "problem_structuring", label: "構造化", value: 0.72 },
  { id: "quantitative_rigor", label: "定量精度", value: 0.58 },
  { id: "hypothesis_evidence", label: "仮説検証", value: 0.64 },
  { id: "synthesis_judgment", label: "統合判断", value: 0.68 },
  { id: "communication", label: "伝達", value: 0.76 },
  { id: "collaboration_adaptability", label: "協働適応", value: 0.61 },
] as const;
```

These values test asymmetric geometry and have no user meaning. They must not be
exported through IPC, persisted, or reused by Phase 2. Phase 2 deletes this
constant when real payload wiring is introduced.

## 8. Phase 1 implementation map

### 8.1 `apps/desktop/src/components/InterviewTab.tsx`

- Add and export `redactHiddenReasoning` near the existing GD parser.
- In the message render loop, compute visible text before branch selection.
- Pass visible text to `GdThreadMessage`; never pass raw text to the GD parser.
- Plain AI and feedback bubbles render visible text only.
- User bubbles remain unchanged.
- Preserve `shouldRenderGdThread`, `renderAs`, streaming placeholder behavior,
  phase gates, correlation ID filtering, and final replacement semantics.
- Do not change backend calls, session config, report handling, or persistence.

### 8.2 `apps/desktop/src/components/TensorRadarChart.tsx`, new

- Implement the API, validation, geometry, SVG, and accessible value list in §7.
- React only; no side effects, browser storage, network calls, dates, randomness,
  or new package.

### 8.3 `apps/desktop/src/components/ProfileTab.tsx`

- Add the fixed preview constant and mount `TensorRadarChart` in a single new
  `TENSOR_PROFILE_6D` section.
- Display `PHASE 1 PREVIEW / NOT MEASURED` directly above the chart.
- Do not call a new engine command and do not alter existing lazy-load behavior.
- Do not display evidence text, quotes, `fact_text`, `line_text`, or names.

### 8.4 `apps/desktop/src/App.css`

- Add only `.tensor-radar-*` classes required by the SVG, legend, and preview
  label.
- Use existing design tokens and spacing/radius variables.
- Do not add a new literal color, animation, nested card, or mobile overflow.

### 8.5 `tests/test_engine_tensor_profiling_ui_contract.py`, new

Static contract tests are acceptable because this repository has no frontend
test runner and new npm dependencies are forbidden. The tests must verify:

- the redactor is exported and used before both plain and GD-thread rendering;
- partial-tag, unmatched-open, multiple-block, and no-tag behavior are represented
  by a local table of examples or explicit implementation branches;
- `TensorRadarChart` uses `Math.sin`/`Math.cos`, exactly six axes, `<svg>`,
  `<title>`, `<desc>`, and no chart dependency;
- `ProfileTab` mounts the chart with all six canonical IDs and contains the exact
  `PREVIEW / NOT MEASURED` wording;
- forbidden tokens are absent from the changed frontend surface: `fetch(`,
  `axios`, `localStorage`, `sessionStorage`, `Math.random`, `Date.now`,
  `dangerouslySetInnerHTML`, `.quote`, `fact_text`, and `line_text`;
- `package.json` and `package-lock.json` are unchanged.

## 9. Tests and green gates

### 9.1 Phase 1 mandatory cases

The implementation must satisfy all of the following:

1. **Streaming split opener:** accumulated chunks `"<th"`, then
   `"ink>secret"`, then `"</think>answer"` render `""`, `""`, then
   `"answer"`; neither the marker fragment nor `secret` flashes.
2. **Unclosed block:** `"prefix<think>secret"` renders only `"prefix"` in both
   streaming and final states.
3. **Multiple blocks:** visible text around two hidden blocks is preserved in
   order.
4. **GD composition:** hidden text containing `[学生A]:` is removed before
   `parseGdSpeakerTurns`; visible GD lines still form the existing thread UI.
5. **Mode regression:** no-tag output for `interview_sim`, `es_review`, feedback,
   debrief, mentor, and `gd_sim` is unchanged.
6. **Chart geometry:** six axes, four grid rings, six finite polygon points, value
   clamping, and stable two-decimal coordinates.
7. **Missing value semantics:** `null` is shown as `N/A`, not a measured zero.
8. **Preview integrity:** the chart is visibly and accessibly marked
   `PREVIEW / NOT MEASURED` and causes no IPC call or persistence.

Run from repository root:

```powershell
cd apps\desktop
npx.cmd tsc --noEmit
npm.cmd run build
cd ..\..

py -3 -m pytest tests/test_engine_tensor_profiling_ui_contract.py -q
py -3 -m pytest tests/test_feature_gd_ui_contract.py -q
py -3 -m pytest tests/test_ui_orphan_integration_contract.py -q
py -3 -m pytest tests/test_ui_smoke.py tests/test_probe_ui_contract.py tests/test_probe_ui_ipc.py -q

git diff --check
git diff -- package.json package-lock.json
git diff -- src/python
git status --short data/
```

If `py -3` is unavailable, use the repository's discovered virtual-environment
Python executable directly. Do not install Python or npm dependencies.

### 9.2 Phase 2 future gates

- 100-turn synthetic session keeps dynamic context at or below 12,000 characters.
- Context selection is byte-for-byte identical for identical transcript, model
  hash, prompt version, and query.
- Every memory atom and tensor evidence quote validates against its source turn.
- Missing evidence yields `score=null`; observed negative evidence may yield
  `0.00` only after threshold satisfaction.
- Hidden reasoning never occurs in chunk events, final response, logs, reports,
  indexes, or persisted transcript.
- Compression fallback works with the extractor unavailable.
- Long-session TTFT benchmark records p50/p95 and does not regress p95 by more
  than 20% from the same-model 20-turn baseline; the exact hardware and model
  hash must be reported.
- D1/D2/PROBE suites remain unchanged and green.

## 10. Definition of done for Phase 1

Phase 1 is complete only when:

- only the five authorized frontend/test files in §8 are changed or created;
- no Python source, IPC contract, package manifest, lockfile, existing test, data,
  or prior specification is modified;
- all gates in §9.1 pass;
- GD streaming still renders per-speaker threads;
- hidden reasoning is absent from rendered DOM during streaming and after final
  replacement;
- the radar chart is native SVG, deterministic, responsive, accessible, and
  unmistakably a preview;
- no commit or push occurs until the commander explicitly orders it.

## 11. Sources

Research sources:

- Packer et al., [MemGPT: Towards LLMs as Operating Systems](https://arxiv.org/abs/2310.08560)
- Jha et al., [Characterizing Prompt Compression Methods for Long Context Inference](https://arxiv.org/abs/2407.08892)
- Pan et al., [LLMLingua-2: Data Distillation for Efficient and Faithful Task-Agnostic Prompt Compression](https://arxiv.org/abs/2403.12968)
- Zhang et al., [SCOPE: A Generative Approach for LLM Prompt Compression](https://arxiv.org/abs/2508.15813)
- Huang et al., [Large Language Models Cannot Self-Correct Reasoning Yet](https://arxiv.org/abs/2310.01798)

Public interview-practice sources:

- [McKinsey: Interviewing](https://www.mckinsey.com/careers/interviewing/)
- [BCG: Case Interview Preparation](https://careers.bcg.com/global/en/case-interview-preparation)
- [Bain: Interviewing](https://www.bain.com/careers/interview-preparation/experience-interview.aspx)
