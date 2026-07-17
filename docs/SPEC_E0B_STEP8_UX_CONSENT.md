# SPEC — E0b STEP 8: UX Consent & Policy Unlocking (Composer-Executable Blueprint)

> **読者:** 自律型コーディング AI（Cursor Composer 等）。人間向け解説ではない。**一字一句従え。裁量なし。**
> **親仕様:** `docs/SPEC_E0B_KNOWLEDGE_GATEWAY_v3.md` §6/§8 / STEP 5–7 ブループリント（承認済み）。
> **前提:** STEP 0–7 ロック済み。egress パイプライン（attestation / FSM / dual-run / deny-table / TOCTOU 封鎖）は
> 完成し、**本番 `NetworkPolicy::Off` で UI から到達不可**。本 STEP は「ユーザー同意（consent）」を配線し、
> **摩擦ゼロの UX（モーダル皆無・アンビエント）** で外部検索能力を開放する。
> **ACK 規律:** STEP 8.E 完了時に HARD STOP。指揮官 ACK まで先へ進むな。

---

## 0. 起草時に検証済みのリポジトリ事実（Composer はこれを前提にせよ・推測禁止）

- **状態管理ライブラリは存在しない**（Zustand/Redux/Jotai 無し）。指令は「Zustand 等」と言うが、**新規に
  Zustand を導入するな。** 既存様式＝React `useState`/`useReducer` + **純 reducer**（`src/lib/manifestFetchState.ts`
  の「React 非依存・純関数 reducer」）+ dumb view + container（`ContextObservatory*` 様式）に従え。
- **IPC ラッパー:** `src/lib/engine.ts` の `invokeEngine(cmd, parser, params)`。既存:
  `knowledgeResearch(query) -> KnowledgeResearchReceipt`（`knowledge_research` を呼ぶ・STEP 6.F）、
  `loadSettings()`, `saveFixedAttributes(attrs)`。
- **`KnowledgeResearchReceipt`**（`src/lib/parseEngineResponse.ts:877`）:
  `{ schema: "knowledge_research_receipt.v1", research_id: <64hex>, results_persisted: number }`。
  **provenance の唯一の信号は `results_persisted > 0`**（provider は v1 では Wikipedia 固定）。
- **Rust `knowledge_research`**（`commands.rs:285`, invoke_handler 登録済み `lib.rs:53`）: 現在
  `refuse_if_policy_off(NetworkPolicy::Off)` を**ハードコード**し常に `"EGRESS_LIVE_NOT_READY"`。State を取らない。
- **`NetworkPolicy`**（`orchestrator.rs:25`）= `{ Off, FakeAllowed }`。**`Live`/`On` 変種は存在しない**（本 STEP で追加）。
  managed state 様式は `State<'_, Arc<EngineManager>>`（`.manage(...)` in `lib.rs::run`）。
- **consult 応答**（`parseConsultResponse.ts` の `ConsultResponse`）= `{query, mode, answer, report?, romance_analysis?}`。
  **provenance フィールドは無い**（exact-keys 厳格）。→ **consult boundary schema を変えず、provenance は
  `KnowledgeResearchReceipt` から導出する**（§1.4）。
- **Settings UI:** `SettingsTab.tsx` は `useState` + `loadSettings()` + `saveFixedAttributes(attrs)` + 既存 `Toggle`
  コンポーネント（`apple_calendar_available` 等で使用）。
- **テスト:** `apps/desktop/tests-runtime/*.test.ts`（**依存ゼロ自作 harness**・`scripts/run-boundary-tests.ps1`・
  `npm.cmd run test:boundary`）。**React コンポーネントの render テスト基盤は無い。** よって UI の**ロジックは
  純 reducer / 純関数へ括り出し boundary harness でテスト**し、React 側は薄い dumb view に留める。

---

## 1. アーキテクチャと UX 規則 (Constraints & UX Rules)

## 1.1 会話のテンポを殺さない（モーダル・ブロッキング UI の絶対禁止）

- **`window.alert` / `window.confirm` / モーダルダイアログ / 全画面オーバーレイ / 入力・スクロールを奪う要素を
  一切使うな。** 同意はトグル（Settings）で 1 回、実行中はアンビェント（境界の発光・控えめスピナー）のみ。
