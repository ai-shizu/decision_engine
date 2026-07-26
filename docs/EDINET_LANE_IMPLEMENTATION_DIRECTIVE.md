# EDINETレーン凍結解除 — 実装AI向け実装指示書

この文書は、Cursor / Claude / Codex等の実装AIへそのまま渡すための命令書である。記載された順序、停止条件、安全制約を変更してはならない。

> **設計正本（2026-07-26 正式承認）:**  
> `docs/architecture/EDINET_LANE_DESIGN_V3.md`（V3＋V3.1＋V3.2訂正の統合契約）。  
> 本指示書と契約が競合する場合は **DESIGN_V3 を優先**し、本指示書の手順・禁止事項は DESIGN_V3 と矛盾しない範囲で従う。

## 0. 最優先命令

### 最初の応答では実装してはならない

**いきなり全コードを書かないこと。最初の応答ではファイルを編集せず、次の5点だけを提示して停止し、承認を待つこと。**

1. 現行コード監査の要約と、変更予定ファイル
2. `Cargo.toml` に追加・変更する依存関係の正確な候補
3. 新規・変更関数、型、traitのシグネチャ案
4. メモリ・cancel・fallbackを含む処理フロー
5. unit / integration / iOS実機テスト計画

最初の応答に、関数本体、巨大なコードブロック、ファイル編集、`cargo add`、lockfile更新を含めてはならない。末尾に必ず以下を記載すること。

> 設計レビュー待ちです。承認されるまで実装・依存追加・lockfile更新を行いません。

### 前提の訂正

現行 `commands_sim.rs` は `CompanyFacts::default()` をそのまま返すモックではない。

`fetch_edinet_company_facts` は、企業名のみを保持したfallback用の基礎 `CompanyFacts` を `..CompanyFacts::default()` で初期化し、`resolve_company_facts` へ渡している。この初期化は削除しないこと。

既に以下は実装済みである。

- EDINET API v2 書類一覧取得
- fixed URL生成・検証
- EDINETコード・正規化企業名による選択
- Network Policy / `egress-live` gate
- APIキー不在、通信失敗、解析失敗時のsoft-fallback
- `HttpTransport` / `ResponseBody`
- `reqwest::` を `net_gateway.rs` に限定する憲法テスト

真のミッションは、**既存の一覧検索・固定送信先・通信抽象・フォールバックを維持し、選択済み `docID` の後段に、上限付きZIP取得とXBRL/CSV/Inline XBRLのストリーム抽出を接続すること**である。

## 1. ミッション

`apps/desktop/src-tauri/src/llm/commands_sim.rs` のEDINET経路を、一覧メタデータだけを返す現状から、EDINET API v2による実データ増強パイプラインへ進化させる。

完成時の挙動:

1. 手動入力、Vault、Wikipedia等から得た既存 `CompanyFacts` を `base` として保持する。
2. 書類一覧APIの日別一覧から対象企業の直近の有価証券報告書原本を選ぶ。
3. `docID` を用いて書類取得APIから `type=5` CSV ZIPと `type=1` 本文/XBRL ZIPを必要に応じて直列取得する。
4. 圧縮レスポンスをアプリcache directoryの一時ファイルへ上限付きで逐次保存する。
5. ZIP全体を展開せず、許可したエントリだけを1件ずつストリーム解析する。
6. 財務値をXBRL-to-CSVから抽出し、利用不能時だけXBRLへfallbackする。
7. 事業等のリスク、事業概要等を `XBRL/PublicDoc` のXBRL / Inline XBRLから抽出する。
8. 非空かつ検証済みのEDINETフィールドだけを `base` へマージする。
9. 原文証拠を `docID`、提出日、対象期間、概念名付きでVault/RAGへ保存する。
10. いかなるEDINET失敗でも、Wikipedia等の基礎データを失わず返す。

## 2. 変更対象と責務

最初の設計案では、少なくとも次のファイルを監査すること。

