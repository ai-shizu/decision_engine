# PKB 最高アーキテクト補佐 引き継ぎ書

> **更新日:** 2026-07-11
> **対象:** Phase 4-A Context Observatory / RetrievalManifestV1 **完了 (As-Built)**
> **リポジトリ:** `C:\Users\badger\Documents\cursur\decision_engine`
> **ブランチ:** `main`
> **origin/main:** `412c1c2d9e02accb726909500faef75a5fe1d3c5` (未push — Phase 4-Aは全てローカルコミット)
> **Phase 4-A実装コミット:** `ff14ace feat(phase-4a): complete Context Observatory and secure IPC verification` (20 files)
> **本書同期コミット:** 本書 + `docs/AI_SKILLS.md` §16 を後続のローカルコミットで確定 (docs同期)。
> **重要:** local `main` は origin より先行。**push は指揮官の明示指示があるまで固く禁止。**

---

## 0. 30秒で把握する現在地

PKBは、完全オフラインのTauri + React + Python意思決定支援アプリである。Phase 3-Bまでは`main`へpush済み。現在はPhase 4-Aとして、12,000文字のSemantic Compressionが「何を採用し、何を棄却したか」を決定論的に可視化するContext Observatoryを実装中である。

完了・検収済み (全STEP完了):

1. **STEP 1:** Python `RetrievalManifestV1`、context compiler計装、厳格永続化
2. **STEP 2:** TypeScript Manifest型契約
3. **STEP 3:** 静的`ContextObservatory` UI / SVG
4. **STEP 4:** Python薄層IPC endpoint `context.manifest.latest`
5. **STEP 5:** frontend runtime boundary — runtime parser (`parseManifest.ts`)、`engine.ts` wrapper (`latestContextManifest`)、純reducer (`manifestFetchState.ts`)、`ContextObservatoryContainer`
6. **MOUNT:** `ContextObservatoryContainer` を **PROFILE タブ (`apps/desktop/src/components/ProfileTab.tsx`)** の `CONTEXT_OBSERVATORY` セクションへマウント済み (新タブ増設なし・App.tsx不変)。

検証 (As-Built):

- boundary runtime テスト: **42/42 GREEN** (`tests-runtime/manifest_boundary.test.ts`、越境golden T-42含む)
- Python回帰: **170 passed** (7スイート)
- `tsc --noEmit` / `vite build`: PASS

残タスク:

- INCIDENT_LEDGER への INC-PHASE4A-02/03/04 追記 → **完了** (`docs/architecture/INCIDENT_LEDGER.md`)
- 本書 As-Built 同期 + `AI_SKILLS.md` §16 (SKILL-PKB-BOUNDARY-V3) 登録 → **本コミットで完了**
- **push は指揮官裁定待ち** (依然禁止)

**Phase 4-A のデータフローは全結線・全マウント完了。** 静的UIプレビューではなく実IPC経路で稼働する。

---

## 1. 新チャット開始時の必須手順

次任者は、設計・レビュー・編集の前に必ず以下を実行する。

1. 本書を全文読む。
2. `docs/AI_SKILLS.md` §0 の読み込みプロトコルを読む。§1 は必ず読む。§0 の表から現在のタスクに対応する節だけを選ぶ。横断的変更・分類不能時は関連しそうな行を複数選ぶ。「とりあえず全文」は行わない。不変条件が他文書と競合する場合は AI_SKILLS を優先する。
3. `docs/architecture/INCIDENT_LEDGER.md`を読む。
4. 下記コマンドでHEADとworktreeを照合する。

```powershell
cd C:\Users\badger\Documents\cursur\decision_engine
git rev-parse HEAD
git rev-parse origin/main
git status --short
git diff --check
```

5. 下記の凍結hashを再取得し、本書の値と照合する。

```powershell
Get-FileHash src/python/core/retrieval_manifest.py -Algorithm SHA256
Get-FileHash src/python/core/session_memory.py -Algorithm SHA256
Get-FileHash src/python/core/consultation_engine.py -Algorithm SHA256
Get-FileHash src/python/core/paths.py -Algorithm SHA256
Get-FileHash apps/desktop/src/lib/manifest.ts -Algorithm SHA256
Get-FileHash apps/desktop/src/components/ContextObservatory.tsx -Algorithm SHA256
```

差分を「汚れ」と判断して戻してはいけない。Phase 4-Aの正規成果物である。

---

