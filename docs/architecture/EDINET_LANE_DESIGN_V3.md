# EDINETレーン設計 — V3 / V3.1 / V3.2（正式承認）

- **承認日:** 2026-07-26
- **状態:** 正式承認（実装開始可）。追加の設計改稿は不要。
- **上位文書:**
  - `docs/EDINET_LANE_IMPLEMENTATION_DIRECTIVE.md`（実装手順・禁止事項）
  - `docs/architecture/EDINET_LANE_UNFREEZE_REPORT.md`（凍結解除評価）
- **本文書の役割:** V3 本体 + V3.1 追補 + V3.2 訂正を統合した**契約の正本**。実装は本契約に従う。

### 機械的注意（承認時補足）

- `SubjectKey` には `Serialize` / `Deserialize` derive と `#[serde(try_from = "String", into = "String")]` が必須。
- 未知 prefix / 空文字は `IdentityAmbiguous` ではなく **wire 入力不正 → `InvalidArgument`**。
- 複数企業候補のときのみ `IdentityAmbiguous`。

---

## 0. ミッション（変更なし）

既存の一覧検索・固定送信先・通信抽象・soft-fallback を維持し、選択済み `docID` の後段に上限付き ZIP 取得と XBRL/CSV/Inline XBRL のストリーム抽出を接続する。

**前提訂正:** `fetch_edinet_company_facts` の `CompanyFacts { company_name, ..Default::default() }` 初期化は削除しない。

**完成時の骨格:**

1. base（手動 / Vault復元 / Wikipedia）を保持
2. 書類一覧から適格有報 120 を厳格選定（coverage・prefix 完全性付き）
3. `csvFlag==1` なら type=5 → extract → delete。続けて type=1 → extract → delete（同時 TempArchive 最大1）
4. フィールド別 Origin/Storage で merge
5. facts + latest-selected + pending evidence を同一 Vault transaction
6. 埋め込みは別 lease。claim + latest-selected 一致後にのみ vec0 / meta / rag-ready を原子更新
7. いかなる失敗でも sanitize 済み base を失わない

---

## 1. Origin / Storage / CAS 永続化

### 1.1 直交軸

```rust
pub enum FactOrigin {
    Manual,
    Wikipedia,
    Edinet,
    UnknownProtected, // provenance欠落の既存非空（上書き禁止）
    Unknown,
}

pub enum FactStorage {
    Session,
    Vault,
    Live,
}
```

`Vault` は provenance ではない（Wikipedia も Vault に保存される）。

**書き込み優先度:** `Manual` = `UnknownProtected` > `Edinet` > `Wikipedia` > `Unknown`  
同一 origin 間は `submitted_at`（EDINET）/ `fetched_at`（Wikipedia）で鮮度比較。欠落時は既存維持。

### 1.2 SubjectKey（wire 固定）

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum SubjectKey {
    Name(String),   // "name:<normalized>"
    Edinet(String), // "edinet:<code>"
}
```

- 未知 prefix / 空 → `InvalidArgument`
- 複数企業候補 → `IdentityAmbiguous`

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubjectTransition {
    Rekey { from: SubjectKey, to: SubjectKey },
    Switch { from: SubjectKey, to: SubjectKey },
}
// {"kind":"rekey","from":"name:トヨタ自動車","to":"edinet:E02144"}
```

| 遷移 | 意味 |
|---|---|
| `Rekey` | 名前検索後にコード確定。同一性確認済みの原子的 key 昇格 |
| `Switch` | 別企業へ変更。企業名セルのみ持ち越し。他 Manual は旧 subject に残す |

**rekey 衝突規則:**

- 正規化名の完全一致候補が複数 → rekey 禁止 → `IdentityAmbiguous`
- alias 候補 ≥2 → `IdentityAmbiguous`
- name-key と code-key の両方に既存行 → 単純 UPDATE 禁止。cells / coverage / cursor ごとの transactional merge（origin・鮮度・revision）
- rekey 更新対象: `company_fact_cells`, `company_doc_pointers`, `company_filing_index`, `edinet_scan_cursor`, `edinet_list_coverage`, `subject_alias_candidate`, pending evidence の subject_key

### 1.3 API 分離

- `user_patch` — Manual 強制、常に上書き可
- `apply_enrichment` — 優先度・鮮度のみ。Manual は踏まない

### 1.4 `company_fact_cells`（値と provenance 同居）

```sql
CREATE TABLE company_fact_cells (
    subject_key TEXT NOT NULL,
    field TEXT NOT NULL,
    value TEXT NOT NULL,
    origin TEXT NOT NULL,
    storage TEXT NOT NULL,
    doc_id TEXT,
    submitted_at TEXT,
    fetched_at INTEGER,
    revision INTEGER NOT NULL,
    schema_version INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (subject_key, field)
);
```

