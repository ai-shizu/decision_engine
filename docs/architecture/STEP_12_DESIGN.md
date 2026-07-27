# STEP 12 設計書 — Outer Fallback の完成

- **作成日:** 2026-07-27
- **正本順位:** `EDINET_LANE_DESIGN_V3.md` > `EDINET_LANE_IMPLEMENTATION_DIRECTIVE.md` §Step 12 > 本書
- **対象読者:** 実装担当AI（Cursor）。本書は Diff 指示書であり、ここに無い構造変更は禁止。
- **前提:** Step 11 受理済み（cancel 橋 / ZIP 直列パイプライン / partials→acquisition 変換は完成・変更禁止）。Phase F 繰延項目（実フィクスチャ資産・実機計測・OFF 全再走）を本書の前提に**していない**。
- **状態:** 設計承認待ち → 承認後に実装開始

---

## 1. 監査要約 — 現状の穴（実測確認済み）

作業対象は `apps/desktop/src-tauri/src/llm/commands_sim.rs` のみ。行番号は現ワーキングツリー（Step 11 実装後）。

| # | 場所 | 実測内容 | 判定 |
|---|---|---|---|
| A1 | `:846` `Err(error) if base.is_empty() => return Err(error.message)` | base（`FactCells` BTreeMap）が空のときだけ、inner の分類済み失敗が**生 String として V3 wire へ脱出**する唯一の経路。FE はこれを「データなし」と誤変換し得る | **修正対象** |
| A2 | `:878-879` `SubjectKey::try_from(...).map_err(...)?` / `:881` `return Err("confirmed identity does not match rekey target")` | **取得成功後**の Err 脱出。`merged`（EDINET 取得済みフィールドを含む）を破棄して UI へエラー文字列だけ返す。指示書 §Step 12「抽出済みフィールドは画面用の一時結果として返してよい」違反 | **修正対象** |
| A3 | `:1026`（`current_v3_response` 内 `vault.fact_cells(...).map_err(map_vault_err)?`）/ `:1064`（`persistence_response` の Persisted 分岐の read-back `?`） | Vault **読み戻し**失敗で Err 脱出。:1064 は **CAS 永続化が成功した後**に merged を破棄する — 最悪の順序（書けたのに UI にはエラー） | **修正対象** |
| A4 | base 確保位置 | `:823-832` の base 構築は `resolve_typed_edinet_acquisition`（`:833`）より前で正しい。`:739-773` の schema / fact_cells / transition 検証と `sanitize_company_facts(...)?` は base 構築より**前**の Err 脱出だが、これらは**呼出側入力の棄却**であり EDINET レーン失敗ではない | **境界線の明文化のみ**（§2.1）。コード変更なし |
| A5 | `soft_fallback_named_base`（`:621`） | V3 経路では未使用。使用箇所は `resolve_company_facts_with`（`:485` `:490` `:498` `:579`）= **interview レーン**（`start_interview_session` 等が使う `CompanyFacts` 直返し経路）のみ | §2.3 で収斂を決定 |
| A6 | `resolve_company_facts_with :512` `:518`（**追加発見**） | interview レーン内 `ListCandidateFilter::by_edinet_code/by_filer_name` の `ok_or_else(...)?` が、**injected base が存在しても** Err 脱出する。不正な code/name 入力で Wikipedia base ごと失う。`:483` の自コメント「interview must not die」と矛盾 | **修正対象** |
| A7 | `:794-797` vault 初回読取失敗 | `read_failed = true` として空 cells で続行（soft）。ただし応答 warnings に一切出ない。`EdinetWarningCode::VaultReadFailed` は **wire enum に定義済み（`:228`）かつ未使用** | **修正対象**（warning 付与のみ） |
| A8 | 排他順序 | `EdinetJobGuard` / `_watch` / `_cancel_on_drop` は `resolve_typed_edinet_acquisition` のローカルに閉じ、`ResolvedEdinetAcquisition` / `EdinetResolveFailure` のフィールドに guard/token は**含まれない**（`:233-251` で確認）。よって outer fallback は構造上必ず guard drop（= gate open）後に走る | **現状維持を禁止事項で固定** |

**inner の分類（Step 11 §3.5 決定表・`resolve_failure` 群 `:1470-1723`）は完成しており正しい。Step 12 は outer が status を保ったまま base を返す「出口の一本化」だけを行う。inner の status 写像には触れない。**

---

## 2. 境界契約と型

### 2.1 一本線 — 入力棄却層 / fallback 層