## 2. Git / worktreeの正確な状態

`origin/main == 412c1c2d9e02accb726909500faef75a5fe1d3c5`(未push)。local `main` はこれより先行し、Phase 4-A成果物はローカルコミット `ff14ace`(実装20ファイル)+ 本 docs同期コミットに確定済み。

Phase 4-A実装コミット `ff14ace` に含まれる20ファイル(`git show --stat ff14ace`):

```text
src/python/core/retrieval_manifest.py     (新設 STEP 1)
src/python/core/session_memory.py         (STEP 1 計装)
src/python/core/consultation_engine.py    (STEP 1 接続)
src/python/core/paths.py                  (STEP 1 保存先)
src/python/core/facade.py                 (STEP 4 薄層API)
src/python/engine_stdio.py                (STEP 4 dispatch)
tests/test_retrieval_manifest.py          (STEP 1 契約79件)
tests/test_context_manifest_ipc.py        (STEP 4 IPC契約7件)
apps/desktop/src/lib/manifest.ts          (STEP 2 型契約)
apps/desktop/src/lib/parseManifest.ts     (STEP 5 runtime parser)
apps/desktop/src/lib/manifestFetchState.ts(STEP 5 純reducer)
apps/desktop/src/lib/engine.ts            (STEP 5 wrapper 追記)
apps/desktop/src/components/ContextObservatory.tsx          (STEP 3 dumb view)
apps/desktop/src/components/ContextObservatoryContainer.tsx (STEP 5 container)
apps/desktop/src/components/ProfileTab.tsx                  (MOUNT)
apps/desktop/tests-runtime/manifest_boundary.test.ts       (STEP 5 境界42件)
apps/desktop/tests-runtime/gen_manifest_fixture.py         (越境golden生成)
apps/desktop/tsconfig.boundary.json                        (テスト専用tsconfig)
docs/HANDOFF.md
docs/architecture/INCIDENT_LEDGER.md
```

`.claude/` は依然として未追跡・**stage禁止**。docs同期(本書 + `AI_SKILLS.md`)は後続コミットで確定する。

### 所有権

| パス | 意味 |
|---|---|
| `src/python/core/retrieval_manifest.py` | STEP 1新設。Manifestモデル・validator・永続化 |
| `src/python/core/session_memory.py` | STEP 1。既存context選択を変えない計装 |
| `src/python/core/consultation_engine.py` | STEP 1。Manifest生成・保存経路の接続 |
| `src/python/core/paths.py` | STEP 1。Manifest保存先 |
| `src/python/core/facade.py` | STEP 4。latest Manifest薄層API |
| `src/python/engine_stdio.py` | STEP 4。`context.manifest.latest` dispatch |
| `tests/test_retrieval_manifest.py` | STEP 1契約・敵対テスト79件 |
| `tests/test_context_manifest_ipc.py` | STEP 4 IPC契約テスト7件 |
| `apps/desktop/src/lib/manifest.ts` | STEP 2型契約 |
| `apps/desktop/src/lib/parseManifest.ts` | STEP 5。unknown入力のruntime parser (Python鏡像) |
| `apps/desktop/src/lib/manifestFetchState.ts` | STEP 5。React非依存の純reducer (stale/monotonicガード) |
| `apps/desktop/src/lib/engine.ts` | STEP 5。`latestContextManifest()` wrapper追記 (`pkbInvoke<unknown>`経由) |
| `apps/desktop/src/components/ContextObservatory.tsx` | STEP 3。dumb view (凍結) |
| `apps/desktop/src/components/ContextObservatoryContainer.tsx` | STEP 5。loading/ready/empty/error所有container |
| `apps/desktop/src/components/ProfileTab.tsx` | MOUNT。`CONTEXT_OBSERVATORY` section を追加 |
| `apps/desktop/tests-runtime/manifest_boundary.test.ts` | STEP 5境界テスト42件 (依存ゼロ自作ハーネス) |
| `apps/desktop/tests-runtime/gen_manifest_fixture.py` | 越境golden生成 (Python実出力→TS parser受理の証明) |
| `apps/desktop/tsconfig.boundary.json` | 境界テスト専用tsconfig |
| `docs/architecture/INCIDENT_LEDGER.md` | Phase 4-A事故台帳。INC-01〜04を実ファイル化済み |
| `docs/AI_SKILLS.md` | §16 SKILL-PKB-BOUNDARY-V3 登録済み |
| `.claude/` | ユーザー環境由来の未追跡。触らない、stageしない |

