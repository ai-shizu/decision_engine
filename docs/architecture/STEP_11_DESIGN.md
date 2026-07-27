# STEP 11 設計書 — iOS Memory Coordinator への EDINET パイプライン統合

- **作成日:** 2026-07-27
- **上位文書:**
  - `docs/architecture/EDINET_LANE_DESIGN_V3.md`（契約正本。本書と競合したら V3 優先）
  - `docs/EDINET_LANE_IMPLEMENTATION_DIRECTIVE.md` §Step 11
  - `docs/architecture/EDINET_LANE_UNFREEZE_REPORT.md` §6
- **対象読者:** 実装担当AI（Cursor）。本書は **Diff指示書** である。ここに書かれていない構造変更は一切行ってはならない。
- **状態:** 設計承認待ち → 承認後に実装開始

---

## 0. 実装AIへの最重要通達 — 現状監査の結果を先に読め

**Step 11 の排他制御コアは既に実装済みである。再実装するな。**

Step ≤10 の実装により、以下は `apps/desktop/src-tauri/src/llm/service.rs` に**存在し、テスト済み**である。監査で確認済みの事実であり、疑って作り直してはならない。

| 契約項目 | 実装済みシンボル | 場所 |
|---|---|---|
| Admission bitset（BG/MEM/THERMAL） | `ADMISSION_BACKGROUND/MEMORY_PRESSURE/THERMAL_PRESSURE` + `LlmMemoryGovernor::admission_bits: AtomicU32` / `set_admission_bit` / `set_background_restricted` | `service.rs:85-87, 188, 241-259` |
| Heavy gate（LLM heavy job の一時拒否） | `LlmMemoryGovernor::edinet_gate_closed: AtomicBool` + `LlmHandle::ensure_llm_heavy_admitted` / `enqueue_heavy` | `service.rs:189, 689-710` |
| 排他取得シーケンス（cancel → Barrier ACK → purge → Park ACK → admission/headroom 再確認） | `LlmHandle::acquire_edinet_job` / `acquire_edinet_job_with_headroom` | `service.rs:712-789` |
| purge 完了の期限付き確認 | worker の `LlmCommand::Park` ハンドラが `commit_pending_purge` 後に purge 未コミットなら `Err("llm purge was not committed")` を返し、`park_ack.recv_timeout(EDINET_COORDINATOR_TIMEOUT=30s)` で待つ。**これが「`is_loaded() == false` を期限付き確認」の実装形である** — Park ACK 成功 = モデル非常駐が worker スレッド上で確定 | `service.rs:1098-1109, 765-776` |
| 古い遅延 Park の拒否（epoch） | `epoch <= parked_epoch` で拒否、`edinet_acquire_epoch` / `edinet_parked_epoch` | `service.rs:1102, 190-191` |
| Job guard（Arc / 最後の Drop で Unpark + gate open + phase Idle） | `EdinetJobGuard(Arc<EdinetJobGuardInner>)` + `Drop for EdinetJobGuardInner` | `service.rs:595-625` |
| headroom 定数 256MiB / 128MiB | `EDINET_MIN_START_HEADROOM_BYTES` / `EDINET_CANCEL_HEADROOM_BYTES` | `service.rs:82-83` |
| `os_proc_available_memory` FFI（Apple 限定・非キャッシュ） | `monitor::probe::os_proc_available_memory_bytes()`（`#[cfg(target_vendor = "apple")]`、他は `None`） | `monitor/probe.rs:36-49` |
| `MemPhase::EdinetFetch` / `EdinetExtract` | `monitor/mod.rs` の enum + `as_u8/from_u8` 双方向対応済み | `monitor/mod.rs:29-63` |
| ZIP stream-to-temp（上限・deadline・cancel・magic 検証・単一飛行） | `edinet_archive::download_edinet_archive_to_temp` + `ArchiveGate` / `ArchivePermit` / `TempArchive`（drop で削除） | `edinet_archive.rs:370-` |
| 財務抽出（cancel を entry/read/row 毎に確認） | `edinet_csv::extract_financials_from_type5_archive` | `edinet_csv.rs:253` |
| ナラティブ抽出（cancel を event 毎に確認） | `edinet_xbrl::extract_narratives_from_type1_archive` | `edinet_xbrl.rs:74` |

**真のギャップ（= Step 11 の全作業）は次の3点だけである:**

1. **`EdinetCancelContext` の不在。** 抽出器・downloader は `tokio_util::sync::CancellationToken` を受け取るが、本番でこの token を生成し、admission bits / headroom 崩壊 / orchestrator の drop と**接続する橋が存在しない**。現状 `EdinetJobGuard::is_cancelled()` は admission bits をポーリングするだけで、`spawn_blocking` 内で回る解析ループを止める手段がない。
2. **パイプライン未接続。** `download_edinet_archive_to_temp` / `extract_financials_from_type5_archive` / `extract_narratives_from_type1_archive` には**本番呼出箇所が1つもない**（各モジュールのテストのみ）。`commands_sim.rs::resolve_typed_edinet_acquisition` は guard 取得後、`acquisition_from_document_meta_with_body(&selected, filing_text)`（一覧metadata＋注入テキストのみ）を呼んで終わっている。`commands_sim.rs:1206-1207` の `set_phase(EdinetFetch); set_phase(EdinetExtract);` 連続呼びは placeholder である。
3. **抽出結果 → `EdinetFactAcquisition` の変換関数の不在。** `PartialEdinetFacts`（財務＋ナラティブ）から Step 10 のマージ入力である `EdinetFactAcquisition` を組み立てる関数がない。