- `apps/desktop/src-tauri/Cargo.toml`
- `apps/desktop/src-tauri/src/knowledge/net_gateway.rs`
- `apps/desktop/src-tauri/src/knowledge/edinet_client.rs`
- `apps/desktop/src-tauri/src/knowledge/mod.rs`
- `apps/desktop/src-tauri/src/llm/commands_sim.rs`
- `apps/desktop/src-tauri/src/llm/service.rs`
- `apps/desktop/src-tauri/src/monitor/mod.rs`
- `apps/desktop/src-tauri/src/rag/commands_rag.rs`
- `apps/desktop/src-tauri/src/lib.rs`
- `apps/desktop/src/lib/companyFactsEnrich.ts`
- `apps/desktop/src/lib/useCompanyFactsEnrichment.ts`
- `tests/test_e0b_constitutional_guard.py`

推奨する責務分割:

- `net_gateway.rs`
  - 既存の唯一の `reqwest` 境界
  - `Content-Length` の取得
  - body chunkを、累計上限・deadline・cancel付きでasync writerへ保存する汎用処理
- `edinet_client.rs`
  - API URL、レスポンス、一覧metadata、書類選定、outer fallback
  - `type=1` / `type=5` の型安全なURL生成
- 新規 `knowledge/edinet_archive.rs`
  - temp archive
  - EOCD / central directory preflight
  - ZIP entry allowlist、size、path、CRC、compression guard
- 新規 `knowledge/edinet_csv.rs`
  - UTF-16LE TSVの逐次decode
  - 財務概念・期間・連結区分・単位の選択
- 新規 `knowledge/edinet_xbrl.rs`
  - XBRL / Inline XBRLのイベント駆動抽出
  - text block、namespace、`continuedAt`、出力上限
- `commands_sim.rs`
  - base保持
  - heavy-operation gate
  - fetch → extract → merge → fallbackのオーケストレーションだけ
- `llm/service.rs` / `monitor/mod.rs`
  - LLMとの排他、purge完了確認、memory pressure cancel、`MemPhase::EdinetExtract`
- `commands_rag.rs`
  - アーカイブ・parser buffer解放後の証拠保存・分割・埋め込み

`commands_sim.rs` にZIP、XML、CSVの詳細ロジックを直接書かないこと。

## 3. 依存関係の設計条件

最初の応答で、以下を初期候補として提示し、現行toolchain、lockfile、license、feature tree、iOS targetとの整合を説明すること。

```toml
quick-xml = { version = "=0.41.0", default-features = false, features = ["encoding"] }
zip = { version = "=8.6.0", default-features = false, features = ["deflate-flate2"] }
flate2 = { version = "=1.1.9", default-features = false, features = ["rust_backend"] }
csv-core = "=0.1.13"
encoding_rs_io = "=0.1.7"
tempfile = "=3.27.0"
tokio-util = { version = "=0.7.18", default-features = false, features = ["rt"] }
```

また、既存 `tokio` に必要なら `fs` / `io-util` featureを追加する案を示すこと。

**Phase 0 確定（2026-07-26）:** `deflate-flate2-zlib-rs` は採用しない。既存グラフの `flate2` `rust_backend`/`miniz_oxide` へ anchor し、二重 backend を禁止する。高水準 `csv` ではなく `csv-core`。

厳守事項:

- `reqwest = 0.13.4` とrustls-onlyの既存構成を維持する。
- 新しいHTTPクライアントを追加しない。
- `reqwest::` を `net_gateway.rs` 以外に書かない。
- `zip` のdefault featuresを有効にしない。
- AES、bzip2、lzma、xz、zstd等を初期実装へ持ち込まない。
- `quick-xml` のSerde全体deserialize機能を使わない。
- `roxmltree`、`scraper`、`kuchiki`、DOM TreeBuilderを追加しない。
- 既存の推移依存を直接利用する場合も、`Cargo.toml` に直接依存として宣言する。
- exact pinは `cargo add --dry-run` とiOS cross-compileの確認後に確定する。

依存を減らす代替案を示すことは許可するが、ストリーム処理、cancel、安全性を弱めてはならない。

## 4. 最初に提示すべき型・関数シグネチャ

最初の応答では、以下と同等の責務を持つ型とシグネチャを提示すること。名前の改善は許可するが、責務の統合・省略は不可とする。

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdinetDocumentKind {
    FilingAndXbrl, // API type=1
    XbrlCsv,       // API type=5
}

