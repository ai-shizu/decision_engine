# T4-E-1 / T4-E-2 完遂報告 — 到達可能性の棚卸しと計器の陽性対照

- **相位**: T4-E-1 + T4-E-2（設計案 `docs/T4E_ARENA_DEAD_REASON_PLAN.md` / e81756a）
- **枝**: `feature/t4e-arena-dead-reason`（作業ツリー・未コミット。C-7）
- **基線 HEAD（着手時）**: `e81756a36a3e22707d88d4525c542f092023cf0b`
- **base**: `a2f4b5c55ae81695b847a694b6f9188e49870108`
- **採取時刻 (UTC)**: `2026-08-04T13:24:35Z`
- **証跡ディレクトリ**: `/tmp/t4e-evidence`

---

## H-1 — 4 コード到達経路表

`die()` 本番サイト（`rg 'die\(FailureReason'` → `die_sites.txt`）を起点に、呼び出し元と発火条件を辿った。

| コード | `FailureReason` | 本番 `die()` サイト | 呼び出し連鎖 | 引き金 | 到達判定 |
|---|---|---|---|---|---|
| 1 | `AccountingBreach` | `director.rs:665`（`settle` → `close_period` の `SettleError::AccountingBreach` 腕） | `Session::settle` → `close_period` → `CashFlowStatement::verify` | CF 純増減 ≠ Cash 増減 | **発火不能（構造）**。`close_period` は `cash_flow.verify` の前に必ず `apply_plans` を呼び、`apply_plans` は `verify_zero_sum` で非零和帳簿を `Compile(Ledger(Unbalanced))` にする。そのエラーは隣の腕で **code 4** になる。零和を保ったまま CF が壊れることは `section_of` の分割定理（`settle.rs` 先頭）により起きない。実測: `unbalanced_books_die_as_internal_invariant_not_accounting_breach` が poison → code 4 を確認（`host_test_companion.log` SHA `245c83aa62e6ed7bbf0284d72a702110419ac74cb54139ece610f1ea8c764e43`） |
| 2 | `SnapshotDigestMismatch` | `director.rs:676`（`settle` → `capture` 直後の `generations.verify`） | `Session::settle` → `GenerationStore::capture` → `GenerationStore::verify(same id)` | `payload_binding ≠ payload_digest` | **発火不能（構造）**。`capture` は同一 `bytes` から `payload` と `payload_digest` を同時に書き、直後の `verify` は恒真。`generations.verify` の本番呼び出しはこの 1 箇所のみ（`rg` 実測） |
| 3 | `ReplayDivergence` | **ABSENT**（`die(FailureReason::ReplayDivergence)` の本番呼び出し 0） | — | — | **発火不能**。出現は enum / コード表 / fsm テスト / `arena_terminal_codes` のみ（`replay_divergence_refs.txt`） |
| 4 | `InternalInvariantBroken` | `director.rs:590`（`execute`←`operate_tick`）/ `:668`（`settle` 非 AccountingBreach）/ `:692`（`report` で `pending_operations=None`） | 代表: `Session::execute` → `operate_tick` → `verify_inventory_reconciled` | ledger Inventory ≠ firm inventory（他: Overflow / Unbalanced / InventoryDesync at close） | **到達可能**。Inventory を double-entry 外で +1 したうえで `execute` すると `Dead{InternalInvariantBroken}`。陽性対照: `driven_session_inventory_desync_emits_dead_reason_4`（`host_test_code4.log` SHA `cf9e9d37bc454c8c5d51ebcf564c0143eb4a36d09ebc88ea18aa4c4009f18d66`） |

正当なプレイヤー intent だけで Dead に落とす経路は、本フェーズでは **ABSENT**（未構築・未観測）。上記 code 4 は「破損検出後の fail-close」経路であり、C-5 のとおりそれ自体は欠陥ではない。

---

## H-2 — `BridgeError::ReplayDivergence` 判定

**判定: 設計**（FSM 側 variant は未配線の遺物）。

根拠（推測ではなく文書・コミット・モジュール境界）:

