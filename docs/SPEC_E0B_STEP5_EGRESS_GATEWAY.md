# SPEC — E0b STEP 5: Egress Gateway (Composer-Executable Blueprint)

> **読者:** 自律型コーディング AI（Cursor Composer 等）。人間向け解説ではない。**一字一句従え。裁量なし。**
> **親仕様:** `docs/SPEC_E0B_KNOWLEDGE_GATEWAY_v3.md` §6/§7/§9 / STEP 3・STEP 4 ブループリント（承認済み）。
> **前提:** STEP 0–4 ロック済み。`knowledge::{canonicalize,pii_snapshot,attestation,dual_run(verify_and_gate),
> fsm(ResearchSlot,Txn)}` が確定・検証済み。**本 STEP は E0b で初めて実 egress 能力を導入する最終・最高リスク関門。**
> **本 STEP の目的:** 検証（STEP 3）と FSM（STEP 4）を通過した Intent を Wikipedia API へ送り、**SSRF・
> Decompression Bomb・OOM・キャンセル漏れを物理的に封じて** title+snippet を取得する Egress Gateway を作る。
> **ACK 規律:** STEP 5.E 完了時に HARD STOP。指揮官 ACK まで STEP 6（Python integrate / IPC 配線）へ進むな。

---

## 0. 起草時に検証済みのリポジトリ事実（Composer はこれを前提にせよ）

- **reqwest 0.13.4 は既に tauri 経由の transitive 依存**であり、現状の有効 feature は **`json, stream` のみ**
  （`cargo tree -f "{p} {f}" --target all | grep reqwest` で確認済み）。**gzip / brotli / deflate / native-tls /
  default-tls / socks / cookies は有効化されていない。** → `default-features=false, features=["rustls-tls","stream"]`
  を direct 依存として加えても union は `json, stream, rustls-tls` に留まり、**reqwest の自動解凍は起きない**
  （Decompression Bomb 防御が feature unification で無効化されない）。**STEP 5.A でこの不変を必ず再検証する。**
- `tokio 1.52.3` が `features=["time"]` で直接依存。`tokio-util 0.7.18` は transitive で存在。
- STEP 0 ガード（`test_rust_has_no_http_client_dependency` / `test_rust_src_has_no_outbound_network_calls`）は
  `reqwest::` と `tokio::net` を**全 .rs で禁止**。本 STEP でこれを**反転**する（§2 STEP 5.A）。
- **【指揮官裁定・TLS feature 隔離】** このホスト（`aarch64-pc-windows-msvc`）は aws-lc-sys / ring の C ソースを
  ビルドする C/clang toolchain を欠く。**STEP 5 のテストは全て無網（seam 注入）で TLS プロバイダを必要としない**
  ため、TLS を **`egress-live` cargo feature 配下へ隔離**する。既定ビルド＝ `reqwest = features=["stream"]`
  （既にコンパイル済みの `json,stream` union を再利用・**toolchain 不要で緑**）。実 TLS transport / hickory /
  live 接続だけが `--features egress-live` で有効化され、その時のみ toolchain を要する（実 egress ビルドまで遅延）。
  → §1.6 と STEP 5.A はこの隔離方式を実装する。

---

# セクション 1: アーキテクチャと制約 (Constraints & Safety Rules)

## 1.1 データフロー（STEP 3/4 との結線）