### Git禁止事項

- ユーザーの明示指示なしに`git add`、commit、pushを行わない。
- **push は指揮官の明示指示があるまで固く禁止** (origin/main は `412c1c2` のまま)。
- `.claude/**`をstageしない。
- コミット済みのPhase 4-Aコミット (`ff14ace` 等) を revert / reset しない。
- `git reset --hard`、`git checkout --`を使用しない。

---

## 3. Phase 4-Aの目的

既存の12,000文字Semantic Compressionを変更せず、選択過程をManifestへ記録し、Reactで可視化する。

絶対原則:

- **Observabilityは純粋な計装。** context、候補順、採択集合を1 byteも変えない。
- SLM/LLMはManifestの採否・会計・Reason Codeへ関与しない。
- Pythonだけが候補採否、予算、重複、状態を確定する。
- UIは検証済みManifestを表示するだけ。再採点・補正・推測をしない。
- 生本文、quote、任意の第三者実名をManifest/UIへ出さない。

データフローの完成予定形:

```text
session_memory.py
  -> RetrievalManifestV1
  -> validate_manifest()
  -> save_retrieval_manifest()
  -> latest.json + immutable {manifest_id}.json
  -> facade.latest_context_manifest()
  -> engine_stdio.dispatch("context.manifest.latest", {})
  -> engine.ts latestContextManifest() wrapper (pkbInvoke<unknown>)
  -> parseContextManifestResponseV1() runtime parser
  -> ContextObservatoryContainer (useReducer + 明示ボタン)
  -> ContextObservatory dumb view
  -> ProfileTab CONTEXT_OBSERVATORY section (mounted)
```

全経路結線・マウント完了 (As-Built)。

---

## 4. STEP 1: Backend Manifest / Context計装

### 実装済み

`src/python/core/retrieval_manifest.py`:

- `CandidateStatus`
- `ContextLane`
- `SourceType`
- `ReasonCode`
- frozen `RetrievalCandidateV1`
- frozen `LaneUsageV1`
- frozen `RetrievalManifestV1`
- `build_retrieval_manifest()`
- `validate_manifest()`
- `manifest_to_dict()` / `manifest_from_dict()`
- `save_retrieval_manifest()` / `load_latest_retrieval_manifest()`
- BLAKE2b-128 canonical `manifest_id`

### 固定lane

| Lane | candidate content budget |
|---|---:|
| `CURRENT` | 2400 |
| `RECENT_TRANSCRIPT` | 3600 |
| `WORKING_MEMORY` | 4000 |
| `RETRIEVED_EVIDENCE` | 2000 |

`budget_chars`はheader/separatorを除くcandidate content budget。会計は次である。

```text
lane.used_chars - lane.formatting_chars <= lane.budget_chars
sum(candidate.included_chars) + formatting_overhead_chars == used_chars
sum(lane_usage.used_chars) == used_chars
total_budget_chars == 12000
```

### Retrievedの凍結挙動

旧Phase 3-Bの挙動を維持する。

1. 上位8件を先に確定する。
2. Working Memoryとの重複を`DEDUPLICATED_HIGHER_LANE`へ変換する。
3. 重複除去後の空き枠を下位Atomで補充しない。

### Privacy

Manifestの`speaker_alias`は次のみ許可する。

- 固定内部role
- 検証済み`C-xxxxxxxx` contact alias
- `None`

未知roleや実名はcompiler境界で`None`にする。永続化読込時にはsanitizationせず、不正aliasを`ValueError`で拒否する。

### 永続化境界

- hashはdataclass生成前に計算し、完成dataclassを一度だけ生成する。
- `__post_init__`でもIDを再計算する。
- `latest.json`のIDとpayload内部IDを比較する。
- 既存immutableファイルIDと保存対象IDを比較する。
- 不一致は修復・上書きせず`ValueError`。
- corrupt Manifestを`NO_MANIFEST`へ退化させない。

---

## 5. STEP 2: TypeScript型契約

ファイル: `apps/desktop/src/lib/manifest.ts`

実装済み:

- `CandidateStatus`
- `ContextLane`
- `SourceType`
- `ReasonCode`
- `RetrievalCandidateV1`
- `LaneUsageV1`
- `RetrievalManifestV1`
- `ContextManifestResponseV1`

IPC responseは次のdiscriminated union。

