# SPEC — E0b STEP 4: Transaction State Machine (Typestate FSM) (Composer-Executable Blueprint)

> **読者:** 自律型コーディング AI（Cursor Composer 等）。人間向け解説ではない。**一字一句従え。裁量なし。**
> **親仕様:** `docs/SPEC_E0B_KNOWLEDGE_GATEWAY_v3.md` §5 /
> `docs/SPEC_E0B_STEP3_RUST_VERIFIER.md`（承認済み・フレーミング仕様は最終版として追認済み）。
> **前提:** STEP 0–3 ロック済み。STEP 3 で `knowledge::{canonicalize, pii_snapshot, attestation, dual_run}`
> と `AttestedIntentPayload` / `VerifyError` / `verify_and_gate` が確定・検証済み（14 tests GREEN）。
> **本 STEP の目的:** 外部通信の**前段**として、リクエストのライフサイクルを型で統べる典型状態機械（typestate FSM）
> と、nonce single-use / 辞書 revision drift / single-flight を実装する。**egress クライアントはまだ追加しない。**
> **ACK 規律:** STEP 4.D 完了時に HARD STOP。指揮官 ACK まで STEP 5（orchestrator / network）へ進むな。

---

## 0. 何を作るか

新規ファイル **`apps/desktop/src-tauri/src/knowledge/fsm.rs`**（`knowledge/mod.rs` へ `pub mod fsm;` を追加）。

```text
状態マーカー（ZST）: Pending / Fetching / ReadyToIntegrate / Completed   ← Clone/Copy 一切派生しない
Txn<S>            : typestate 値。所有権 move でのみ遷移。Clone/Copy なし。SlotGuard と payload を保持
  Txn<Pending>::transition_to_fetching(self) -> Txn<Fetching>          // self を消費
  Txn<Fetching>::transition_to_ready(self)   -> Txn<ReadyToIntegrate>
  Txn<ReadyToIntegrate>::transition_to_completed(self) -> Txn<Completed>
  Txn<S>::abort(self, reason)                -> AbortReason            // 消費して slot 解放（drop でも解放）
ResearchSlot      : 共有マネージャ（Arc で共有可）。単一 Mutex で {occupied, used_nonces, current_dict_hash}
  ResearchSlot::new(dict_hash) -> Self
  ResearchSlot::set_dictionary(&self, new_hash)                        // revision 更新
  ResearchSlot::begin(self: &Arc<Self>, payload) -> Result<Txn<Pending>, FsmError>
SlotGuard         : RAII。Drop で occupied=false（terminal / abort / 途中 drop すべてで slot 解放）
FsmError          : Busy | ReplayDetected | DictionaryDrift | MalformedNonce
```

> **★依存追加ゼロ。** STEP 3 で入れた `hex` と std（`Arc`/`Mutex`/`HashSet`/`PhantomData`）だけで完結する。
> `tokio::sync::Mutex` は不要（STEP 4 は同期・非 await。network を跨ぐ排他は STEP 5 で SlotGuard 方式のまま拡張）。

---

# セクション 1: アーキテクチャと制約 (Constraints & Safety Rules)

## 1.1 Typestate パターンの基本要件（所有権 move による消費）

- 遷移メソッドは **`self`（値）を受け取り**、新しい状態型を返す。`&self`/`&mut self` は使わない。
  → 遷移後、旧状態の変数は **move 済み**で再利用不能（借用チェッカが二重実行をコンパイル時に拒否）。
- 遷移メソッドは**その状態専用の impl ブロック**にのみ定義する。
  例: `transition_to_fetching` は `impl Txn<Pending>` にだけ書く。`Txn<Fetching>` には存在しない
  → `Txn<Fetching>` に対する再 Fetch は**「メソッドが無い」コンパイルエラー**になる（フェーズ逆行・二重実行の物理排除）。
- `Txn<S>` の `S` は `PhantomData<S>` として保持。状態マーカーは中身のない ZST struct。

## 1.2 複製・共有の禁止（commander 最重要規律）