---

## 1. Step 11 の目標とスコープ

### 1.1 目標

選択済み有報（`YuhoSelection`）に対し、**既存の Heavy 排他 guard の内側で** `type=5` → `type=1` の ZIP 取得・抽出を完全直列に実行し、結果を Step 10 の provenance マージへ渡す。全経路で cancel / memory pressure / background が **HTTPチャンク・ZIP entry・XML event・CSV row の粒度で**効くこと。

### 1.2 追加・変更するコンポーネントの責務

| コンポーネント | 状態 | 責務 |
|---|---|---|
| `EdinetCancelCause` + guard 内 `CancellationToken`（`service.rs`） | **追加** | admission bits / headroom floor / drop と token の橋。cancel 理由の1回書き記録 |
| `spawn_edinet_cancel_watch`（`service.rs`） | **追加** | job 存命中のみ admission bits と headroom を低頻度ポーリングし token を cancel する監視 task（**`Weak` 保持**） |
| `run_edinet_zip_pipeline`（`commands_sim.rs`） | **追加** | 完全直列オーケストレーション: type=5 DL → extract → drop → type=1 DL → extract → drop。**ZIP/XML/CSV の詳細は一切書かない** |
| `extract_on_blocking`（`commands_sim.rs`） | **追加** | `spawn_blocking` 境界。guard clone と `TempArchive` の所有権を blocking closure へ移し、archive を closure 内で drop する |
| `acquisition_from_selected_with_partials`（`edinet_client.rs`） | **追加** | `PartialEdinetFacts` ×2 → `EdinetFactAcquisition` + 証拠 + 警告。既存の検証・上限を再利用 |
| `resolve_typed_edinet_acquisition`（`commands_sim.rs`） | **変更** | placeholder 部をパイプライン呼出しへ置換。`filing_text: Some` の注入経路は温存 |
| `enrich_company_facts_from_edinet` / `fetch_edinet_company_facts`（`commands_sim.rs`） | **変更** | Tauri `AppHandle` から app cache dir を解決し `temp_dir` として注入 |
| `EdinetJobGuard`（`service.rs`） | **変更** | token accessor / cause accessor の追加。`Drop` に token cancel を追加 |

### 1.3 スコープ外（やってはならない）

- `heavy_coordinator.rs` の新設・coordinator ロジックの `service.rs` からの移設（V3 §9 のファイル表にあるが、実装は既に `service.rs` に収まって全テストを通過している。**移設は純粋な churn であり Step 11 では禁止**。将来の refactor milestone へ委ねる）
- Evidence の Vault 永続化・pending claim・埋め込み lease（V3 実装順序 8。`EvidencePersistence::NotAttempted` のまま維持。§4.6 に interface point のみ規定）
- Keychain（V3 §7。`subscription_key_from_env` を維持）
- worker loop（`Barrier`/`Park`/`Unpark`/`commit_pending_purge`）の変更
- FE（TypeScript）の変更。IPC 形状は不変（`AppHandle` は Tauri がサーバ側で注入するため wire 契約に影響しない）
- 依存追加。**`Cargo.toml` は一切触らない**（`tokio-util` は導入済み）

---

## 2. 型定義と関数シグネチャ

### 2.1 `service.rs` — cancel 橋

```rust
/// EDINET job が cancel された理由。watcher / drop / 直接 cancel が
/// 最初の1回だけ書き込む（AtomicU8, compare_exchange で first-writer-wins）。
/// 応答ステータスへの写像は §3.5 の表に従う。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdinetCancelCause {
    None,            // = 0 未 cancel
    Background,      // = 1 ADMISSION_BACKGROUND による
    MemoryPressure,  // = 2 ADMISSION_MEMORY_PRESSURE または headroom floor 割れ
    ThermalPressure, // = 3 ADMISSION_THERMAL_PRESSURE による
    Dropped,         // = 4 orchestrator future の drop（コマンド中断/タイムアウト）
}
```

`EdinetJobGuardInner` へのフィールド追加（構造体定義 `service.rs:595` 付近）:

```rust
struct EdinetJobGuardInner {
    tx: Arc<Mutex<mpsc::SyncSender<LlmCommand>>>,
    governor: Arc<LlmMemoryGovernor>,
    epoch: u64,
    monitor: Arc<MemoryMonitor>,
    cancel: tokio_util::sync::CancellationToken, // 追加
    cancel_cause: AtomicU8,                      // 追加（EdinetCancelCause）
}
```

`EdinetJobGuard` の公開 API（既存 `is_cancelled` / `set_phase` は維持しつつ拡張）:

```rust
impl EdinetJobGuard {
    /// download / spawn_blocking へ渡す協調 cancel token（clone）。
    pub fn cancel_token(&self) -> tokio_util::sync::CancellationToken;

    /// 理由を記録して cancel。既に cancel 済みなら理由は上書きしない。
    pub fn cancel_with_cause(&self, cause: EdinetCancelCause);

    /// 最初に記録された理由（未 cancel なら None バリアント）。
    pub fn cancellation_cause(&self) -> EdinetCancelCause;

    /// 変更: admission bits に加えて token も見る。
    /// admission_bits() != 0 || token.is_cancelled()
    pub fn is_cancelled(&self) -> bool;

    pub fn set_phase(&self, phase: MemPhase);
}
```