```text
[orchestrator: knowledge::net_gateway::research_fetch]
 1. slot.begin(payload)                         → Txn<Pending>   (STEP4: single-flight/nonce/drift)
       Busy/Replay/Drift/Malformed は即 Err、egress ゼロ
 2. verify_and_gate(payload, dict_terms, k_spawn) → Ok           (STEP3: HMAC ∧ 独立 PII 再検査 AND)
       いずれか失敗で即 Err、egress ゼロ（txn drop → slot 解放）
 3. txn.transition_to_fetching()                → Txn<Fetching>  (STEP4)
 4. tokio::select! {
        _ = cancel        => abort,             // socket 即 drop
        _ = sleep(deadline) => abort,           // socket 即 drop
        r = gateway.fetch_all(&queries) => r,   // 各 query 逐次
    }
      各 query: provider.build_request(q) → resolver.resolve(host) → deny_table 濾過(§9)
              → reqwest .resolve(host, vetted_addr) pin → GET(identity, https, redirect:none, no_proxy)
              → status==200 & content-type==json & content-encoding∈{identity,無} 検査
              → stream を 1 MiB hard cap で読取（超過即打切）→ serde 抽出(bounded) → title+snippet
 5. txn.transition_to_ready()                   → Txn<ReadyToIntegrate>
 [STEP 6 で Python integrate へ。STEP 5 は Rust 内 fetch まで。invoke_handler 登録はしない]
```

## 1.2 メモリ枯渇（OOM）とパニックの物理封鎖

- **全ボディを一括バッファしない。** `bytes_stream()` を chunk ごとに読み、**解凍前 wire バイトを逐次計数**、
  累計が `MAX_RESPONSE_BYTES = 1 MiB` を超えた瞬間に**その chunk を append せず stream を drop**（接続打切）。
- **`Content-Length` を信用しない**（参考 pre-check 可）。chunked・length 詐称・無限ストリームは同一 cap 経路で死ぬ。
  trickle は fetch total timeout が上界。
- **char 境界パニック禁止。** title/snippet の byte cap 切詰は `s.char_indices()` で境界を求める。`&s[..n]`
  （バイトスライス）を禁止（`clippy::string_slice`）。
- **serde bounded 抽出。** results 配列は上限 `MAX_RESULTS_PER_QUERY=3` を超えたら早期 Err（全件 deserialize 前）。
  serde_json 既定再帰上限（128）を無効化しない。1 MiB cap が全体量を上界化する。
- **no-panic lint** を全新規ファイル冒頭に宣言し、`.unwrap()`/`.expect()`/`panic!`/添字/バイトスライスを禁止:
  ```rust
  #![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic,
          clippy::indexing_slicing, clippy::string_slice)]
  ```
  reqwest/stream/JSON/DNS の全 `Result` を伝播（`GatewayError` へ写像）。**untrusted 入力で unwrap するな。**

## 1.3 SSRF・Decompression Bomb（最重要脅威）

- **URL は Provider が固定テンプレートからのみ構築**。scheme=`https`・host=allowlist 定数・port=443・path 固定・
  可変は percent-encode された検索 param 1 個のみ。ユーザ/LLM/外部データは scheme/host/port/path を構成できない。
- **送信直前アサート**（全入力で実行時検査、違反 = `GatewayError::UrlViolation`）:
  `scheme=="https" && host==ALLOWLIST && port==443 && userinfo.is_empty() && fragment.is_none() &&
   path==固定 && query キー集合==固定`。
- **reqwest 設定（`ReqwestTransport` に集約・逸脱不能に）:** `default-features=false` + `use_rustls_tls()` +
  `min_tls_version(TLS 1.2)` + `https_only(true)` + `redirect(Policy::none())` + `no_proxy()` +
  `connect_timeout(5s)` + `timeout(15s)`。**gzip/brotli/deflate/native-tls/cookies を有効化しない。**
- **`Accept-Encoding: identity` を明示送出**し、応答 `Content-Encoding` が `identity`/不在**以外**なら body を
  読まず `GatewayError::WireViolation`。（§0 の通り reqwest は解凍 feature 無効のため自動解凍しない＝
  `Content-Encoding` を消さない＝この検査が有効。STEP 5.A で feature 不変を再検証してこの前提を守る。）
- **HTTP は 200 のみ受理。** `Content-Type` が `application/json`(±charset) 以外は body を読まず `StatusRejected`。
  3xx は `redirect::Policy::none()` で追従しない上、200 以外一律拒否。