`enrich_company_facts_from_edinet_inner` を次の2層に**行位置で**分割する。境界は base 確定行（現 `:823-832`）である。

```text
┌ 入力棄却層（base 確定より前）────────────────────────────┐
│ schema_version / subject_revision / fact_cells 形状 /       │
│ subject_transition 整合 / sanitize_company_facts(injected) / │
│ SubjectKey::try_from(request 由来)                           │
│ → Err(String) を許可（wire 入力不正 = InvalidArgument 級。   │
│   V3 冒頭「機械的注意」の契約どおり）。facts は返さない。    │
├ base 確定（let base = …）───────────────────────────────┤
│ fallback 層（base 確定より後）                               │
│ → **Err 禁止**。すべての EDINET レーン事象・Vault 事象は      │
│   §3 決定表に従い Ok(EnrichOutcome) を返す。base を所有し     │
│   続け、いかなる分岐でも非空フィールドを失わない。            │
└──────────────────────────────────────────────────┘
```

- `:773` の sanitize は入力棄却層に**属したまま**でよい（呼出側が上限超過・不正 bytes を送った = 入力不正）。位置の移動は不要。**明文化されたこの1本線が A4 の回答である。**
- 例外はただ1つ: `fetch_edinet_company_facts`（旧コマンド）の互換 Err（§2.2 `legacy_error`）。これは fallback 層が **Ok の中に運ぶ**値であり、`?` 脱出ではない。

### 2.2 V3 レーンの出口型（新設）

```rust
/// fallback 層の唯一の戻り値。inner の Err は入力棄却層専用になる。
struct EnrichOutcome {
    response: EdinetEnrichmentResponseV3,
    /// Some(...) は「base が空 かつ EDINET レーン失敗」の1ケースのみ。
    /// 旧 fetch_edinet_company_facts がこれを Err(message) として表面化し、
    /// 「エラー文字列は現行のまま」（V3 §2.2）の互換を守る。
    /// V3 コマンドは常に無視して response を返す。
    legacy_error: Option<String>,
}

/// V3 レーン唯一の outer fallback コンストラクタ。
/// inner 失敗の status を**無加工で**転記し、facts には base をそのまま載せる。
/// （outer での status 丸め — 例: Cancelled→NetworkFailed — は §5 で不採用）
fn fallback_enrichment_response(
    base: &FactCells,
    revision: i64,
    subject: SubjectKey,
    failure: &EdinetResolveFailure,
    extra_warnings: &[EdinetWarningCode], // read_failed 時の VaultReadFailed 等
) -> EdinetEnrichmentResponseV3;
// 実体: v3_response(wires_from_cells(base, revision), subject, revision,
//        failure.fetch, failure.extraction, failure.coverage, failure.result,
//        FactPersistence::NotAttempted, failure.served_from, None,
//        failure.correction_available, [SoftFallback] + extra_warnings)
```

関数シグネチャの変更:

```rust
// 旧 inner を改名し戻り値を変更（V3/旧コマンド双方の共有 core）:
async fn enrich_company_facts_from_edinet_core(
    vault: &VaultHandle,
    store: &NetworkPolicyStore,
    llm: &LlmHandle,
    temp_dir: Option<&std::path::Path>,
    request: EdinetEnrichmentRequestV3,
) -> Result<EnrichOutcome, String>;   // Err = 入力棄却層のみ

// enrich_company_facts_from_edinet:   core(..).map(|o| o.response)
// fetch_edinet_company_facts:         core(..).and_then(|o| match o.legacy_error {
//                                       Some(msg) => Err(msg),
//                                       None => Ok(o.response.facts),
//                                     })
```

- `base.is_empty()`（= company_name cell すら無い）で EDINET が失敗した場合、V3 応答の facts は空 cells 由来の空 `CompanyFacts` になる。これは **V3 §12「失敗時の空 `CompanyFacts::default()` 返却禁止」に抵触しない** — 禁止の対象は「非空 base の破棄・すり替え」であり、base が本当に空のとき、空 facts + 真実の status（`fetch=network_failed` 等）こそが「EDINET未取得」を「データなし」から区別する唯一の表現である。旧コマンドは `legacy_error` により従来どおり Err を返すので、旧 wire 上の挙動は1bitも変わらない。

### 2.3 収斂の決定 — 「1レーン1本」（A5 の回答）

**決定: V3 レーンの outer fallback は `fallback_enrichment_response` の1本、interview レーンは `soft_fallback_named_base` の1本。相互流用・重複実装を禁止する。**