- **メインチャット UI をフリーズさせるな。** research は `async/await` で非同期に走らせ、待機中も入力・送信・
  スクロールを一切ブロックしない。research の解決/失敗は UI スレッドを止めずに state 更新で反映する。

## 1.2 ゼロトラストの不変（UX が防御を弱めない）

- **二要素 egress ゲート:** 実 egress には (a) **ユーザー同意トグル ON**（本 STEP）**かつ** (b) **egress-live ビルド**
  （STEP 7・別デプロイ判断）の両方が必要。同意 ON でも egress-live 非ビルドなら `knowledge_research` は
  `"EGRESS_LIVE_NOT_READY"` を返す（既存メッセージ）。トグルは egress の**必要条件であって十分条件ではない**。
- **同意は既定 OFF。** 初回起動・未設定時は必ず `Off`。トグルは明示的にユーザーが ON にした時だけ `Live` へ昇格。
- **トグルは検証パイプラインを迂回しない。** 同意 ON でも research は STEP 3/4/5/7 の attestation ∧ FSM ∧
  dual-run PII ∧ deny-table ∧ TOCTOU 封鎖を**そのまま通る**。トグルが増やすのは「Off→Live の 1 分岐」だけ。
- **provenance/インジケーターに PII・生テキスト・エラー値・URL を出さない**（既存の UI 秘匿規律の適用拡張）。

## 1.3 データフロー（トグル → Rust ポリシー → research）

```text
[Settings トグル ON/OFF]
  → engine.ts: setKnowledgeResearchPolicy(enabled: boolean)  → Tauri cmd "knowledge_policy_set"
      → Rust: NetworkPolicyStore（managed state Arc<Mutex<NetworkPolicy>>）を Off/Live へ更新 + 永続化
  → 起動時: getKnowledgeResearchPolicy() → "knowledge_policy_get" → 保存済み consent で初期化（既定 Off）
[チャット送信（同意 ON 時）]
  → researchUiReducer: dispatch(START) → isResearching=true（アンビエント表示・非ブロッキング）
  → knowledgeResearch(query)（内部で Rust が stored policy を読む。Off なら即 refuse・Live なら full pipeline）
  → 解決: dispatch(DONE, receipt) → isResearching=false、receipt.results_persisted>0 なら provenance を記録
  → consult(query)（STEP 6.D により ingest 済み外部知識が dynamic_suffix に載る）
  → メッセージ描画時: 当該 research の provenance があれば「🔗 Wikipediaより参照」chip を末尾に付与
```

## 1.4 provenance は receipt 由来（consult boundary を変えない）

- consult 応答 schema は**不変**（exact-keys 厳格・変更は広範な boundary 改修になる）。代わりに、**その相談で
  発火した `knowledgeResearch` の `KnowledgeResearchReceipt.results_persisted > 0`** を provenance 信号とする。
- 純関数 `deriveProvenance(receipt: KnowledgeResearchReceipt | null): Provenance | null`
  （`results_persisted>0` → `{ source: "wikipedia", label: "Wikipediaより参照" }`、else `null`）を新設し、
  boundary harness でテスト。React は返り値があれば小さな chip を描画するだけ（dumb）。

## 1.5 影響範囲 (Blast Radius)

**新規作成:**
```text
apps/desktop/src/lib/researchPolicyState.ts     # 純 reducer + deriveProvenance（React 非依存）
apps/desktop/src/lib/parseKnowledgePolicy.ts     # knowledge_policy_get/set 応答の runtime parser
apps/desktop/tests-runtime/research_ux_boundary.test.ts   # 純 reducer / deriveProvenance / policy parser の boundary テスト
apps/desktop/src-tauri/src/knowledge/policy_store.rs      # NetworkPolicyStore（managed state + 永続化）
```
**改変を許可:**
```text
apps/desktop/src-tauri/src/knowledge/orchestrator.rs   # NetworkPolicy に `Live` 追加（Off/FakeAllowed は維持）
apps/desktop/src-tauri/src/commands.rs                 # knowledge_policy_set/get 追加、knowledge_research が
                                                       #   stored policy を読むよう修正（ハードコード Off を除去）
apps/desktop/src-tauri/src/lib.rs                      # 2 command 登録 + NetworkPolicyStore を .manage()
apps/desktop/src/lib/engine.ts                         # setKnowledgeResearchPolicy/get wrapper 追加
apps/desktop/src/components/SettingsTab.tsx            # 同意トグル追加（既存 Toggle 使用）
apps/desktop/src/components/ConsultTab.tsx             # isResearching アンビェント表示 + provenance chip（dumb）
```
**触れてはならない:**
```text
parseConsultResponse.ts の ConsultResponse schema（provenance を足さない）/ STEP1-7 の Rust 検証コア /
tests/test_e0b_constitutional_guard.py（reqwest:: 封じ込め・safe-config を維持）/ Python コア / golden
```