```typescript
export type ContextManifestResponseV1 =
  | { manifest: RetrievalManifestV1; reason: null }
  | { manifest: null; reason: "NO_MANIFEST" };
```

注意: TypeScript型はruntime防御ではない。`pkbInvoke<ContextManifestResponseV1>()`のgeneric castだけで済ませてはいけない。

---

## 6. STEP 3: 静的Context Observatory UI

ファイル: `apps/desktop/src/components/ContextObservatory.tsx`

実装済みcomponent tree:

```text
ContextObservatory
├── ContextSummary
├── ContextBudgetMeter
├── SourceTypeBreakdown
├── CandidateFilters
└── CandidateLedger
    └── CandidateLedgerRow
```

特性:

- 純粋React + SVG `viewBox="0 0 100 8"`
- 外部chart依存なし
- 新規hex色、正のletter-spacing、リテラルpxなし
- IPCなし
- `useEffect`、時刻、乱数、DOM測定なし
- candidate順を維持し、`sort()`しない
- `speaker_alias`、`document_id`、`content_hash`、raw text、quoteを表示しない
- Previewは`STATIC PREVIEW / NO IPC`と明示

`CONTEXT_OBSERVATORY_PREVIEW_MANIFEST`は表示確認専用であり、永続化・IPC・runtime parserの正当性証明に使用してはいけない。canonical `manifest_id`を持つ本番Manifestとして扱わない。

現在、このcomponentは`App.tsx`へ登録されていない。これは意図どおり。

---

## 7. STEP 4: Backend IPC endpoint

### facade

`core.facade.latest_context_manifest()`:

```python
manifest = load_latest_retrieval_manifest()
if manifest is None:
    return {"manifest": None, "reason": "NO_MANIFEST"}
return {"manifest": manifest_to_dict(manifest), "reason": None}
```

例外をcatchしない。loader/serializerを迂回しない。Engine/LLMを起動しない。

### stdio command

```text
context.manifest.latest
```

空dict paramsだけを受理する。

```python
if type(params) is not dict or params:
    raise ValueError("context.manifest.latest accepts no params")
```

### 重要な意味

- `latest.json`不存在のみ`NO_MANIFEST`。
- corrupt JSON、schema違反、alias違反、hash違反、pointer-payload不一致はhard failure。
- endpointは`get_engine()`もLLMも呼ばない。

---

## 8. 凍結hash

以下はSTEP 4検収時のSHA-256。次の実装開始前に一致を確認する。

| ファイル | SHA-256 |
|---|---|
| `src/python/core/retrieval_manifest.py` | `A5271917C49531D2956B7BEDA491E0E4FEA5513149C8FB2776AEB986FC9CE94A` |
| `src/python/core/session_memory.py` | `B2252DA64DACDC5B8F5CEFBC2117F245C37306BB3A4B567FDECC1E8E4A47B078` |
| `src/python/core/consultation_engine.py` | `3CB1C2632D21B13FD04445A209ABEE4C1BD813CBB196CDC98F7AEAEA8097806F` |
| `src/python/core/paths.py` | `7C1CEF6EFE50BD6377409C0F0BCD2022AA1F10380A76506B70CD5DCCAAFF023D` |
| `apps/desktop/src/lib/manifest.ts` | `A3AFFA2611DBEDA0B3466C3208F0696D3F7EF5D57DB1673DD9429B491500B294` |
| `apps/desktop/src/components/ContextObservatory.tsx` | `09F3CFC68B5F944F45491ACCD448E4C8EE0E8ACD8E7B24771E92F27128C2EFBD` |

`facade.py`と`engine_stdio.py`はSTEP 4で正当に変更されているため、この表の凍結対象には含めていない。

---

## 9. 四度のPhase 4-Aインシデント

新規設計・レビュー前に必ず適用する。

### INC-PHASE4A-01: Observabilityが選択を変更

- Retrieved重複除去後に下位Atomで再充填し、contextを変更した。
- 任意roleをaliasへ転記し、実名漏洩可能にした。
- lane会計、Reason Code、validatorが実分岐と一致しなかった。
- 同じ新実装を呼ぶwrapper同士を比較する循環テストを作った。

裁定: 観測機能はbyte-for-byte不変をgolden fixtureで証明する。上位8件確定後に補充しない。

### INC-PHASE4A-02: Dummy construction / implicit coercion

