# SPEC — E0b STEP 6: System Integration & Rendering Safety

> Status: **EXECUTION BLUEPRINT — not yet implemented**
>
> Parent: `SPEC_E0B_KNOWLEDGE_GATEWAY_v3.md`, STEP 0–5 implementation specs
>
> Locked baseline: STEP 5 offline Rust Gateway, commit `d58ecfc`
>
> Completion authority: Commander ACK only

## 0. Mission, baseline, and rulings

STEP 6 wires the already-built E0b attestation, Rust verifier, typestate FSM, and
offline Gateway into Python persistence/prompt integration and a narrow Tauri
surface. It must remain fail-closed and networkless by default.

The following rulings are binding:

1. **Do not redesign STEP 1–5.** Their byte contracts, exact-key parsers, fixed
   errors, FSM ordering, and feature isolation are locked. A defect found while
   integrating must be reported as a blocker rather than silently changing a
   locked contract.
2. **Live egress remains prohibited.** `resolve_and_pin()` currently validates an
   address but `ReqwestTransport` can independently resolve again. Until the
   Egress Live phase wires the vetted address into the actual connection and
   tests DNS rebinding, no production command may construct or reach
   `ReqwestTransport`.
3. **Default production policy is OFF with no environment override.**
   STEP 6 may exercise the complete pipeline only through injected fake
   transport/resolver/clock/bridge test seams. The default test suite must not
   enable `egress-live` or compile a TLS provider.
4. **E0a remains frozen.** Do not modify
   `src/python/core/knowledge_fetcher.py`,
   `src/python/core/privacy_search.py`, or the exact
   `facade.fetch_pending_knowledge()` raise-stub. E0b gets distinct names and
   storage.
5. **No automatic research.** No mount hook, consult hook, retry, polling loop,
   timer, profiler hook, or external-text-triggered fetch may call E0b. The only
   eventual entry is an explicit user action.
6. **No raw external text or URL crosses into the WebView.** Success receipts
   contain identifiers and counts only. External evidence is rendered only into
   the local LLM prompt through the dedicated Python renderer.

### 0.1 Tauri command-name correction

`plugin:knowledge|fetch_external` is Tauri v2's internal naming form for a
**plugin command**. This repository uses application commands registered by
`tauri::generate_handler!`, not a separate plugin crate. Rust identifiers also
cannot be declared with `:` or `|`.

Therefore the application wire name for this STEP is:

```text
knowledge_fetch_external
```

Isolation is enforced by the closed `knowledge_external_*` command family,
command-specific request structs, exact runtime parsers, and the absence of a
generic dispatcher. Do not fake a plugin namespace with string rewriting and do
not add a Tauri plugin solely to obtain the textual prefix. The internal
Rust-to-Python commands remain dotted and are never renderer-visible:

```text
knowledge.intent.build
knowledge.integrate
knowledge.external.list
knowledge.external.delete
```

`knowledge.research` must **not** be added to Python dispatch: phase B is owned
by Rust. This is the required correction to the original STEP 0 negative set.

---

## 1. Architecture and constraints

### 1.1 Trust boundaries and data flow

```text
WebView
  └─ invoke("knowledge_fetch_external", exact {query})
       └─ Rust command-specific policy gate
            ├─ STEP 6 production: fixed EGRESS_LIVE_NOT_READY, no Python/HTTP
            └─ injected networkless test service only:
                 A. Python knowledge.intent.build (NoReplay, no events)
                 B. Rust verify_and_gate + FSM + FakeTransport/FakeResolver
                 C. Python knowledge.integrate (NoReplay, no events)
                      └─ one atomic sidecar record

Normal consult with optional external_research_id
  └─ Python strict sidecar loader
       └─ render_external_evidence() sanitizes every render
            └─ dynamic prompt context lane
                 └─ local LLM
                      └─ WebView <pre>{text}</pre> text node
```

The production Tauri command is intentionally present but backed by an
`OfflineExternalResearchService` in STEP 6. The complete orchestration function
must be testable with injected fakes, but `lib.rs` must not manage a live service
and `commands.rs` must not mention or construct `ReqwestTransport`.

### 1.2 Ownership and collision rules