- 「両方残す」の不採用条件は「同一レーンに2規律」を指す。現状の真の問題は、V3 レーンに `:846` / `:879` / `:881` / `:1026` / `:1064` という**5つのアドホック出口**が散在していることであり、Step 12 でこれを1本へ収斂する。
- `soft_fallback_named_base` の削除は不採用。理由: (1) interview レーンは `CompanyFacts` 直返しの別 wire 契約で、4つの本番呼出点とテストを持つ。(2) interview レーンを V3 core 経由に統合すると、**面接開始時に Heavy guard 取得（LLM cancel + purge）と ZIP ダウンロードが走る** — これは面接 UX とメモリ契約の両方に対する明白な退行であり、レーン分離は意図された設計である（interview レーンは discovery + 一覧 metadata のみで ZIP を踏まない）。
- ただし interview レーンにも同一規律を適用する: `:512` `:518` の `?` 脱出を `soft_fallback_named_base` 経由へ修正する（§4.3）。

---

## 3. 決定表 — 失敗事象 → 4-tuple（唯一の典拠）

**読み方:** (a) facts = 応答 `facts`/`fact_cells` の内容。(b)(c) は inner（Step 11 §3.5）が確定した値を outer が**無加工転記**。(d) は `fact_persistence` + `warnings`。`coverage` / `result` / `served_from` / `correction_available` / `freshness` も同様に inner 値を転記（表中に特記ある行のみ上書き）。**SF** = `SoftFallback` warning。base 空のときは facts=空 cells + `legacy_error=Some(message)`（旧コマンドのみ Err 化）で、(b)(c)(d) は同一。

| # | 事象 | inner message / 典拠行 | (a) facts | (b) FetchStatus | (c) ExtractionStatus | (d) FactPersistence + warnings |
|---|---|---|---|---|---|---|
| 1 | EDINETコードなし（名前もなし） | `company_name_required` :1500 | base | `not_attempted` | `none` | `not_attempted` + SF |
| 2 | 対象有報なし（窓完走） | `edinet_no_eligible_filing` :1569 | base | `succeeded` | `none` | `not_attempted` + SF（result=`no_eligible_in_window`） |
| 3 | API key なし | `edinet_api_key_missing` :1475 | base | `api_error` | `none` | `not_attempted` + SF。**Keychain phase で `not_attempted`+専用 warning へ移行（V3 §7）— 本 Step では現行値を凍結。warning enum 追加は wire 変更のため禁止** |
| 4 | Network Policy off / `egress-live` off | `edinet_not_ready` :1470 / `egress_unavailable` :1748 | base | `not_attempted` | `none` | `not_attempted` + SF |
| 5 | transport 生成失敗 | `edinet_transport_unavailable` :1477 | base | `network_failed` | `none` | `not_attempted` + SF |
| 6 | DNS / TLS / timeout（discovery・DL） | `edinet_discovery_failed` :1541 / `edinet_network_failed` :1700 | base | `network_failed` | `none` | `not_attempted` + SF |
| 7 | 400 / 401 / 404 / 429 / 500 | 同上（AuthStopped 等）:1535-1538 / `edinet_api_error` :1702 | base | `api_error` | `none` | `not_attempted` + SF |
| 8 | HTTP 200 + error JSON | pipeline 分類 → `edinet_api_error` | base | `api_error` | `none` | `not_attempted` + SF |
| 9 | maintenance HTML / InvalidContentType | 同上（API 系に分類） | base | `api_error` | `none` | `not_attempted` + SF |
| 10 | ZIP 上限超過（TooLarge、両側成果ゼロ） | `edinet_parse_failed` :1717 | base | `succeeded`（転送自体は発生） | `parse_failed` | `not_attempted` + SF |
| 11 | ZIP 不正 / CRC 失敗（両側成果ゼロ） | 同上 | base | `succeeded` | `parse_failed` | `not_attempted` + SF |
| 12 | unsupported compression / encoding（両側成果ゼロ） | 同上 | base | `succeeded` | `parse_failed` | `not_attempted` + SF |
| 13 | XML / CSV parse 失敗（両側成果ゼロ） | 同上 | base | `succeeded` | `parse_failed` | `not_attempted` + SF |
| 13b | **片側のみ**失敗（10-13 の partial） | inner は `Ok(resolved)` :1726 | **merged**（成功側を採用） | `succeeded` | `financials_only` / `narratives_only` | 通常 persist フロー（#17 系）。「片方失敗で全部捨て」禁止 |
| 14 | memory pressure（watch / chunk 検知） | `edinet_cancelled` :1685 | base | `memory_pressure` | `cancelled` | `not_attempted` + SF |
| 15 | LLM purge 未完了 / acquire 失敗（headroom・admission・Barrier/Park timeout） | acquire error :1578 | base | `memory_pressure` | `none` | `not_attempted` + SF |
| 16 | cancel / background / job deadline | `edinet_cancelled`（cause=Background/Dropped） | base | `cancelled` | `cancelled` | `not_attempted` + SF |
| 17 | Vault 保存失敗（CAS 以外の書込失敗） | 現 :1081-1094（既に正しい） | **merged**（画面用一時結果） | resolved.fetch | resolved.extraction | **`failed`** + `VaultWriteFailed`。revision は旧値のまま =「保存済みと表示しない」の実装形 |
| 17b | 保存成功後の read-back 失敗（現 :1064 `?`） | — | **merged**（`revision+1` の wires = 書き込んだ値そのもの） | resolved.fetch | resolved.extraction | **`persisted`** + `VaultReadFailed`。CAS 成功は事実なので `failed` に格下げしない |
| 17c | Conflict 応答時の read-back 失敗（現 :1026 `?`） | — | 手元の既知 wires（§4.2-3） | `not_attempted` | `none` | **`conflict`** + `VaultReadFailed`。Conflict は FE の再試行制御に必要なので保持 |
| 17d | 初回 vault 読取失敗（:794、続行済み） | — | （当該応答の facts に影響なし） | — | — | 最終応答 warnings に `VaultReadFailed` を追加するのみ |
| 18 | rekey 確定同一性の不一致（現 :881）/ 確定コードの SubjectKey 化失敗（現 :879、防御） | — | **base**（merged を採用しない — 同定不成立の subject に EDINET cells を書かない。V3 §1.2 rekey 衝突規則） | resolved.fetch | resolved.extraction | `not_attempted` + SF。**result=`identity_ambiguous` に上書き**。freshness に doc_id を載せない（acquisition を渡さない） |