- **DNS/内網拒否（§9）:** resolve → 全解決 IP を deny_table 濾過 → 1 つでも非 global なら `DnsDenied`（接続ゼロ）→
  合格 addr へ pin。「合格分だけ使う」はしない（fail-closed）。

## 1.4 Egress の単一ファイル封じ込め（ratchet 維持）

- **`reqwest::` を書いてよいのは `knowledge/net_gateway.rs` ただ 1 ファイルのみ。** STEP 5.A でガードをこの 1 点
  だけ許可する形へ反転する。他の全 .rs では `reqwest::` を含め egress シンボル禁止のまま。
- **`tokio::net` / `TcpStream` / `UdpSocket` / `hyper::` は net_gateway.rs でも禁止のまま。** raw socket egress を
  gateway 内でも許さない（reqwest 経由のみ）。DNS は `tokio::net::lookup_host` を使わず resolver seam で行う
  （hickory 等クレート内部の socket は本リポジトリの .rs に現れないので対象外）。
- **production 実体（`ReqwestTransport` + `HickoryResolver`）は `#[cfg(feature = "egress-live")]` 配下に置く。**
  `HttpTransport` / `HostResolver` トレイト・fake 実装・`deny_table` 純関数・orchestration は**常時コンパイル**され、
  既定 `cargo test` は fake で全通過する（TLS プロバイダ非コンパイル＝toolchain 不要）。`.use_rustls_tls()` 等
  reqwest の TLS API を呼ぶコードは egress-live 配下にのみ書く（既定では cfg で除外され存在しない）。

## 1.5 テストは実ネットワークを叩かない（プロジェクト不変規律・AI_SKILLS §7.2）

- fetch ロジックは **`HttpTransport` トレイト**（seam）越しにのみ通信する。既定テストは**fake transport 注入**で
  status/headers/stream を模擬し、cap・identity・status・抽出・cancel を**無網で**証明する。
- DNS は **`HostResolver` トレイト**（seam）。テストは fake resolver に `127.0.0.1`/`10.x` 等を返させ `DnsDenied` を証明。
- **実 Wikipedia を叩くテストは `#[ignore]` + 明示 feature/env でのみ実行**（既定 `cargo test` では走らない）。
  CI/サンドボックスは無網前提。実網テストを既定スイートへ入れることを禁止。

## 1.6 依存追加（バージョン固定・§0 検証済み前提）

`apps/desktop/src-tauri/Cargo.toml`（**TLS 隔離方式**・§0 指揮官裁定）:
```toml
[dependencies]
# 既定ビルドは TLS プロバイダを一切コンパイルしない（toolchain 不要・既存 json,stream union を再利用）。
reqwest = { version = "=0.13.4", default-features = false, features = ["stream"] }
#   ↑ base に rustls-tls を入れない。gzip/brotli/deflate/native-tls/default-tls/socks/cookies を絶対に足すな。
tokio  = { version = "1", features = ["time", "rt", "rt-multi-thread", "macros", "sync"] }  # 既存 time に追記
futures-util = "=0.3.31"   # StreamExt（bytes_stream 消費）
bytes = "=1.10.1"          # トレイトが reqwest 型に依存しないよう Bytes を直接使う（version は cargo 解決値へ）
hickory-resolver = { version = "=<cargo解決値>", default-features = false, features = ["tokio"], optional = true }

[features]
# opt-in。実 egress ビルド時のみ TLS プロバイダ（aws-lc-rs/ring）と hickory をコンパイル → toolchain 必要。
egress-live = ["dep:hickory-resolver", "reqwest/rustls-tls"]

[dev-dependencies]
tokio = { version = "1", features = ["test-util"] }   # paused-time で deadline/cancel を決定論テスト
```
- **バージョンは `cargo add` が解決する実値を `=` ピンで固定**し、`cargo tree -d` で重複世代が増えないことを確認。
- **既定 `cargo build`/`cargo test`（egress-live なし）は toolchain 不要で緑**であること（本ホストの受入条件）。
- `--features egress-live` のビルドは C toolchain（aws-lc-sys 用 clang-cl/nasm、または ring）を要する。
  aws-lc-rs が困難なら egress-live 側で `reqwest/rustls-tls-manual-roots` 等 + `rustls{ring}` へ差替可。
  **いずれにせよ deny_table 純関数と `HostResolver`/`HttpTransport` seam が SSRF/wire 安全性の正本**で、production
  実体は egress-live 配下の差替可能物。既定テストは fake seam で全証明する（TLS 実体を要しない）。