| Concern | Sole owner | Forbidden shortcut |
|---|---|---|
| Renderer request validation | `ipc_contract.rs` request type | `Value`, map, generic command |
| Python IPC routing | `engine_stdio.py` exact branch | `knowledge.research` in Python |
| User-data API | `core/facade.py` thin E0b wrappers | changing E0a stub |
| HMAC intent | existing `e0b_intent.py` | signer call from UI/Rust |
| Independent verification | existing Rust `verify_and_gate` | trusting Python result alone |
| Fetch ordering | existing Rust FSM/Gateway | Python networking |
| External persistence | new `core/e0b_external.py` | `.md`/`.txt` knowledge ingestion |
| Prompt entry | `render_external_evidence()` only | direct f-string of record fields |
| WebView display | React text nodes only | Markdown renderer/raw HTML |

The normal consult request may gain one optional
`external_research_id: string | null`. It is accepted only for `mode="consult"`.
It must be rejected for `interview_sim`, `gd_sim`, `es_review`, and
`romance_analysis`; this preserves simulator information isolation.

### 1.3 Spawn identity and no-replay wiring

Before any enabled fake integration path can run, `EngineManager` must own a
per-Python-spawn context:

```text
K_spawn             32 random bytes, child env PKB_EGRESS_KEY as 64 lower hex
session_id          16 random bytes, child env PKB_EGRESS_SESSION as 32 lower hex
sidecar_generation  monotonic u64, child env PKB_EGRESS_GENERATION
```

Rules:

- Generate all values in Rust exactly once for each debug or release Python
  spawn. Add a direct, pinned `getrandom` dependency (no TLS/network feature);
  RNG failure aborts boot.
- Host environment values with those names cannot override generated values.
- Clear/zeroize Rust secret buffers on failed spawn, failed ready handshake,
  restart, and shutdown. Never log keys, session IDs, tags, nonces, queries, or
  external text.
- `offline_subprocess_environment()` remains an allowlist and must omit every
  `PKB_EGRESS_*` variable from llama.cpp and all other Python children.
- Capture the generation before phase A and compare it again before fetch and
  phase C. A restart or mismatch aborts; no stale result is integrated.
- Python phase A requires the supplied session and generation to equal
  `PKB_EGRESS_SESSION` and `PKB_EGRESS_GENERATION`. Rust separately requires the
  returned session/generation/epoch to equal its current spawn/policy state.
- `knowledge.intent.build` and `knowledge.integrate` use
  `ReplayPolicy::NoReplay`, emit no intermediate event, and have no hidden
  retry.

Use the existing locked framing and payload shape. Do not retrofit the broader
v3 framing fields that STEP 2 explicitly deferred.

### 1.4 PII dictionary snapshot

Implement the missing supplier/loader needed to make the existing verifier
usable:

```text
data/processed/pii_dictionary/latest.json
{
  "schema": "pkb.pii_dictionary.v1",
  "revision": <u64>,
  "match_terms": ["<canonical term>", ...],
  "hash": "<64 lowercase hex>"
}
```

- Put supplier logic in `core/e0b_dictionary.py`; add that file to the
  constitutional guard's sanctioned Scope E modules.
- Source terms only from explicit structured local profile fields. At minimum:
  `fixed_attributes.birthday`, `address`, `occupation`, `line_self_name`; and
  explicitly typed alias/old-name/project-abbreviation arrays when present.
  Do not scrape free-form diary, consultation, LLM output, or external records.
- Reuse `PIISnapshot.build()`. If canonical terms are unchanged, reuse the
  existing snapshot bytes. If changed, increment revision and atomically replace
  the whole snapshot. Corruption, hash mismatch, or an empty effective
  dictionary fails closed with a fixed sentinel.
- Python phase A builds `EgressSanitizer` from this snapshot. Rust independently
  parses the exact-key file, recomputes the hash with
  `snapshot_hash_hex()`, and supplies the terms to `verify_and_gate`.
- Dictionary completeness is not claimed. Egress Live cannot be ACKed until
  structured PII source coverage is separately reviewed.

### 1.5 External sidecar record and atomicity

Add to `core/paths.py`:

```text
EXTERNAL_KNOWLEDGE_DIR = DATA_KNOWLEDGE / "external"
PII_DICTIONARY_LATEST = DATA_PROCESSED / "pii_dictionary" / "latest.json"
```