---

# セクション 2: 実行フェーズ計画 (TDD Loops for UX)

各 STEP は **RED → 検証コマンド → GREEN → 完了条件**。UI ロジックは**純 reducer/純関数を boundary harness で**
テスト（React render テスト基盤は無い）。Rust 側は `cargo test`。

---

### STEP 8.A: グローバル・オプトインの配線（Policy State RED）

1. **RED（Rust: tests, Frontend: tests-runtime）:**
   - **Rust:** `NetworkPolicy::Live` を追加。`NetworkPolicyStore`（`Arc<Mutex<NetworkPolicy>>`・既定 `Off`・
     `set(enabled: bool)` は ON→`Live`/OFF→`Off`・`get()`・永続化 load/save）。`knowledge_research` を
     **stored policy を読む**よう修正（`refuse_if_policy_off(store.get())`）。テスト:
     `store 既定 == Off`；`set(true) → Live`；`set(false) → Off`；`Off で research は EGRESS_LIVE_NOT_READY`；
     **`Live` かつ egress-live 非ビルドでも `EGRESS_LIVE_NOT_READY`**（二要素ゲート：transport 不在で refuse）；
     永続化 round-trip（save→load で同値）。
   - **Frontend:** `parseKnowledgePolicy.ts`（`knowledge_policy_get/set` 応答 `{ schema:"knowledge_policy.v1",
     enabled: boolean }` の runtime parser・exact-keys・unknown 拒否・違反値を message に出さない）。
     `engine.ts` に `setKnowledgeResearchPolicy(enabled)` / `getKnowledgeResearchPolicy()` wrapper。
     `research_ux_boundary.test.ts`: parser の受理/拒否マトリクス。
2. **検証コマンド:**
   ```
   cargo test -p pkb-desktop --lib policy -- --nocapture
   cd apps/desktop && npm.cmd run test:boundary
   npm.cmd exec tsc --noEmit
   ```
3. **GREEN（実装）:** 上記 Rust/Frontend を実装。`lib.rs` に 2 command 登録 + `.manage(NetworkPolicyStore::new())`。
   SettingsTab に同意トグル（既存 `Toggle`・ラベル「🌐 外部ネットワーク検索による知識補強を許可する」・
   `checked` は起動時 `getKnowledgeResearchPolicy()`・`onChange` で `setKnowledgeResearchPolicy(v)`）。
   **同意既定 OFF・モーダル無し。**
4. **完了条件:** policy round-trip / 二要素ゲート / parser マトリクスが全 PASS。**→ コミット。**

---

### STEP 8.B: アンビエント UI の構築（Transient UX RED）

1. **RED（tests-runtime/research_ux_boundary.test.ts, 純 reducer）:**
   - `researchUiReducer`（React 非依存・`manifestFetchState.ts` 様式）: state `{ phase: "idle"|"researching",
     seq: number }`。`START` → `researching`・seq++；`DONE`/`FAIL` → `idle`（**stale seq の DONE/FAIL は無視**＝
     古い research の解決が新しい状態を壊さない・単調 seq ガード）。
   - テスト: `START→researching`、`DONE(matching seq)→idle`、`DONE(stale seq)→無視`、
     `連続 START→START` の単調性、`FAIL→idle`。
   - **非ブロッキング契約:** reducer に modal/alert/blocking を表す状態が無いこと（`researching` は表示ヒントのみ）。
2. **検証コマンド:**
   ```
   cd apps/desktop && npm.cmd run test:boundary
   ```