**表の外の解釈は一切許可しない。** 実装中に表に無い事象へ到達した場合は、実装せず停止して報告すること。

---

## 4. ファイル別 Diff 指示

**変更許可ファイル: `apps/desktop/src-tauri/src/llm/commands_sim.rs` のみ。** `edinet_client.rs` は変更不要の見込み — diff が必要と感じたら停止して報告。`service.rs` / worker / `Cargo.toml` / lockfile / FE / migrations に diff が出た設計は即不採用。

### 4.1 出口の一本化（`:732-931` 周辺）

1. `enrich_company_facts_from_edinet_inner` を `enrich_company_facts_from_edinet_core` に改名し、戻り値を `Result<EnrichOutcome, String>` へ変更（§2.2）。既存の入力棄却層（`:739-773`）は**無変更**で Err のまま。
2. `EnrichOutcome` / `fallback_enrichment_response` を追加（`v3_response` の直前に配置）。`fallback_enrichment_response` は §2.2 の転記仕様のみ — 新しい status 判断を書いた時点で不採用。
3. **`:844-863` の match を置換:**
   ```text
   Err(error) => return Ok(EnrichOutcome {
       legacy_error: base.is_empty().then(|| error.message.clone()),
       response: fallback_enrichment_response(&base, revision, subject, &error, &extra),
   })
   ```
   `Err(error) if base.is_empty() => return Err(...)` の arm は**削除**。`extra` は :794 の `read_failed` から組む（`[VaultReadFailed]` or `[]`、§4.2-4）。
4. **成功経路の全 return を `Ok(EnrichOutcome { legacy_error: None, response: … })` へ機械的に包む**（`:804` `:915` `:930` および rekey 内の各 return）。
5. 両コマンド（`:707` `:662`）を §2.2 の写像へ変更。`fetch_edinet_company_facts` の入力棄却層 Err（`:674` `:683`）は従来どおり素通し。

### 4.2 取得成功後の `?` 排除