1. **モジュール責務** — `bridge.rs` 先頭 doc: owned `ReplayInput` をリプレイし、digest 不一致時は `BridgeError::ReplayDivergence` で **プロファイル推定を拒否**する。ライブ `Session` を殺す役割ではない（「degraded determinism must refuse to write a profile」）。
2. **憲法** — `docs/AI_SKILLS.md` Phase 6-A §20-10: 「不一致は推定を中止して `ReplayDivergence` で書込を拒否せよ — 歪んだプロファイルを書くな」。対象は書込ゲートでありセッション FSM ではない。
3. **導入時期** — `FailureReason` は Phase 3（`a6e6f47`）で FSM 表に先行。`BridgeError::ReplayDivergence` は Phase 6-A（`ad9a240`）で別 enum として追加。bridge は fresh `Session::start` 上のリプレイであり、ライブ FSM の `die()` に接続する相手セッションが存在しない。
4. **SPEC_FLAVOR** — フレーバ汚染で `ReplayDivergence` による書込拒否が意味を失う、と明記（プロファイル経路）。

**接続提案（本フェーズは実装しない・C-3）**: ライブ経路で Decide-time digest を再検証するなら、その検出点から `die(FailureReason::ReplayDivergence)` を呼ぶのが対応表と一致する。現状の bridge 失敗を FSM に横流しすると、推定拒否とセッション死を混同する。遺物を消すならコード表の `3` を退役させる別裁定が要る。

---

## H-3 / H-4 — 陽性対照と変異ドリル

### H-3

到達可能と判定したコードは **4 のみ**。host テスト:

- `driven_session_inventory_desync_emits_dead_reason_4` — Session を駆動 → `Dead` → `arena_terminal_metrics` が `("arena.dead_reason", 4)`（状態を手構築しない）。
- 除外: code 1/2/3（H-1）。companion `unbalanced_books_die_as_internal_invariant_not_accounting_breach` が code 1 非発火を固定。

判断ロジックは ungated の `arena_terminal_metrics`（H-5）。

### H-4

`scripts/arena_dead_reason_mutation_drill.sh`: `arena_terminal_codes` を常に `(0,0)` に破壊 → 上記テスト RED（exit 101）→ 復元 GREEN。

- RED log SHA: `2108109d145abcac11dab59fd78970ffc253b2c0a2211dfcc6b13c14ba933eb7`
- GREEN log SHA: `b925eb026456c65910d9d7a8a9031a977118320db1e3c05b3deb5b6536f07dfc`
- 結果: `DRILL[arena_dead_reason_instrument]: PASS`

---

## H-5 — `log_arena_terminal_ios` 判断の抽出

- `arena_terminal_metrics(state, turns) -> [(label, u64); 3]` を ungated で追加。
- `#[cfg(target_os = "ios")] fn log_arena_terminal_ios` は `log_model_u64` 呼び出しのみ（FFI 境界）。
- host テストが `arena_terminal_metrics` を直接検証。

---

## H-6 / H-7 — ios-cfg `#[test]` 禁止ゲート

- ゲート: `scripts/ios_cfg_test_forbid_gate.sh` — 初回 GREEN（`ios_cfg_forbid_green.txt` SHA `700b6accba5c263056b66e8d29f47163daa66577c0e9724b4af88476923ca13a`）。
- 変異: `scripts/ios_cfg_test_forbid_mutation_drill.sh` — `log_arena_terminal_ios` 内に `#[test]` を植える → RED → 復元 GREEN。`DRILL[ios_cfg_test_forbid]: PASS`。
  - RED SHA: `b746f9a73e8400d72957f152d2a39460370216047c8fd2e0551979459d09d4b5`
- CI 接続: 新規 WF `.github/workflows/ios-cfg-test-forbid-gate.yml`（既存封緘 WF 無改変・C-2）。**push 前のため run ID は ABSENT**（H-9 / C-7）。ruleset 必須化は未実施（15/15 を割らない）。

---

## H-8 — 18 ブロック分類

