# SPEC — E0b STEP 3: Rust Dual-Run Verifier Core (Composer-Executable Blueprint)

> **読者:** 自律型コーディング AI（Cursor Composer 等）。人間向け解説ではない。**一字一句従え。裁量なし。**
> **親仕様:** `docs/SPEC_E0B_KNOWLEDGE_GATEWAY_v3.md` §3–§4 /
> `docs/SPEC_E0B_STEP1_ATTESTATION.md` / `docs/SPEC_E0B_STEP2_ATTESTATION.md`（承認済み）。
> **前提:** STEP 0（憲法ガード）+ STEP 1（`canonicalize_for_match`/`PIISnapshot`）+ STEP 2
> （framing+HMAC+All-or-Nothing）はロック済み。越境 golden は commit 済み・アーキテクト検証済み。
> **本 STEP の目的:** Python が送る署名付き Intent を **Rust が 1 byte の狂いもなく再計算・独立再検査**する
> Dual-Run 検証コア（ネットワーク直前まで）を作る。**egress クライアントはまだ追加しない**（STEP 3 は検証のみ）。
> **ACK 規律:** STEP 3.D 完了時に HARD STOP。指揮官 ACK まで STEP 4（Txn FSM）へ進むな。

---

## 0. 何を作るか（Rust `knowledge` モジュール）

新規 Rust モジュール **`apps/desktop/src-tauri/src/knowledge/`**（`lib.rs` へ `pub mod knowledge;` を 1 行追加）。
Python 3 資産の **鏡像実装**（byte-exact）+ 新規 Dual-Run ゲート:

```text
knowledge::canonicalize::canonicalize_for_match(&str) -> String     # STEP 1 の鏡像
knowledge::pii_snapshot::snapshot_preimage(&[String], u64) -> Vec<u8>
knowledge::pii_snapshot::snapshot_hash_hex(&[String], u64) -> String # STEP 1 の鏡像
knowledge::attestation::attestation_framing(...) -> Vec<u8>          # STEP 2 の鏡像
knowledge::attestation::verify_tag(k_spawn, framing, received_tag) -> Result<(), VerifyError>  # 定数時間
knowledge::dual_run::verify_and_gate(payload, dict_terms, k_spawn) -> Result<(), VerifyError>  # AND ゲート
```

> **★egress 可否は本 Rust 再検査が単独で決める（v3 監査契約 A1）。** Python 側の preflight/HMAC の正しさは
> この保証の前提ではない。したがって STEP 3.C のテストは「Python を完全にバイパスした入力」に対しても
> Rust が PII を弾くこと・HMAC 不一致を弾くことを証明する。

---

# セクション 1: アーキテクチャと制約 (Constraints & Cargo Rules)

## 1.1 データフロー（本 STEP が検証する対象）

```text
受信 payload {session_id, txn_nonce, sidecar_generation, policy_epoch, dict_hash, queries[], attestation}
  → verify_and_gate:
      (A) attestation_framing を再構築 → HMAC-SHA256 再計算 → subtle::ct_eq で received tag と定数時間比較
          不一致 → Err(AttestationMismatch)
      (B) 各 query を canonicalize_for_match → dict_terms（canonical）と aho-corasick 独立照合
          hit → Err(PiiRejected)
      (A) ∧ (B) 成功のみ Ok(())  ← ここを通った時だけ将来 egress が許される（STEP 5+）
```

## 1.2 Cargo 依存（バージョン固定・既存生成に整合）

`apps/desktop/src-tauri/Cargo.toml` の `[dependencies]` へ**厳密ピン（`=`）**で追加:

```toml
# 既存を再利用: sha2 = "=0.11.0"（digest 0.11.3 世代）。★sha2 のバージョンを絶対に変えるな。
hmac = "=0.13.0"                 # digest 0.11 世代。sha2 0.11 の Sha256 と Hmac::<Sha256> が型整合する
unicode-normalization = "=0.1.24"
caseless = "=0.2.1"              # Unicode 完全ケースフォールド（Python str.casefold() 相当）
aho-corasick = "=1.1.3"          # Dual-Run の線形・非 backtrack 照合（最大 10 万語対応）
hex = "=0.4.3"
subtle = "=2.6.1"                # 定数時間比較
```