1. **rekey ブロック（`:877-915`）:**
   - `:878-879` の `map_err(...)?` と `:880-881` の `return Err(...)` を、決定表 #18 の応答を返す1つのヘルパ呼出へ置換:
     ```rust
     fn identity_mismatch_response(
         base: &FactCells, revision: i64, subject: SubjectKey,
         resolved: &ResolvedEdinetAcquisition,
     ) -> EdinetEnrichmentResponseV3
     // v3_response(wires_from_cells(base, revision), subject, revision,
     //   resolved.fetch, resolved.extraction, resolved.coverage,
     //   DiscoveryResult::IdentityAmbiguous, FactPersistence::NotAttempted,
     //   resolved.served_from, None, resolved.correction_available, [SoftFallback])
     ```
   - facts が **base であって merged でない**こと、acquisition（= freshness の doc_id）を渡さないことは契約（V3 §1.2「企業同定が曖昧なら何も上書きしない」）。「merged の方が情報が多い」という最適化は不採用。
2. **`current_v3_response`（`:1017-1042`）:** シグネチャに `known: Vec<FactCellWire>` を追加。`vault.fact_cells` が `Err` のとき `?` せず、`known` + persistence 据え置き + `VaultReadFailed` warning で `v3_response` を組む（決定表 #17c）。呼出3箇所の `known`:
   - `:804`（stale conflict）→ 手元の `vault_wire`（再読取自体を廃止してよい: 直前 `:794` で読了済み）
   - `:905`（rekey IdentityAmbiguous）→ `wires_from_cells(&merged, revision)`
   - `:1052`（persist Conflict）→ `wires_from_cells(merged, previous_revision)`
3. **`persistence_response` Persisted 分岐（`:1062-1079`）:** read-back `map_err(...)?`（`:1064`）を廃止。`Err` 時は `wires_from_cells(merged, previous_revision + 1)`（= CAS で書いた内容そのもの）で `persisted` + `VaultReadFailed` を返す（決定表 #17b）。
4. **`:794` read 失敗の可視化:** `read_failed` を最終応答（成功・fallback 双方）の warnings へ `VaultReadFailed` として合流させる。warning の重複は排除（`Vec` に同値を2つ入れない）。

### 4.3 interview レーンの同規律化（`resolve_company_facts_with`）

- `:512` `:518` の `ok_or_else(...)?` を廃止し、filter 構築失敗時は `return soft_fallback_named_base(injected_sanitized, "edinet_invalid_argument")` とする（所有権の都合で分岐位置の前倒しが必要なら、`injected_sanitized` の move より前に判定を移すだけの並べ替えは許可。ロジック追加は不可）。
- base が `None`／無名のときの Err はこの関数の既存契約（`soft_fallback_named_base` が Err を返す）に委ねる — 新しいエラー文字列を発明しない。
- このレーンのその他の構造（`:464-481` の早期 return、`:571-580` の terminal match）は**無変更**。

### 4.4 順序・所有の固定（コード変更なしの検証項目）

- `EdinetJobGuard` / token / watch handle を `ResolvedEdinetAcquisition` / `EdinetResolveFailure` / `EnrichOutcome` のフィールドへ**追加しない**こと（A8 の構造保証の維持。追加した時点で「fallback が gate open 前に走らない」保証が崩れる）。
- fallback 層が保持してよい大きさは `FactCells`（sanitize 済み上限内）のみ。`TempArchive` / evidence 原文 / 中間バッファを fallback のために生かす変更は禁止（`:1658` の `_evidence` 破棄は現状維持）。

---

## 5. テスト必須項目

すべて fake transport / fake vault。実ネットワーク 0 件。**各テストは「base の非空フィールドが1つも失われない」assert を必ず含む。**

### 決定表の行カバレッジ（V3 コマンド）

1. 事象 #1-#9 各1件: inner を対応する失敗に誘導し、`Ok` + base 転記 + (b)(c)(d) が表と一致。
2. #10-13: fixture 誘導（TooLarge / InvalidZip / parse 失敗）→ `succeeded` + `parse_failed` + base。
3. #13b: 片側成功 → `financials_only` / `narratives_only` + merged + persist 実行。
4. #14 / #16: cause=MemoryPressure → `memory_pressure`+`cancelled`、cause=Background → `cancelled`+`cancelled`。
5. #15: `acquire_edinet_job` Err 注入 → `memory_pressure` + base。
6. #17: `fact_cells_persist` Err → `failed` + `VaultWriteFailed` + merged + 旧 revision。
7. #17b: persist Ok / read-back Err → `persisted` + `VaultReadFailed` + `revision+1` の wires。
8. #17c: persist Conflict / read-back Err → `conflict` + `VaultReadFailed` + 既知 wires。
9. #17d: 初回読取 Err → 応答 warnings に `VaultReadFailed`（成功経路でも）。
10. #18: rekey target ≠ 確定コード → `identity_ambiguous` + facts==base（merged 非採用）+ freshness の doc_id が None。

