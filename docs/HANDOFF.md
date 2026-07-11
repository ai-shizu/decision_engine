# PKB 最高アーキテクト補佐 引き継ぎ書

> **更新日:** 2026-07-11
> **対象:** Phase 4-A Context Observatory / RetrievalManifestV1 進行中
> **リポジトリ:** `C:\Users\badger\Documents\cursur\decision_engine`
> **ブランチ:** `main`
> **HEAD / origin/main:** `412c1c2d9e02accb726909500faef75a5fe1d3c5`
> **最新コミット:** `412c1c2 feat(consult): implement romance analysis with strict fallback schema (Calculus Phase 3-B final)`
> **重要:** Phase 4-A成果物はすべて未コミット。commit / pushは禁止されたまま。

---

## 0. 30秒で把握する現在地

PKBは、完全オフラインのTauri + React + Python意思決定支援アプリである。Phase 3-Bまでは`main`へpush済み。現在はPhase 4-Aとして、12,000文字のSemantic Compressionが「何を採用し、何を棄却したか」を決定論的に可視化するContext Observatoryを実装中である。

完了・検収済み:

1. **STEP 1:** Python `RetrievalManifestV1`、context compiler計装、厳格永続化
2. **STEP 2:** TypeScript Manifest型契約
3. **STEP 3:** 静的`ContextObservatory` UI / SVG
4. **STEP 4:** Python薄層IPC endpoint `context.manifest.latest`

未実装:

- TypeScript runtime validator
- `engine.ts`のIPC wrapper
- loading / ready / empty / errorを所有するReact container
- Context Observatoryの実画面への配置
- frontend runtime境界テスト
- Phase 4-A as-built、commit、push

**次の作業はfrontend runtime validatorとIPC結合である。** 静的UIを完成扱いしてはいけない。

---

## 1. 新チャット開始時の必須手順

次任者は、設計・レビュー・編集の前に必ず以下を実行する。

1. 本書を全文読む。
2. `docs/AI_SKILLS.md`を全文読む。不変条件が競合する場合はAI_SKILLSを優先する。
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

`HEAD == origin/main == 412c1c2d9e02accb726909500faef75a5fe1d3c5`。

2026-07-11時点の期待される`git status --short`:

```text
 M src/python/core/consultation_engine.py
 M src/python/core/facade.py
 M src/python/core/paths.py
 M src/python/core/session_memory.py
 M src/python/engine_stdio.py
?? .claude/
?? apps/desktop/src/components/ContextObservatory.tsx
?? apps/desktop/src/lib/manifest.ts
?? docs/architecture/
?? src/python/core/retrieval_manifest.py
?? tests/test_context_manifest_ipc.py
?? tests/test_retrieval_manifest.py
```

本書更新後は、これに` M docs/HANDOFF.md`が加わる。

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
| `apps/desktop/src/components/ContextObservatory.tsx` | STEP 3静的UI |
| `docs/architecture/INCIDENT_LEDGER.md` | Phase 4-A事故台帳。現状はINC-01のみ実ファイル化 |
| `.claude/` | ユーザー環境由来の未追跡。触らない、stageしない |

### Git禁止事項

- ユーザーの明示指示なしに`git add`、commit、pushを行わない。
- `.claude/**`をstageしない。
- 未コミットのPhase 4-A差分をrevertしない。
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
  -> engine.ts wrapper [未実装]
  -> TypeScript runtime parser [未実装]
  -> ContextObservatory container [未実装]
  -> ContextObservatory dumb view
```

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

## 10. 最新GREENゲート

最高アーキテクト補佐が実環境で再実行した結果:

```text
170 passed in 1.32s
```

対象:

- `tests/test_context_manifest_ipc.py`: 7
- `tests/test_retrieval_manifest.py`: 79
- `tests/test_engine_tensor_profiling_backend_contract.py`: 28
- `tests/test_integration.py`: 42
- `tests/test_probe_ui_ipc.py`: 2
- `tests/test_source_code.py` + `tests/test_probe_funnel.py`: 12

コマンド:

```powershell
& 'C:\Users\badger\AppData\Local\Programs\Python\Python312-arm64\python.exe' -m pytest `
  tests/test_context_manifest_ipc.py `
  tests/test_retrieval_manifest.py `
  tests/test_engine_tensor_profiling_backend_contract.py `
  tests/test_integration.py `
  tests/test_probe_ui_ipc.py `
  tests/test_source_code.py `
  tests/test_probe_funnel.py -q
```