---

# セクション 2: 実行フェーズ計画 (TDD Loops for Egress Gateway)

各 STEP は **RED → 検証コマンド → GREEN → 完了条件**。実網テスト禁止（seam 注入）。ビルドは重いので絞れ。

---

### STEP 5.A: HTTP クライアント安全導入 + 憲法ガード反転（Config RED）

1. **RED（ガード反転）:** `tests/test_e0b_constitutional_guard.py` の**当該 2 関数のみ**を反転
   （STEP 0.E docstring の inversion obligation に従う。他関数は 1 文字も変えるな）:
   - `test_rust_has_no_http_client_dependency` → **`test_rust_reqwest_safe_config`**（TLS 隔離方式）:
     Cargo.toml の `reqwest` 依存 + `[features]` を parse し、
     (a) reqwest が direct 依存として存在、(b) `default-features = false` を含む、
     (c) reqwest の **base `features` が `{"stream"}` の部分集合**（base に TLS を入れない）、
     (d) **禁止 feature `gzip/brotli/deflate/native-tls/default-tls/socks/cookies` を base・`egress-live` の
     どちらにも含まない**、(e) `[features] egress-live` が存在する場合、その中身は
     `{"dep:hickory-resolver","reqwest/rustls-tls"}`（＝rustls 系のみ）に限られ **native-tls/default-tls を
     マップしない**、(f) `hyper/isahc/ureq/curl/surf` は direct 依存に無い、を assert。
     さらに**canary**: 合成 `reqwest = { features = ["gzip"] }` と `egress-live = ["reqwest/native-tls"]` を
     検出器が「unsafe」と判定すること。
   - `test_rust_src_has_no_outbound_network_calls` → **scoped 反転**:
     `EGRESS_ALLOWED_FILES = {"net_gateway.rs"}`。`reqwest::` は **net_gateway.rs でのみ許可**、他ファイルは違反。
     `TcpStream/tokio::net/UdpSocket/hyper::/isahc::/ureq::/surf::` は**全ファイル（net_gateway.rs 含む）で禁止**のまま。
     canary（`reqwest::get` を非許可ファイル名で違反判定 / net_gateway.rs で許可）を維持。
   - **RED 実測:** 反転直後、net_gateway.rs も reqwest 依存も未追加なので、
     `test_rust_reqwest_safe_config` は「reqwest direct 依存が無い」で **FAIL（RED）**。
2. **検証コマンド:**
   ```
   python -m pytest tests/test_e0b_constitutional_guard.py -v      # 反転テストが RED であることを実測
   ```
3. **GREEN（実装）:** §1.6 の **TLS 隔離方式**依存を Cargo.toml へ設定
   （**現在の作業ツリーにある `rustls-no-provider`/`rustls{ring}`/`egress-live` 未使用の記述を、この方式へ置換**）。
   `cargo tree -f "{p} {f}" --target all | grep reqwest` で **既定 union が `json,stream`（rustls-tls / gzip 等が
   無い）**ことを確認（gzip 等があれば union 汚染 = ブロッカー報告）。空の `knowledge/net_gateway.rs`
   （`#![deny(...)]` + reqwest への最小参照）と `mod.rs` 登録で反転テストを GREEN 化。
