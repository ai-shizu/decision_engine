# SPEC — E0b STEP 0: Constitutional Guard (Composer-Executable Blueprint)

> **読者:** これは人間向けの解説ではない。**自律型コーディング AI（Cursor Composer 等）が読み込み、
> 一切の曖昧さなく逐次実行するための絶対的実行指示書**である。各命令に裁量はない。逸脱は禁止。
> **親仕様:** `docs/SPEC_E0B_KNOWLEDGE_GATEWAY_v3.md`（承認・ロック済み）の §12 STEP 0 を実行に落とす。
> **本 STEP の性質:** **テストのみ。プロダクションコードを 1 行も書かない。**
> STEP 0 は既存の無菌（networkless）状態を機械可読な契約として**物理ロック**する ratchet である。
> **ACK 規律:** STEP 1（プロダクション実装）への着手は指揮官 ACK まで固く禁止。

---

## 0. 前提として検証済みのリポジトリ事実（Composer はこれを信じてよい／再確認してもよい）

これらは本ブループリント起草時に実コードを走査して確定した。**deny-set の設計はこの事実に依存する。**

1. `src/python/` 全域でネットワーク egress モジュール（`socket`/`requests`/`urllib`/`http.client`/`httpx`/
   `aiohttp` 等）の import は **1 件も存在しない**（= 無菌）。唯一のネットワーク系 import は
   [offline_runtime.py:91-92](../src/python/core/offline_runtime.py) の **bare `asyncio`**（ローカル stdio
   イベントループ用。Windows の `windows_events` 取得）であり、これは egress ではない。
2. **`subprocess` と `ctypes` はローカル基盤として正当かつ広範に使用されている:**
   `subprocess` → [llm_backend.py](../src/python/core/llm_backend.py)（llama.cpp spawn）、
   [search_daemon.py](../src/python/core/search_daemon.py)（C++ 検索 daemon）、
   [consultation_engine.py](../src/python/core/consultation_engine.py)・[cli.py](../src/python/core/cli.py)（profiler spawn）。
   `ctypes` → [secure_identity.py](../src/python/core/secure_identity.py)（Windows DPAPI）、
   [llm_transport.py](../src/python/core/llm_transport.py)（owner-only Named Pipe）。
3. 既存 E0a 契約テスト [tests/test_e0a_egress_lockdown.py](../tests/test_e0a_egress_lockdown.py) が存在し、
   その `_DENY_TOP` は `subprocess`/`ctypes` を**`knowledge_fetcher.py` という限定文脈でのみ**禁止している。
4. `tests/conftest.py` が存在し Sandbox を提供する。Python テストは `python -m pytest` で実行する。

> **★最重要の落とし穴（Composer は絶対に踏むな）:** `subprocess`/`ctypes` を **ツリー全体の deny-set に
> 入れてはならない。** 入れると事実 2 の正当なローカルコードで大量の偽 RED が発生し、それを「修正」して
> ローカル llama.cpp・C++ 検索・DPAPI・Named Pipe を破壊するのは**憲法違反**である。§1.4 の scope 定義を厳守せよ。

---

# セクション 1: アーキテクチャと制約 (Architecture & Hard Constraints)

## 1.1 データの流れ（STEP 0 が扱う対象）

STEP 0 は**副作用のない静的検査**のみを行う。ネットワークもプロダクションコード変更も一切ない。

- 入力 A: `src/python/**/*.py` のソース（**読み取り専用**）→ `ast.parse` → import/呼び出しノード走査。
- 入力 B: `apps/desktop/src-tauri/Cargo.toml` と `apps/desktop/src-tauri/src/**/*.rs`（**読み取り専用**）
  → テキスト走査（HTTP クライアント crate が未リンクであることの確認）。
- 入力 C: `src/python/engine_stdio.py` の dispatch（**読み取り専用**）→ E0b egress コマンドが未配線であることの確認。
- 出力: `tests/` 配下の**新規テストファイル**の PASS/FAIL 実測値のみ。プロダクション成果物はゼロ。

## 1.2 絶対禁止事項（Composer がやりがちな手抜きの事前封鎖）

以下はいずれも即時失格。1 つでも破ったら作業を止めて報告せよ。

1. **プロダクションコードを書くな。** STEP 0 の成果物は `tests/` 配下のテストファイルだけ。
   `src/`・`apps/` 配下の**いかなるファイルも作成・編集しない**（E0b の本体は STEP 1 以降・ACK 後）。
2. **`reqwest` 等の HTTP クライアント crate を `Cargo.toml` へ追加するな。** STEP 0 は「未リンクである」ことを
   ロックする段階である。追加は STEP（依存導入）まで禁止。