- **`Txn<S>`・全状態マーカー・`SlotGuard` に `#[derive(Clone)]` / `Copy` を付けるな。** 状態の複製は FSM の崩壊。
  §2.A の `compile_fail` doc-test が「`Txn<Pending>: !Clone`」をコンパイル時に強制する。
- **`Arc<Mutex<Txn<S>>>` で FSM をバイパスするな。** 遷移は `self`（値）を要求するため、`Arc` 越しには
  そもそも遷移メソッドを呼べない（`Arc` から move-out できない）＝ Arc 包みは無意味。これを利用して典型状態を守る。
- **`Arc` を使ってよいのは共有マネージャ `ResearchSlot` のみ**（複数の begin 呼び出し元が 1 つの slot を参照するため）。
  マネージャの共有と**状態値 `Txn` の一意性は別物**。混同するな。
- **実行時パニックへ逃げるな。** `fsm.rs` 冒頭に no-panic lint を宣言し、`.unwrap()`/`.expect()`/`panic!`/
  添字/バイトスライスを禁止する（下記）。Mutex poison は `unwrap_or_else(PoisonError::into_inner)` で回収。

```rust
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic,
        clippy::indexing_slicing, clippy::string_slice)]
```

## 1.3 begin() のアトミック検査順（単一 Mutex・TOCTOU 排除）

`ResearchSlot::begin` は**単一の Mutex ロック内で**全チェックを行い、**成功時のみ occupied=true にする**
（早期 occupancy → 解放漏れの複雑さを構造的に消す）。順序:

```text
lock(inner):                                   # unwrap_or_else(PoisonError::into_inner)
  1. if inner.occupied            -> Err(Busy)            # single-flight
  2. if payload.dict_hash != inner.current_dict_hash -> Err(DictionaryDrift)   # revision lock
  3. nonce = parse_nonce(payload.txn_nonce) or Err(MalformedNonce)  # 64hex -> [u8;32], panic 不可
  4. if inner.used_nonces.contains(nonce) -> Err(ReplayDetected)    # anti-replay
  5. inner.used_nonces.insert(nonce); inner.occupied = true         # ここで初めて占有
unlock; Ok(Txn::<Pending>::new(SlotGuard{ slot: Arc::clone(self) }, payload.clone()))
```

- **nonce は成功時のみ消費**（drift/malformed では消費しない。同一 nonce は Rust 生成で一意のため実害なし）。
- **消費した nonce は terminal/abort 後も ledger に残す**（後続の再送を ReplayDetected で恒久拒否）。
- `SlotGuard::drop` は再 lock して `occupied=false`（terminal・abort・途中 drop すべてで解放）。nonce は消さない。

## 1.4 影響範囲 (Blast Radius)

**新規作成:**
```text
apps/desktop/src-tauri/src/knowledge/fsm.rs
apps/desktop/src-tauri/tests/knowledge_fsm.rs        # 実行時テスト（既存 tests/ 契約方式）
```
**改変を許可（限定）:**
```text
apps/desktop/src-tauri/src/knowledge/mod.rs          # `pub mod fsm;`（＋必要なら `pub use`）を追加するのみ
```
**触れてはならない:**
```text
apps/desktop/src-tauri/src/knowledge/{canonicalize,pii_snapshot,attestation,dual_run}.rs  # STEP1-3 確定
  （fsm.rs は dual_run の AttestedIntentPayload を import して使う＝改変ではない）
apps/desktop/src-tauri/{Cargo.toml,src/lib.rs}       # 依存追加ゼロ・mod 登録は mod.rs 側。触るな
src/python/** / tests/golden/**                        # 触るな
invoke_handler                                         # fsm を Tauri command に晒さない（STEP 5 orchestrator 経由のみ）
```

## 1.5 言語トラップ（panic 安全）

- `parse_nonce`: `hex::decode(nonce)` は `Result`（不正 hex は Err）。`<[u8;32]>::try_from(vec)` で長さ検証
  （31/33 byte は Err）。**`.unwrap()` するな。** すべて `Option`/`Result` で返す。
- 添字・バイトスライス禁止（`clippy::indexing_slicing`/`string_slice`）。HashSet/Vec 操作は panic しない API のみ。
- Mutex は std。`.lock()` の `PoisonError` は `unwrap_or_else(std::sync::PoisonError::into_inner)` で回収（panic せず）。
- `sidecar_generation`/`policy_epoch` は payload で `u64`。範囲 panic なし。

