# SPEC — E0b STEP 7: Live Egress & TOCTOU DNS Pinning (Composer-Executable Blueprint)

> **読者:** 自律型コーディング AI（Cursor Composer 等）。人間向け解説ではない。**一字一句従え。裁量なし。**
> **親仕様:** `docs/SPEC_E0B_KNOWLEDGE_GATEWAY_v3.md` §9 / `docs/SPEC_E0B_STEP5_EGRESS_GATEWAY.md`（承認済み）。
> **前提:** STEP 0–6 ロック済み（HEAD `89a391c`、全ゲート GREEN）。STEP 5 で `net_gateway.rs` / `dns_guard.rs`
> の deny-table・seam・bounded fetch が確定。**STEP 5 レビューで発覚した DNS TOCTOU（`resolve_and_pin` が
> 検証済み IP を破棄し、`reqwest` が OS リゾルバで独立再解決する）を本 STEP で物理的に閉じる。**
> **本 STEP の目的:** 実ネットワーク通信を解禁しつつ、唯一の侵入経路である DNS Rebinding（SSRF）を、
> reqwest の解決経路内で deny-table を適用するカスタムリゾルバによって断ち切る。
> **ACK 規律:** STEP 7.E 完了時に HARD STOP。指揮官 ACK まで先へ進むな。

---

## 0. 起草時に検証済みの reqwest 0.13.4 事実（Composer はこれを前提にせよ）

`reqwest-0.13.4` のソースを読んで確定（`~/.cargo/registry/src/.../reqwest-0.13.4/src/dns/resolve.rs`, `lib.rs`, `async_impl/client.rs`）:

- **`pub mod dns;` は feature gate されていない**（`lib.rs:378`）。→ `reqwest::dns::{Resolve, Name, Addrs, Resolving}`
  は **既定ビルド（`features=["stream"]`・TLS 無し・toolchain 不要）でも使える**。カスタムリゾルバ本体と
  deny-enforcement は **常時コンパイル・無網テスト可能**にできる（hickory の実解決と TLS だけを egress-live に隔離）。
- リゾルバ契約（**正確な型**・逸脱不可）:
  ```rust
  pub type Addrs = Box<dyn Iterator<Item = std::net::SocketAddr> + Send>;
  pub type Resolving = std::pin::Pin<Box<dyn std::future::Future<Output = Result<Addrs, BoxError>> + Send>>;
  //  BoxError = Box<dyn std::error::Error + Send + Sync>
  pub trait Resolve: Send + Sync { fn resolve(&self, name: Name) -> Resolving; }  // Name::as_str() -> &str
  ```
  戻りは **`SocketAddr`（IP+port）**。deny 判定は `addr.ip()` に対して行う。
- 注入 API（`async_impl/client.rs:2310`）: `pub fn dns_resolver<R>(self, resolver: R) -> ClientBuilder where R: IntoResolve`。
  `IntoResolve` は `R: Resolve + 'static` と `Arc<R>` と `Arc<dyn Resolve>` に実装済み（`resolve.rs:154-176`）。
  → `ClientBuilder::dns_resolver(SafeKnowledgeResolver::new(...))` でよい（自動で `Arc` 包み）。
- `hickory-dns` は reqwest の feature（`Cargo.toml:126`）だが、**それは reqwest 内蔵リゾルバであり deny-filter を
  挟めない**。我々は `reqwest::dns::Resolve` を**自前実装**し、実解決は別途 `hickory-resolver` クレートで行う。

---

# セクション 1: アーキテクチャと安全規則 (Constraints & Safety Rules)

## 1.1 TOCTOU 再生産の禁止（旧破棄ロジックの完全削除）

STEP 5 の「解決してチェックし、結果を捨てて `reqwest` が独立再解決する」構造は **本 STEP で完全に削除する**:

- **削除:** `net_gateway.rs::resolve_and_pin`（検証済み IP を `_pinned` で捨てる関数）。
- **削除:** `fetch_one` / `research_fetch` の `resolver: &R`（`HostResolver` seam）引数と、その呼び出し。
- **削除 or 転用:** `dns_guard.rs::HostResolver` トレイト（STEP 5 の「別経路チェック」用）。DNS 解決は今後
  **reqwest クライアントに注入したカスタムリゾルバの内部だけ**で起き、`fetch_one` は DNS に一切触れない。
- **維持:** `dns_guard.rs::{is_disallowed_ip, extract_embedded_v4}`（deny-table 本体＝正本）。これは STEP 5 で
  全域テスト済み。**判定ロジックを 1 byte も緩めるな。**

> **不変条件:** egress で名前解決は **1 回**しか起きず（reqwest がカスタムリゾルバを呼ぶ）、その 1 回の結果が
> deny-table を通り、**同じ IP へ接続**される。「チェック用の解決」と「接続用の解決」が分岐する余地を消す。

## 1.2 カスタムリゾルバの Fail-Closed 要件（最重要）

`SafeKnowledgeResolver`（`reqwest::dns::Resolve` 実装）は次を**物理的に**満たす:

- 解決で得た **全 `SocketAddr` を deny-table に掛ける**。**1 つでも** `is_disallowed_ip(addr.ip())` が真なら
  **解決全体を `Err` で abort**（Fail-closed）。**「安全な IP だけ抽出して続行」は禁止（Fail-open の禁止）。**
- 解決結果が**空**でも `Err`（fail-closed）。
- deny-enforcement は**純関数 `enforce_deny_table(Vec<SocketAddr>) -> Result<Vec<SocketAddr>, GatewayError>`**
  に括り出し、**常時コンパイル・無網で網羅テスト**する（§2 STEP 7.A）。リゾルバ本体はこの純関数を呼ぶだけ。
- 実 DNS 解決（`hickory-resolver`）は **`AsyncLookup` seam** の背後に置く。既定テストは fake lookup を注入して
  private/mixed IP を返させ、**リゾルバが Fail-closed する**ことを**無網**で証明する。

## 1.3 二重ビルド規律（既定=無網・無 TLS / egress-live=実網・TLS）

- **既定 `cargo build`/`cargo test`（feature 無し）は完全オフライン・TLS コンパイル不要・C toolchain 不要**を維持。
  `SafeKnowledgeResolver` / `enforce_deny_table` / `AsyncLookup` トレイト / fake lookup テストは**常時コンパイル**。
- **`--features egress-live` でのみ** TLS プロバイダ（`reqwest/rustls` → aws-lc-rs/ring）と `hickory-resolver`、
  実 `ReqwestTransport`（`.dns_resolver(...)` 注入）がコンパイルされる。**このビルドには C toolchain が必要**
  （本 STEP 要件 2 で導入）。既定ビルドはこれを一切コンパイルしない。
- **`native-tls`/`default-tls` は永久禁止**（toolchain が面倒でも妥協禁止）。egress-live は `reqwest/rustls` のみ。
  aws-lc-rs が toolchain で通らなければ `rustls{ring}` へ差替可、しかし **native-tls は不可**。

## 1.4 パニック・OOM 規律（STEP 5 継承）

- 全新規/変更ファイル冒頭に no-panic lint を維持:
  ```rust
  #![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic,
          clippy::indexing_slicing, clippy::string_slice)]
  ```
  `#[cfg(test)]`/`tests/*.rs` は `#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` 可（既定パターン）。
- リゾルバの `Resolving` future 内で `.unwrap()`/`panic!` を使うな。lookup/enforce の `Result` を `BoxError` へ写像。
- 1 MiB cap・identity 強制・bounded 抽出・`tokio::select!` cancel/deadline drop は STEP 5 のまま不変。

## 1.5 影響範囲 (Blast Radius)