3. **GREEN（実装）:** `researchPolicyState.ts` に reducer 実装。`ConsultTab.tsx` は `useReducer(researchUiReducer)` +
   `knowledgeResearch` を `async` で呼び、`phase==="researching"` の間だけ**控えめなアンビェント表示**
   （境界発光 or 小スピナー・`aria-busy`・**入力欄/送信/スクロールは disabled にしない**）。完了で自然に消える。
   **`alert`/`confirm`/モーダルを import も使用もしない**（grep で 0 件）。
4. **完了条件:** reducer 遷移・stale seq 破棄・単調性が PASS、ConsultTab に blocking UI が無い（静的確認）。**→ コミット。**

---

### STEP 8.C: 取得痕跡のレンダリング（Provenance RED）

1. **RED（tests-runtime/research_ux_boundary.test.ts）:**
   - `deriveProvenance(receipt): Provenance | null`:
     `results_persisted > 0` → `{ source:"wikipedia", label:"Wikipediaより参照" }`；
     `results_persisted === 0` → `null`；`receipt === null` → `null`。
   - テスト: `results_persisted=2 → chip`；`=0 → null`；`null → null`；
     **provenance オブジェクトに research_id・生テキスト・URL・PII を含まない**（`source`/`label` の固定 2 値のみ）。
2. **検証コマンド:**
   ```
   cd apps/desktop && npm.cmd run test:boundary
   ```
3. **GREEN（実装）:** `deriveProvenance` を `researchPolicyState.ts` に実装。`ConsultTab.tsx` は、その相談で
   `results_persisted>0` だった時だけ、assistant メッセージ末尾に**小さな chip**「🔗 Wikipediaより参照」を
   描画（`deriveProvenance` の返り値がある時のみ・dumb）。**外部知識を含まない回答には付与しない。**
4. **完了条件:** provenance 導出の受理/非付与/秘匿が PASS。**→ コミット。**

---

### STEP 8.D: フロントエンド・フル回帰 + 品質ゲート

1. **検証コマンド:**
   ```
   cd apps/desktop && npm.cmd run test:boundary        # 新 research_ux_boundary + 既存 boundary 全 PASS
   npm.cmd exec tsc --noEmit                            # 型 GREEN
   npm.cmd run build                                    # vite build GREEN
   cd ../.. && cargo test -p pkb-desktop --lib          # Rust 非回帰（policy 含む・既定は無網）
   python -m pytest tests/test_e0b_constitutional_guard.py -v   # reqwest:: 封じ込め維持
   cargo clippy -p pkb-desktop --all-targets -- -D warnings     # knowledge/ クリーン
   ```
2. **完了条件:** 全 boundary + tsc + vite build + Rust 非回帰 + clippy + 憲法ガード GREEN。**→ HARD STOP。**

---

### STEP 8.E: HARD STOP

全ゲート GREEN・コミット済みで、**出力を停止しユーザー（指揮官）へ報告し ACK を待て。**

---

# セクション 3: コミットと停止規律 (Checkpoints & HARD STOP)

## 3.1 コミット規律

`git status --short` で目視。当該 STEP の対象のみ stage（`git add -A`/`.` 禁止）。**`.claude/` を stage しない。
push 禁止。** revert/reset/`checkout --` 禁止。STEP0–7 資産・golden・Python コア・凍結ファイルを改変しない。

| STEP | stage 対象 | コミットメッセージ |
|---|---|---|
| 8.A | `src-tauri/src/knowledge/{policy_store,orchestrator,mod}.rs`, `src-tauri/src/{commands,lib}.rs`, `src/lib/{engine,parseKnowledgePolicy}.ts`, `src/components/SettingsTab.tsx`, `tests-runtime/research_ux_boundary.test.ts` | `feat(e0b-step8): STEP 8.A — consent toggle wired to Rust NetworkPolicy store (two-factor egress gate)` |
| 8.B | `src/lib/researchPolicyState.ts`, `src/components/ConsultTab.tsx`, `tests-runtime/research_ux_boundary.test.ts` | `feat(e0b-step8): STEP 8.B — ambient non-blocking research indicator (pure reducer, stale-seq guard)` |
| 8.C | `src/lib/researchPolicyState.ts`, `src/components/ConsultTab.tsx`, `tests-runtime/research_ux_boundary.test.ts` | `feat(e0b-step8): STEP 8.C — subtle Wikipedia provenance chip derived from research receipt` |

各コミット末尾:
```
Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
```
（8.D はコード差分なしなら空コミットを作らない。）