---

# セクション 2: 実行フェーズ計画 (TDD Loops for Rust FSM)

各 STEP は **RED → 検証コマンド → GREEN → 完了条件**。
検証は Tauri 依存でビルドが重い。`cargo test -p pkb-desktop fsm` でフィルタ。**doc-test も必ず実行される**
（`cargo test` 出力の `Doc-tests pkb_desktop_lib` セクションに `compile_fail` 件数が計上されること）。

---

### STEP 4.A: Typestate FSM（Compile-time Safety RED）

1. **RED:** `tests/knowledge_fsm.rs` と `fsm.rs` のドキュメントに以下を用意（モジュール未実装で RED）。
   - **正遷移の実行時テスト（tests/knowledge_fsm.rs）:** `ResearchSlot::new(dict_hash)` → `Arc` →
     `begin(payload)` で `Txn<Pending>` → `transition_to_fetching()` → `transition_to_ready()` →
     `transition_to_completed()` が成功し、**Completed 到達（または途中 drop）で slot が解放**され、
     次の `begin(別 nonce)` が成功すること。
   - **`compile_fail` doc-test（fsm.rs の doc コメント・依存ゼロ）を 4 本**。各々の直前に、
     **同一構成の "正常版" passing doc-test** を置き、「setup は正しくコンパイルし、違法操作**だけ**が落ちる」
     ことを担保せよ（compile_fail が別理由で成立する偽陽性を防ぐ）:
     1. **no-Clone:**
        ```compile_fail
        # use pkb_desktop_lib::knowledge::fsm::{Txn, Pending};
        fn needs_clone<T: Clone>() {}
        needs_clone::<Txn<Pending>>(); // ERROR: Txn<Pending>: !Clone
        ```
     2. **二重 Fetch（メソッド不在）:** `Txn<Fetching>` に対し `transition_to_fetching()` を呼ぶ → コンパイル不能。
     3. **逆行不能:** `Txn<Fetching>` から `Txn<Pending>` へ戻る手段が存在しない → コンパイル不能。
     4. **move 後再利用:** `let f = p.transition_to_fetching(); let _ = p.transition_to_fetching();`
        → `p` は move 済み → `use of moved value` でコンパイル不能。
     （doc-test 内の payload 生成は `unimplemented!()` 等でよい。`compile_fail` は**コンパイル**のみ判定し実行しない。）
2. **検証コマンド:**
   ```
   cargo test -p pkb-desktop --test knowledge_fsm -- --nocapture
   cargo test -p pkb-desktop --doc knowledge     # compile_fail doc-test（件数 > 0 を確認）
   ```
3. **GREEN（実装）:** `fsm.rs` に状態マーカー（ZST・Clone/Copy なし）、`Txn<S>`（Clone/Copy なし・
   `PhantomData<S>` + `SlotGuard` + `AttestedIntentPayload` 保持）、状態別 impl の遷移メソッド（`self` 消費）、
   `abort`、`SlotGuard`（Drop で解放）、`ResearchSlot`（骨格）を実装。`mod.rs` に `pub mod fsm;`。
4. **完了条件:** 正遷移 runtime PASS + `compile_fail` 4 本が**期待どおりコンパイル失敗**（doc-test GREEN）
   + 正常版 passing doc-test が成功。**→ コミット。**

---

### STEP 4.B: Nonce 消費と Replay 拒否（Anti-Replay RED）

1. **RED（tests/knowledge_fsm.rs）:**
   - **single-use:** `begin(nonce=N1)` → `Txn<Pending>`。terminal まで進めて slot 解放後、
     **同じ N1 で `begin` → `Err(FsmError::ReplayDetected)`**。別 N2 → `Ok`。
   - **abort 後も消費維持:** `begin(N1)` の `Txn` を**途中 drop（abort）**した後、`begin(N1)` → `ReplayDetected`
     （nonce は成功時に消費され、abort でも ledger に残る）。
   - **malformed nonce:** `txn_nonce` が 63/65 桁・非 hex → `Err(FsmError::MalformedNonce)`。かつ**その後
     slot は占有されていない**（valid な begin が続けて成功する）＝ 失敗時に occupied を立てていないこと。