**新規作成:**
```text
apps/desktop/src-tauri/tests/knowledge_resolver.rs   # enforce_deny_table + SafeKnowledgeResolver(fake lookup) 無網テスト
apps/desktop/src-tauri/tests/knowledge_live.rs       # #[ignore] live E2E（PKB_E0B_LIVE=1 時のみ）
```
**改変を許可:**
```text
apps/desktop/src-tauri/src/knowledge/net_gateway.rs  # resolve_and_pin 削除 / SafeKnowledgeResolver・enforce_deny_table
                                                     #  ・AsyncLookup 追加 / fetch_one の resolver 引数削除 /
                                                     #  egress-live: HickoryLookup + ReqwestTransport .dns_resolver 注入
apps/desktop/src-tauri/src/knowledge/dns_guard.rs    # HostResolver 削除（is_disallowed_ip/extract は維持）
apps/desktop/src-tauri/src/knowledge/mod.rs          # re-export 調整
apps/desktop/src-tauri/Cargo.toml                    # egress-live に dep:hickory-resolver 追加
apps/desktop/src-tauri/tests/knowledge_gateway.rs    # 旧 resolver seam に依存する STEP5 テストを §2 の新テストへ移行
```
**触れてはならない:**
```text
STEP1-4 資産（canonicalize/pii_snapshot/attestation/dual_run/fsm/render_guard）/ Python コア / tests/golden/**
tests/test_e0b_constitutional_guard.py（STEP5 反転済み。`reqwest::` は net_gateway.rs 限定のまま維持）
```
`reqwest::` は **net_gateway.rs のみ**（憲法ガード scoped）。`tokio::net`/`TcpStream`/`UdpSocket`/`hyper::` は
**全ファイル禁止のまま**——実 DNS は `hickory-resolver` クレート内部で起き、我々の .rs に `tokio::net` は書かない。

---

# セクション 2: 実行フェーズ計画 (TDD Loops for Egress Live)

各 STEP は **RED → 検証コマンド → GREEN → 完了条件**。**既定テストは無網（toolchain 不要）を維持**。
egress-live テストは `--features egress-live`（toolchain 必要・要件 2 で導入）で走る。

---

### STEP 7.A: カスタム DNS リゾルバの構築（TOCTOU Closure RED・無網）

1. **RED（tests/knowledge_resolver.rs, 常時コンパイル・無網）:**
   - **`enforce_deny_table` 純関数（fail-closed）:**
     `enforce_deny_table(vec![global]) == Ok(vec![global])`；
     `enforce_deny_table(vec![global, 10.0.0.5:443]) == Err(DnsDenied)`（**一部 private で全体 Err**、抽出継続しない）；
     `enforce_deny_table(vec![]) == Err(DnsDenied)`（空も Err）；
     `enforce_deny_table(vec![::ffff:127.0.0.1:443]) == Err`（v4-mapped）；`64:ff9b::7f00:1 → Err`（NAT64）。
     deny/allow 代表値は STEP 5 の `is_disallowed_ip` 全域 fixture を再利用（`SocketAddr` へ port 付与）。
   - **`SafeKnowledgeResolver` 経由（reqwest::dns::Resolve 実装を実際に await）:**
     fake `AsyncLookup` が `[8.8.8.8:443, 10.0.0.5:443]`（mixed）を返すと、`resolver.resolve(name).await` が **`Err`**
     （future が Err に解決）；`[8.8.8.8:443, 2606:4700::1111:443]`（all global）なら `Ok(Addrs)` で当該 addr を列挙。
     `[]` → `Err`。**「global だけ返す」挙動を持たない**ことをアサート（mixed 入力で Ok を返さない）。
2. **検証コマンド（無網・toolchain 不要）:**
   ```
   cargo test -p pkb-desktop --test knowledge_resolver -- --nocapture
   ```