Frontend:

```powershell
cd apps\desktop
npx.cmd tsc --noEmit
npm.cmd run build
```

最新再検証:

- TypeScript: PASS
- Vite build: PASS、60 modules transformed、766ms
- `git diff --check`: PASS
- `git status --short data/`: clean
- package files: 差分なし

テストは必ず`python -m pytest`で実行し、`tests/conftest.py`のSandboxを通す。実`data/`へ1 byteも書かない。

---

## 11. 次の実装: Frontend runtime boundary

### 目的

`context.manifest.latest`をReactへ接続する。ただしTypeScript generic castをruntime検証の代用にしない。

### 推奨micro-step

#### A. Runtime parser

対象候補: `apps/desktop/src/lib/manifest.ts`

- 入力は`unknown`。
- exact key集合を検証する。
- Enum literalをallowlist検証する。
- integerは`typeof value === "number" && Number.isInteger(value)`。
- nonnegative、32文字lowercase hex、固定schema、固定lane順、固定budgetを検証する。
- candidate ID一意、lane件数、文字数会計を検証する。
- responseは次の2分岐だけを許可する。

```text
manifest != null && reason == null
manifest == null && reason == "NO_MANIFEST"
```

- extra key、optional、`undefined`、NaN、Infinity、bool、floatを拒否する。
- 不正値をclamp、`null`化、削除、既定値化しない。
- 外部npm依存を追加しない。Zodは現状未導入なので、stdlib TypeScriptの明示parserを優先する。
- frontendでBLAKE2bを独自再実装しない。暗号学的ID結合は検収済みPython境界が所有する。ただしhash形式は検証する。

#### B. engine wrapper

対象候補: `apps/desktop/src/lib/engine.ts`

禁止:

```typescript
return pkbInvoke<ContextManifestResponseV1>("context.manifest.latest");
```

推奨:

```typescript
const raw = await pkbInvoke<unknown>("context.manifest.latest");
return parseContextManifestResponseV1(raw);
```

paramsは送らない。

#### C. Container state

静的`ContextObservatory`はdumb viewとして維持する。別containerまたは明確な親で次を所有する。

```text
state: "loading" | "ready" | "empty" | "error"
manifest: RetrievalManifestV1 | null
errorMessage: string
```

- request開始時に旧Manifestと旧成功表示を消す。
- `NO_MANIFEST`はempty。
- parser失敗、IPC失敗はerror。
- error時に不正payloadや個人情報を画面へdumpしない。
- refreshは明示ボタンのみ。polling、時刻依存、自動再試行を追加しない。
- stale response対策は既存correlation ID規律へ従う。

#### D. UI配置

`ContextObservatory.tsx`はまだAppへ登録されていない。どのタブへ置くかは未裁定。勝手に新タブを増やさない。PROFILE内panel、独立タブ等の配置は指揮官の明示裁定を得てから実装する。

### 次段階の必須テスト

- valid responseを受理
- exact `NO_MANIFEST`を受理
- missing / extra key拒否
- Enum未知値拒否
- float / bool / NaN / Infinity拒否
- lane重複・順序違反・固定budget違反拒否
- candidate/lane/count/accounting不整合拒否
- `manifest:null, reason:null`等の不正組合せ拒否
- parser失敗時に旧Manifestを表示しない
- IPC error時にsafe error state
- `pkbInvoke<unknown>`経路を静的契約で確認
- `tsc --noEmit`、Vite build、Python 170回帰

### 次の停止条件

- 実装前にRED契約を作る。
- backend coreを変更しない。
- testを通すために型を緩和しない。
- App配置まで一気に進まない。
- commit / pushしない。

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
docs/HANDOFF.mdを全文読み、git HEAD/statusと凍結hashを照合せよ。
docs/AI_SKILLS.mdとdocs/architecture/INCIDENT_LEDGER.mdの防衛命令をロードせよ。
Phase 4-A STEP 1〜4は検収済み未コミット成果物として保護せよ。
次はfrontend runtime validator / IPC wrapper / containerをmicro-stepで設計する。
TypeScript castだけの見せかけのGREEN、silent sanitization、backend変更、App配置への先回りを禁止する。
commit / pushは指揮官の明示指示まで行うな。
```

---

*本書は2026-07-11の実worktreeと、最高アーキテクト補佐が再実行した170件のGREENを基準に作成した。*