### 境界と互換

11. base 空 + EDINET 失敗: V3 コマンド → `Ok`（空 facts + 真実 status）。**旧コマンド → `Err`（メッセージが現行文字列と完全一致**、例 `edinet_not_ready`）。
12. 入力棄却層: `schema_version != 3` / 不正 fact_cells / transition 不整合 / sanitize 失敗 → 従来どおり `Err`（回帰スナップショット）。
13. `egress-live` **OFF** ビルドで1件: inner が `egress_unavailable` → V3 `Ok` + `not_attempted` + base 維持（§2 残件トリアージの必須1件）。
14. interview レーン: 不正 edinet_code + injected base あり → base が返る（`:512` 回帰）。base なし → Err。
15. fallback 層に `?` / `unwrap` / `expect` が存在しないことの clippy / grep ガード（既存 constitutional テストの走査対象に含まれることを確認 — 含まれないなら停止して報告）。
16. temp 残留 0 assert の既存テストが green のまま（Step 11 資産の回帰）。

### feature matrix（コンパイル + テスト）

17. default / `egress-live` / Apple `pocket-brain,secure-vault` の3構成 green + `aarch64-apple-ios` cross-compile 通過。warning 0。

---

## 6. Cursor への厳格な禁止事項

**1つでも犯した実装は全体差し戻し。**

1. **排他制御に触るな。** `service.rs` / worker loop / `LlmMemoryGovernor` / `acquire_edinet_job` / `EdinetJobGuard` に 1 行の diff も出すな。outer fallback は guard を所有しない（§4.4）。
2. **inner の status 写像（Step 11 §3.5、`resolve_typed_edinet_acquisition` 内の分類）を変更・「改善」するな。** outer は転記のみ。cancel を `network_failed` に丸める等の独自マッピングは不採用。決定表 #3 の Keychain 移行も本 Step では実施しない。
3. **wire 契約を変更するな。** `EdinetEnrichmentResponseV3` のフィールド追加・削除、`status_enum!` 群への variant 追加（`EdinetWarningCode` 含む — `VaultReadFailed` は**既存**variant の初使用であり追加ではない）、serde rename、FE ファイルの diff、`consult` スキーマへの接触 — すべて不採用。
4. **`catch_unwind` を導入するな。** panic 安全は「panic させない」（clippy ゲート + JoinError→`Parse` 写像は Step 11 済み）で達成する。`catch_unwind` は `EdinetJobGuardInner::drop` の Unpark/gate 開放順序と `TempArchive` の RAII 削除を素通りさせ得るため原則禁止。例外は認めない。
5. **`CompanyFacts::default()` の新規返却で base を置換するな。base の非空フィールドを空へ戻すな**（V3 §12）。§2.2 の「base が本当に空」のケースはこの禁止の対象外だが、その判定は `base.is_empty()`（cells ゼロ件）**のみ**を根拠とし、「ほぼ空だから空扱い」の類推を書くな。
6. **エラー文字列を発明・改変するな。** 旧コマンドの `legacy_error` は inner の `message` をそのまま運ぶ。新しい message 定数の追加は入力棄却層含め禁止（テスト 11-12 が検出する）。
7. **ログ・応答に API key / 完全 URL / 原文本文を出すな。** `map_vault_err` / `map_edinet_err` の中身を詳細化するな。
8. **fallback のためにメモリを持つな。** evidence 原文・`TempArchive`・中間バッファを `EnrichOutcome` や static へ退避する設計は Jetsam 契約違反で不採用。
9. **interview レーンを V3 core 経由へ「統合」するな**（§2.3 の決定。面接開始時に Heavy 排他と ZIP が走る退行になる）。今回の interview レーン変更は `:512` `:518` の2箇所のみ。
10. **最初の応答で実装するな。** 指示書 §0 のとおり監査要約・変更ファイル・シグネチャ案・フロー・テスト計画の5点を提示して停止し、「設計レビュー待ちです。承認されるまで実装・依存追加・lockfile更新を行いません。」で締めろ。本書に無いファイルへ diff が必要になったら、書かずに停止して報告せよ。
11. **虚偽の完了報告禁止。** テスト未実行・feature matrix 未確認・iOS cross-compile 未実施は「未完了」と明記して報告すること。

---

*本書は V3 契約の §Step 12 局所への写像である。決定表と V3 本文に矛盾を発見した実装AIは、実装せず停止・報告すること。*