`persist_fact_cells(..., expected_prev_revision)` — CAS。不一致 → `FactPersistence::Conflict`。

---

## 2. versioned IPC と独立ステータス軸

### 2.1 Serde

- struct fields: `#[serde(rename_all = "camelCase")]`
- enum variants: `#[serde(rename_all = "snake_case")]`（例 `"not_attempted"`, `"succeeded"`）
- TypeScript も snake_case 文字列でスナップショット固定

### 2.2 Request / Response

```rust
pub struct EdinetEnrichmentRequestV3 {
    pub schema_version: u16, // = 3
    pub subject_key: SubjectKey,
    pub subject_revision: i64, // expected_prev_revision
    pub subject_transition: Option<SubjectTransition>,
    pub fact_cells: Option<Vec<FactCellWire>>,
    pub company_facts: Option<CompanyFacts>,
    pub edinet_code: Option<String>,
    pub edinet_date: Option<String>,
}

pub struct EdinetEnrichmentResponseV3 {
    pub schema_version: u16,
    pub facts: CompanyFacts,
    pub fact_cells: Vec<FactCellWire>, // storage/fetched_at/origin 完全往復
    pub subject_key: SubjectKey,
    pub subject_revision: i64,
    pub field_provenance: Vec<FieldProvenance>,
    pub fetch: FetchStatus,
    pub extraction: ExtractionStatus,
    pub discovery_coverage: DiscoveryCoverage,
    pub discovery_result: DiscoveryResult,
    pub fact_persistence: FactPersistence,
    pub evidence_persistence: EvidencePersistence,
    pub served_from: ServedFrom,
    pub freshness: FilingFreshnessReport,
    pub warnings: Vec<EdinetWarningCode>,
}
```

```text
FetchStatus:        not_attempted | succeeded | network_failed | api_error | cancelled | memory_pressure
ExtractionStatus:   none | financials_only | narratives_only | both | parse_failed | cancelled
DiscoveryCoverage:  not_run | window_complete | window_incomplete | pinned
DiscoveryResult:    not_run | selected | no_eligible_in_window | identity_ambiguous
FactPersistence:    not_attempted | persisted | conflict | failed
EvidencePersistence: not_attempted | saved_pending_embedding | ready | failed
ServedFrom:         live | cache | mixed | not_applicable
Tristate:           yes | no | unknown   // correction_available 等
```

- `Conflict` 時: Vault の現行 revision + 現行 `fact_cells` を返す。stale 入力を採用しない
- 旧 `fetch_edinet_company_facts` は互換維持（内部 V3 → `.facts` 縮退、エラー文字列は現行のまま）
- 正本コマンド: `enrich_company_facts_from_edinet`
- FE: `types.ts` / `api.ts` にラッパー追加。`mergeCompanyFactsPreferFilled` は `applyEnrichment` 純関数へ置換

---

## 3. Heavy Coordinator（LLM 排他）

### 3.1 状態遷移

```text
Idle
 → gate close + EdinetAcquireGuard(acquire_epoch)
 → drain_epoch 発行
 → 実行中 generation を cancel
 → queued command を submit_epoch で拒否
 → active worker-owned lease count == 0
 → FIFO Barrier ACK
 → purge + purge_committed_epoch ACK
 → Park(acquire_epoch) ACK（古い遅延 Park 拒否）
 → admission / headroom 再確認
 → EdinetActive（EdinetJobGuard）
失敗 → Guard Drop: Unpark(epoch) + gate open + Idle
```

### 3.2 契約

- LLM lease: load〜generate/embed 終了まで1つ。**nested 禁止**
- 各 worker command が lease clone を保持。caller timeout では active count を減らさない
- **EdinetJobGuard = `Arc`**: async download と各 `spawn_blocking` が clone。最後の Drop でのみ unpark + gate open
- `spawn_blocking` 解析は自身が guard を保持（外側 abort でも blocking 完了まで解放しない）

### 3.3 Admission bitset

```rust
pub struct AdmissionBits(AtomicU32);
pub const ADMISSION_BACKGROUND: u32 = 1 << 0;
pub const ADMISSION_MEMORY_PRESSURE: u32 = 1 << 1;
pub const ADMISSION_THERMAL_PRESSURE: u32 = 1 << 2;
```

- 各監視元は自 bit のみ set/clear
- foreground 復帰は `BACKGROUND` のみ clear（MEMORY/THERMAL は解除しない）
- any bit set → Restricted: 新規 EDINET / deferred embed 拒否
- ObjC コールバックは lock-free store のみ（§1.1）

### 3.4 `EdinetCancelContext`

`CancellationToken` + cancel epoch + admission bitset を HTTP / ZIP / XML / CSV 各地点で統合判定。