`Drop for EdinetJobGuardInner` の変更 — 冒頭に token cancel を1行追加（順序厳守）:

```text
1. self.cancel.cancel()          // 追加。遅延 blocking loop を確実に脱出させる
2. Unpark { epoch } try_send     // 既存
3. edinet_gate_closed = false    // 既存
4. monitor.set_phase(Idle)       // 既存
```

watcher（`service.rs` 内の自由関数。guard 内部へアクセスするため同ファイルに置く）:

```rust
/// admission bits / headroom floor を低頻度ポーリングし、成立時に
/// cancel token を立てる監視 task。**`Weak<EdinetJobGuardInner>` を保持**
/// し、job の最終 guard drop と同時に自然終了する（guard を strong 保持
/// すると gate が永遠に閉じたままになる — 絶対に strong で持つな）。
///
/// ループ本体（250ms tick, tokio::time::sleep）:
///   1. weak.upgrade() 失敗 → return（job 終了済み）
///   2. token.is_cancelled() → return（他所で cancel 済み）
///   3. bits = governor.admission_bits(); bits != 0 →
///        cause = BACKGROUND bit なら Background、
///                MEMORY bit なら MemoryPressure、THERMAL bit なら ThermalPressure
///        （複数 bit 同時は MemoryPressure > Thermal > Background の優先で1つ）
///        cancel_with_cause → return
///   4. os_proc_available_memory_bytes() が Some(v) かつ
///      v < EDINET_CANCEL_HEADROOM_BYTES →
///        cancel_with_cause(MemoryPressure) → return
///      （None = 非Apple / 取得不能は「floor 判定しない」— cancel しない）
pub fn spawn_edinet_cancel_watch(
    guard: &EdinetJobGuard,
) -> tauri::async_runtime::JoinHandle<()>;
```

> **設計根拠（変更禁止）:** V3 §3.3 は「ObjC コールバックは lock-free store のみ」と規定する。したがって iOS callback から `CancellationToken::cancel()` を直接呼ぶ設計は**不可**（token の cancel は waker 起床を伴い lock-free 保証がない）。callback → AtomicU32（既存）→ watcher が token 化、という2段が契約適合の唯一解である。250ms は「chunk/row/event 粒度の同期チェック（即応）」＋「非同期側の水門（最悪250ms遅延）」の妥協点であり、監視自体のメモリ・CPU コストは無視できる。

### 2.2 `commands_sim.rs` — パイプライン（`#[cfg(feature = "egress-live")]` ブロック内）

```rust
/// ZIP 経路の per-download / per-parse / job 全体の deadline。
/// download 120s は edinet_archive::DEFAULT_ARCHIVE_DOWNLOAD_DEADLINE として
/// Step 5 で既に承認・実装済みの値（指示書の暫定 30s はこの承認で更新済み）。
/// job = 2 download + 2 parse + マージ余裕。iOS 実機受入で調整する。
const EDINET_PARSE_DEADLINE: Duration = Duration::from_secs(20);
const EDINET_ZIP_JOB_DEADLINE: Duration = Duration::from_secs(300);

/// パイプラインの soft 結果。Err を返さない（fallback は呼出側の既存機構）。
struct EdinetZipOutcome {
    financials: Option<PartialEdinetFacts>, // type=5 成果（csvFlag!=1 なら None）
    narratives: Option<PartialEdinetFacts>, // type=1 成果
    fetch: FetchStatus,                     // 終端分類（§3.5 の表で決定）
    extraction_failed: bool,                // 片側でも parse 失敗があったか
    warnings: Vec<EdinetWarning>,
}

/// 完全直列: [csvFlag=="1"] type=5 DL→extract→drop → type=1 DL→extract→drop。
/// TempArchive を2つ同時に生かさない（ArchiveGate が単一飛行を強制するため、
/// 直列を破ると2本目の DL が TempArchiveBusy で即死する — これは安全側の設計）。
async fn run_edinet_zip_pipeline<T: crate::knowledge::net_gateway::HttpTransport>(
    transport: &T,
    selected: &crate::knowledge::edinet_client::YuhoSelection,
    subscription_key: &str,
    temp_dir: &std::path::Path,
    guard: &crate::llm::service::EdinetJobGuard,
) -> EdinetZipOutcome;

/// spawn_blocking 境界。guard clone / token clone / TempArchive 所有権を
/// closure へ move。archive は closure 内で extract 後に必ず drop される
/// （temp 削除の fs 操作を async スレッドに載せない）。
/// JoinError（= closure panic は禁止済みだが防御）→ Err(EdinetError::Parse)。
async fn extract_on_blocking<Fx>(
    guard: &crate::llm::service::EdinetJobGuard,
    archive: crate::knowledge::edinet_archive::TempArchive,
    extract: Fx,
) -> Result<crate::knowledge::edinet_csv::PartialEdinetFacts, EdinetError>
where
    Fx: FnOnce(
            &mut crate::knowledge::edinet_archive::TempArchive,
            &tokio_util::sync::CancellationToken,
        ) -> Result<crate::knowledge::edinet_csv::PartialEdinetFacts, EdinetError>
        + Send
        + 'static;
```