One research operation produces exactly one immutable record:

```text
data/knowledge/external/ext_{research_id}.json
```

`research_id` is the existing 64-lowercase-hex transaction nonce. The record is
an exact-key `pkb.external_knowledge.v1` object containing only:

- `schema`
- `research_id`
- `origin` (v1 enum: exactly `"wikipedia"`)
- `policy_epoch`
- `sidecar_generation`
- `dict_hash`
- `fetched_at_unix_ms`
- `query_sha256` (digest only; do not persist the query)
- `items`, each exact-key:
  `ordinal`, `source`, `title`, `content`, `content_sha256`

Limits are inherited from STEP 5: at most three items, title at most 256 UTF-8
bytes, content at most 2048 UTF-8 bytes after sanitization. `source` is the
closed value `"wikipedia"`. URL, request URL, redirect, raw HTML, raw query,
attestation, key, session ID, and exception strings are not record fields.

The integrate IPC request carries strict `results_by_query` with the canonical
attested query and its results. Rust verifies that query/order against the FSM
payload; Python computes `query_sha256` itself. The caller cannot provide a
free-standing digest for Python to trust. STEP 6 v1 accepts one renderer query
and one attested outbound query; multi-query abstraction requires a later
schema/ACK even though the locked lower layer supports up to four.

Persistence algorithm:

1. Strictly validate **all** integrate keys, nested keys, types, enum values,
   lower-hex fields, u64 fields, list sizes, UTF-8 byte caps, query ordering,
   and content.
2. Build the complete record in memory.
3. Serialize deterministically with UTF-8, sorted keys, compact separators, and
   one trailing LF.
4. If the final file exists, accept only byte-identical content and return the
   same receipt. Any difference is `E0B_RECORD_CONFLICT`.
5. Otherwise call existing `durable_atomic_write_text()` exactly once.

No validation failure may touch the filesystem. A research has zero public
record bytes or one complete record; never one file per query/item.

`load_knowledge_chunks()` remains unchanged and non-recursive. The external
subdirectory plus JSON extension are two independent exclusion barriers.
`sync_knowledge_index()` and intent generation must not read this directory.

### 1.6 Sidecar binding to consult

Do not place external records into the existing local knowledge index. STEP 6
uses a stricter explicit sidecar binding:

- The fetch success receipt returns `research_id` and `results_persisted`.
- React keeps that ID in component memory only and clears it whenever the query
  or consult mode changes.
- `consult(..., external_research_id=...)` loads exactly one record by validated
  filename. It canonicalizes the current query with the existing outbound
  canonicalizer and requires its SHA-256 to equal `query_sha256`.
- A mismatch, missing/corrupt record, or invalid ID fails closed with a fixed
  error. Never search the directory by attacker input.

This explicit lane is intentionally narrower than a semantic external index:
it prevents stale external content from being silently attached to unrelated
consults, avoids the existing `^##` splitter entirely, and makes deletion
immediate. A future external vector index requires a separate ACK.

### 1.7 Canonical external-text sanitizer

Implement the same bounded algorithm in:

- Rust: `src/knowledge/render_safety.rs`
- Python: `core/e0b_external.py`

Freeze shared vectors in:

```text
tests/fixtures/e0b_render_sanitization_vectors.json
config/e0b_special_tokens.json
```

The JSON token inventory is a regression inventory for the current
Qwen/DeepSeek/Llama families. Character-class neutralization is the primary
defense; matching only a finite token list is not sufficient.

For every title/content, in this exact order:

1. Reject invalid UTF-8/lone surrogates.
2. Decode a closed named set (`amp`, `lt`, `gt`, `quot`, `apos`, `nbsp`) plus
   valid decimal/hex numeric HTML entities to a bounded fixed point (maximum
   four passes, no expansion beyond the field cap). Reject surrogate/out-of-
   range numeric entities. This handles double encoding before tag recognition
   without depending on language-specific full HTML entity tables.
3. Normalize NFKC **before** replacement, so compatibility-width spellings
   cannot normalize back into a control token later.