#[derive(Clone, Debug)]
pub struct EdinetLimits {
    pub max_compressed_bytes: u64,
    pub max_central_directory_bytes: u64,
    pub max_entries: usize,
    pub max_single_xbrl_bytes: u64,
    pub max_single_csv_bytes: u64,
    pub max_selected_uncompressed_bytes: u64,
    pub max_candidate_files: usize,
    pub max_section_bytes: usize,
    pub max_total_evidence_bytes: usize,
    pub download_deadline: std::time::Duration,
    pub parse_deadline: std::time::Duration,
    pub job_deadline: std::time::Duration,
}

#[derive(Clone, Debug, Default)]
pub struct PartialEdinetFacts {
    pub business_summary: Option<ExtractedField<String>>,
    pub business_risks: Option<ExtractedField<String>>,
    pub performance_summary: Option<ExtractedField<String>>,
    pub financials: Vec<ExtractedFinancialFact>,
    pub evidence: Vec<EdinetEvidenceSection>,
    pub warnings: Vec<EdinetWarning>,
}

#[derive(Clone, Debug)]
pub struct EdinetProvenance {
    pub doc_id: String,
    pub edinet_code: String,
    pub submitted_at: String,
    pub period_start: Option<String>,
    pub period_end: Option<String>,
    pub concept: Option<String>,
    pub archive_kind: EdinetDocumentKind,
    pub truncated: bool,
}

#[derive(Clone, Debug)]
pub struct ExtractedField<T> {
    pub value: T,
    pub provenance: EdinetProvenance,
}

#[derive(Debug)]
pub enum EdinetError {
    MissingCompanyIdentity,
    NoEligibleFiling,
    ApiKeyMissing,
    Network,
    Timeout,
    Cancelled,
    MemoryPressure,
    TooLarge,
    InvalidContentType,
    ApiResponse { status: String },
    InvalidZip,
    UnsupportedArchive,
    UnsupportedEncoding,
    Parse,
    Persist,
}
```

`EdinetError` にAPI key、完全URL、外部本文、個人情報を保持しないこと。

```rust
pub fn build_document_download_url(
    doc_id: &str,
    kind: EdinetDocumentKind,
    subscription_key: &str,
) -> Result<String, EdinetError>;

pub fn select_latest_eligible_yuho(
    docs: &[EdinetDocumentMeta],
    identity: &CompanyIdentity,
) -> Result<SelectedFiling, EdinetError>;