### 2.3 `edinet_client.rs` — 抽出結果 → 取得結果の変換

```rust
/// PartialEdinetFacts（財務/ナラティブ）から Step 10 マージ入力を組み立てる。
/// - facts のフィールド上限・sanitize・doc_id/edinet_code 整合検証は
///   既存の merge_company_facts / EdinetFactAcquisition::new の規律を再利用
/// - EdinetFieldAcquisition は実際に値が立ったフィールドのみ true
/// - 返す evidence は build_evidence_sections 済みのもの（呼出側は Step 11
///   では永続化しない — §4.6）
/// - 両 partial が None/空でも Err にしない（fields 全 false の acquisition）。
///   ただし metadata 検証失敗（is_eligible_yuho_original 相当）は従来通り Err
pub fn acquisition_from_selected_with_partials(
    selected: &YuhoSelection,
    financials: Option<&PartialEdinetFacts>,
    narratives: Option<&PartialEdinetFacts>,
) -> Result<
    (
        EdinetFactAcquisition,
        Vec<EdinetEvidenceSection>,
        Vec<EdinetWarning>,
    ),
    EdinetError,
>;
```

### 2.4 コマンド署名の変更（wire 契約は不変）

```rust
#[tauri::command]
pub async fn enrich_company_facts_from_edinet(
    app: tauri::AppHandle,            // 追加（Tauri が注入。FE 変更不要）
    vault: State<'_, VaultHandle>,
    store: State<'_, NetworkPolicyStore>,
    llm: State<'_, LlmHandle>,
    request: EdinetEnrichmentRequestV3,
) -> Result<EdinetEnrichmentResponseV3, String>;

// fetch_edinet_company_facts（互換コマンド）も同様に app: AppHandle を追加。

// inner 関数はテスト可能性維持のため PathBuf の Option を受ける:
//   temp_dir: Option<std::path::PathBuf>
// None（unit test / 非egress）→ ZIP 経路は fetch=NotAttempted 相当の soft-fail。
// コマンド wrapper では app.path().app_cache_dir() を解決し、
// .join("edinet-tmp") を Some で渡す。cache dir 解決失敗は
// resolve_failure("edinet_temp_dir_unavailable", FetchStatus::NotAttempted) 系の
// soft-fail（panic/unwrap 禁止）。ディレクトリは std::fs::create_dir_all を
// spawn_blocking 外で1回だけ（数μsの単発 mkdir は blocking 隔離不要）。
```

---

## 3. 状態遷移とデータフロー

### 3.1 ライフサイクル全体（正常系）

```text
[light] discovery（Heavy lease 不要 — V3 §5。既存実装のまま）
   │  selected: YuhoSelection 確定, may_transition_to_zip == true
   ▼
acquire_edinet_job()                                  ← 既存。変更点は token 生成のみ
   ├─ admission_bits != 0            → Err（開始拒否 → base fallback）
   ├─ os_proc_available < 256MiB     → Err（同上）
   ├─ gate CAS closed                → Err "already active"（concurrency = 1）
   ├─ request_cancel → Barrier ACK   （実行中 generation の cancel + queue 排出確認）
   ├─ request_purge  → Park(epoch) ACK（worker が purge commit を確認できなければ Err
   │                                   = 「purge 未確認なら開始しない」の実装点）
   ├─ admission / headroom(128MiB) 再確認 → 崩れていれば Err（Unpark + gate open して返る）
   ▼
EdinetJobGuard 誕生（token: 非cancel, cause: None, gate: closed）
   │   ここで orchestrator は直ちに:
   │     let token = guard.cancel_token();
   │     let _cancel_on_drop = token.clone().drop_guard();   // ← future drop 保険
   │     let _watch = spawn_edinet_cancel_watch(&guard);     // ← Weak 監視開始
   ▼
tokio::time::timeout(EDINET_ZIP_JOB_DEADLINE, run_edinet_zip_pipeline(..)) 内で:
   │
   ├─ if csv_flag == "1":
   │    set_phase(EdinetFetch)
   │    download(type=5) ── chunk毎: token/deadline/memory_pressure ──→ TempArchive₅
   │    set_phase(EdinetExtract)
   │    extract_on_blocking(guard, TempArchive₅, extract_financials…)
   │       └─ blocking thread: guard clone 保持。entry/read/row 毎に token 確認
   │       └─ extract 完了 → **closure 内で TempArchive₅ drop（temp 削除 + permit 解放）**
   │
   ├─ set_phase(EdinetFetch)
   │    download(type=1) ──→ TempArchive₁      ← permit 解放済みなので取得できる
   │    set_phase(EdinetExtract)
   │    extract_on_blocking(guard, TempArchive₁, extract_narratives…)
   │       └─ 同上。closure 内で drop
   ▼
acquisition_from_selected_with_partials(selected, fin, narr)
   │   （scratch/decoder/inflater は各 extractor 内で drop 済み。ここに残るのは
   │     上限適用済みの PartialEdinetFacts 小型データのみ）
   ▼
Step 10 既存マージ（cells_from_edinet_acquisition → merge_fact_cells → CAS persist）
   ▼
guard / watcher / _cancel_on_drop がスコープ終端で drop
   └─ 最後の EdinetJobGuardInner::drop:
        token.cancel() → Unpark(epoch) → gate open → set_phase(Idle)
   └─ LLM は再ロードしない（enqueue_heavy が gate open 後に受付再開 = lazy load）
```