4. Strip bounded raw HTML tags with a single-pass state machine. No regex and no
   DOM parser. Unterminated or overlong tag candidates are treated as text and
   neutralized by step 6.
5. Remove C0/C1 controls and the explicit invisible/Bidi set:
   `U+00AD`, `U+061C`, `U+180E`, `U+200B–U+200F`,
   `U+2028–U+202E`, `U+2060–U+2064`, `U+2066–U+2069`,
   `U+FEFF`; also reject/remove Unicode tag and private-use control payloads.
6. In one pass, replace structural trigger characters with visible
   **non-compatibility** alternatives that do not NFKC back to ASCII:

   ```text
   <→‹  >→›  |→¦  `→ˋ  ~→∼  [→〔  ]→〕
   (→❨  )→❩  !→ǃ  \→⧵  /→∕  &→⅋  *→∗  _→‗
   ```

   At line start (after at most three ASCII spaces), also neutralize Markdown
   heading, quote, bullet, thematic-break, and ordered-list markers. Use
   `♯`, `−`, `⊕`, and `·` as visible replacements. The mapping must make code
   fences, tilde fences, headings, links, images, autolinks, raw HTML, tables,
   blockquotes, and known prompt delimiters impossible to reconstruct.
   Detect URL-like `scheme:`/`www.` prefixes in the same bounded pass and
   neutralize their separator/dot so incidental snippet text cannot re-create a
   clickable or prompt-visible external locator.
7. Truncate by UTF-8 bytes at a scalar boundary to the field cap. Do not append
   an ellipsis beyond the cap. Reject an item whose title or content becomes
   empty.

Required properties:

- `S(S(x)) == S(x)`.
- No output contains an exact configured special token.
- NFKC of each replacement character does not equal its ASCII trigger.
- No HTML entity is emitted as the escaping mechanism; this avoids a later
  decoder reactivating markup.
- Complexity is O(input bytes), with fixed entity-pass and output bounds.

Rust applies the sanitizer immediately after STEP 5 result extraction and
before phase C. Python independently sanitizes/validates direct
`knowledge.integrate` input and applies it again on **every prompt render**.

### 1.8 Prompt placement and residual risk

`render_external_evidence()` is the only external-record-to-prompt function.
It binds each item's warning, provenance (`research_id`, `origin`, ordinal,
digest), title, and content into one render unit. It never emits a URL.

Prompt order remains:

```text
build_static_prefix()                                      # unchanged
build_dynamic_suffix():
  local diary context
  local trusted knowledge context
  Future Context
  external UNTRUSTED evidence lane                         # new
  user query
  OUTPUT_FRAMEWORK                                         # remains authoritative last