4. **完了条件:** `test_rust_reqwest_safe_config` + scoped source テスト GREEN、かつ
   **既定 `cargo build -p pkb-desktop --lib`（egress-live なし）が toolchain 不要で成功**。
   （`--features egress-live` のビルドは toolchain 導入後に別途検証。STEP 5.A のコミット条件には含めない。）**→ コミット。**

---

### STEP 5.B: Network/DNS ガード（SSRF RED）

1. **RED（tests/knowledge_gateway.rs, 無網）:**
   - **deny_table 純関数の全域 fixture:** `is_disallowed_ip(IpAddr) -> bool` を、下記 CIDR の代表値
     （境界含む）と global 代表値で網羅。**v4-mapped(`::ffff:a.b.c.d`) と NAT64(`64:ff9b::a.b.c.d`) は
     埋込 IPv4 を取り出して再判定**すること。
     ```
     禁止 v4: 0.0.0.0, 10.0.0.1, 100.64.0.1(CGNAT), 127.0.0.1, 169.254.0.1, 172.16.0.1, 172.31.255.255,
             192.0.0.1, 192.0.2.1, 192.168.1.1, 198.18.0.1, 198.51.100.1, 203.0.113.1,
             224.0.0.1(mcast), 240.0.0.1, 255.255.255.255
     禁止 v6: ::, ::1, fc00::1(ULA), fe80::1(link-local), 2001:db8::1(doc), ff02::1(mcast),
             ::ffff:127.0.0.1(v4-mapped loopback), 64:ff9b::7f00:1(NAT64 → 127.0.0.1)
     許可: 8.8.8.8, 1.1.1.1, 2606:4700:4700::1111 等 global unicast のみ
     ```
   - **injected resolver → DnsDenied:** fake `HostResolver` が `[127.0.0.1]` / `[10.0.0.5]` /
     `[8.8.8.8, 10.0.0.5]`（一部でも private）を返すと、gateway が **`DnsDenied` で abort し transport を一切
     呼ばない**（fake transport の call-count == 0）。全 IP が global の時だけ transport が呼ばれる。
   - **URL 不変条件:** `build_request` が任意の検索文字列に対し scheme=https/host=allowlist/port=443/path 固定/
     query キー固定を満たす。IP リテラル host・別ホスト・別 path・trailing dot・punycode 混入・
     userinfo・fragment を送信直前アサートが `UrlViolation` で弾く（transport call-count 0）。
   - **decode 一致:** 送出 URL の検索 param を 1 回 percent-decode したものが入力クエリと byte 一致。
2. **検証コマンド:**
   ```
   cargo test -p pkb-desktop --test knowledge_gateway dns -- --nocapture
   cargo test -p pkb-desktop --test knowledge_gateway url -- --nocapture
   ```
3. **GREEN（実装）:** `knowledge/dns_guard.rs`（`is_disallowed_ip` 純関数＝手動オクテット判定、unstable
   `is_global` に依存しない / `HostResolver` トレイト / production `HickoryResolver`）と
   `knowledge/net_gateway.rs` の `WikipediaProvider::build_request` + 送信直前アサート + resolve→deny→pin フロー。
4. **完了条件:** deny_table 全域 + resolver abort + URL 不変条件 全 PASS。**→ コミット。**

---

### STEP 5.C: 非同期 Fetch とサイズ上限（Bounded Stream RED）