3. **`subprocess`/`ctypes` をツリー全体の network deny-set に入れるな**（§0 ★・§1.4）。
4. **bare `asyncio` を deny-set に入れるな。** `offline_runtime.py` が正当に必要とする。禁止するのは
   `asyncio.subprocess` サブモジュールのみ（E0a と同一方針）。
5. **テストを弱めて GREEN を捏造するな。** 予期せぬ RED が出たら、テストを緩める・allowlist へ足す・
   assertion を消すのは**禁止**。RED は実在の憲法違反の可能性を意味する → §3 の HARD STOP で人間へ escalate。
6. **モックで egress を偽装するな。** STEP 0 に mock は不要。実ソースの AST を検査する。ネットワークを
   叩くテスト・実ネットワーク前提のテストを書くこと自体が禁止（親仕様の規律）。
7. **E0a 資産を改変するな。** `knowledge_fetcher.py`・`tests/test_e0a_egress_lockdown.py`・
   `facade.py` の E0a stub を触らない。STEP 0 は E0a を**参照・継承**するが上書きしない。
8. **検査対象から自分自身（tests/）を除外し忘れるな。** deny-set の文字列リテラルを含むのはテスト側であり、
   スキャンは `src/python/` と `src-tauri/` の**プロダクションのみ**を対象にする（テストを走査すると自己言及で偽陽性）。
9. **「assert 何もしない」テストを書くな。** 各検出器は §2 の**カナリア自己検査**（合成違反を必ず検出する
   こと）を先に GREEN にしてから本番ソースへ適用する。検出力のないテストは偽 GREEN として却下。

## 1.3 影響範囲 (Blast Radius)

**作成を許可（唯一の書き込み対象）:**
```text
tests/test_e0b_constitutional_guard.py     # 本 STEP の全テストをここに集約（新規）
```

**読み取り専用（検査対象・改変禁止）:**
```text
src/python/**/*.py
apps/desktop/src-tauri/Cargo.toml
apps/desktop/src-tauri/src/**/*.rs
src/python/engine_stdio.py
tests/test_e0a_egress_lockdown.py          # 様式の参照元
tests/conftest.py                          # Sandbox
docs/SPEC_E0B_KNOWLEDGE_GATEWAY_v3.md       # 親仕様
```

**触れてはならない（変更・作成・削除いずれも禁止）:**
```text
src/python/core/knowledge_fetcher.py        # E0a stub — 凍結
src/python/core/facade.py                   # E0a entrypoint — 凍結
src/python/core/privacy_search.py           # E0a Sanitizer — 凍結
apps/desktop/src-tauri/src/*.rs             # gateway 未存在。STEP 0 で新設しない
config/**, data/**, models/**, .claude/**   # 対象外
上記「作成を許可」以外の全ファイル
```

## 1.4 deny-set の正確な定義（scope を混同するな）

**Scope T（Tree-wide network egress ban）— `src/python/**/*.py` 全域に適用。現時点 GREEN（§0 事実 1）。**
```text
DENY_NET_TOP = {
  "urllib", "socket", "requests", "httpx", "aiohttp", "ftplib",
  "websockets", "smtplib", "telnetlib", "poplib", "imaplib",
  "ssl", "xmlrpc", "webbrowser",
}
DENY_NET_SUBMODULE = { "http.client", "asyncio.subprocess" }   # bare http / bare asyncio は対象外
# 注: bare "asyncio" は許可。"http" 単独パッケージは client を含むときのみ違反（E0a と同一判定）。
```

**Scope E（E0b-file-scoped strict ban）— E0b で新設される Python モジュールにのみ適用。**
```text
対象ファイル判定: src/python/ 配下で basename が {"knowledge_gateway*.py", "*e0b*.py"} に一致するもの、
                かつ本 STEP 時点では 0 件（E0b 本体は未実装）。
DENY_E0B = DENY_NET_TOP ∪ DENY_NET_SUBMODULE ∪ {"subprocess", "ctypes", "multiprocessing"}
          ∪ 動的バイパス {"__import__", "import_module", "eval", "exec", os.system, os.popen, Popen 等}
# 対象 0 件なので現時点は空回り GREEN。STEP 1 で E0b モジュールが生まれた瞬間から効力を持つ「予約された檻」。
```

> **なぜ 2 scope か:** Scope T は「外へ出る口」を全域封鎖する。Scope E は「E0b の新モジュールだけ」に
> ローカル process 生成（subprocess/ctypes）すら禁じ、egress は必ず Rust 経由に強制する。既存のローカル
> 基盤（llm_backend 等）は Scope T の対象だが Scope E の対象ではないので壊れない。