### 3.2 排他の2層構造（デッドロック不能性の論証）

```text
外層: Heavy gate（edinet_gate_closed）… LLM heavy jobs ⟂ EDINET job
内層: ArchiveGate（busy）           … TempArchive 単一飛行（type=5 ⟂ type=1）
```

- **取得順序は常に 外層 → 内層。** 内層保持中に外層を触る経路は存在しない（blocking closure は LlmHandle を持たない — §5 禁止事項で固定）。順序が全域で一方向なので circular wait は構造的に不可能。
- 内層は `try_acquire`（非ブロッキング）。待ち合わせ自体が存在しないため、внутренний permit 起因の待機停止はない。二重取得は `TempArchiveBusy` で即 Err → soft-fail。
- worker スレッドとの待ち合わせ（Barrier/Park ACK）はすべて `recv_timeout(30s)` で期限付き。無限待ちはない。
- `spawn_blocking` の JoinHandle を待つ orchestrator future が drop されても: `_cancel_on_drop` が token を立て、blocking loop は次のチェック点（≤1 row/event/entry）で `Cancelled` を返して脱出し、closure 内 guard clone の drop で gate が開く。**guard の解放は必ず blocking 完了後**であり、「abort したが blocking が走り続けて LLM と並走する」事故は構造的に起きない。

### 3.3 cancel 伝播マトリクス

| 事象 | 検知者 | token化 | HTTP chunk | ZIP entry | XML event | CSV row |
|---|---|---|---|---|---|---|
| iOS memory warning / Critical | callback → `ADMISSION_MEMORY_PRESSURE`（既存, lock-free） | watcher ≤250ms | `memory_pressure()` closure で**即** | token | token | token |
| background 遷移 | callback → `ADMISSION_BACKGROUND`（既存） | watcher ≤250ms | token | token | token | token |
| Serious（thermal） | `set_degradation` → `ADMISSION_THERMAL_PRESSURE`（既存） | watcher ≤250ms | token | token | token | token |
| headroom < 128MiB | watcher（`os_proc_available_memory_bytes` 都度取得・非キャッシュ） | ≤250ms | `memory_pressure()` closure で**即** | token | token | token |
| コマンド future drop / job deadline | `DropGuard` | 即 | token | token | token | token |
| download/parse deadline | 各関数内 `deadline_at`（既存実装） | — | 既存 | 既存 | 既存 | 既存 |

`download_edinet_archive_to_temp` の `memory_pressure: F` には次の closure を渡す:

```text
|| guard.is_cancelled()
   || os_proc_available_memory_bytes()
        .is_some_and(|v| v < EDINET_CANCEL_HEADROOM_BYTES)
```

（watcher の250msを待たず chunk 粒度で floor を検知する経路。値のキャッシュ禁止 — 毎回呼ぶ。）

### 3.4 失敗パスの網羅（各段階 → 挙動）

| 段階 | 失敗 | 挙動 |
|---|---|---|
| acquire | admission / headroom / gate busy / Barrier・Park timeout / purge 未コミット | `Err` → 既存 `resolve_failure(…, FetchStatus::MemoryPressure)` → **base fallback**。EDINET は開始しない |
| type=5 DL | `Cancelled` / `MemoryPressure` | **job 全体を中断**（type=1 へ進まない）。§3.5 で status 化 |
| type=5 DL | その他（TooLarge / ApiResponse / Network / InvalidZip …） | warning に記録し **type=1 へ続行**（財務 fallback は XBRL 側にあるため。V3 §0-3） |
| type=5 extract | `Cancelled` / `MemoryPressure` | job 中断 |
| type=5 extract | parse系 | `extraction_failed = true`、type=1 へ続行 |
| type=1 DL/extract | `Cancelled` / `MemoryPressure` | job 中断（**取得済み financials は捨てない** — partial として §3.5 の分類へ） |
| type=1 DL/extract | その他 | narratives = None、warning 記録 |
| 変換 | `acquisition_from_selected_with_partials` Err | `resolve_failure` 既存経路（extraction = ParseFailed）→ base fallback |
| JoinError（blocking panic 防御） | — | `EdinetError::Parse` として当該側 soft-fail。**re-panic しない** |
| job deadline (300s) | `timeout` Elapsed | `Dropped` cause で token 済み（DropGuard）。`FetchStatus::Cancelled` 系へ |

**大原則（V3 §12 再掲）:** 上記のどの行でも、Wikipedia/manual base は関数冒頭で確保済みのものが必ず返る。`CompanyFacts::default()` の新規返却・base の空値化は全経路で禁止。

### 3.5 status 写像（決定表 — この表以外の解釈禁止）

`FetchStatus` の決定（優先順に評価）:

| 条件 | FetchStatus |
|---|---|
| job 中断 かつ `cancellation_cause() == MemoryPressure or ThermalPressure` | `MemoryPressure` |
| job 中断 かつ cause == `Background` or `Dropped` | `Cancelled` |
| 全 DL が network 系失敗のみで成果ゼロ | `NetworkFailed` |
| 全 DL が API error のみで成果ゼロ | `ApiError` |
| それ以外（少なくとも一方の DL が完了） | `Succeeded` |