- ダミーManifest IDで一度生成し、後で正しいIDへ帳尻を合わせた。
- `model_hash or ""`で`None`を握り潰した。
- float、bool、listをstrict型として拒否できなかった。

裁定: hash-before-construction。direct constructor / factory / from_dictの全入口でstrict型検証し、例外を`ValueError`へ統一する。

### INC-PHASE4A-03: Persistence読込時のSilent Sanitization

- 改ざんaliasを読込時に`None`へ変換して受理した。
- `validate_manifest()`がpass-throughだった。

裁定: deserializerは修復しない。raw値をdataclassへ渡し、`__post_init__`でhard rejectする。serializer/writer/readerの全境界でvalidatorを通す。

### INC-PHASE4A-04: Pointer-payload decoupling

- `latest.json`のIDとpayload内部IDを個別検証したが、相互一致を検証しなかった。
- Manifest Aの位置へ自己整合したManifest Bを置くすり替えが成立した。

裁定: 参照元IDとpayload IDを全永続化境界で比較する。不一致時はファイルとpointerを変更せず`ValueError`。

### 防衛命令

- 合計値一致だけで正しさを宣言しない。各lane、Reason、分岐を個別に証明する。
- 同じ新実装を呼ぶAPI同士、未注入mock、自己生成期待値をGREEN証明に使わない。
- malformed入力だけでなく「自己整合した別の有効payload」によるすり替えを試験する。
- runtime境界でcast、silent clamp、optional化、catch-and-defaultを行わない。

`docs/architecture/INCIDENT_LEDGER.md`の実ファイルには現時点でINC-01だけが記録されている。INC-02〜04は会話上の承認済みドラフトであり、Phase 4-A最終化前にappend-onlyで追記する必要がある。既存entryを編集・上書きしてはいけない。

---

## 10. 検証ゲートの正本と実行規律

### 所有権

- 完了判定は `docs/AI_SKILLS.md` §3.5 の全 DoD に従う。§3.5 が唯一の正本である。
- 本節は全コマンドや結果を複製しない。正本への案内・実行規律・boundary 入口だけを所有する。
- 実行時点の全ゲートが成功するまで完了報告を禁止する。
- 実測結果は対応する finding の監査台帳 (`docs/AUDIT_FINDINGS_*.md`) へ記録する。
- 過去の結果を現在の成功証明として再利用しない。

### boundary 入口

```powershell
cd apps/desktop
npm.cmd run test:boundary
```

- runner (`scripts/run-boundary-tests.ps1`) が fixture 生成、専用 tsconfig コンパイル、全 runtime suite 実行、生成物削除を所有する。
- 通常の `tsc --noEmit` は `tests-runtime` を対象にしないため代替不可。
- suite 名や件数を本節へ固定しない。

### Python 隔離規律

- Python テストは必ず `python -m pytest` で実行する。
- `tests/conftest.py` の Sandbox を通す。
- 実 `data/` へ書き込まない。
- Python 実行ファイルのユーザー固有絶対パスを本節に書かない。

---

## 11. Frontend runtime boundary — As-Built (STEP 5 完了)

`context.manifest.latest` は React へ完全結線済み。TypeScript generic cast を runtime 検証の代用にしていない。実装は以下で確定 (`ff14ace`)。

### A. Runtime parser — `apps/desktop/src/lib/parseManifest.ts` (新設)

- `parseContextManifestResponseV1(raw: unknown)`。型は `import type` のみ (runtime import ゼロ)。
- Python `retrieval_manifest.py` の `__post_init__` / `validate_manifest()` の**鏡像**: exact key 集合、Enum allowlist、`Number.isSafeInteger` + 非負、strict 文字列 (空・改行拒否)、32文字lowercase hex、固定schema、固定lane順、固定budget、candidate ID一意、status/reason連動、included/char関係、会計(candidate和・lane和・lane別再計算)。
- 応答は 2 分岐のみ許可 (`manifest≠null&&reason==null` / `manifest==null&&reason=="NO_MANIFEST"`)。
- 修復・clamp・null化・削除・既定値化・catch-and-default を全面排除。違反は `ManifestParseError` (path付き、**違反値はmessageへ埋め込まない** = 個人情報漏洩遮断)。
- `any` / `as` cast ゼロ。enum判定は type predicate。BLAKE2b再計算はせず形式のみ検証 (暗号学的ID結合はPython境界が所有)。外部npm依存なし。