1. **RED（tests/knowledge_gateway.rs, 無網・fake transport）:**
   - **1 MiB cap:** fake transport が `MAX_RESPONSE_BYTES-1` / `=cap` / `+1` byte を返す 3 ケース。
     `+1` は **`WireViolation`（打切）** で、**受信バッファが cap を一度も超えて確保されない**こと
     （chunk append 前に判定）。chunked・巨大単一 chunk・length 詐称も同経路で打切。
   - **identity 強制:** 応答 `Content-Encoding: gzip` → body を読まず `WireViolation`。`identity`/不在は許可。
   - **status/type:** 301/302/404/500/204/206 → `StatusRejected`。`Content-Type: text/html` → `StatusRejected`。
   - **cancel/deadline で socket 即 drop:** fake transport が**完了しない stream**（pending）を返す。
     `tokio::time::pause()` 下で deadline 経過 or cancel シグナルにより、`tokio::select!` の fetch arm が drop され、
     fake stream の **drop-probe フラグが即座に立つ**こと（ゾンビ task が残らない）。abort 後に transport が
     追加の副作用を起こさないこと。
   - **panic 安全:** 空 body・不正 UTF-8 body・多バイト境界での cap 切詰で panic しない（char_indices 切詰）。
2. **検証コマンド:**
   ```
   cargo test -p pkb-desktop --test knowledge_gateway stream -- --nocapture
   cargo test -p pkb-desktop --test knowledge_gateway cancel -- --nocapture
   ```
3. **GREEN（実装）:** `net_gateway.rs` に `HttpTransport` トレイト（fake 可）+ 1 MiB streaming cap 読取 +
   content-encoding/status/type 検査 + `tokio::select!` による cancel/deadline drop + char 安全切詰。
4. **完了条件:** cap 3 境界 + identity + status/type + cancel drop-probe + panic 安全 全 PASS。**→ コミット。**

---

### STEP 5.D: JSON 抽出（Bounded Extraction RED）

1. **RED（tests/knowledge_gateway.rs, 無網・fixture JSON）:**
   - **正常抽出:** 保存した Wikipedia 検索 API JSON fixture（`query.search[].title` /
     `...snippet`）から **title+snippet のみ**抽出。未知フィールドが落ちること。
   - **bounded:** `results` が `MAX_RESULTS_PER_QUERY=3` を超える配列で 4 個目到達時に早期停止/切詰（全件
     alloc 前）。title>256B / snippet>2048B は char 境界で切詰。
   - **敵対 JSON:** 深いネスト / 型 confusion / 巨大配列 / `searchmatch` markup を含む snippet /
     不可視文字 → panic せず typed 処理（markup/不可視の**除去は STEP 6 render で行う**ため、STEP 5 は
     生 title/snippet を byte-cap して返すに留め、**特殊トークン無効化は STEP 6**）。
2. **検証コマンド:**
   ```
   cargo test -p pkb-desktop --test knowledge_gateway extract -- --nocapture
   ```
3. **GREEN（実装）:** `WikipediaProvider::extract(&[u8]) -> Result<Vec<SearchResult{title,snippet}>, GatewayError>`
   を serde typed + bounded で実装。char 安全切詰。
4. **完了条件:** 抽出正常 + bounded + 敵対 JSON 非 panic 全 PASS。**→ コミット。**

---

### STEP 5.E: Egress Gateway 統合 + HARD STOP（End-to-End）

1. **RED（tests/knowledge_gateway.rs）:**
   - **E2E（無網・fake transport+resolver）:** `research_fetch(payload, dict_terms, k_spawn, &slot, &transport,
     &resolver, cancel, deadline)` が begin→verify_and_gate→transition_to_fetching→fetch_all→transition_to_ready を
     貫通し、fixture Wikipedia JSON から title+snippet を得ること。
   - **AND ゲート連動:** PII を含む query（辞書 hit）や nonce replay / dict drift / 不正 HMAC の各々で
     **fetch 前に abort し transport call-count == 0**（egress ゼロ）。slot が解放されること。
   - **（任意）実網 live test:** `#[ignore]` + `--ignored` かつ env `PKB_E0B_LIVE=1` の時のみ実 `ja.wikipedia.org`
     へ接続。既定 `cargo test` では**走らない**。CI/無網では skip。