`ExtractionStatus` は既存 `extraction_from_acquisition`（fields ベース: Both / FinancialsOnly / NarrativesOnly / None）を維持し、次の2上書きのみ追加:

| 条件 | ExtractionStatus |
|---|---|
| job 中断（cancel/memory）で fields 全 false | `Cancelled` |
| `extraction_failed == true` かつ fields 全 false | `ParseFailed` |

（片側成功時は partial 成功をそのまま `FinancialsOnly` / `NarrativesOnly` で返す — 部分成功は正式な状態。）

### 3.6 MemPhase 遷移（placeholder の置換後）

```text
Idle ──acquire──▶ (guardあり)
  EdinetFetch    : 各 download の直前に set
  EdinetExtract  : 各 extract_on_blocking の直前に set
  （type=5→type=1 で Fetch/Extract を2往復する — 1往復目の phase を使い回さない）
Idle             : 最後の guard drop（既存 Drop 実装）
```

---

## 4. 実装担当AI（Cursor）への具体的な Diff 指示

> 各項目は「ファイル → 場所 → 構造変更」の形式。**関数の中身のアルゴリズムを発明する余地はない** — 中身は §2 のシグネチャ、§3 の決定表・遷移図の機械的な写像である。

### 4.1 `apps/desktop/src-tauri/src/llm/service.rs`

1. **use 追加**（ファイル先頭の import 群）: `tokio_util::sync::CancellationToken`。`AtomicU8` は既に import 済みか確認し、無ければ追加。
2. **`EdinetCancelCause` enum を追加**（`EdinetJobGuardInner` 定義の直前）。`as_u8`/`from_u8` は `MemPhase` と同じ private パターンで実装。
3. **`EdinetJobGuardInner`（595行付近）** に `cancel: CancellationToken` と `cancel_cause: AtomicU8` を追加。
4. **`acquire_edinet_job_with_headroom` の成功終端（783行付近）**: `EdinetJobGuardInner` 構築に `cancel: CancellationToken::new()`, `cancel_cause: AtomicU8::new(0)` を追加。**取得シーケンス自体（Barrier/Park/再確認の順序・fail クロージャ）は1文字も変えない。**
5. **`Drop for EdinetJobGuardInner`（602行付近）**: 既存3操作の**前**に `self.cancel.cancel();` を1行追加。
6. **`impl EdinetJobGuard`（617行付近)**: §2.1 の4メソッド（`cancel_token` / `cancel_with_cause` / `cancellation_cause` / `is_cancelled` 拡張）を追加・変更。`cancel_with_cause` は `compare_exchange(0, cause)` で first-writer-wins → その後 `cancel()`。
7. **`spawn_edinet_cancel_watch` を追加**（`EdinetJobGuard` impl の直後）。§2.1 の疑似コード通り。`Weak` の取得は同ファイル内なので `Arc::downgrade(&guard.0)` で直接行う。ループの sleep は `tokio::time::sleep(Duration::from_millis(250))`。
8. **テスト追加**（既存 `#[cfg(test)] mod tests`）: §4.5 参照。

### 4.2 `apps/desktop/src-tauri/src/llm/commands_sim.rs`

1. **定数追加**（`EDINET_FETCH_DEADLINE`（54行）の直後）: `EDINET_PARSE_DEADLINE`, `EDINET_ZIP_JOB_DEADLINE`（§2.2 の値とコメント）。
2. **`extract_on_blocking` / `run_edinet_zip_pipeline` / `EdinetZipOutcome` を追加**（`resolve_typed_edinet_acquisition` の直前、`#[cfg(feature = "egress-live")]`）。
   - `extract_on_blocking`: `tauri::async_runtime::spawn_blocking`（本ファイル既存の慣用に一致させる。`tokio::task::spawn_blocking` を直接使わない）。closure へ move するのは `guard.clone()`, `guard.cancel_token()`, `archive`（値）, `extract`。closure 内: `let result = extract(&mut archive, &token); drop(archive); drop(guard_clone); result`。await 側: `JoinError → Err(EdinetError::Parse)`（`.unwrap()` 禁止）。
   - `run_edinet_zip_pipeline`: §3.1 の直列フローと §3.4 の分岐表の写像。download は `edinet_archive::download_edinet_archive_to_temp(transport, doc_id, kind, key, temp_dir, &production_archive_gate(), &guard.cancel_token(), DEFAULT_ARCHIVE_DOWNLOAD_DEADLINE, /*max_compressed=*/既存 PROD 上限, memory_pressure_closure)`。extract は既存 `extract_financials_from_type5_archive` / `extract_narratives_from_type1_archive` を `FinancialExtractMeta` / `NarrativeExtractMeta`（`selected.original` の doc_id/edinet_code/submit_date_time/period_start/period_end から構築）とともに渡す。**ZIP/XML/CSV のロジックをこのファイルに書いた時点で不採用**（指示書 §2）。