| # | 場所 | 分類 | 理由 |
|---|---|---|---|
| 1 | `flavor_slot.rs:120` | FFI 境界 | `ios_oslog::log_model_u64` |
| 2 | `flavor_slot.rs:174` | FFI 境界 | 同上（counters 一括 emit） |
| 3 | `handle.rs:411` | 抽出可能 | `turns_completed` 読取のみ（ログ用ローカル） |
| 4 | `handle.rs:426` | FFI 境界 | `log_arena_terminal_ios` 呼び出し |
| 5 | `handle.rs:1019` | FFI 境界 | `log_arena_terminal_ios` 本体（判断は抽出済） |
| 6 | `db/mod.rs:37` | FFI 境界 | `lifecycle` モジュール（UIKit 自動 lock） |
| 7 | `net_gateway.rs:624` | 抽出可能寄り | `ReqwestTransport::new` の resolver 選択。Lookup 抽象は既にあるが OS 分岐そのものはプラットフォーム |
| 8 | `lib.rs:16` | FFI 境界 | `mod ios_oslog` |
| 9 | `lib.rs:144` | FFI 境界 | `ios_oslog::install()` |
| 10 | `lib.rs:524` | FFI 境界 | vault clone for lifecycle |
| 11 | `lib.rs:539` | FFI 境界 | `lifecycle::install_auto_lock` |
| 12 | `model_path.rs:120` | 抽出可能 | ログレベル分岐（error vs debug）— 純判断 |
| 13 | `service.rs:1436` | FFI 境界 | `log_model_u64(MODEL_FORCE_CPU)` |
| 14 | `service.rs:1445` | FFI 境界 | `log_model_identity_ios` 呼び出し |
| 15 | `service.rs:1453` | FFI 境界 | `log_model_identity_ios` 本体 |
| 16 | `monitor/mod.rs:66` | 抽出可能 | `phase_label` 純マッチ |
| 17 | `monitor/mod.rs:277` | FFI 境界 | `log_footprint_bytes` |
| 18 | `paths.rs:50` | FFI 境界 | iOS `user_data_root`（コンテナ HOME） |

**以降フェーズの抽出候補（本フェーズは棚卸しのみ・C-4）**: #3, #7（要判断）, #12, #16。

---

## H-9 — CI 15/15

- 既存必須チェック・封緘ゲート（`ios_archive_scan.sh` / `gguf_three_point_sha_gate.sh` / 既存 WF）は **無改変**。
- 新規 WF は追加のみ。ruleset 必須化・push・run ID は **ABSENT**（指揮官承認待ち）。

---

## H-10 — 実施しなかったこと・確かめなかったこと

1. `BridgeError::ReplayDivergence` → FSM `die` の実装（C-3）。提案のみ。
2. `log_arena_terminal_ios` 以外の ios-cfg 抽出（C-4）。
3. Dead をバグとして FSM を変更すること（C-5）。
4. commit / push（C-7）。
5. **アリーナ `advance` 成功経路での `Dead` OSLog 発火**: `die()` は `execute`/`settle`/`report` が `Err` を返すため、`log_arena_terminal_ios`（成功後の terminal 分岐）に **Dead 状態が届く経路は ABSENT**。本フェーズの陽性対照は `arena_terminal_metrics`（判断）であり、OSLog 配線の Dead 到達は未修復・未実証。
6. 正当 intent のみでの Dead 再現（T4-E-3/4 相当）— 未実施。
7. 新 WF の GitHub Actions run — push 未実施のため ABSENT。
8. `cargo test` 全 suite / 既存 15 必須ジョブの再実行 — 本マシンでは該当 host テストと静的ゲートのみ。CI 全体 GREEN の再証明は push 後。

---

## H-11 — 成果物一覧

| 種別 | パス |
|---|---|
| 判断抽出 + 陽性対照テスト | `apps/desktop/src-tauri/src/blackbox_arena/handle.rs` |
| テスト用 poison | `ledger.rs` `test_add_unchecked` / `director.rs` `test_books_mut` |
| ios-cfg テスト禁止ゲート | `scripts/ios_cfg_test_forbid_gate.sh` |
| 変異ドリル | `scripts/ios_cfg_test_forbid_mutation_drill.sh` / `scripts/arena_dead_reason_mutation_drill.sh` |
| 新規 WF | `.github/workflows/ios-cfg-test-forbid-gate.yml` |

---

## 検証コマンド（実施済）

```bash
cargo test --features blackbox-sim --lib driven_session_inventory_desync_emits_dead_reason_4
cargo test --features blackbox-sim --lib unbalanced_books_die
bash scripts/ios_cfg_test_forbid_gate.sh
bash scripts/ios_cfg_test_forbid_mutation_drill.sh
bash scripts/arena_dead_reason_mutation_drill.sh
bash scripts/verify_report.sh <本報告> /tmp/t4e-evidence
```