- **依存整合の必須確認（STEP 3.0）:** 追加後 `cargo tree -d` を実行し、`digest` が `0.10` と `0.11` の
  **既存 2 世代のまま**（3 世代目が増えない）こと、`hmac` が `digest 0.11.x` を使うことを確認せよ。
  もし `hmac 0.13.0` が sha2 0.11 と型整合しない（コンパイルエラー）場合は、**sha2 を降格せず**
  （`artifact_auth.rs` が依存）、digest 0.11 世代と組める hmac の patch を `cargo` で特定し、
  それでも解決不能なら**ブロッカーとして報告し停止**せよ。
- **★egress クレート追加禁止:** `reqwest`/`hyper`/`isahc`/`ureq`/`curl`/`surf` を追加すると STEP 0 の
  `test_rust_has_no_http_client_dependency` が RED になる。STEP 3 はネットワーク直前までで、クライアントは
  後続 STEP。追加禁止。

## 1.3 影響範囲 (Blast Radius)

**新規作成:**
```text
apps/desktop/src-tauri/src/knowledge/mod.rs           # pub use 再輸出 + VerifyError enum + no-panic lint
apps/desktop/src-tauri/src/knowledge/canonicalize.rs
apps/desktop/src-tauri/src/knowledge/pii_snapshot.rs
apps/desktop/src-tauri/src/knowledge/attestation.rs
apps/desktop/src-tauri/src/knowledge/dual_run.rs
apps/desktop/src-tauri/tests/knowledge_verifier.rs    # 越境アンカー統合テスト（既存 tests/ 契約方式に準拠）
```