---

# セクション 2: 実行フェーズ計画 (Execution Phasing)

各 STEP は厳密に **RED → 検証コマンド → GREEN → 完了条件** の順で閉じる。
STEP 0 は ratchet のため多くの本番 assertion は最初から PASS するが、**各検出器はカナリア自己検査で
「検出力があること」を先に証明**してから本番へ適用する（偽 GREEN の構造的排除）。
すべてのテストは新規ファイル **`tests/test_e0b_constitutional_guard.py`** に追記していく。

---

### STEP 0.A: AST 検出器のカナリア自己検査（検出力の証明）

1. **RED:** `tests/test_e0b_constitutional_guard.py` を新規作成し、以下を実装:
   - `E0a` テストの `_denied_imports` / `_has_dynamic_bypass` に相当する AST 走査ヘルパを本ファイル内に実装
     （E0a からのコピー可、ただし §1.4 の 2 scope に対応させる）。
   - **カナリアテスト** `test_detector_flags_synthetic_violation`: 文字列で合成した違反スニペット
     （`"import socket\n"`, `"from urllib import request\n"`, `"import http.client\n"`,
     `"from asyncio import subprocess\n"`, `"__import__('os').system('x')\n"`）を `ast.parse` にかけ、
     検出器が**それぞれを確実に hit として返す**ことを assert。
   - **陰性カナリア** `test_detector_allows_legitimate_local_substrate`: `"import asyncio\n"`,
     `"import subprocess\n"`, `"import ctypes\n"` を Scope T 検出器に通し、**hit ゼロ**であること
     （= bare asyncio と subprocess/ctypes を Scope T が誤検出しない）を assert。
   - エッジケース必須: `import a.b.c` の top-level 抽出、`from x import y as z` の alias、
     `from http import client`、`from asyncio import subprocess`、ネスト（関数内）import、
     条件分岐内 import、`importlib.import_module("socket")` 動的 import。
2. **検証コマンド:**
   ```
   python -m pytest tests/test_e0b_constitutional_guard.py -k detector -v
   ```
3. **GREEN（実装）:** プロダクションコードなし。検出器ヘルパ + カナリア 2 種を GREEN にする。
   検出器がカナリアを取りこぼす場合はヘルパのロジックを修正（テスト側のみ）。
4. **完了条件:** `test_detector_flags_synthetic_violation` と
   `test_detector_allows_legitimate_local_substrate` が PASS。検出器に検出力があることが証明された。

---

### STEP 0.B: Tree-wide 無菌ロック（Scope T を本番ソースへ適用）

1. **RED（実測):** `test_python_tree_has_no_network_egress_imports` を追加:
   - `src/python/` を再帰走査し（`__pycache__`・`tests/` を除外）、各 `*.py` を `ast.parse` → Scope T 検出器適用。
   - すべての hit を集約し、**hit ゼロ**を assert。hit があれば `AssertionError` にファイルパスと違反形を含めて出す。
   - **予測される実測:** §0 事実 1 より **PASS（GREEN）**。これは無菌状態の証明でありロックである。
   - **エッジケース:** `offline_runtime.py` の bare `asyncio` を**誤検出しない**こと（Scope T の陰性確認）。
2. **検証コマンド:**
   ```
   python -m pytest tests/test_e0b_constitutional_guard.py -k tree_has_no_network -v
   ```
3. **GREEN（実装):** プロダクション変更なし。既存の無菌状態が assertion を満たす。
4. **完了条件:** テストが PASS し、実測 PASS をターミナルログとして取得。
   **万一 RED の場合** → §3 HARD STOP-1 を発動（テストを緩めるのではなく、違反モジュールを報告して人間裁定を待つ）。

---

### STEP 0.C: E0a 凍結の継承と E0b scope の予約檻

1. **RED:** 2 テストを追加:
   - `test_e0a_stub_freeze_still_holds`: `knowledge_fetcher.py` を AST 解析し、`_http_get`/
     `default_online_fetcher`/`process_pending` が**厳密に** `raise NotImplementedError("Egress blocked by
     E0a strict lockdown.")` の 1 文であること、`online_fetch_allowed` が `return False` のみで環境を読まない
     ことを assert（E0a の `_is_exact_e0a_raise` 相当を再利用）。これは E0a と重複するが、**E0b 側からの
     二重ロック**として意図的に持つ（E0a テストが将来変更されても E0b の前提が守られる）。
   - `test_e0b_scoped_modules_absent_or_caged`: §1.4 Scope E の対象ファイル集合を列挙し、
     **(a) 現時点で 0 件であること**を assert（= E0b 本体未実装の証明）。**かつ**、将来 1 件以上出現した
     場合に `DENY_E0B` を適用する検査本体を実装しておく（0 件なら空ループで PASS、出現時に自動で檻が効く）。