pub async fn download_edinet_archive_to_temp<T: HttpTransport>(
    transport: &T,
    request: &EdinetArchiveRequest<'_>,
    temp_dir: &std::path::Path,
    limits: &EdinetLimits,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<TempArchive, EdinetError>;

pub fn extract_edinet_archive(
    archive: TempArchive,
    filing: &SelectedFiling,
    limits: &EdinetLimits,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<PartialEdinetFacts, EdinetError>;

pub fn extract_financials_from_tsv<R: std::io::Read>(
    reader: R,
    filing: &SelectedFiling,
    limits: &EdinetLimits,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<PartialEdinetFacts, EdinetError>;

pub fn extract_narratives_from_xbrl<R: std::io::BufRead>(
    reader: R,
    filing: &SelectedFiling,
    limits: &EdinetLimits,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<PartialEdinetFacts, EdinetError>;

pub fn merge_company_facts_by_provenance(
    base: &CompanyFacts,
    edinet: &PartialEdinetFacts,
) -> Result<CompanyFacts, EdinetError>;

pub async fn try_enrich_company_facts_from_edinet<T: HttpTransport>(
    transport: &T,
    request: &EdinetEnrichmentRequest<'_>,
    runtime: &EdinetRuntime,
) -> Result<EdinetEnrichment, EdinetError>;

pub async fn enrich_company_facts_with_fallback(
    base: CompanyFacts,
    request: EdinetEnrichmentRequest<'_>,
    runtime: &EdinetRuntime,
) -> CompanyFacts;
```

outer fallback関数は、EDINET内部の `Result` をAPI/UI境界へそのまま伝播させないこと。安全にサニタイズ済みの `base` が存在する限り、必ずそれを返せる構造にする。

## 5. 実装ステップ

### Step 1 — 現行契約を固定する

実装前に以下をテストで固定する。

- policy offで外部通信0件
- `egress-live` offで外部通信0件
- API keyなしで外部通信0件
- EDINET失敗時に既存Wikipedia factsが完全に維持される
- 企業名のみの `CompanyFacts { ..Default::default() }` が有効なfallback入力である
- `reqwest::` が `net_gateway.rs` 以外に現れない

`ReqwestTransport::new()` の失敗を、fallbackより外側で `?` 伝播している現行箇所をfallback境界内へ移動する。

### Step 2 — 一覧metadataを完全化する

`EdinetDocumentMeta` は外部JSONのnullを許容する。最低でも以下を `Option<String>` 等で保持する。

- `docID`
- `edinetCode`
- `filerName`
- `ordinanceCode`
- `formCode`
- `docTypeCode`
- `periodStart`
- `periodEnd`
- `submitDateTime`
- `parentDocID`
- `withdrawalStatus`
- `docInfoEditStatus`
- `disclosureStatus`
- `xbrlFlag`
- `csvFlag`
- `legalStatus`

`results` を無条件に先頭500件で切ってから検索しない。Wikipedia用の共通1MiB上限は変更せず、書類一覧にはEDINET専用の有限上限を設ける。初期候補は8MiBとし、実データ計測で調整する。

一覧全体を `serde_json::Value` として構築せず、`serde_json::Deserializer` のcustom visitor等で全 `results` を逐次検査し、対象企業の候補だけを上限付きで保持する。候補保持数にも小さい上限を設定し、超過時は曖昧な選択をせずエラーにする。

候補選定条件:

- EDINETコード一致を最優先
- 企業名一致は正規化完全一致を優先し、曖昧な包含一致は診断付きのfallbackに限定
- `docTypeCode == "120"` を原本として選ぶ
- `withdrawalStatus == "0"`
- 開示中
- `legalStatus == "1"` または `"2"`
- `xbrlFlag == "1"`
- `submitDateTime` で決定的にsort
- `docID` 重複をdedupe

`120` がない場合に同企業の任意書類を選ばない。

`130` 訂正有報は `parentDocID` で原本へ関連付ける。初期版で訂正適用まで行わない場合は、原本120を使い `correction_available` を診断に残す。130単独を完全有報として採用しない。

### Step 3 — 最新書類探索とキャッシュ

一覧APIは日付単位であり、企業検索パラメータもpaginationもない。したがって、対話中に過去1年を総当たりしてはならない。

実装する戦略:

1. 同一 `date` の一覧から、120/130等の必要metadataと `processDateTime` だけを正規化してVaultへキャッシュする。raw JSON全体を日付ごとに永続化しない。
2. cache hitでは通信しない。
3. API一覧は原則1分より短い間隔で再取得しない。
4. 既知提出日がある場合はその日を使う。
5. キャッシュ未構築時のscanは承認された有限窓に限定する。
6. 見つからない場合は `NoEligibleFiling` としてbaseへfallbackする。
7. 401は即停止、429は当該jobを停止、400/404は再試行しない。
8. 500/timeoutだけを少数回・上限付きbackoffの対象にする。

限定窓で見つかった書類を、索引が不完全なまま「絶対に最新」と表示しない。最新性の保証レベルを内部診断に残す。

### Step 4 — 書類取得APIを型安全にする

`type` を生の整数や文字列で呼出側から渡さない。`EdinetDocumentKind` からのみ `1` / `5` を生成する。

URL:

```text
GET https://api.edinet-fsa.go.jp/api/v2/documents/{docID}?type=1&Subscription-Key=...
GET https://api.edinet-fsa.go.jp/api/v2/documents/{docID}?type=5&Subscription-Key=...
```

厳守事項:

- `docID` とkeyを既存ルールで検証・percent encode
- host、path、query key、typeをsend直前にも検証
- redirect禁止
- TLS 1.2以上、rustls-only
- `Accept-Encoding: identity`
- API keyを含む完全URLをlogしない

開発時の `PKB_EDINET_API_KEY` は維持してよいが、本番iOSでprocess environmentだけを供給経路にしない。ユーザー自身のkeyを既存Keychain境界へ保存・読出しする設計案を提示し、バイナリへの共有key直書きを禁止する。Keychain対応を同一変更に含めない場合は、本番有効化の明示blockerとして残す。

成功判定:

- HTTP statusだけを信用しない
- ZIP成功は `application/octet-stream`
- `application/json` はHTTP 200でもAPI error
- HTML/Sorry pageは拒否
- JSON errorは `metadata.status` 文字列型と `StatusCode` 数値型の両方を解析
- content type確認後にZIP magicを確認

### Step 5 — EDINET専用stream-to-tempを追加する

Wikipedia用 `MAX_RESPONSE_BYTES = 1 MiB` は変更しない。

EDINET専用downloaderは以下を満たす。

- `ResponseBody::next_chunk()` ごとに一時ファイルへ書く
- `Content-Length` は早期拒否の参考にするが、未指定・虚偽を想定する
- 実測累計byteで必ず上限を強制する
- deadline、cancel、memory pressureを各chunkで確認する
- partial fileを成功扱いしない
- success時にflush、rewindし、`TempArchive`へ所有権移動
- error / cancel / timeout / dropで削除
- temp directoryはTauri app cache directory配下
- ファイル名にAPI key、企業名、docIDを含めない
- 同時downloadは1
- `type=5` と `type=1` は直列

全レスポンスを `Vec<u8>` へ集める既存 `fetch_document_bytes` を本番ZIP経路で使用しない。削除する場合は既存テスト・public APIへの影響を説明する。

### Step 6 — ZIPをpreflightする

`ZipArchive::new()` が中央ディレクトリを収集する前に、可能な範囲でEOCDを固定小バッファで検査する。

初期上限:

```text
compressed archive             64 MiB
central directory               2 MiB
ZIP entry count                   512
single XHTML/XBRL entry         32 MiB
single UTF-16 TSV entry         64 MiB
selected uncompressed total     96 MiB
candidate files parsed              16
```

以下を拒否する。

- ZIP64
- 複数ディスク
- 暗号化
- nested ZIP等のarchive
- Stored/Deflate以外の圧縮方式
- overlapping files
- 絶対パス、`..`、不正なenclosed path
- entry数、central directory、個別展開量、総展開量の超過
- 許可ディレクトリ外

許可対象:

- 財務: `XBRL_TO_CSV/` 配下の提出本文CSV
- 非財務: `XBRL/PublicDoc/` 配下の `.htm` / `.xhtml` / `.xbrl`

`AuditDoc`、`AttachDoc`、`EnglishDoc`、画像、CSS、JSは初期対象外とする。

宣言 `size()` / `compressed_size()` だけを信用しない。読み取り中の実測byteでも停止する。対象entryは最後まで読みCRC検証を完了する。

禁止:

- `ZipArchive::extract`
- 全entryの展開
- `read_to_end`
- `read_to_string`
- `mmap` によるアーカイブ全体保持

### Step 7 — 財務TSVを逐次抽出する

EDINET `type=5` のCSVはUTF-16LE、CRLF、タブ区切り、全項目引用符付きである。`.csv` という拡張子だけでUTF-8カンマ区切りと判断しない。

公式レイアウトの9列を、順序とヘッダー名を検証して扱う。

1. 要素ID
2. 項目名
3. コンテキストID
4. 相対年度
5. 連結・個別
6. 期間・時点
7. ユニットID
8. 単位
9. 値

処理:

1. BOMとencodingを確認
2. `encoding_rs` / `encoding_rs_io` で固定バッファを使い逐次UTF-8化
3. `csv::ReaderBuilder::delimiter(b'\t')` で行単位処理
4. 期待列数と各列上限を検証
5. allowlist概念だけを保持
6. 相対年度、連結/個別、期間/時点、unitを明示選択
7. 同一概念の曖昧候補は未取得
8. U+FFFDを含む財務対象行は捨て、warningを残す
9. 数値は表示文字列だけでなく概念、context、unitと一体保存

初期財務概念:

- Revenue / NetSales相当
- OperatingIncomeLoss相当
- ProfitLoss相当
- Assets相当
- Equity / NetAssets相当
- CashFlowsFromUsedInOperatingActivities相当

タクソノミ年度、J-GAAP、IFRS、US-GAAPごとのallowlistをデータとして管理する。未知概念を名前の類似だけで推測しない。

財務抽出に失敗した場合:

- `type=1` 内XBRLのイベント駆動抽出へfallbackしてよい
- ただしDOM化は禁止
- それも失敗したら財務だけ未取得とし、リスク抽出を継続

CSVのテキスト値は30,000文字で切られるため、事業等のリスクには使用しない。

### Step 8 — XBRL / Inline XBRLを逐次抽出する

必ず `quick_xml::Reader<BufRead>` または `NsReader<BufRead>` と `read_event_into()` を使用する。同じscratch `Vec<u8>` を各event後に `clear()` する。

絶対禁止:

- `roxmltree`
- XML全体のSerde deserialize
- `String::from_utf8` で文書全体を保持
- DOM
- `read_to_string`
- 入力全体の `to_string()`

対象概念はallowlistで管理し、namespace prefix文字列そのものを固定しない。

初期候補:

- `BusinessRisksTextBlock`
- `DescriptionOfBusinessTextBlock`
- 経営者による財政状態、経営成績及びキャッシュ・フローの分析に対応するtext block

Inline XBRL:

- `ix:nonNumeric` の `name` 属性を検証
- 対象fact内のText / CDATA / 必要な子要素テキストをcapture
- `script` / `style` / `noscript` を無視
- `continuedAt` を件数・総量上限付きで解決
- block境界を適切な改行・空白へ変換
- entity decode後も出力byte上限を強制

XBRL text block:

- escaped HTML断片をそのまま最終表示へ渡さない
- 外部参照を行わない
- 断片もtoken/event型でプレーンテキスト化

安全:

- `DOCTYPE` を拒否
- 外部DTD / schema / entityを解決しない
- event数、depth、attribute数、attribute長、capture量を制限
- parse deadlineとcancelをeventごとに確認
- malformed文書はsoft-fail

XMLとして不正なHTML4へ初期版でDOM parserを追加しない。公式fixtureで必要性が確認された場合のみ、`html5ever::Tokenizer` と独自 `TokenSink` を別設計レビューに出す。TreeBuilderは禁止する。

### Step 9 — 原文証拠と表示用factsを分離する

`CompanyFacts` に有報全文を入れない。

- `CompanyFacts`
  - UIと短いLLM prompt用
  - 各フィールドの小さい既存上限を維持
  - 4,096 × 3 > 12,000 の総量上限矛盾を解消
- `EdinetEvidenceSection`
  - RAG用原文
  - `docID`、提出日、対象期間、概念、truncatedを保持
  - 単一128KiB、総量256KiBを初期上限

安全上限で切れた場合は `truncated=true` とし、「全文」と表示しない。無言truncateは禁止する。

抽出テキストはEDINET由来でも信頼済み命令ではない。HTML/XMLタグを除いた後も外部証拠として扱い、既存 `render_guard::sanitize_external_text` と同等の規律をRAG chunkごとに適用する。外部本文をsystem instructionへ連結せず、明示的な引用・証拠境界内に置く。サニタイズ失敗したchunkだけを破棄し、他の検証済みfieldとbaseは維持する。

アーカイブ、ZIP entry、decoder、XML/CSV scratchをdropした後でのみ、RAG保存・埋め込みへ進む。

### Step 10 — provenance付きでマージする

単一 `source: String` だけで混合出典を表現しない。内部ではフィールド別provenanceを必須とする。

優先順位:

```text
ユーザー手動編集 > EDINET一次開示 > Wikipedia > 空値
```

ルール:

- EDINETの空値でbaseを上書きしない
- 財務失敗でEDINETリスクを捨てない
- リスク失敗で財務を捨てない
- 企業同定が曖昧なら何も上書きしない
- manual値を自動取得で上書きしない
- Wikipediaが先に入っていても、regulated fieldの検証済みEDINET値を採用できる
- source混在をフィールド別に保持

UI互換のため当面 `CompanyFacts` IPCを維持する場合でも、内部provenanceをVaultに保存し、互換用 `source` は `mixed:manual+edinet+wikipedia` 等の決定的な値にする。最終的には型付きstatus/provenance responseへの移行案を提示する。

### Step 11 — iOS memory coordinatorへ統合する

EDINET解析とLLM生成・埋め込み・loadを同時実行しない。

必要な契約:

- EDINET job concurrency = 1
- 新規heavy LLM jobを一時拒否または待機
- 実行中generationをcancel
- headroom不足なら `request_purge()`
- `is_loaded() == false` を期限付き確認
- purge未確認ならEDINETを開始せずbaseへfallback
- `MemPhase::EdinetFetch` / `MemPhase::EdinetExtract` を追加
- memory warning、Serious/Critical、backgroundでEDINET tokenをcancel
- job終了後に全temp/bufferをdrop
- LLMは即時再ロードせず、次回必要時にlazy load

暫定headroom:

```text
minimum headroom to start       256 MiB
cancel/purge headroom           128 MiB
additional phys_footprint P95    24 MiB以下
```

`os_proc_available_memory()` を追加する場合はApple専用の小さいFFI境界へ隔離し、現在値としてのみ使用する。値をキャッシュせず、上限まで使い切るために使わない。

ZIP/XML/CSV解析は `tokio::task::spawn_blocking` へ移す。ただし `JoinHandle::abort()` だけではblocking処理は停止しないため、次の各地点で `CancellationToken::is_cancelled()` を確認する。

- ZIP entryループ
- entry readループ
- XML eventループ
- CSV rowループ
- text captureループ

### Step 12 — outer fallbackを完成させる

`base` を関数冒頭でサニタイズし、EDINET処理中は所有しておく。

次のすべてで、サニタイズ済み `base` を返す。

- EDINETコードなし
- 対象有報なし
- API keyなし
- Network Policy off
- `egress-live` off
- transport生成失敗
- DNS/TLS/timeout/cancel
- 400/401/404/429/500
- HTTP 200 + error JSON
- maintenance HTML
- ZIP上限超過
- ZIP不正・CRC失敗
- unsupported compression/encoding
- XML/CSV parse失敗
- memory pressure
- LLM purge未完了
- Vault/RAG保存失敗（抽出済みフィールドは画面用の一時結果として返してよいが、保存済みとは表示しない。抽出結果も成立していなければbaseを返す）

絶対禁止:

- `panic!`
- `unwrap()`
- `expect()`
- 未検証の `[]` indexing
- 未検証の文字列byte slicing
- error時に `CompanyFacts::default()` を新しく返すこと
- Wikipedia等から取得済みの値を空へ戻すこと

内部診断は残してよいが、API key、完全URL、原文全文をlogしない。UIには「EDINET未取得」「一部取得」を状態として返し、「この企業のデータがない」へ誤変換しない。

## 6. テスト必須項目

デフォルトテストは実ネットワークを一切使わない。fake transportと小さいfixtureを使う。

### URL・一覧

- `type=1` / `type=5` URLだけが通る
- host/path/queryの改変を拒否
- keyをlogしない
- null fieldをpanicせず解析
- withdrawn / undisclosed / expiredを拒否
- `120` を厳格選択
- 任意書類fallbackをしない
- `130` を原本として誤採用しない
- `docID` dedupe
- 日付cache hitで通信0件
- 401後に次の日を走査しない
- 429後に再試行嵐を起こさない

### HTTP body

- 小さいchunk列をtempへ正しく保存
- `Content-Length` 超過をbody read前に拒否
- `Content-Length` なしでも実測上限を強制
- 宣言より大きいbodyを実測で拒否
- deadline
- cancel
- chunk error
- partial file cleanup
- HTTP 200 + JSON error
- HTTP 200 + HTML
- invalid content type
- invalid ZIP magic

### ZIP

- 正常Stored
- 正常Deflate
- entry数上限
- central directory上限
- expansion上限
- path traversal
- absolute path
- nested archive
- encryption
- unsupported compression
- overlap
- ZIP64
- CRC不正
- target directory外を無視
- candidate file数上限
- cancel時cleanup

### 財務TSV

- UTF-16LE BOM
- CRLF / tab / quoted 9 columns
- current consolidated context選択
- previous periodを誤選択しない
- individualを連結より優先しない
- unitを保持
- ambiguous candidateを未取得
- malformed row
- oversized field
- U+FFFD対象行を破棄
- cancel / deadline
- J-GAAP / IFRSの代表fixture

### XBRL / Inline XBRL

- namespace prefixが異なってもlocal conceptを抽出
- `BusinessRisksTextBlock`
- `DescriptionOfBusinessTextBlock`
- nested XHTML text
- `continuedAt`
- script/style除外
- entity / CDATA
- max depth
- max events
- max attributes
- max output
- DOCTYPE拒否
- external referenceを取得しない
- malformed XMLでsoft-fail
- cancel / deadline

### 統合

- Wikipedia only + EDINET失敗 = Wikipedia完全維持
- manual + EDINET成功 = manual維持
- Wikipedia summary + EDINET risk = 混合source
- financial only成功
- risk only成功
- EDINET空値でbaseを上書きしない
- transport生成失敗でもbaseを返す
- RAG保存失敗でも画面用factsを返す
- `CompanyFacts` 総量上限境界
- truncated provenance

### compile / guard

- default feature
- `egress-live`
- Apple + `pocket-brain` + `secure-vault`
- iOS cross-compile
- clippyのpanic / unwrap / expect / indexing / string_slice禁止
- `reqwest::` の憲法テスト
- default testでlive DNS/TLS 0件

実ネットワークE2Eは、ユーザー自身のAPI keyを必要とする `#[ignore]` / 明示環境変数付きtestだけにする。CIや通常testでEDINETへ接続しない。

## 7. iPhone実機受入試験

Simulatorだけで完了判定しない。

最低試験:

- サポート対象で最もメモリの少ないiPhone
- 代表的な新しいiPhone
- LLM未ロード
- 3Bモデルロード直後
- 最大対応モデルロード直後
- generation中にEDINET開始
- EDINET中にbackground
- EDINET中にmemory warning
- 上限直前ZIP
- 上限超過ZIP
- `type=1` / `type=5` 両方
- 連続10企業

記録:

- `phys_footprint` baseline / peak / delta
- `os_proc_available_memory` 参考値
- model purge完了時間
- download bytes / time
- parsed entry / uncompressed bytes
- extraction time
- cancel latency
- temp file残留
- Jetsam event report

合格条件:

- Jetsam 0件
- foreground消失 0件
- main thread stallなし
- baseデータ消失 0件
- temp file残留 0件
- 追加 `phys_footprint` P95 24MiB以下、またはレビューで承認した値以下
- cancel後に期限内で停止
- 抽出値がgolden fixtureと一致

## 8. 実装完了報告の書式

実装後は次の順で報告すること。

1. 変更概要
2. 前提訂正をどう反映したか
3. 変更ファイル一覧
4. Cargo依存とfeature
5. メモリ上限とfallback契約
6. 書類選定規則
7. 財務・リスク抽出規則
8. 実行したtestと結果
9. iOS実機計測値
10. 既知の未対応taxonomy / 書類
11. 残存リスク

「テスト未実行」「iOS未検証」を完了と報告しない。未実施項目は明確に未完了とする。

## 9. 公式仕様

実装前に最新版を確認し、非公式ブログだけを根拠にしない。

- [EDINET API仕様書 Version 2](https://disclosure2dl.edinet-fsa.go.jp/guide/static/disclosure/download/ESE140206.pdf)
- [EDINET API FAQ](https://disclosure2dl.edinet-fsa.go.jp/guide/static/disclosure/WZEK0090_001.html)
- [書類閲覧操作ガイド（CSV仕様）](https://disclosure2dl.edinet-fsa.go.jp/guide/static/disclosure/download/ESE140133.pdf)
- [提出書類ファイル仕様書](https://disclosure2dl.edinet-fsa.go.jp/guide/static/disclosure/download/ESE140104.pdf)
- [報告書インスタンス作成ガイドライン](https://disclosure2dl.edinet-fsa.go.jp/guide/static/disclosure/download/ESE140112.pdf)
- [EDINET利用規約](https://disclosure2dl.edinet-fsa.go.jp/guide/static/disclosure/WZEK0030.html)

## 10. 最終禁止事項

以下のいずれかを含む実装は不採用とする。

- ZIP全体を `Vec<u8>` へ読み込む
- XML / HTML全体を `String` へ読み込む
- DOMツリーを構築する
- `roxmltree` を使う
- `read_to_end` / `read_to_string` を使う
- 展開ディレクトリへ全ファイルを出す
- LLM generationとZIP解析を並列実行する
- API error JSONをZIPとして解析する
- HTTP 200だけで成功判定する
- `docTypeCode=120` がない時に任意書類を採用する
- 訂正有報130を無条件で完全原本として採用する
- API keyや完全URLをlogする
- `panic!` / `unwrap()` / `expect()` を使う
- EDINET失敗時に空の `CompanyFacts::default()` を返す
- 手動入力またはWikipedia基礎データを失う
- 最初の応答から全コードを実装する