3. **GREEN（実装・net_gateway.rs, 常時コンパイル部）:**
   ```rust
   pub fn enforce_deny_table(addrs: Vec<SocketAddr>) -> Result<Vec<SocketAddr>, GatewayError> {
       if addrs.is_empty() { return Err(GatewayError::DnsDenied); }
       for a in &addrs { if is_disallowed_ip(a.ip()) { return Err(GatewayError::DnsDenied); } }
       Ok(addrs)
   }
   pub trait AsyncLookup: Send + Sync {
       fn lookup(&self, host: String)
         -> Pin<Box<dyn Future<Output = Result<Vec<SocketAddr>, GatewayError>> + Send>>;
   }
   pub struct SafeKnowledgeResolver<L> { lookup: Arc<L> }
   impl<L: AsyncLookup + 'static> reqwest::dns::Resolve for SafeKnowledgeResolver<L> {
       fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
           let lookup = Arc::clone(&self.lookup);
           let host = name.as_str().to_string();
           Box::pin(async move {
               let raw = lookup.lookup(host).await
                   .map_err(|e| Box::new(e) as reqwest::dns::... /* BoxError */)?;
               let vetted = enforce_deny_table(raw)
                   .map_err(|e| Box::new(e) as /* BoxError */ _)?;   // fail-closed
               Ok(Box::new(vetted.into_iter()) as reqwest::dns::Addrs)
           })
       }
   }
   ```
   （`BoxError` の正確な path は `reqwest` 0.13.4 に合わせよ。`GatewayError` は `std::error::Error` 実装済み。）
4. **完了条件:** enforce_deny_table 全域 + SafeKnowledgeResolver fail-closed（mixed→Err）が無網 PASS。**→ コミット。**

---

### STEP 7.B: Egress-Live トランスポートの結線（Transport Wiring RED）

1. **RED（旧ロジック削除 + 結線）:**
   - `resolve_and_pin` を**削除**。`fetch_one`/`research_fetch` から `resolver` 引数と DNS 呼び出しを**削除**
     （fetch_one は URL 構築 → 送信直前アサート → `transport.get` → meta 検証 → bounded read → 抽出、に単純化。
     DNS は transport 内のクライアントが担う）。`dns_guard.rs::HostResolver` を削除。
     `knowledge_gateway.rs` の旧 resolver 依存テスト（`e2e_dns_denied_...` 等）を STEP 7.A/7.B の新テストへ移行。
   - **egress-live 結線（`#[cfg(feature="egress-live")]`）:** `HickoryLookup`（`AsyncLookup` 実装・hickory-resolver で
     実解決）と、`ReqwestTransport` の**クライアント構築時に `.dns_resolver(SafeKnowledgeResolver::new(lookup))`**
     を注入。`ReqwestTransport::with_lookup(L)`（テスト注入用）と `::new()`（production=HickoryLookup）を用意。
   - **egress-live wiring テスト（`--features egress-live`・無網）:** `ReqwestTransport::with_lookup(FakeLookup→[10.0.0.5:443])`
     で `get(WIKI_URL)` を呼ぶと、**カスタムリゾルバが private IP を弾き、パケットを 1 つも出さずに DNS エラーで失敗**
     すること（＝`.dns_resolver` が確かに注入・発火している証明。実網なし）。`FakeLookup→[global]` かつ
     到達不能 addr なら接続失敗（resolver は通過した＝deny ではない）を区別。
   - **Cargo.toml:** `egress-live = ["dep:hickory-resolver", "reqwest/rustls"]`、
     `hickory-resolver = { version="=<cargo解決値>", default-features=false, features=["tokio"], optional=true }`
     （dnssec を有効化しない＝ring/aws-lc を hickory から引かない。TLS 側は reqwest/rustls が引く）。
2. **検証コマンド:**
   ```
   cargo test -p pkb-desktop --test knowledge_gateway            # 無網・旧テスト移行後も緑
   cargo build -p pkb-desktop --lib --features egress-live       # toolchain 導入後：TLS+hickory コンパイル
   cargo test -p pkb-desktop --features egress-live --test knowledge_gateway wiring   # wiring テスト（無網）
   ```
3. **GREEN:** 上記実装。旧 discard ロジックが grep で消えていること（`resolve_and_pin` 定義ゼロ）。
4. **完了条件:** 旧ロジック削除・既定テスト緑（無網）・egress-live wiring テストで private IP がパケット送出前に
   弾かれることを証明。**→ コミット。**

---

### STEP 7.C: 隔離された実網テスト（Opt-in Live E2E RED）