2. **検証コマンド:**
   ```
   cargo test -p pkb-desktop --test knowledge_fsm replay -- --nocapture
   ```
3. **GREEN（実装）:** `ResearchSlot` の `used_nonces: HashSet<[u8;32]>` と §1.3 の begin ロジック、
   `parse_nonce`（`hex::decode` + `try_into`、panic 不可）を実装。
4. **完了条件:** replay 系全 PASS。**→ コミット。**

---

### STEP 4.C: 辞書 Drift 検査（Revision Lock RED）

1. **RED（tests/knowledge_fsm.rs）:**
   - `ResearchSlot::new(hashA)`。`begin(payload{dict_hash: hashA})` → `Ok`。
   - `begin(payload{dict_hash: hashB}, hashB != hashA)` → `Err(FsmError::DictionaryDrift)`。かつ**その後
     slot 未占有**（drift 失敗で occupied を立てていない）。かつ**N は消費されていない**
     （drift された nonce を、正しい辞書で後から使えることを確認）。
   - `set_dictionary(hashB)` 後、`begin(payload{dict_hash: hashB, 新 nonce})` → `Ok`。
   - **drift が single-flight/replay より安全側:** 占有中に drift payload が来ても `Busy` か `Drift` の
     いずれかで必ず `Err`（`Ok` を返さない）。
2. **検証コマンド:**
   ```
   cargo test -p pkb-desktop --test knowledge_fsm drift -- --nocapture
   ```
3. **GREEN（実装）:** `current_dict_hash: String` と `set_dictionary`、begin の drift 比較（§1.3 手順 2）を実装。
4. **完了条件:** drift 系 + single-flight 排他（占有中 begin → `Busy`、drop 後 → `Ok`）全 PASS。**→ コミット。**

---

### STEP 4.D: 全回帰 + no-panic + 非回帰 + HARD STOP

1. **検証コマンド:**
   ```
   cargo test -p pkb-desktop fsm -- --nocapture
   cargo test -p pkb-desktop --test knowledge_verifier      # STEP 3 非回帰（14 passed 維持）
   python -m pytest tests/test_e0b_constitutional_guard.py -v   # STEP 0 非回帰（10 passed）
   ```
   （`fsm.rs` の no-panic は `#![deny(clippy::…)]` により clippy で強制。既存ファイルの pre-existing lint とは
   分離して `knowledge/` 配下が警告ゼロであることを確認。grep で `fsm.rs` に `.unwrap()`/`panic!`/添字が無いこと。）
2. **完了条件:** fsm 全テスト（runtime + compile_fail doc）PASS + STEP 3/0 非回帰 + `knowledge/` panic-free。**→ HARD STOP。**

---

# セクション 3: コミットと停止規律 (Checkpoints & HARD STOP)

## 3.1 コミット規律

`git status --short` で目視。当該 STEP の対象のみ stage（`git add -A`/`.` 禁止）。**`.claude/` を stage しない。
push 禁止。** revert/reset/`checkout --` 禁止。既存コミット・STEP1-3 資産・golden・Python・Cargo.toml・lib.rs を改変しない。

| STEP | stage 対象 | コミットメッセージ |
|---|---|---|
| 4.A | `src/knowledge/fsm.rs`, `src/knowledge/mod.rs`, `tests/knowledge_fsm.rs` | `feat(e0b-step4): STEP 4.A — typestate Txn FSM (move-consuming transitions, compile_fail proofs)` |
| 4.B | `src/knowledge/fsm.rs`, `tests/knowledge_fsm.rs` | `feat(e0b-step4): STEP 4.B — single-use nonce ledger (ReplayDetected on reuse)` |
| 4.C | `src/knowledge/fsm.rs`, `tests/knowledge_fsm.rs` | `feat(e0b-step4): STEP 4.C — dict revision-drift lock + single-flight slot exclusion` |

各コミット末尾:
```
Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
```
（STEP 4.A で `fsm.rs` を作成した時点でコンパイルが通る最小骨格＋テストを含め、常に緑を保つ。
STEP 4.D はコード差分なしのため空コミットを作らない。）