2. **検証コマンド:**
   ```
   cargo test -p pkb-desktop --test knowledge_gateway              # 全 gateway テスト（無網）
   cargo test -p pkb-desktop --test knowledge_verifier             # STEP3 非回帰(14)
   cargo test -p pkb-desktop --test knowledge_fsm                  # STEP4 非回帰(9)
   cargo test -p pkb-desktop --doc knowledge                       # STEP4 compile_fail 非回帰
   python -m pytest tests/test_e0b_constitutional_guard.py -v      # 反転後ガードが GREEN
   cargo clippy -p pkb-desktop -- -D warnings                      # knowledge/ 配下警告ゼロ（既存 pre-existing は分離）
   #（実網 live は手動のみ: set PKB_E0B_LIVE=1 && cargo test ... -- --ignored live）
   ```
3. **完了条件:** 全 gateway テスト + STEP0/3/4 非回帰 GREEN + clippy(knowledge)クリーン + egress は
   検証通過時のみ発生。**→ HARD STOP。**

---

# セクション 3: コミットと停止規律 (Checkpoints & HARD STOP)

## 3.1 コミット規律

`git status --short` で目視。当該 STEP の対象のみ stage（`git add -A`/`.` 禁止）。**`.claude/` を stage しない。
push 禁止。** revert/reset/`checkout --` 禁止。STEP1-4 資産・golden・Python コア・既存 Rust を改変しない
（反転する `test_e0b_constitutional_guard.py` の 2 関数のみ例外）。

| STEP | stage 対象 | コミットメッセージ |
|---|---|---|
| 5.A | `Cargo.toml`, `tests/test_e0b_constitutional_guard.py`, `src/knowledge/{net_gateway.rs,mod.rs}` | `feat(e0b-step5): STEP 5.A — reqwest(rustls,stream) safe config + constitutional guard reversal` |
| 5.B | `src/knowledge/{dns_guard.rs,net_gateway.rs,mod.rs}`, `tests/knowledge_gateway.rs` | `feat(e0b-step5): STEP 5.B — deny-table SSRF guard + URL invariant + resolve/pin` |
| 5.C | `src/knowledge/net_gateway.rs`, `tests/knowledge_gateway.rs` | `feat(e0b-step5): STEP 5.C — 1MiB bounded stream + identity + tokio::select cancel/deadline drop` |
| 5.D | `src/knowledge/net_gateway.rs`, `tests/knowledge_gateway.rs` | `feat(e0b-step5): STEP 5.D — bounded Wikipedia title/snippet extraction` |
| 5.E | `src/knowledge/net_gateway.rs`, `tests/knowledge_gateway.rs` | `feat(e0b-step5): STEP 5.E — research_fetch E2E wiring (FSM ∧ verify gate ∧ gateway)` |

各コミット末尾:
```
Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
```
（STEP 5.A で反転テストと最小 net_gateway.rs を同一コミットに含め常に緑を保つ。5.E は差分あればコミット、
コード差分無しの純検証なら空コミットを作らない。）

## 3.2 HARD STOP（絶対命令）

STEP 5.E で全テスト GREEN・コミット済みに到達したら、**出力を停止しユーザー（指揮官）へ報告し ACK を待て。**
報告に必ず含めるもの:
1. `cargo test --test knowledge_gateway` の生出力（dns/url/stream/cancel/extract/e2e の各件数と PASS）。
2. **feature 検証**: 既定ビルドで `cargo tree -f "{p} {f}" | grep reqwest` が `json,stream` のみ
   （rustls-tls は egress-live 時のみ、gzip/brotli/native-tls は**常に不在**）。既定 `cargo build`（egress-live なし）が
   **C toolchain 無しで成功**したこと。