```

“Last lane” means the last **context** lane, not the final prompt bytes. External
text must never follow the user's query or `OUTPUT_FRAMEWORK`. It must never
enter `build_static_prefix()`, preserving KV-prefix pinning.

Add an explicit docstring to `render_external_evidence()` and to
`ConsultationEngine.build_dynamic_suffix()` (or the exact prompt-composition
method if refactored):

> External text is untrusted. Structural and tokenizer controls are physically
> neutralized and the data cannot trigger further egress, but the fundamental
> RAG limitation remains: plausible plain-language prompt injection can never
> be reduced to zero. The design limits blast radius; it does not prove semantic
> truth or model non-manipulation.

There is no `prompt_builder.py` in the current repository. Do not create one
solely to satisfy a filename mentioned in the directive; document the limit at
the actual integration points.

### 1.9 WebView rendering rules

- Keep consultation output in React text children:
  `<pre className="chat-text">{m.text}</pre>`.
- Do not add `dangerouslySetInnerHTML`, a Markdown renderer, linkifier, raw HTML
  parser, iframe, external image, or anchor generated from external data.
- Receipts and record lists must reject unknown keys at runtime. They must not
  accept `content`, `title`, `snippet`, `url`, `query`, or arbitrary `message`.
- Record-list UI may show only `research_id` and deletion state. Deletion
  requires an inline two-step confirmation and operates on one validated ID.
- CSP and `capabilities/default.json` remain unchanged. Do not add HTTP, shell,
  process, opener, webview, clipboard, or filesystem permissions.

### 1.10 Fixed errors and privacy

Define a closed error mapping. Suggested internal sentinels:

```text
EGRESS_LIVE_NOT_READY
E0B_DICTIONARY_UNAVAILABLE
E0B_VALIDATION_REJECTED
E0B_RECORD_CONFLICT
E0B_PERSISTENCE_FAILED
E0B_EXTERNAL_CONTEXT_UNAVAILABLE
E0B_EXTERNAL_CONTEXT_QUERY_MISMATCH
```

Python E0b exceptions contain only one of these constants. Rust maps internal
errors to fixed existing/new UI-safe categories. UI catches errors and renders
only `uiErrorMessage(...)`. Never include query, term, position, title, content,
path, URL, resolver answer, raw exception, tag, key, nonce, or session in
stdout/stderr/log/error/receipt.

---

## 2. Execution phase plan — mandatory TDD loops

Every substep is RED → GREEN → focused gate → atomic checkpoint. Do not combine
later GREEN work into an earlier RED commit. Do not continue after an
unexplained failure.

### STEP 6.A — IPC command wiring and constitutional guard reversal

#### 6.A.1 RED: reverse, do not delete, STEP 0 guards

Modify the existing tests in
`tests/test_e0b_constitutional_guard.py`:

1. Rewrite `test_no_e0b_ipc_command_wired` into an affirmative test that
   requires exactly `knowledge.intent.build` and `knowledge.integrate` in
   `engine_stdio.dispatch`.
2. Keep an explicit negative assertion that `knowledge.research` is absent from
   Python dispatch.
3. Rewrite `test_facade_has_no_e0b_egress_entrypoint` into an affirmative test
   requiring the exact facade names `knowledge_intent_build` and
   `knowledge_integrate`.
4. In the same test, retain the exact E0a
   `fetch_pending_knowledge()` raise-stub assertion.
5. Add static call-graph checks:
   - intent facade calls the PII snapshot supplier and existing
     `build_attested_intent`;
   - integrate facade calls only the strict external persistence function;
   - neither facade function imports/calls network actors.
6. Add `e0b_dictionary.py` and `e0b_external.py` to
   `SANCTIONED_E0B_MODULES`; Scope E denial still applies.
7. Extend the closed Tauri command-set test in
   `tests/test_fsa_2026_07_13_03_webview_ipc_boundary.py` with
   `knowledge_fetch_external`, `knowledge_external_list`, and
   `knowledge_external_delete`.

Run and record the expected RED failures. The test functions must appear as
modified/replaced in the diff; deleting them without affirmative successors is
not acceptable.

#### 6.A.2 GREEN: exact contracts and fail-closed command surface

Implement:

- strict request structs in `ipc_contract.rs`:
  - `KnowledgeFetchExternalRequest { query }`
  - `KnowledgeExternalDeleteRequest { research_id }`
  - optional `external_research_id` on `ConsultRequest`
- exact Python dispatch branches with explicit key-set validation before facade
  calls;
- thin E0b facade functions while preserving the E0a stub;
- `EngineManager` spawn identity and egress child environment;
- `knowledge_fetch_external` registered in `lib.rs`, backed in production only
  by `OfflineExternalResearchService`;
- local list/delete commands, also command-specific;
- `ReplayPolicy::NoReplay` assertions for phase A/C command strings.

Tauri registration RED is GREEN only when:

- `generate_handler!` contains the exact command family;
- invoking fetch in the default build returns the fixed disabled result/error;
- no Python dispatch, transport, resolver, or filesystem write occurs on that
  disabled path;
- no capability/CSP expansion occurs.

#### 6.A.3 Focused gate

```powershell
python -m pytest -q tests/test_e0b_constitutional_guard.py
python -m pytest -q tests/test_fsa_2026_07_13_03_webview_ipc_boundary.py
python -m pytest -q tests/test_e0b_integration_step6.py -k "wiring or dictionary or disabled"
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --test ipc_contract
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked engine
```

Checkpoint message:

```text
feat(e0b): wire fail-closed STEP 6 command contracts
```

### STEP 6.B — Special-token and Markdown invalidation

#### 6.B.1 RED: cross-language golden vectors

Create the shared sanitizer/token fixtures first. Required adversarial vectors:

- backtick and tilde fences, including title-based fence closure;
- `#` headings after LF/CRLF/U+2028 and up to three leading spaces;
- inline/reference links, images, autolinks, `javascript:` and data-like link
  destinations;