1. **RED（tests/knowledge_live.rs）:**
   - 全 live テストに **`#[ignore]`** を付与し、本体先頭で `if std::env::var("PKB_E0B_LIVE").as_deref() != Ok("1")
     { return; }`（env 不在なら即 return）。**既定 `cargo test` では走らない**（`#[ignore]` + env 二重ガード）。
   - `#[cfg(feature="egress-live")]` かつ `PKB_E0B_LIVE=1` の時のみ、実 `ReqwestTransport::new()`（HickoryLookup +
     SafeKnowledgeResolver）で `ja.wikipedia.org` の検索 API を叩き、(a) 200/JSON、(b) title+snippet 抽出、
     (c) 1 MiB cap 内、(d) render_guard による無害化（特殊トークン/markup 除去）が実データで動くことを確認。
   - live テストは CI/無網では**必ず skip**。ネットワーク不達時も既定スイートを壊さない。
2. **検証コマンド（手動のみ）:**
   ```
   # 通常は走らせない。手動検証時のみ（toolchain + ネットワーク必要）:
   set PKB_E0B_LIVE=1 && cargo test -p pkb-desktop --features egress-live --test knowledge_live -- --ignored --nocapture
   ```
3. **GREEN:** live テスト実装。**既定 `cargo test` に live が 1 件も混入しない**こと（`--test knowledge_live` を
   flag 無しで実行すると `0 passed; N ignored` になる）。
4. **完了条件:** live テストが `#[ignore]`+env で隔離され、既定スイートは無網維持。手動 live 実行は指揮官裁量。**→ コミット。**

---

### STEP 7.D: フル回帰テストと品質ゲート

1. **検証コマンド:**
   ```
   # (A) 既定・無網・toolchain 不要
   cargo build -p pkb-desktop --lib                       # TLS/aws-lc/ring を 1 つもコンパイルしない
   cargo test -p pkb-desktop --lib --tests               # 全既定テスト（live は ignored）
   cargo clippy --all-targets -- -D warnings              # 既定 clippy
   python -m pytest tests/test_e0b_constitutional_guard.py -v   # reqwest:: は net_gateway.rs 限定を維持
   cargo tree -f "{p} {f}" | grep reqwest                # 既定 union に rustls/gzip/native-tls 不在
   # (B) egress-live・toolchain 必要
   cargo build -p pkb-desktop --lib --features egress-live
   cargo test -p pkb-desktop --features egress-live --lib --tests   # live は依然 ignored
   cargo clippy --all-targets --features egress-live -- -D warnings
   cargo tree -f "{p} {f}" --features egress-live | grep -E "reqwest|rustls|native-tls"  # rustls 有・native-tls 不在
   ```
2. **完了条件:** (A) 既定が無網・無 TLS・toolchain 不要で全緑、(B) egress-live も clippy 込みで全緑かつ
   **native-tls/gzip/brotli/deflate が union に不在**、STEP 0–6 非回帰。**→ HARD STOP。**

---

### STEP 7.E: HARD STOP

STEP 7.D 完了で全ゲート GREEN・コミット済みに到達したら、**出力を停止しユーザー（指揮官）へ報告し ACK を待て。**

---

# セクション 3: コミットと停止規律 (Checkpoints & HARD STOP)

## 3.1 コミット規律

`git status --short` で目視。当該 STEP の対象のみ stage（`git add -A`/`.` 禁止）。**`.claude/` を stage しない。
push 禁止。** revert/reset/`checkout --` 禁止。STEP0–6 資産・golden・Python コア・凍結ファイルを改変しない。

| STEP | stage 対象 | コミットメッセージ |
|---|---|---|
| 7.A | `src/knowledge/net_gateway.rs`, `tests/knowledge_resolver.rs` | `feat(e0b-step7): STEP 7.A — SafeKnowledgeResolver + fail-closed enforce_deny_table (offline)` |
| 7.B | `src/knowledge/{net_gateway,dns_guard,mod}.rs`, `Cargo.toml`, `tests/knowledge_gateway.rs` | `feat(e0b-step7): STEP 7.B — delete resolve_and_pin discard logic; inject dns_resolver (egress-live)` |
| 7.C | `tests/knowledge_live.rs` | `feat(e0b-step7): STEP 7.C — opt-in #[ignore]+PKB_E0B_LIVE live Wikipedia E2E` |