## 3.2 HARD STOP（絶対命令・報告必須項目）

1. `npm.cmd run test:boundary`（新 research_ux + 既存全件）、`tsc --noEmit`、`vite build`、
   `cargo test --lib`（policy 含む）、`clippy`、python 憲法ガードの各実測。
2. **二要素ゲートの明示:** 同意既定 OFF、トグル ON→`NetworkPolicy::Live`、**Live でも egress-live 非ビルドは
   `EGRESS_LIVE_NOT_READY`**、research は STEP3/4/5/7 検証を迂回しないこと、を、どのテストがどう証明したか。
3. **モーダル皆無・非ブロッキングの明示:** `alert`/`confirm`/モーダルが grep 0 件、research 中も入力/送信/
   スクロールが disabled にならないこと。
4. **provenance が receipt 由来**で consult boundary schema を変えていないこと（`ConsultResponse` 差分ゼロ）、
   chip に PII/生テキスト/URL/research_id が出ないこと。
5. `git log --oneline`（8.A–8.C）。STEP0–7 非回帰。

**ACK なしに先へ進むな。** 実 egress の常時 ON 化・live E2E の CI 常駐・egress-live のデフォルトビルド化を勝手に行うな。

## 3.3 HARD STOP（異常時）

- **Zustand 等の新規状態ライブラリを入れたくなった:** 禁止。既存 `useReducer` + 純 reducer 様式を使う。
- **provenance のため consult boundary schema を変えたくなった:** 禁止。receipt 由来で導出する（§1.4）。
- **同意 ON だけで egress してしまう（egress-live 非ビルドで実通信）:** 二要素ゲート違反。transport 不在時は
  必ず `EGRESS_LIVE_NOT_READY`。実装を直す。
- **モーダル/alert/入力ブロックを使いたくなった:** UX 至上命題違反。アンビェント表示に直す。
- **トグルが検証パイプラインを迂回:** attestation/FSM/dual-run/deny-table を通らない経路を作ったら設計違反。停止・報告。

## 3.4 STEP 8 完了の定義 (DoD)

- 同意トグル（既定 OFF・モーダル無し）が Rust `NetworkPolicyStore` へ同期し永続化。トグル ON で `Live` 昇格。
- **二要素ゲート**（consent ∧ egress-live ビルド）維持。Live でも非 egress-live は `EGRESS_LIVE_NOT_READY`。
  research は STEP3/4/5/7 の全検証を迂回しない。
- アンビェント表示は純 reducer（stale-seq ガード）で、research 中も入力/送信/スクロールを一切ブロックしない。
  `alert`/`confirm`/モーダル 0 件。
- provenance は `KnowledgeResearchReceipt.results_persisted>0` 由来の小 chip。consult boundary schema 不変。
  PII/生テキスト/URL/research_id 非表示。
- 全 boundary + tsc + vite build + Rust 非回帰 + clippy + 憲法ガード GREEN。STEP0–7 非回帰。
- HARD STOP・ACK 待ち。

---

# セクション 4: 指揮官裁定が推奨される点（実装は上記で進められる）

1. **consent の永続化先:** `NetworkPolicyStore` の save/load を (a) Rust 所有ファイル、(b) 既存 Python settings
   （`saveFixedAttributes`）のどちらにするか。**推奨 (a)**（Rust ポリシーを Python settings に結合させない・
   egress 決定を Rust 内に閉じる）。UI 初期化は `getKnowledgeResearchPolicy()` で Rust から読む。
2. **research の発火点:** 本 STEP は「同意 ON 時、チャット送信で `knowledgeResearch(query)` → then `consult`」を
   ConsultTab に配線する最小形。research を自動発火にするか明示ボタンにするかは UX 裁定（v3 は深度 1・明示操作
   志向）。**推奨: 送信時に同意 ON なら自動 1 回**（アンビェントゆえ摩擦ゼロ）だが、明示ボタン化も可。
3. **実 egress の本番開放（egress-live デフォルトビルド化 + NetworkPolicy 常時 On 運用）は本 STEP のスコープ外**。
   それは STEP 7 で私が指摘した「最後の一線」であり、(a) egress-live wiring の実機再確認、(b) live E2E 運用方針、
   (c) レート制限/監査、を伴う独立レビュー・別 ACK を要する。