3. **`resolve_typed_edinet_acquisition`（1198-1237行）の置換**:
   - `let guard = llm.acquire_edinet_job()…`（1198行）は維持。
   - **1206-1214行の placeholder（`set_phase` 連続2呼び + `is_cancelled` 早期 return + `let _guard = guard;`）を削除**し、次へ置換:
     1. `let _cancel_on_drop = guard.cancel_token().drop_guard();`
     2. `let _watch = crate::llm::service::spawn_edinet_cancel_watch(&guard);`
     3. `filing_text` が `Some` → 従来どおり `acquisition_from_document_meta_with_body(&selected_meta, filing_text)`（**注入テスト経路。既存テストの互換を壊さない**）。
     4. `filing_text` が `None` かつ `temp_dir` が `Some` → `tokio::time::timeout(EDINET_ZIP_JOB_DEADLINE, run_edinet_zip_pipeline(…)).await` → `EdinetZipOutcome` → §3.5 の決定表で status 化 → 成功側は `acquisition_from_selected_with_partials`。
     5. `temp_dir` が `None` → `resolve_failure("edinet_zip_unavailable", FetchStatus::NotAttempted)`。
   - `EdinetResolveFailure` 構築時、`fetch`/`extraction` は §3.5 の表のみを典拠とする。
4. **`temp_dir` の貫通**: `resolve_typed_edinet_acquisition` と `enrich_company_facts_from_edinet_inner`（および `fetch_edinet_company_facts` の inner 経路）に `temp_dir: Option<&std::path::Path>` パラメータを追加。両 `#[tauri::command]` に `app: tauri::AppHandle` を追加し、`app.path().app_cache_dir()` → `join("edinet-tmp")` → `create_dir_all`（失敗は soft-fail、§2.4）→ `Some(path)` を inner へ。既存 unit tests は `None` を渡して現挙動維持。
5. **`YuhoSelection` の受け渡し**: 現在 1215 行で `selected`（`EdinetDocumentMeta`）から `acquisition_from_document_meta_with_body` を呼んでいる。discovery outcome は `correction_available` を持つので、ZIP 経路では `YuhoSelection { original: selected, correction_available: outcome由来 }` を構築して §2.3 の新関数へ渡す（既存 `map_tristate` の入力と二重管理にならないよう、bool 化は discovery outcome の値を単純写像）。

### 4.3 `apps/desktop/src-tauri/src/knowledge/edinet_client.rs`

1. **`acquisition_from_selected_with_partials` を追加**（`acquisition_from_selected_with_body`（1268行付近）の直後）。実装は既存部品の合成のみ:
   - facts 本体: `selected.original` から従来どおり基礎 facts を構築（`merge_company_facts` の分解・再利用。**新しい sanitize / 上限ロジックを発明しない**）。
   - `financials` → `performance_summary` 文字列化は `PartialEdinetFacts.financials` の `ExtractedFinancialFact`（concept/period/unit/値が一体）を既存フィールド上限内で決定的に整形。曖昧値は入れない（extractor 側で既に一意化済み — 再選別するな）。
   - `narratives` → `business_summary` / `business_risks` は `NarrativeConcept` の対応で写像。
   - evidence は入力 `PartialEdinetFacts.evidence` の連結を**そのまま**返す（再 sanitize しない — `build_evidence_sections` が実施済み）。
   - `EdinetFieldAcquisition` は「実際に非空で facts に載ったフィールドのみ true」。
2. 既存 `acquisition_from_document_meta_with_body` は**無変更**（注入テスト経路が使用）。

### 4.4 `apps/desktop/src-tauri/src/lib.rs` / FE / Cargo.toml

- **変更なし**。`AppHandle` は Tauri のコマンドマクロが注入するため handler 登録・TS 型・invoke 呼出しに影響しない。確認のみ行うこと。

### 4.5 テスト必須項目（Step 11 追加分）

`service.rs`（fake worker / fake headroom の既存テスト基盤を使う。実ネットワーク0件）:

1. `cancel_token` は acquire 直後に非 cancel、guard 最終 drop 後に cancelled。
2. `cancel_with_cause` の first-writer-wins（2回目の cause が上書きしない）。
3. watcher: admission bit set → ≤1s で token cancelled + cause 一致（bit 別に3ケース）。
4. watcher: guard 全 drop → watcher が自然終了し、**gate が開いている**（Weak 保持の検証。strong 保持へ退行したらこのテストが hang/fail する）。
5. headroom floor 割れ（fake headroom fn）→ MemoryPressure cause。
6. `is_cancelled` が token 単独 cancel でも true。

`commands_sim.rs`（fake transport + fixture ZIP。既存 fixture を再利用）:

7. csvFlag=1: type=5→type=1 が**直列**に呼ばれる（transport 呼出記録で順序検証）、両成功 → `Both` + facts 反映。
8. csvFlag!=1: type=5 を呼ばない、narratives のみ → `NarrativesOnly`。
9. type=5 が TooLarge → warning + type=1 続行 → `NarrativesOnly`。
10. type=1 network 失敗 + type=5 成功 → `FinancialsOnly` + `FetchStatus::Succeeded`。
11. extract 中に token cancel（fixture 側 hook または事前 cancel）→ `ExtractionStatus::Cancelled` + cause 写像（Background→`Cancelled` / Memory→`MemoryPressure`）+ **base 完全維持**。
12. 全経路で temp ファイル残留 0（temp_dir を走査して assert）。
13. `filing_text: Some` の注入経路が従来スナップショットと一致（回帰）。
14. `temp_dir: None` → `edinet_zip_unavailable` soft-fail + base 維持。
15. 憲法テスト維持: `reqwest::` が `net_gateway.rs` 以外に出現しない / clippy の `panic`/`unwrap`/`expect`/indexing/string_slice 禁止が新規コードにも適用されること（`tests/test_e0b_constitutional_guard.py` は対象ディレクトリ走査型なら無変更で効く — 変更不要を確認せよ）。