各コミット末尾:
```
Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
```
（7.A で新モジュールがコンパイル通る最小状態を保つ。7.D はコード差分なしなら空コミットを作らない。）

## 3.2 HARD STOP（絶対命令・報告必須項目）

1. 既定（無網）: `cargo build --lib`（TLS/aws-lc/ring 非コンパイルのログ）、`cargo test --lib --tests`、
   `cargo clippy -D warnings`、python 憲法ガード、`cargo tree | grep reqwest`（rustls/gzip/native-tls **不在**）。
2. egress-live: `cargo build --features egress-live` 成功、`cargo test --features egress-live`（**live は ignored**）、
   `cargo clippy --features egress-live -D warnings`、`cargo tree --features egress-live`（**rustls 有・native-tls 不在**）。
3. **TOCTOU 封鎖の明示:** `resolve_and_pin`（旧 discard）が**削除された**こと（grep 定義ゼロ）、DNS 解決が
   `SafeKnowledgeResolver` の**1 経路のみ**であること、mixed IP がリゾルバで fail-closed され**パケット送出前に**
   弾かれること（wiring テスト）を、どのテストがどう証明したか。
4. live E2E が `#[ignore]`+`PKB_E0B_LIVE=1` で隔離され既定スイートに混入しないこと。
5. `git log --oneline`（7.A–7.C）。STEP0–6 非回帰。

**ACK なしに先へ進むな。live テストを既定スイート/CI へ組み込むな。native-tls へ妥協するな。**

## 3.3 HARD STOP（異常時）

- **リゾルバが mixed IP で Ok を返す（Fail-open）:** 設計違反。`enforce_deny_table` は「一部 private→全体 Err」。
  テストを緩めず実装を直す。「安全な IP だけ抽出して続行」は絶対禁止。
- **旧 `resolve_and_pin`/`HostResolver` 経路が残存:** TOCTOU 再生産。完全削除するまで完了としない。
- **egress-live ビルドが aws-lc-sys/ring で失敗:** toolchain（VS Build Tools C++ + clang-cl + nasm）未導入。
  **native-tls へ逃げず**、toolchain 導入 or `rustls{ring}` で解決。解決不能ならブロッカー報告・停止。
- **live テストが既定 `cargo test` で走ってしまう:** `#[ignore]` + env return の二重ガードを確認。
- **`reqwest::` が net_gateway.rs 以外に出現 / `tokio::net` を書いた:** 憲法ガード RED。集約し直す（DNS は hickory 内部）。

## 3.4 STEP 7 完了の定義 (DoD)

- DNS 解決は `SafeKnowledgeResolver`（reqwest に注入）の**1 経路のみ**。`resolve_and_pin` 等の discard/別経路チェック
  は**完全削除**。TOCTOU/rebinding の分岐が構造的に存在しない。
- `SafeKnowledgeResolver`/`enforce_deny_table` は **fail-closed**（1 つでも非 global→全体 Err、空→Err、Fail-open なし）、
  常時コンパイル・無網テストで証明。wiring テストで「private IP はパケット送出前に弾かれる」を egress-live で証明。
- 既定ビルド/テストは**完全オフライン・TLS 非コンパイル・toolchain 不要**を維持。TLS/hickory は egress-live 隔離。
- `native-tls`/`default-tls`/`gzip`/`brotli`/`deflate` は既定・egress-live いずれの union にも**不在**。
- live E2E は `#[ignore]`+`PKB_E0B_LIVE=1` 隔離。既定・CI は無網。
- panic-free（knowledge/ clippy クリーン）、`reqwest::` は net_gateway.rs 限定、STEP0–6 非回帰。
- HARD STOP・ACK 待ち。