- blockquotes, unordered/ordered lists, thematic breaks, tables, inline code,
  emphasis;
- raw, entity-encoded, numeric-encoded, and double-encoded HTML;
- ChatML, Qwen, Llama, and DeepSeek token families, including controls inserted
  between delimiters;
- ANSI/OSC controls, NUL/C0/C1, soft hyphen, ZWSP/ZWNJ/ZWJ, bidi overrides and
  isolates, BOM, Unicode line/paragraph separators;
- fullwidth/compatibility spellings that NFKC into ASCII triggers;
- empty-after-cleaning, lone surrogate (Python), malformed JSON Unicode (Rust),
  and 255/256/257 plus 2047/2048/2049 UTF-8 byte boundaries;
- multibyte truncation and idempotence.

Tests must fail before implementation in both languages.

#### 6.B.2 GREEN: one algorithm, two implementations

Implement §1.7 without regex backtracking, unbounded entity recursion, or
allocation proportional to attacker-declared sizes. Apply it:

- in Rust immediately after each `SearchResult` is extracted;
- in Python integrate independently;
- in Python `render_external_evidence()` on every call.

Add a static test proving all configured replacement characters are NFKC-stable
away from their ASCII triggers.

#### 6.B.3 Focused gate

```powershell
python -m pytest -q tests/test_e0b_render_safety_step6.py
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked render_safety
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --test knowledge_gateway
```

Checkpoint message:

```text
feat(e0b): neutralize external render controls
```

### STEP 6.C — Atomic sidecar persistence and dedicated loader

#### 6.C.1 RED

Add `tests/test_e0b_integration_step6.py` coverage for:

- exact top-level/nested key sets and type confusion (`bool` is not `int`);
- origin enum, 64-lower-hex IDs/hashes, u64 bounds, list/cardinality/byte caps;
- query digest and deterministic serialization;
- all-or-nothing validation (late bad item leaves no file/temp file);
- successful one-record write;
- same-ID/same-bytes idempotence;
- same-ID/different-bytes conflict with original bytes unchanged;
- injected write/fsync/replace failures expose no partial final file;
- strict load, invalid ID/path traversal, corruption, unknown keys, symlink/reparse
  surprises where supported;
- `load_knowledge_chunks()` and `sync_knowledge_index()` never include external
  JSON;
- intent builder/supplier never reads `data/knowledge/external`;
- list returns IDs only; delete affects exactly one record and is idempotent or
  returns one fixed not-found result, as chosen by the contract.

#### 6.C.2 GREEN

Implement `core/e0b_external.py`, path constants, facade, and dispatch using
existing durable persistence. Do not add a second atomic-write implementation.
No URL field is permitted even if future providers expose one.

#### 6.C.3 Focused gate

```powershell
python -m py_compile src/python/core/e0b_dictionary.py src/python/core/e0b_external.py src/python/core/facade.py src/python/engine_stdio.py
python -m pytest -q tests/test_e0b_integration_step6.py -k "persist or record or loader or exclusion or delete"
python -m pytest -q tests/test_e0a_egress_lockdown.py tests/test_e0b_constitutional_guard.py
```

Checkpoint message:

```text
feat(e0b): persist isolated external sidecars atomically
```

### STEP 6.D — Prompt integration and blast-radius documentation

#### 6.D.1 RED

Add byte-level prompt tests:

1. persist a malicious record;
2. load it only by explicit research ID;
3. bind it to the matching canonical query digest;
4. call `render_external_evidence()`;
5. build the final prompt.

Assert:

- every item retains research ID, origin, ordinal, and digest in the same render
  unit as its sanitized text;
- no configured special token, fence, heading, link, image, raw HTML, URL, or
  external delimiter escape appears in the external lane;
- external context is after local/Future context and before user query and
  `OUTPUT_FRAMEWORK`;
- `build_static_prefix()` bytes/hash are unchanged with or without external
  evidence;
- missing, corrupt, mismatched-query, or unknown-key record fails closed;
- no external context reaches simulation/review modes;
- external records do not affect a later `knowledge.intent.build`;
- required RAG-limit docstrings exist at both actual integration points.