## 3.2 HARD STOP（絶対命令）

STEP 4.D で全テスト GREEN・3 コミット済みに到達したら、**出力を停止しユーザー（指揮官）へ報告し ACK を待て。**
報告に必ず含めるもの:
1. `cargo test --test knowledge_fsm` の生出力（transition/replay/drift の各件数と PASS）。
2. `cargo test --doc` の `Doc-tests` セクション出力（`compile_fail` が **件数 > 0 で PASS**＝違法遷移が
   コンパイル不能であることの証明）。
3. STEP 3 `knowledge_verifier` **14 passed** / STEP 0 憲法ガード **10 passed**（非回帰）。
4. `git log --oneline`（4.A/4.B/4.C の 3 コミット、対象ファイルのみ）。
5. **FSM 不変条件の明示:** typestate（複製不能・逆行/二重実行はコンパイル不能）/ nonce single-use /
   dict drift abort / single-flight を、どのテストがどう証明したか。
6. **STEP 5 未着手の明言**（orchestrator・network クライアント・IPC 配線は未着手・`invoke_handler` 登録ゼロ）。

**ACK なしに STEP 5（orchestrator / egress）へ進むな。**

## 3.3 HARD STOP（異常時）

- **`compile_fail` が別理由で成立（偽陽性）:** 対応する "正常版" passing doc-test を先に緑にし、setup が
  正しくコンパイルすることを確認してから compile_fail を評価。setup が原因で落ちているなら fixture を直す。
- **`compile_fail` が実は**コンパイル**通ってしまう（＝ FSM が緩い）:** `Txn` に Clone が付いた / 遷移メソッドが
  全状態に実装された等。**テストを緩めず**、typestate 設計（状態別 impl・self 消費・Clone 非派生）を直す。
- **runtime パニック / clippy が unwrap 検出:** `Result`/`Option` 伝播・`PoisonError::into_inner` へ直す。テストは allow 可。
- **`Arc<Mutex<Txn>>` を使いたくなった:** 設計誤り。遷移は `self` 値消費。共有が要るのは `ResearchSlot` のみ。停止して報告。

## 3.4 STEP 4 完了の定義 (DoD)

- `Txn<S>` が move 消費遷移のみを持ち、Clone/Copy 非派生。逆行・二重 Fetch・move 後再利用・複製が
  すべて `compile_fail` で**コンパイル不能**であることを doc-test が証明。
- `ResearchSlot::begin` が単一 Mutex 内で single-flight（Busy）/ drift（DictionaryDrift）/ malformed nonce /
  replay（ReplayDetected）を判定し、**成功時のみ占有**、`SlotGuard` drop で確実に解放。
- nonce は成功時に single-use 消費、abort/terminal 後も ledger に残り再送を恒久拒否。
- `knowledge/` panic-free（unwrap/expect/panic/添字/バイトスライス皆無、Mutex poison 回収）。
- 依存追加ゼロ・`invoke_handler` 登録ゼロ・STEP 0/3 非回帰・既存資産差分ゼロ（新規 + mod.rs 1 行のみ）。
- STEP 5 未着手・ACK 待ちで停止。

---

# セクション 4: STEP 5 への申し送り（実装しない・設計メモ）

本 STEP は制御フローの骨組みだけを固める。次段（ACK 後）で:
- `Txn<Pending>::transition_to_fetching` の**直前に** STEP 3 `verify_and_gate(payload, dict_terms, k_spawn)` を
  orchestrator が呼び、HMAC ∧ Dual-Run PII の AND を通過した payload だけを Fetching へ進める
  （FSM と検証コアの結線。STEP 4 では両者を分離したまま）。
- network 跨ぎの排他は現行 `SlotGuard`（occupied フラグ）方式のまま維持（Mutex guard を await 跨ぎで保持しない）。
  `tokio::select!` による cancel/deadline は STEP 5 で fetch フェーズにのみ導入（v3 §6/§9）。
- egress クライアント（reqwest 等）は STEP 5 以降。STEP 4 で追加すると STEP 0 ガードが RED。