3. STEP0 反転ガード GREEN / STEP3(14) / STEP4(9+doc) 非回帰。clippy(knowledge)クリーン。
4. `git log --oneline`（5.A–5.E コミット、対象ファイルのみ）。
5. **egress 封じ込めの明示:** `reqwest::` が **net_gateway.rs のみ**に存在し他ファイルゼロ（scoped ガードが担保）。
   SSRF（deny_table 全域 + resolve/pin）/ Decompression Bomb（identity + 1MiB cap + feature 不在）/ cancel drop /
   AND ゲート前段 abort（egress ゼロ）を、どのテストがどう証明したか。
6. **STEP 6 未着手の明言**（Python integrate / provenance 永続化 / render 特殊トークン無効化 / IPC 配線 /
   `invoke_handler` 登録 / `knowledge.research` Tauri command は未着手）。

**ACK なしに STEP 6 へ進むな。実 Wikipedia への live test を既定スイートへ入れるな。**

## 3.3 HARD STOP（異常時）

- **reqwest feature union に gzip/brotli/deflate/native-tls が混入:** 他 dep が有効化している。
  **勝手に無効化を試みず**（他 dep を壊す恐れ）、`cargo tree -e features -i <feature>` で発生源を特定し
  ブロッカーとして報告・停止（Decompression Bomb 防御の前提が崩れるため）。
- **deny_table が global を誤って禁止 / private を誤って許可:** テストを緩めず判定関数を直す。
  v4-mapped/NAT64 の埋込 v4 再判定を忘れていないか確認。
- **cancel で drop-probe が立たない（ゾンビ task）:** `spawn`/`spawn_blocking` で fetch を detach していないか。
  fetch は `tokio::select!` の arm が所有し、arm drop = future drop = socket close であること。
- **clippy が unwrap/panic/byte-slice を検出 / char 境界 panic:** `Result` 伝播・`char_indices` 切詰へ直す。
- **`reqwest::` が net_gateway.rs 以外に出現:** scoped ガードが RED。egress を net_gateway.rs へ集約し直す。

## 3.4 STEP 5 完了の定義 (DoD)

- reqwest が `default-features=false` + base `{stream}` で direct 導入され、TLS は `egress-live` feature 隔離。
  既定ビルドは toolchain 不要で緑・union に rustls-tls/解凍/native-tls 不在。production 実体は `#[cfg(egress-live)]`。
- 憲法ガード反転済み（safe-config 肯定＝base stream + egress-live は rustls のみ + `reqwest::` は net_gateway.rs
  スコープ限定、他 egress シンボルは全面禁止維持）。
- SSRF: deny_table 全域（v4-mapped/NAT64 含む）+ URL 固定不変条件 + resolve→deny→pin、非 global で接続ゼロ。
- Decompression/OOM: identity 強制 + `Content-Encoding` 非 identity 拒否 + 1 MiB streaming cap（chunk append 前打切）
  + bounded serde + char 安全切詰、全ボディ一括バッファ皆無。
- Async: `tokio::select!` で cancel/deadline 時に fetch future drop = socket close、drop-probe で証明、ゾンビ無し。
- 結線: research_fetch が begin→verify_and_gate→transition→fetch を貫通、検証/FSM 失敗時は fetch 前 abort（egress 0）。
- panic-free（knowledge/ clippy クリーン）、実網テストは `#[ignore]`+env のみ、STEP0/3/4 非回帰。
- STEP 6 未着手・ACK 待ちで停止。

---

# セクション 4: STEP 6 への申し送り（実装しない）

- 取得した title/snippet の **render 時特殊トークン無効化・不可視/markup 除去・隔離 namespace 永続化**
  （v3 §8）は Python 側 STEP 6（`knowledge.integrate` / `render_external_evidence`）。STEP 5 は生 title/snippet を
  byte-cap して返すに留める。
- `knowledge.research` Tauri command / `invoke_handler` 登録 / NetworkPolicy（既定 OFF・明示同意）/ IPC 往復
  （Rust→Python intent.build / integrate）は STEP 6 以降。**STEP 5 で WebView へ晒すな。**
- 実網 live test の常時実行・CI 組込みは行わない（無網規律）。