2. **検証コマンド:**
   ```
   python -m pytest tests/test_e0b_constitutional_guard.py -k "e0a_stub_freeze or scoped_modules" -v
   ```
3. **GREEN（実装):** プロダクション変更なし。
4. **完了条件:** 両テスト PASS。E0a 凍結が E0b 側からも二重に保証され、E0b モジュール用の檻が予約された。

---

### STEP 0.D: Fail-Closed 証明（E0b egress コマンドが未配線であること）

1. **RED:** 2 テストを追加:
   - `test_no_e0b_ipc_command_wired`: `src/python/engine_stdio.py` を AST/文字列走査し、dispatch 内に
     `"knowledge.intent.build"` / `"knowledge.research"` / `"knowledge.integrate"` のいずれの文字列リテラルも
     **存在しない**ことを assert（= E0b の IPC 入口が今は無い = 外へ出る扉が塞がっている）。
   - `test_facade_has_no_e0b_egress_entrypoint`: `facade.py` に `intent`/`research`/`integrate` 系の
     新規 egress 公開関数が無いことを assert（E0a の `fetch_pending_knowledge` は raise stub のまま許容）。
   - **エッジケース:** コメント・docstring 内の文字列を誤検出しないよう、可能なら AST の
     `ast.Constant`(str) ノードのみを対象にし、dispatch 関数本体に限定する。
2. **検証コマンド:**
   ```
   python -m pytest tests/test_e0b_constitutional_guard.py -k "no_e0b_ipc or no_e0b_egress" -v
   ```
3. **GREEN（実装):** プロダクション変更なし。現状は fail-closed（扉が無い）。
4. **完了条件:** 両テスト PASS。**この 2 テストは STEP 5/8 で E0b コマンド配線時に「反転」する** ——
   その時点で本テストを削除ではなく**肯定形（コマンドが存在し、かつ attestation/dual-run gate を通る）へ
   書き換える**義務を、テスト docstring に明記しておくこと。

---

### STEP 0.E: Rust 側「ネットワーククライアント未リンク」ロック

1. **RED:** 2 テストを追加（Python から Rust ソースをテキスト検査。cargo は起動しない）:
   - `test_rust_has_no_http_client_dependency`: `apps/desktop/src-tauri/Cargo.toml` を読み、
     `reqwest`/`hyper`/`isahc`/`ureq`/`curl`/`surf` の依存が**無い**ことを assert（= 無菌ビルド）。
   - `test_rust_src_has_no_outbound_network_calls`: `apps/desktop/src-tauri/src/**/*.rs` を走査し、
     `reqwest::`・`TcpStream`・`tokio::net`・`hyper::`・`UdpSocket` 等の送出系シンボルが**登場しない**ことを assert
     （`os_sandbox.rs` の WinSock は AppContainer の**網能力剥奪**用であり egress ではない —— これを誤検出
     しないよう、許可リストに `Win32_Networking_WinSock`（feature 宣言）と sandbox 用シンボルを明示除外する）。
   - **カナリア:** 合成文字列 `"reqwest::get(...)"` を検出器が hit すること、
     `"windows-sys ... Win32_Networking_WinSock"`（Cargo feature 行）を hit **しない**ことを assert。
2. **検証コマンド:**
   ```
   python -m pytest tests/test_e0b_constitutional_guard.py -k rust -v
   ```
3. **GREEN（実装):** プロダクション変更なし。現状 Rust 側も egress クライアント未リンク。
4. **完了条件:** 両テスト + カナリア PASS。Rust 側の無菌が機械ロックされた。
   **RED の場合** → §3 HARD STOP-1（既に HTTP クライアントが紛れ込んでいる = 重大発見。報告して裁定待ち）。

---

### STEP 0.F: 全 STEP 0 スイートの一括実測

1. **RED（最終実測):** フィルタ無しで全新規テストを実行し、**RED/GREEN の実数**を取得する。
2. **検証コマンド:**
   ```
   python -m pytest tests/test_e0b_constitutional_guard.py -v
   ```
   （必要なら差分確認に `python -m pytest tests/ -q` で既存スイートの非回帰も確認。）