**完了報告では、未実行のテスト・iOS 実機未検証を「未完了」と明記すること**（指示書 §8。simulator/実機計測は本 Step の実装完了条件ではないが、虚偽の完了報告は不採用事由）。

---

## 5. Cursor への厳格な禁止事項

**以下のいずれか1つでも犯した実装は、全体を不採用として差し戻す。「動くから良い」は通らない。**

### 5.1 排他制御の自己流実装 — 全面禁止

1. **`acquire_edinet_job` のシーケンス（cancel→Barrier→purge→Park→再確認）を変更・複製・「改善」するな。** これは Steps ≤10 でレビュー済み・テスト済みの契約実装である。お前の仕事は token の橋を架けることだけだ。
2. **worker loop（`Barrier`/`Park`/`Unpark`/`commit_pending_purge`）に触るな。** 1行たりとも。
3. **`LlmMemoryGovernor` に `Mutex` / `RwLock` / channel を追加するな。** governor は lock-free 契約（V3 §3.3）。token を governor に持たせる案も**禁止**（callback から cancel() を呼ぶ誘惑を構造的に断つため、token は guard にのみ属する）。
4. **watcher が `EdinetJobGuard` を strong 保持する実装は論理破綻**（gate が永遠に閉じ、LLM が二度と動かない）。必ず `Weak`。§4.5 テスト4が検出する。
5. **`JoinHandle::abort()` を cancel 手段として使うな。** blocking task は abort で止まらない（指示書 §Step 11 明記）。cancel は token 経由のみ。既存 extractor 内のチェック密度（entry/read/row/event 毎）を**間引くな**。
6. **`spawn_blocking` closure の外で `TempArchive` を drop する構造にするな。** temp 削除（fs 操作）を async スレッドへ載せない。また closure が guard clone を持たない実装は「abort 後に LLM と並走」の穴を開ける — 不採用。
7. **EDINET job 終了後に LLM を自動再ロードするな。** gate open = 受付再開であり、ロードは次のユーザー操作による lazy load（既存挙動）。`load()` をパイプラインから呼んだ時点で不採用。
8. **`os_proc_available_memory_bytes()` の値をキャッシュ・平均化・「上限まで使う」判定に使うな。** 都度取得の現在値としてのみ（V3 §3.4）。FFI は `probe.rs` に既に隔離済み — 新しい FFI を書くな。

### 5.2 パイプライン実装の禁止事項

9. `commands_sim.rs` に ZIP/XML/CSV の解析ロジック・バイト操作を書くな（指示書 §2）。既存 extractor の呼出しのみ。
10. `type=5` と `type=1` を並行 download するな。`futures::join!` / `tokio::join!` がこの2つに触れた時点で不採用（ArchiveGate が Err にするが、それは安全網であって設計ではない）。
11. `fetch_document_bytes`（全体 `Vec<u8>` 蓄積）を ZIP 経路で使うな。`MAX_RESPONSE_BYTES = 1MiB`（Wikipedia 用）を変更するな。
12. 新規依存を追加するな。`Cargo.toml` / lockfile に diff が出た時点で不採用。
13. `panic!` / `unwrap()` / `expect()` / 未検証 `[]` indexing / 未検証 byte slicing 禁止（clippy ゲート済み）。JoinError も `Err` に写像しろ。
14. エラー・ログに API key / 完全 URL / 原文本文を含めるな。`EdinetError` に新フィールドを足すときも同じ（そもそも本 Step で `EdinetError` の変更は不要のはずである — 必要に感じたら実装を止めて報告せよ）。
15. **失敗時に `CompanyFacts::default()` を新規返却する・base の非空フィールドを空へ戻す実装は、どんな経路でも不採用**（V3 §12）。片側成功は partial として返す — 「どちらか失敗したら全部捨てる」単純化は禁止。
16. status の解釈を発明するな。`FetchStatus` / `ExtractionStatus` の値は §3.5 の決定表が唯一の典拠。「良さそうな」独自マッピング（例: cancel を一律 `NetworkFailed` に丸める）は不採用。

### 5.3 手順の禁止事項

17. **最初の応答で実装するな。** 指示書 §0 のとおり、監査要約・変更ファイル・シグネチャ案・フロー・テスト計画の5点を提示して停止し、「設計レビュー待ちです。承認されるまで実装・依存追加・lockfile更新を行いません。」で締めろ。本設計書があっても、この停止条件は免除されない（本書との差分に気づいたら実装ではなく**報告**するのがお前の唯一の裁量である）。
18. 本書に列挙されていないファイルへの変更が必要になったら、変更せずに停止して理由を報告せよ。
19. テスト未実行・feature matrix 未確認（default / `egress-live` / Apple + `pocket-brain,secure-vault` / iOS cross-compile）で「完了」と報告するな。iOS ビルドは `--features pocket-brain,secure-vault` 必須である。

---

*本書は EDINET_LANE_DESIGN_V3.md（正本）の Step 11 局所への写像であり、矛盾があれば V3 が優先する。設計改稿の必要を発見した実装AIは、改稿せず停止・報告すること。*