#### 6.D.2 GREEN

Extend the consult call chain with an optional sidecar ID:

```text
ipc_contract.rs → commands.rs → engine_stdio.py → facade.py
→ ConsultationEngine.consult/build_dynamic_suffix
```

Default/omitted behavior must remain byte-compatible. Do not reorder the static
prefix or make external evidence part of cached state.

#### 6.D.3 Focused gate

```powershell
python -m pytest -q tests/test_e0b_integration_step6.py -k "prompt or consult or feedback or simulation"
python tests/test_gap_analysis.py
python tests/test_integration.py
```

Checkpoint message:

```text
feat(e0b): bind sanitized sidecars to consult prompts
```

### STEP 6.E — Networkless Rust orchestration

#### 6.E.1 RED

Add Rust tests with injected fake bridge/transport/resolver/clock for:

- phase order A → verify/FSM/fetch → C;
- exact mapping of `SearchResult { title, snippet }` into
  `{source:"wikipedia", content:...}`;
- all results grouped into one integrate request and one receipt;
- Python intent payload session/generation/epoch/dictionary equality checks;
- dictionary hash recomputation/drift;
- PII rejection and malformed attestation cause transport call count zero;
- generation change before fetch or integrate causes no later side effect;
- phase A/C use NoReplay and emit no events;
- cancel/deadline/late phase result is discarded by the turnstile;
- 100 concurrent attempts produce exactly one active transaction;
- every error/panic path releases the slot;
- default production service has zero bridge/transport/resolver calls;
- static reachability: `commands.rs`/`lib.rs` do not construct, import, or manage
  `ReqwestTransport`; it remains confined to `egress-live`.

Do not weaken or duplicate existing STEP 4/5 tests.

#### 6.E.2 GREEN

Add a narrow orchestrator around existing `research_fetch()` and typestate
transitions. The orchestrator may be exercised by unit tests with fakes, but the
STEP 6 production service remains offline-only. Completion occurs only after
Python returns `results_persisted` matching the grouped result count, then
`Txn<ReadyToIntegrate>` transitions to completed.

There is no retry, fallback provider, alternate host, detached task, polling, or
background queue.

#### 6.E.3 Focused gate

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked knowledge
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --test knowledge_gateway
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --all-targets -- -D warnings
```

Also prove from the lock/feature graph that the default invocation does not
enable `reqwest/rustls` or a TLS provider. Do **not** run `--all-features`.

Checkpoint message:

```text
feat(e0b): integrate networkless research orchestration
```

### STEP 6.F — Runtime parser, UI safety, and per-record deletion

#### 6.F.1 RED

Extend `apps/desktop/tests-runtime/ipc_response_boundary.test.ts` with exact
success parser vectors:

```text
knowledge_external_receipt.v1:
  schema, research_id, results_persisted

knowledge_external_list.v1:
  schema, research_ids

knowledge_external_delete.v1:
  schema, research_id, deleted