3. **GREEN（実装):** なし。
4. **完了条件:** 全テスト PASS（= 無菌状態が全面ロックされた）。もしくは、いずれかが RED なら
   §3 HARD STOP-1 で「どのガードが何を検出したか」を実測ログ付きで報告。**テストを弱めて緑にしない。**

---

# セクション 3: アトミックコミットと停止規律 (Checkpoints & Commits)

## 3.1 コミット規律（各 STEP GREEN 直後・次へ進む前に必須）

1 つの STEP（0.A〜0.F）が完了条件を満たしたら、**次へ進む前に必ず 1 コミット**を切る。
- ステージは**新規テストファイルのみ**（`git add tests/test_e0b_constitutional_guard.py`）。
- **`.claude/` を絶対に stage しない**（未追跡のユーザー環境資産）。`git add -A` / `git add .` を使うな。
  ステージ前に `git status --short` で差分を目視し、テストファイル以外が乗っていないことを確認。
- コミットメッセージ様式（各 STEP で subject を変える）:
  ```
  test(e0b-step0): <STEP名> — <何をロックしたか>

  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  ```
- **push は禁止**（親仕様・HANDOFF の規律。origin への push は指揮官の明示指示のみ）。
- E0a 実装コミットや既存コミットを revert/reset しない。`git reset --hard`・`git checkout --` を使わない。

## 3.2 HARD STOP ポイント（人間レビュー必須）

以下に到達したら、Composer は**出力を停止しユーザー（指揮官）へ報告**し、ACK を待て。勝手に先へ進むな。

- **HARD STOP-0（本 STEP の正常終了）:** STEP 0.F まで全テスト GREEN（無菌全面ロック）を達成し、各 STEP を
  コミット済みにした時点。以下を報告して**停止**:
  1. 新規ファイル `tests/test_e0b_constitutional_guard.py` のテスト一覧と各 PASS/FAIL 実測値。
  2. `python -m pytest tests/test_e0b_constitutional_guard.py -v` の生ターミナル出力（RED/GREEN の実数）。
  3. 作成したコミットの `git log --oneline`（テストファイルのみを含むこと）。
  4. **STEP 1（プロダクション実装）へは進んでいないことの明言。**
- **HARD STOP-1（予期せぬ RED = 憲法違反の疑い）:** STEP 0.B/0.E 等で無菌ロックが RED になった場合。
  これは「既存コードに egress の芽が紛れ込んでいる」実在の発見である可能性が高い。**テストを緩めず**、
  違反ファイル・違反形・該当行を提示して停止し、指揮官の裁定を待て（自動修正は憲法違反）。
- **HARD STOP-2（スコープ逸脱の誘惑）:** もし GREEN 化のためにプロダクションコード変更・`reqwest` 追加・
  `subprocess`/`ctypes` の tree-wide 禁止・E0a 改変が必要に「見えた」場合、それは設計誤読である。
  作業を止めて、なぜそう見えたかを報告し ACK を待て。

## 3.3 STEP 0 完了の定義（Definition of Done）

- `tests/test_e0b_constitutional_guard.py` が存在し、0.A〜0.F の全テスト（カナリア含む）が PASS。
- 各検出器がカナリア自己検査で**検出力を証明**している（偽 GREEN でない）。
- 実測ログ（RED/GREEN 実数）が取得・報告されている。
- プロダクションコード（`src/`・`apps/`）差分が**ゼロ**。`Cargo.toml` に HTTP クライアント追加**なし**。
- 各 STEP がアトミックにコミットされ、`.claude/` を含まず、push していない。
- **STEP 1 未着手**であることが明言され、ACK 待ち状態で停止している。

---

## 付録: STEP 0 が STEP 1 以降へ残す「反転義務」メモ（Composer への申し送り）

STEP 0 の一部テストは、E0b 実装が進むと**肯定形へ反転**する。削除ではなく書き換える。
- `test_no_e0b_ipc_command_wired`（0.D）→ STEP 5/8 で「コマンドが存在し、Rust 側 attestation + dual-run
  gate を必ず通る」肯定契約へ。
- `test_rust_has_no_http_client_dependency`（0.E）→ 依存導入 STEP で「`reqwest` が `default-features=false`,
  features に `rustls-tls`,`stream` のみ（`gzip`/`native-tls`/`socks` 無し）で入る」肯定契約へ（親仕様 §7.4）。
- Scope E の檻（0.C）→ E0b モジュール出現時に自動で効力を持つ。反転不要・維持のみ。
これらの反転はすべて**別 STEP・別 RED・ACK 後**に行う。STEP 0 では反転しない。