**改変を許可（限定）:**
```text
apps/desktop/src-tauri/src/lib.rs        # `#[doc(hidden)] pub mod knowledge;` を mod 宣言群へ 1 行追加のみ
apps/desktop/src-tauri/Cargo.toml        # §1.2 の依存追加のみ（既存行の改変・削除禁止）
tests/test_e0b_constitutional_guard.py   # 触らない（後述の再実行で緑を確認するだけ）
```

**触れてはならない:**
```text
apps/desktop/src-tauri/src/{engine,ipc_contract,os_sandbox,commands,paths,webview_policy,artifact_auth}.rs
src/python/**                             # Python 資産は STEP 1/2 で確定・改変禁止（golden は読み取り専用の正本）
tests/golden/**                           # 越境 golden は凍結。Rust はここから値を写すだけ・改変禁止
上記「新規/限定改変」以外の全ファイル
```

`invoke_handler` へ **何も登録しない**（`knowledge` の関数を Tauri command に晒さない。STEP 5+ の orchestrator 経由のみ）。

## 1.4 言語間差異トラップ（Python↔Rust）— これを外すと byte-exact が壊れる

1. **`String::len()` は UTF-8 バイト長。** クエリ長前置は `(q.len() as u64).to_be_bytes()` で正しい
   （Python の `len(q.encode())` と一致）。**ただし `String::chars().count()`（スカラー数）を長さに使うな。**
2. **同じ hex 文字列でも用途で扱いが真逆**（STEP 2 と同一トラップ・Rust 版）:
   - `K_spawn`(64hex) は HMAC の**鍵** → `hex::decode(k_spawn)?` で **32 raw bytes**。`k_spawn.as_bytes()`（64B）は誤り。
   - `session_id`/`txn_nonce`/`dict_hash` は**メッセージ**の一部 → `.as_bytes()`（ASCII hex bytes）。
     `hex::decode()`（半分の raw bytes）は誤り。
3. **エンディアン固定。** u64 は `(n as u64).to_be_bytes()`。native/`to_ne_bytes`/`to_le_bytes` 禁止。
4. **サロゲート:** Rust `&str` は lone surrogate を構造的に持てない。payload は serde で受ける際、不正 UTF-8 /
   lone surrogate を含む JSON は**デシリアライズが Err**（panic ではない）になるよう境界を組め。Python が
   surrogate を reject したのと parity が自然に成立する（Rust では表現不能なため）。**境界で unwrap して panic
   させるな。**
5. **UTF-8 境界 panic を絶対に起こすな（最重要）:**
   - `&s[a..b]`（バイトスライス）を untrusted str に使うな（char 境界外で panic）。`clippy::string_slice` で禁止。
   - インデックス `v[i]` を使うな（`clippy::indexing_slicing` で禁止）。`.get(i)` を使う。
   - canonicalize・照合・除去はすべて `.chars()`（scalar 単位）で行う。コードポイント判定は `c as u32`。
   - `hex::decode` / `String::from_utf8` / `Hmac::new_from_slice` は `Result` を返す。**production 経路で
     `.unwrap()`/`.expect()`/`panic!` を使うな**（`#[test]` の既知固定値のみ unwrap 可）。
6. **NFKC/casefold の Unicode 版差:** Rust の `unicode-normalization`/`caseless` の Unicode 版が Python 3.12
   （Unicode 15.x）と一致しないと golden が落ちる。**golden 不一致は「実装バグ or Unicode 版ずれ」の実発見**
   であり、テストや golden を書き換えて緑にするのは厳禁。§3.3 の HARD STOP で報告せよ。
7. **`==` でタグ比較しない。** 必ず `subtle::ConstantTimeEq::ct_eq`。§2.B で強制。

---

# セクション 2: 実行フェーズ計画 (TDD Loops for Rust)

各 STEP は **RED → 検証コマンド → GREEN → 完了条件**。
**越境アンカー（golden/KAT）は commit 済み・アーキテクト検証済みの正本ファイルから値を写せ。Rust で再生成するな。**
検証コマンドは Tauri 依存のため初回ビルドが重い。フィルタで knowledge に絞れ。

正本ファイル:
```text
tests/golden/e0b_canonicalize_golden.jsonl   # 15 件: input_hex → expected_hex（STEP 1）
tests/golden/e0b_pii_snapshot_golden.json     # 6 件: terms,revision → preimage_hex,hash_hex（STEP 1）
tests/golden/e0b_attestation_kat.json         # 3 件: …,queries → framing_hex,tag_hex（STEP 2）
```

---

### STEP 3.A: Rust 厳格正規化 + PII スナップショット鏡像（Canonicalization RED）

**`canonicalize_for_match` 確定アルゴリズム（STEP 1 と 1 手も違えるな）:**
```text
1. REMOVE_SET のコードポイントを除去（下記・STEP1 と同一の明示集合。unicode カテゴリ判定禁止）。
2. NFKC 正規化（unicode-normalization の nfkc）。
3. 完全ケースフォールド（caseless、Python str.casefold() 相当）。
4. NFKC 正規化（冪等ブラケット）。
5. WHITESPACE_SET を ' '(U+0020) へ写像 → 連続空白を 1 個へ畳む → 先頭末尾 trim。
すべて .chars() 走査。バイトスライス禁止。

REMOVE_SET = { 0x00..=0x08, 0x0E..=0x1F, 0x7F..=0x9F,
  0x00AD,0x061C,0x180E, 0x200B,0x200C,0x200D,0x200E,0x200F,
  0x202A,0x202B,0x202C,0x202D,0x202E, 0x2060,0x2061,0x2062,0x2063,0x2064,
  0x2066,0x2067,0x2068,0x2069, 0xFEFF }
WHITESPACE_SET = { 0x09,0x0A,0x0B,0x0C,0x0D, 0x20, 0x2028,0x2029 }
```
**`snapshot_preimage`/`snapshot_hash_hex`（STEP 1 と同一）:**
```text
preimage = b"PKB-PII-DICT-V1" || (revision as u64).to_be_bytes()
        || for t in terms:  (t.len() as u64).to_be_bytes() || t.as_bytes()   # t.len()=UTF-8 バイト長
hash_hex = hex(sha2::Sha256(preimage))                                        # 64 lowercase hex
terms は呼び出し側で「canonical 済み・UTF-8 バイト順 sort・重複除去」であること（Rust 側で sort する場合は
Vec<String>::sort() = バイト順 = Python str sort と一致）。
```

1. **RED:** `tests/knowledge_verifier.rs` を作成し、`use pkb_desktop_lib::knowledge::...;`。
   - **canonicalize golden（15 件）:** `e0b_canonicalize_golden.jsonl` の各 `input_hex/expected_hex` を Rust の
     `const` 配列へ**写経**し、
     `hex::encode(canonicalize_for_match(&String::from_utf8(hex::decode(input_hex)?)?).as_bytes()) == expected_hex`
     を assert。必須スポット確認（値は golden から写す）: `zwsp_evasion`, `dotted_I`(→`69cc87`),
     `combining_nfc`(→`c3a9`), `fullwidth`(→`617473756b69`), `tab_evasion`(→`6a6f686e20736d697468`),
     `halfwidth_kana`(→`e382a2e382a4e382a6`), `empty_after`(→空文字)。
   - **snapshot golden（6 件）:** `e0b_pii_snapshot_golden.json` の各 `terms/revision` について
     `hex::encode(snapshot_preimage(&terms, rev)) == preimage_hex` かつ `snapshot_hash_hex(&terms, rev) == hash_hex`。
     クロスチェック用ハッシュ: `ab_c=1fe182bd…94c2`, `a_bc=95c06961…b630`, `cjk=1fdaaaeb…c2eb`,
     `emoji=6988184a…9d44`, `rev2=08c61fc9…60f1`, `empty=f3b8fd0c…34e0`（全 64 hex は json 正本）。
   - **単射性/バイト長:** `ab_c` と `a_bc` の hash が異なること。`cjk`(日=3B)/`emoji`(😀=4B) が
     `t.len()` のバイト長で framing されていること（スカラー長 1 でないこと）。
   - **panic 安全:** `canonicalize_for_match("")` が panic せず空文字を返す。多バイト境界（"日😀é"）で panic しない。
   - モジュール未実装 → **コンパイルエラーで RED**。
2. **検証コマンド:**
   ```
   cargo test -p pkb-desktop --test knowledge_verifier canonical -- --nocapture
   cargo test -p pkb-desktop --test knowledge_verifier snapshot -- --nocapture
   ```
3. **GREEN（実装）:** `src/knowledge/canonicalize.rs` と `pii_snapshot.rs` を実装。`mod.rs` で
   `pub mod canonicalize; pub mod pii_snapshot;` と再輸出。`lib.rs` に `#[doc(hidden)] pub mod knowledge;` を追加。
   Cargo.toml に §1.2 の依存を追加。`.chars()` 走査・バイトスライス不使用。
4. **完了条件:** canonicalize 15 + snapshot 6 が byte-exact PASS。**→ コミット。**

---

### STEP 3.B: HMAC フレーミングと KAT 検証（Cryptographic Verifier RED）

**`attestation_framing`（STEP 2 §1.4 と同一・KAT が強制）:**
```text
framing = session_id.as_bytes() || txn_nonce.as_bytes()
       || (sidecar_generation as u64).to_be_bytes() || (policy_epoch as u64).to_be_bytes()
       || dict_hash.as_bytes()
       || for q in queries: (q.len() as u64).to_be_bytes() || q.as_bytes()
session_id/txn_nonce/dict_hash は**固定長検証**（32/64/64 lowercase hex）を必須（単射性の不変条件）。
```
**`verify_tag`（定数時間・commander 必須要件）:**
```text
key = hex::decode(k_spawn)   → 32 bytes（不正 hex は Err）
computed = HMAC-SHA256(key, framing)                    # hmac::Hmac<sha2::Sha256>
received = hex::decode(received_tag_hex)  → 32 bytes
if computed_bytes.ct_eq(&received_bytes).into() { Ok(()) } else { Err(AttestationMismatch) }
★ `==`・`!=`・`computed_hex == received_hex` を使うな。必ず subtle::ConstantTimeEq::ct_eq を通す。
（hmac の Mac::verify_slice も定数時間だが、本 STEP は ct_eq 経路を明示的にテストする。）
```

共通コンテキスト（全 KAT・`e0b_attestation_kat.json` 正本）:
```
K_spawn    = 000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f
session_id = 0123456789abcdeffedcba9876543210
txn_nonce  = 00112233445566778899aabbccddeeffffeeddccbbaa99887766554433221100
dict_hash  = f3b8fd0c8070d2127fd0b3daaacd12c25ab5e819cd3c7d17d11cd9fa632d34e0
```
| KAT | gen | epoch | queries | tag_hex（クロスチェック） |
|---|---|---|---|---|
| KAT-1 | 7 | 3 | `["ai career","日本の就職"]` | `6c4390f16549039ca60fca51104a4a6c204252d6ce0583d4237eae27c328d9fa` |
| KAT-2 | 1 | 1 | `["python async"]` | `c36b1fe4c46564af2dfd5310c771b5bf95eb81daff071a2e8b9bb87aac284861` |
| KAT-3 | 7 | 3 | `["😀 test"]` | `dec75bd5da278801c3d75074e329a8349c483263f799519164f5dc17dd9ccee8` |

1. **RED:** `tests/knowledge_verifier.rs` に追加（framing_hex の全長は json 正本から写す）:
   - `hex::encode(attestation_framing(...)) == framing_hex`（3 KAT・バイト列一致）。
   - `verify_tag(K, framing, tag_hex) == Ok(())`（3 KAT・正当タグ受理）。
   - **改竄検出:** tag の末尾 1 文字を変える / gen を 7→8 / query 1 文字変更 →
     `verify_tag(...) == Err(AttestationMismatch)`。
   - **定数時間の静的契約:** `dual_run.rs`/`attestation.rs` に `==` によるタグ比較が無いこと
     （grep 相当を `#[test]` で: 該当ソースを `include_str!` し `.contains(" == ")` 近傍にタグ比較が無い、
     もしくはレビュー観点として明記。最低限、実装は ct_eq のみを通す）。
   - **fail-closed:** 不正 K_spawn（63/65 桁・非 hex）で `verify_tag` が `Err`（panic せず）。
2. **検証コマンド:**
   ```
   cargo test -p pkb-desktop --test knowledge_verifier kat -- --nocapture
   ```
3. **GREEN（実装）:** `src/knowledge/attestation.rs` に `attestation_framing` と `verify_tag`（ct_eq）を実装。
   固定長 hex 検証を含める。`mod.rs` に `VerifyError { MalformedField, AttestationMismatch, PiiRejected }`。
4. **完了条件:** 3 KAT の framing byte-exact + tag 定数時間検証 PASS、改竄で reject。**→ コミット。**

---

### STEP 3.C: 統合検証ゲート（Dual-Run AND Gate RED）

```text
verify_and_gate(payload, dict_terms: &[String], k_spawn: &str) -> Result<(), VerifyError>:
  (A) framing = attestation_framing(payload…);  verify_tag(k_spawn, framing, payload.attestation)?  // HMAC
  (B) 独立 PII 再検査: aho-corasick を dict_terms（canonical 済み）から構築。
      for q in payload.queries:  let c = canonicalize_for_match(q);
        if automaton.is_match(&c) { return Err(PiiRejected); }
  Ok(())   // (A) ∧ (B) の両成功のみ
payload は serde Deserialize + #[serde(deny_unknown_fields)]。gen/epoch は u64。
dict_terms は STEP 4/5 で PIISnapshot からロードする（本 STEP では test が注入。ファイル I/O は書かない）。
```

1. **RED:** `tests/knowledge_verifier.rs` に追加。
   - **両成功 → Ok:** KAT-1 の payload（正当 tag）+ PII を含まない `dict_terms=["himitsu"]` → `Ok(())`。
   - **PII あり → Err(PiiRejected)（HMAC 正当でも）:** dict_terms に `canonicalize_for_match("Himitsu")`=`"himitsu"` を
     置き、queries に `"himitsu project"` を含む payload を**正しく署名**して渡す
     （tag は §2.B の verify_tag が通る値）。それでも `Err(PiiRejected)` になること（AND ゲートの PII 側）。
   - **★ゼロ幅回避を Rust が独自に閉じる:** query に `"himi\u{200B}tsu"`（ZWSP 挿入）を入れると、
     canonicalize_for_match が `"himitsu"` に畳んで dict hit → `Err(PiiRejected)`。
     （E0a の NFKC+casefold では素通りし得るものを、Rust の strict 再検査が捕捉する Dual-Run の核心。）
   - **HMAC 不正 → Err(AttestationMismatch)（PII 無しでも）:** tag を 1 文字改竄した payload + PII 無し dict_terms →
     `Err(AttestationMismatch)`。
   - **順序/独立性:** どちらの失敗でも `Ok` を返さない。両失敗時も `Err`(いずれか固定順) を返す。
   - **panic 安全:** 空 queries・多バイト query・巨大 dict_terms でも panic しない。serde で unknown field 付き
     JSON をデシリアライズすると `Err`（panic せず）。
2. **検証コマンド:**
   ```
   cargo test -p pkb-desktop --test knowledge_verifier gate -- --nocapture
   ```
3. **GREEN（実装）:** `src/knowledge/dual_run.rs` に payload 型 + `verify_and_gate` を実装。aho-corasick は
   `AhoCorasick::new(dict_terms)`（構築失敗は Err）→ `.is_match(&canonical_query)`。AND を厳守（片方でも Err で return）。
4. **完了条件:** AND セマンティクス全証明（PII 単独 reject / HMAC 単独 reject / ゼロ幅回避 reject / 両成功 Ok）。**→ コミット。**

---

### STEP 3.D: 全回帰 + no-panic lint + STEP 0 非回帰 + HARD STOP

1. **検証コマンド:**
   ```
   cargo test -p pkb-desktop --test knowledge_verifier -- --nocapture
   cargo clippy -p pkb-desktop -- -D warnings          # no-panic lint（下記 mod 属性）を強制
   python -m pytest tests/test_e0b_constitutional_guard.py -v   # Cargo/rs 変更後も Rust 無菌ロックが緑
   ```
   `knowledge/mod.rs` 冒頭に no-panic lint を宣言:
   ```rust
   #![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic,
           clippy::indexing_slicing, clippy::string_slice)]
   ```
   （`#[cfg(test)]` には `#![allow(clippy::unwrap_used)]` を付し、テストの既知固定値 unwrap を許容してよい。）
2. **完了条件:** knowledge 全テスト PASS + clippy 警告ゼロ + STEP 0 ガード 10/10 維持
   （`test_rust_has_no_http_client_dependency` / `test_rust_src_has_no_outbound_network_calls` 緑）。**→ HARD STOP。**

---

# セクション 3: コミットと停止規律 (Checkpoints & HARD STOP)

## 3.1 コミット規律

`git status --short` で目視。当該 STEP の対象のみ stage（`git add -A`/`.` 禁止）。**`.claude/` を stage しない。
push 禁止。** revert/reset/`checkout --` 禁止。既存コミット・既存 Cargo 行・golden・Python 資産を改変しない。

| STEP | stage 対象 | コミットメッセージ |
|---|---|---|
| 3.A | `Cargo.toml`, `src/lib.rs`, `src/knowledge/{mod,canonicalize,pii_snapshot}.rs`, `tests/knowledge_verifier.rs` | `feat(e0b-step3): STEP 3.A — Rust canonicalize_for_match + PIISnapshot mirror (STEP1 golden byte-exact)` |
| 3.B | `src/knowledge/{mod,attestation}.rs`, `tests/knowledge_verifier.rs` | `feat(e0b-step3): STEP 3.B — HMAC framing + constant-time (subtle) tag verify (STEP2 KAT)` |
| 3.C | `src/knowledge/{mod,dual_run}.rs`, `tests/knowledge_verifier.rs` | `feat(e0b-step3): STEP 3.C — dual-run AND gate (independent aho-corasick PII re-check)` |

各コミット末尾:
```
Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
```
（`git` パスは `apps/desktop/src-tauri/` 配下。commit はリポジトリルートから相対パスで stage する。
STEP 3.D はコード差分なしのため空コミットを作らない。）

## 3.2 HARD STOP（絶対命令）

STEP 3.D で全 Rust テスト GREEN・clippy クリーン・STEP 0 非回帰・3 コミット済みに到達したら、
**出力を停止しユーザー（指揮官）へ報告し ACK を待て。** 報告に必ず含めるもの:
1. `cargo test --test knowledge_verifier` の生出力（canonical/snapshot/kat/gate の各件数と PASS）。
2. `cargo clippy -- -D warnings` が警告ゼロである出力。
3. `python -m pytest tests/test_e0b_constitutional_guard.py` が **10 passed**（Rust 無菌ロック維持）。
4. `git log --oneline`（3.A/3.B/3.C の 3 コミット、対象ファイルのみ）。
5. 越境アンカー突合の明示: **canonicalize 15 / snapshot 6 / KAT 3 の全 hex が Python 側 golden と byte-exact 一致**。
6. **STEP 4 未着手の明言**（Txn FSM・nonce 消費・辞書 drift 検査・network クライアントは未着手）。

**ACK なしに STEP 4（Txn ステートマシン）へ進むな。**

## 3.3 HARD STOP（異常時）

- **golden/KAT 不一致:** テストや golden を実装へ合わせて書き換える（偽 GREEN）ことは**厳禁**。疑う順序:
  ① `hex::decode(K_spawn)` で鍵を decode したか（`as_bytes` していないか）② session/nonce/dict_hash を `.as_bytes()`
  したか（decode していないか）③ `to_be_bytes()` か ④ クエリ長が `q.len()`(UTF-8) か ⑤ NFKC→casefold→NFKC の順か
  ⑥ `unicode-normalization`/`caseless` の Unicode 版が Python 3.12 と一致するか。それでも解けなければ、
  **どの vector が期待 hex と実 hex でどう違うか**を提示して停止・裁定待ち（Unicode 版ずれは実発見）。
- **依存が組めない（hmac×sha2 0.11 の型不整合）:** sha2 を降格せず、ブロッカーとして報告・停止。
- **STEP 0 が RED:** Cargo に egress クレートが混入 or 新 .rs に network シンボル。除去して報告。
- **clippy が unwrap/panic/slice を検出:** production 経路を `.get()`/`Result` 伝播へ直す。テストは allow 可。

## 3.4 STEP 3 完了の定義 (DoD)

- `canonicalize_for_match` / `snapshot_*` が STEP 1 golden（15+6）を byte-exact 再現。
- `attestation_framing` が STEP 2 KAT（3）を byte-exact 再現、`verify_tag` が **subtle::ct_eq のみ**で検証・改竄拒否。
- `verify_and_gate` が AND ゲート（HMAC ∧ 独立 PII 再検査）を満たし、ゼロ幅回避を Rust が単独で捕捉。
- no-panic: clippy(unwrap/expect/panic/indexing/string_slice) 警告ゼロ、`.chars()` 走査・バイトスライス皆無。
- egress クレート/シンボルゼロ、STEP 0 ガード非回帰、golden/Python 資産・既存 Rust 差分ゼロ（新規 + 限定 2 ファイルのみ）。
- STEP 4 未着手・ACK 待ちで停止。

---

# セクション 4: v3 §4.1 との差分（STEP 2 から継承・実装は KAT で確定）

Rust framing は **STEP 2 の指揮官確定フレーミング**（session/nonce/dict_hash を固定長・非長さ前置で連結）を鏡像
実装し、KAT-1/2/3 がそれを強制する。v3 §4.1 のドメイン分離定数・`provider_id`・`n_queries` カウントは
本 STEP に含まれない（単射性は固定長検証 + クエリ長前置で保たれる）。v3 準拠へ寄せる判断は STEP 4 着手前の
別裁定とし、STEP 3 は KAT（＝Python STEP 2 が生成した実値）に byte-exact で一致させることを唯一の正とする。