`MemPhase::EdinetFetch` / `EdinetExtract` を追加。暫定 headroom: 開始 256MiB / cancel 128MiB / 追加 phys_footprint P95 ≤24MiB。

---

## 4. ZIP / 取得 / bounded 解析

### 4.1 完全直列

```text
Heavy lease（ZIP直前のみ）
→ [csvFlag=="1"] type=5 DL → extract → close/delete
→ type=1 DL → extract → close/delete
→ merge → drop buffers → drop job guard
→ (別途) LlmEmbed lease → evidence embed
```

Wikipedia 用 `MAX_RESPONSE_BYTES = 1MiB` は変更しない。EDINET は stream-to-temp。`fetch_document_bytes`（全体 `Vec<u8>`）は本番 ZIP 経路で使わない。

初期上限: 圧縮 64MiB / CD 2MiB / entry 512 / 単体 XBRL 32 / CSV 64 / 選択展開合計 96 / 候補 16。ZIP64・暗号化・非 Stored/Deflate・path traversal 拒否。

### 4.2 XML（唯一 adapter）

```text
clear → begin_event → read_event_into(&mut scratch)
→ if event_consumed() > MAX_EVENT { TooLargeEvent; entry soft-fail }
```

- scratch は `MAX_EVENT + 1` を一度だけ確保して再利用
- 生 `quick_xml::Reader` を extract 経路から直接呼ばない
- DOCTYPE 拒否、DOM/roxmltree/Serde全体deserialize禁止

### 4.3 CSV（`csv-core`）

- `read_field` 継続。破棄モードも同じ。終端は `ReadFieldResult::Field { record_end: true }`
- `OutputFull` 後に改行探索で skip しない（quoted CRLF 誤認防止）
- 破棄累計が暴走したら entry soft-fail

### 4.4 `continuedAt`

- fact より先の continuation は最大 32件・総 byte ≤ max_section_bytes で保存
- cycle / 重複 ID / 連結段数 8 / forward 解決はパス終了後1回

---

## 5. Discovery（coverage 付き再開可能探索）

### 製品仕様

「直近」ではなく **「探索窓内で見つかった最新の有報」**。全期間最新は保証しない。

- 一覧探索 = **light job**（Heavy lease 不要）
- Heavy lease は ZIP 取得直前のみ
- 決算期推定は探索順の最適化のみ（除外条件にしない）
- 120 厳格。130 は訂正診断のみ（`correction_available: Tristate`）。任意書類 fallback 禁止

### coverage / index

```sql
CREATE TABLE edinet_list_coverage (
    subject_key TEXT NOT NULL,
    date TEXT NOT NULL,
    status TEXT NOT NULL, -- ok | failed | pending
    fetched_at INTEGER,
    process_date_time TEXT,
    error_class TEXT,
    revalidate_after INTEGER, -- 当日分の短い再検証（初期15分）
    PRIMARY KEY (subject_key, date)
);

CREATE TABLE edinet_scan_cursor ( ... );
-- company_filing_index: parent_doc_id, withdrawal/legal/disclosure, xbrl/csv flags, process_date_time 等
```

`coverage=ok` は同一 transaction で:

1. 一覧全件 stream parse 完了
2. 必要な 120/130 metadata の index 保存完了
3. coverage + index を commit

### latest 判定

```rust
pub candidate_prefix_complete: bool
```

- `WindowIncomplete + Selected` でも、アンカー〜候補提出日の全日が ok なら `true` → ZIP 可
- 区間に gap → `false` → 自動 merge 禁止
- 窓完走で適格なし → `window_complete` ∧ `no_eligible_in_window`
- 打ち切りのみ → `window_incomplete`（`no_eligible_in_window` にしない）

---

## 6. Vault / RAG（vec0 非変更・V11）

`LATEST_SCHEMA_VERSION = 11`。vec0 `knowledge_chunks` は無改造。

### 追加テーブル

- `company_fact_cells`
- `subject_alias_candidate` — `PRIMARY KEY (alias_key, canonical_key)`
- `company_doc_pointers` — `latest_selected_doc/rev` + `rag_ready_doc/rev`
- `company_filing_index`（拡張列）
- `edinet_list_coverage` / `edinet_scan_cursor`
- `knowledge_chunk_meta`（chunk_id PK、concept/period/unit 等）
- `edinet_evidence_pending`

### source_id

`edinet-{code}`（docID を含めない）。chunk id: `edinet-{code}::{section_id}::{index:04}`  
現行 `company_source_matches` / `namespace_of` と整合。family replace で最新書類へ置換。

### Pending Evidence

```sql
-- state: pending | embedding | ready | failed
-- claim_id, claimed_at, claim_deadline
-- text ≤128KiB, subject×revision 総 ≤256KiB
```