### B. engine wrapper — `apps/desktop/src/lib/engine.ts` (追記)

```typescript
export async function latestContextManifest(): Promise<ContextManifestResponseV1> {
  const raw = await pkbInvoke<unknown>("context.manifest.latest");
  return parseContextManifestResponseV1(raw);
}
```

params は送らない。generic cast は使用していない (静的grepで0件を確認済み)。

### C. Container — `apps/desktop/src/components/ContextObservatoryContainer.tsx` (新設)

- `useReducer(reduceManifestFetch, INITIAL_MANIFEST_FETCH_STATE)` + `useRef` 単調seq。
- 純reducer (`manifestFetchState.ts`、React非依存) が `loading/ready/empty/error` を所有。`REQUEST_START` で旧Manifest/旧エラーを消去、stale seq応答は同一参照で破棄、FAILUREイベントにmessageフィールドを持たせず例外文言のUI混入を構造的に遮断。
- **`useEffect`自動取得なし・明示ボタンのみ**。polling/自動再試行/時刻依存なし。catch節は`REQUEST_FAILURE`のdispatchのみ (payload/例外文言を描画経路へ渡さない)。
- `NO_MANIFEST`→empty、parser失敗/IPC失敗→error (固定文言のみ表示)。
- **Finding 10:** single-flight 二重防壁を追加 — `inFlightRef` 再入拒否 (IPC前に立て、`finally` で解除) + `phase === "loading"` の button `disabled` / `aria-busy`。第2操作は queue せず無視。seq/stale ガードは維持。

### D. マウント — `apps/desktop/src/components/ProfileTab.tsx`

**PROFILE タブの `CONTEXT_OBSERVATORY` section へマウント済み** (SOURCE_CODE / ECHO_METRICS / ORACLE_REPORT / TWIN_FORECAST / TENSOR_DIAGNOSTICS と並ぶ既存の診断ダッシュボード内)。新タブ増設なし・`App.tsx` 不変。指揮官裁定 (既存診断パネル内へ配置) に準拠。

### 検証済みテスト (`tests-runtime/manifest_boundary.test.ts`、依存ゼロ自作ハーネス、42件)

valid受理 / exact NO_MANIFEST受理 / candidate順保存 / トップレベル・manifest・candidate・lane_usage各層の全拒絶 / status-reason不整合 / 会計違反 / alias値非漏洩 / reducer遷移・staleガード / 越境golden (Python実出力→parser受理) を網羅。`tsc --noEmit`・Vite build・Python 170回帰すべてPASS。

---

## 12. Phase 3-Bまでの確定baseline

HEAD `412c1c2`には以下が含まれる。

- D1 5軸HumanSourceCode
- D2 PROBE funnel / immutable HistoricalNode
- F6 PROBE UI
- Custom Theme
- UI Orphan Integration / PROFILE
- GD Thread UI + streaming
- Project Calculus Phase 1: frontend Hidden CoT redactor / SVG 6D radar
- Phase 2: 6D tensor profile / deterministic Semantic Compression / O(n) redactor
- Phase 3-A: UX overhaul / MBTI preview / tensor tooltips
- Phase 3-B: Romance Analysis strict schema

これらはpush済みbaselineであり、Phase 4-A作業を理由に変更しない。

---

## 13. 新任アーキテクト補佐への最初の指示

次のチャットでは、以下を最初の命令として扱う。

```text
docs/HANDOFF.mdを全文読み、git log/statusと凍結hashを照合せよ。
docs/AI_SKILLS.md (§16 SKILL-PKB-BOUNDARY-V3 含む) と
docs/architecture/INCIDENT_LEDGER.md (INC-01〜04) の防衛命令をロードせよ。
Phase 4-Aは STEP 1〜5 + PROFILEマウント完了・ローカルコミット済み (ff14ace + docs同期)。
これらは検収済み成果物として保護し、コアロジック (src/python/core/) を1 byteも変更するな。
push は指揮官の明示指示があるまで固く禁止 (origin/main は 412c1c2 のまま)。
新規作業は個別SPEC → 憲法ガードRED → 実装のmicro-step順を守る。
TypeScript castだけの見せかけのGREEN、silent sanitization、
自己生成期待値をGREEN証明に使う行為を禁止する。
```

---

*本書は2026-07-11のPhase 4-A As-Built (実装コミット `ff14ace`、boundary 42/42・Python 170 GREEN、PROFILEマウント) を基準に同期した。*