```

For every parser, reject missing/extra keys, wrong literal schema, wrong types,
non-lower-hex ID, negative/fractional count, and any injected
`query/title/content/snippet/url/message/error` field.

Add static UI tests proving:

- only `engine.ts` imports Tauri `invoke`;
- the new wrappers use `invoke<unknown>` followed by the exact parser;
- no fetch occurs on mount, input change, consult submit, timer, or external
  response;
- no raw external field is rendered;
- consultation remains a React text node in `<pre>`;
- no Markdown/HTML/link rendering dependency or sink is added;
- changing input/mode clears the in-memory receipt;
- delete uses two explicit clicks.

#### 6.F.2 GREEN

Add the closed command union/wrappers and fixed UI error category. Put the
explicit research control next to normal consult input. In this STEP it reports
that Egress Live is unavailable; it must not pretend that research succeeded.
When injected/test success is returned, keep only the research ID/count in
React state and pass the ID to the next normal consult.

Add a metadata-only record list and per-record delete control to the existing
Settings or Import surface. Do not display external title/content/query/URL.

#### 6.F.3 Focused gate

From `apps/desktop`:

```powershell
npm.cmd run test:boundary
npx.cmd tsc --noEmit
npm.cmd run build
```

From repository root:

```powershell
python -m pytest -q tests/test_fsa_2026_07_13_03_webview_ipc_boundary.py
python -m pytest -q tests/test_ui_orphan_integration_contract.py
```

Checkpoint message:

```text
feat(e0b): add strict external research UI boundary
```

### STEP 6.G — Error non-disclosure, full regression, and documentation

#### 6.G.1 Adversarial non-disclosure matrix

Inject unique canaries at every failure point in query, dictionary term, title,
content, path-like string, entity, URL-like string, resolver error, persistence
exception, and parser extra field. Assert canaries are absent from:

1. Python stdout protocol lines;
2. Python/Rust stderr and logs;
3. Tauri command errors;
4. frontend state/rendered text;
5. persisted files other than the deliberately accepted sanitized content.

Verify fixed sentinels only. Re-run constant-time attestation tests unchanged.

#### 6.G.2 Documentation update

After GREEN, update:

- `docs/AI_SKILLS.md` with the external sidecar namespace, render-time
  sanitizer, explicit consult binding, no-URL receipt, and Egress Live blocker;
- `docs/CONTEXT.md` with STEP 6 status;
- `docs/HANDOFF.md`/incident ledger if present and currently used.

State explicitly:

- default network policy remains OFF;
- `egress-live` is not production-wired;
- DNS pinning TOCTOU remains the hard blocker;
- PII dictionary coverage needs Egress Live review;
- plain-language RAG prompt injection cannot be eliminated.

#### 6.G.3 Mandatory full gate

Run all commands networkless:

```powershell
python -m pytest -q
python tests/test_calendar_sync.py
python tests/test_apple_calendar_sync.py
python tests/test_gap_analysis.py
python tests/test_integration.py
python tests/ui_smoke.py

cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --all-targets
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --all-targets
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --all-targets -- -D warnings

Push-Location apps/desktop
npm.cmd run test:boundary
npx.cmd tsc --noEmit
npm.cmd run build
Pop-Location
```

Also run `python -m py_compile` over every changed Python file and
`git diff --check`.

Forbidden during this gate:

- `--all-features` or `--features egress-live`;
- real DNS, HTTP, proxy, CDN, telemetry, model download, or external URL;
- skipping a failing test as an environment issue without a demonstrated,
  commander-approved blocker.

Checkpoint message:

```text
docs(e0b): lock STEP 6 integration invariants
```

---

## 3. Atomic checkpoint rules and final HARD STOP

### 3.1 Git discipline

At the start and before every checkpoint:

```powershell
git status --short
git diff --check
git diff -- <phase-owned paths>
```

Rules:

- Never use `git add .`, `git add -A`, `reset --hard`, `checkout --`, clean,
  force-push, amend, or hook bypass.
- Stage only the files owned by the completed substep. Existing unrelated
  modifications/untracked build artifacts remain untouched.
- Include test fixtures/tests with the GREEN implementation they specify.
- Before commit, inspect `git diff --cached` and run the focused gate again.
- If a hook changes files after a successful commit, follow repository amend
  safety rules; if a commit fails, fix and create a new commit rather than
  amending a nonexistent/rejected commit.
- Do not push unless separately ordered.

### 3.2 Final evidence report

After STEP 6.G, report:

- checkpoint commit SHAs and exact subjects;
- changed files grouped by Rust/Python/frontend/docs;
- focused and full test commands with pass counts;
- proof default Cargo features omit TLS;
- proof production command uses the offline service and makes zero network
  calls;
- proof `load_knowledge_chunks()` and intent generation exclude external
  records;
- proof receipts/UI contain no raw external content or URL;
- proof sanitizer golden parity and prompt byte placement;
- remaining Egress Live blockers and the RAG residual-risk statement;
- final `git status --short`, identifying unrelated pre-existing changes.

### 3.3 HARD STOP

After the evidence report:

1. Do not enable live egress.
2. Do not wire `ReqwestTransport`.
3. Do not fix/redesign the DNS pinning ticket inside STEP 6.
4. Do not start a new phase, squash, push, or clean the worktree.
5. Stop and wait for explicit Commander ACK.

STEP 6 is not complete merely because tests pass. Completion requires the full
evidence report and Commander ACK.