- crash recovery: 期限切れ `embedding` を CAS で `pending` へ戻し retry++
- embed commit: `claim_id` 一致必須。遅延旧 worker 拒否
- embed は `latest_selected_doc == (doc_id, revision)` のときだけ vec0 + meta + `rag_ready_doc` を同一 transaction で更新

### 原子性（必須）

**同一 Vault transaction:**

```text
fact cells 保存
+ latest_selected_doc/revision 更新
+ pending Evidence 挿入
```

CAS 競合または Evidence 挿入失敗 → rollback（pointer だけ進めない）。

---

## 7. Keychain / リリースゲート

| 環境 | API key | EDINET |
|---|---|---|
| 開発 | `PKB_EDINET_API_KEY` 可 | 開発専用 |
| 本番 iOS | ユーザー Keychain のみ | 未実装/未保存なら無効 = `not_attempted` + `keychain_required_for_production` |

共有 key のバイナリ埋込禁止。Keychain 完了まで「EDINETレーン本番完了」と判定しない。

---

## 8. 依存候補（承認済み方針・pin は実装時確定）

```toml
quick-xml = { version = "=0.41.0", default-features = false, features = ["encoding"] }
zip = { version = "=8.6.0", default-features = false, features = ["deflate-flate2"] }
flate2 = { version = "=1.1.9", default-features = false, features = ["rust_backend"] }
csv-core = "=0.1.13"
encoding_rs_io = "=0.1.7"
tempfile = "=3.27.0"
tokio-util = { version = "=0.7.18", default-features = false, features = ["rt"] }
# tokio に fs / io-util を追加（既存行へ features 追記）
```

**Phase 0 確定:** `zip` は `deflate-flate2` のみ（`deflate-flate2-zlib-rs` 禁止）。既存 `png`/Tauri 経由の `flate2` `rust_backend`/`miniz_oxide` に feature anchor で統一し、`zlib-rs` 二重 backend を避ける。高水準 `csv` は使わず `csv-core` のみ。`reqwest = 0.13.4` / rustls-only / `egress-live` 維持。`reqwest::` は `net_gateway.rs` 以外禁止（憲法テスト）。

---

## 9. 責務分割（変更予定ファイル）

| 領域 | ファイル |
|---|---|
| 通信 | `net_gateway.rs`（stream-to-writer、Content-Length 参考） |
| EDINET API | `edinet_client.rs` |
| 新規 | `edinet_archive.rs`, `edinet_csv.rs`, `edinet_xbrl.rs`, `bounded_io.rs`, `heavy_coordinator.rs` |
| オーケストレーション | `commands_sim.rs`（ZIP/XML/CSV詳細禁止） |
| LLM | `service.rs`, `embed.rs`, `monitor/mod.rs` |
| RAG | `commands_rag.rs` |
| DB | `migrations.rs` (V11), `knowledge_repo.rs`, vault `worker` |
| FE | `types.ts`, `api.ts`, `companyFactsEnrich.ts`, `useCompanyFactsEnrichment.ts` |
| ガード | `tests/test_e0b_constitutional_guard.py` |

---

## 10. テスト / iOS 受入（要約）

指示書 §6–7 に加え V3 系必須:

- provenance 行列・rekey/Switch・CAS Conflict・factCells 往復
- wire snake_case スナップショット・旧コマンドエラー互換
- Barrier/Park epoch・job guard・admission bit 独立性
- XML 読後 TooLarge・CSV Field{record_end}・continuedAt orphan
- `candidate_prefix_complete`・coverage 同一 txn・light vs heavy
- pending claim recovery・stale embed abort・Ready 無傷
- Jetsam 0・temp 残留 0・追加 footprint P95 ≤24MiB

ライブ E2E は `#[ignore]` + ユーザー自身の key のみ。

---

## 11. 実装フェーズ順序（推奨）

1. 契約固定テスト（policy/egress/key/fallback/`reqwest::`）
2. V11 migration + cells/alias/coverage/pending/pointers
3. Heavy coordinator + admission
4. stream-to-temp + archive preflight
5. csv-core 財務 / bounded XML ナラティブ
6. discovery light + prefix / Heavy ZIP
7. IPC V3 + FE applyEnrichment
8. Evidence pending → embed
9. iOS 実機 Jetsam 受入
10. Keychain（本番ゲート・別完了条件）

---

## 12. 絶対禁止（再掲）

ZIP/XML/HTML 全体の `Vec`/`String` 化、DOM、`roxmltree`、`read_to_end`/`read_to_string`、全展開、LLM と ZIP 並列、HTTP 200 のみ成功判定、120 無しで任意書類採用、130 を原本扱い、API key/完全 URL の log、`panic!`/`unwrap`/`expect`、失敗時の空 `CompanyFacts::default()`、手動/Wikipedia base の消失。
