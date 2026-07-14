# PKB 開発 AI Skills — 軽量モデル向けカスタム・インストラクション

> このファイルは System Prompt / `.cursorrules` にそのままコピペして使う。
> トーンは意図的に命令形。「守るべき理由」より「何をするか」を優先して書いてある。
>
> **文書の役割（混同するな）:**
> - 本書 (`docs/AI_SKILLS.md`): 不変の開発規律。読み込みは §0 のタスク別表を優先（全文読了を強制するな）。
> - `docs/HANDOFF.md`: 現在地・worktree・直近作業（揮発的事実）。
> - `docs/CONTEXT.md`: 安定アーキテクチャと正本への索引（タブ数・IPC 一覧・件数の正本ではない）。
> - `docs/architecture/INCIDENT_LEDGER.md`: 事故と絶対裁定。
> - タブ・IPC コマンド・schema・現在挙動など volatile な事実は実コードで確認せよ。
> 競合時は実コードと INCIDENT_LEDGER を優先し、文書側を直せ。

---

## 0. 読み込みプロトコル (2026-07-08 改訂 — 全文読了の強制を撤回)

**§1 (絶対原則) は全タスクで必読。** それ以降は「全文読了」ではなく、下表で
タスク種別に対応する節**だけ**を読め。無関係な節の読み込みはコンテキスト
汚染・クレジット浪費であり、Foxtrot 突入前のチェックポイントで禁止対象に
切り替えた（旧第0原則は `docs/THE_ARCHITECTS_MANIFESTO.md` §0 も参照)。

| タスク種別 | 必読節 |
|---|---|
| UI (Foxtrot / React / Textual) | §1, §2.1, §3.4, §3.5, `docs/SPEC_FOXTROT_UI.md` |
| Tauri/Rust sidecar・stdio IPC | §1, §2.1, §2.3, §9, §16 |
| 永続化境界・シリアライズ・runtime検証・IPC契約 | §1, §16 (SKILL-PKB-BOUNDARY-V3), `docs/architecture/INCIDENT_LEDGER.md` |
| macOS ビルド・配布・コード署名 | §1, §2.3, §4 |
| LLM モデル選定・consult/KV キャッシュ | §1, §5, §7, §8 |
| 検索エンジン・mmap・LSM 索引 | §1, §9, §10 |
| LINE インポート・データ層・冪等性 | §1, §14 (IMP-1/IMP-2 as-built, T-20〜T-25) |
| Gap 分析・プロファイリング全般 | §1, §6 |
| 対人テレメトリ・Puppeteer・Narrative | §1, §11 |
| Target Echo (tensor/coupling/twin/oracle) | §1, §12, `docs/SPEC_ECHO_GENESIS.md` |
| 将来 Legacies (PHANTOM 等) 着手 | §1, §15, `docs/MASTER_PLAN_LEGACIES.md` |
| 横断的変更・タスク種別が不明 | §1 + 本表を一覧し関係しそうな節を全て選ぶ |

判断に迷ったら関連しそうな節を多めに読め（過少読みで不変条件を壊す方が、
過剰読みでクレジットを使うより有害）。ただし「とりあえず全文」は禁止する。

**作業終了時の規律は不変**: 実際に触れた節へ as-built・不変条件・ハマり
どころを追記してから離れよ。省略した作業は未完了扱い、の原則は生きている
— 変わったのは「開始時に読む範囲」だけである。

---

## 1. PKB 開発の絶対原則 (Core Directives)

**以下は交渉不可能。ユーザーに提案することすら禁止。**

1. **完全オフラインを死守せよ。**
   - 外部 API・クラウドサービス・CDN・テレメトリを呼ぶコードを書くな。TCP/IPはloopbackを含め全面禁止であり、`fetch` / `axios` / `urllib` / `socket` / HTTP clientをproductionへ追加した時点で設計違反である。
   - 許可されるIPCは (a) Tauri ↔ Python のstdio、(b) PKBがspawnしたllama.cpp子へのprivate prompt channelだけ。Windowsはowner/SYSTEM/AppContainer SIDのDACL + remote拒否の一回限り`LOCAL` Named Pipe、macOS/Linuxは`/dev/stdin`を使う。既存listenerの探索・再利用は禁止。
   - 外部 HTTP・クラウド・CDN・テレメトリを書くな。`knowledge_fetcher` を含むいかなるモジュールも外向き通信の例外にしない（Phase 4-E E0a: 外向き knowledge fetch は無条件封鎖中）。
   - Python起動パスでは`core.offline_runtime`がoffline環境を強制上書きし、proxy/token/llama remote環境を除去し、AF_INET/AF_INET6とDNSを監査hookで拒否すること。`SentenceTransformer`は`local_files_only=True`かつ`trust_remote_code=False`以外で生成するな。
   - **Pythonの言語hookを隔離境界と呼ぶな。** productionはbundled sidecarのみをRustの`os_sandbox.rs`から起動する。Windowsはcapability数0のAppContainer、Linuxはarch検証付きseccomp-BPFで`socket(AF_INET/AF_INET6)`を`EACCES`、macOSは署名済みApp Sandbox（network entitlementなし）を必須とする。適用失敗時の通常起動は禁止。System Pythonは`PKB_UNSAFE_DEV_ENGINE=1`を明示したdebug buildだけの非保証モードである。
   - npm パッケージを追加する時、ランタイムで外部通信するもの（アナリティクス、フォント CDN、自動アップデータ）は選ぶな。

2. **個人データを絶対に流出させるな。**
   - `data/raw/`（日記・LINE・家計簿）、`data/processed/`、`models/`、`logs/` は **コミット禁止**（.gitignore 済み。解除するな）。
   - テストは `tests/conftest.py` の Sandbox (`PKB_PROJECT_ROOT`) 上でのみ実行せよ。
     実データ（`diary.md`, `user_profile.json` 等）への書き込みは禁止。
     単体実行は `python -m pytest tests/test_x.py` のみ（`__main__` 直接実行は
     廃止 — conftest 隔離を迂回するため）。
   - ログ・エラーメッセージに日記本文や LINE 本文をそのまま出すな。
   - **`logs/engine.log` は raw stderr dump ではない**（INC-ENGINE-LOG-01 / Finding 12）。
     永続化は exact allowlist `[PKB_DIAG_V1] REQUEST_FAILED` のみ（終端 CRLF は可、marker 内 CR は不可）。
     安全判定は **V1 header + 以降全行の allowlist 検証**。`Path::exists()` 禁止・
     `symlink_metadata` のみ。traceback・exception message・payload・path・query を
     永続化するな。保持は不変契約として **1 MiB × (`engine.log` + `.1` + `.2`)**。
     library `print` は protocol 保護のため stderr へ退避されるが、persistent log では
     allowlist 外として破棄される。Finding 13（UI への生例外表示）と混同するな。

3. **責務分離を守れ。**
   - **C++ (`src/cpp/search_engine.cpp`)**: パフォーマンス限界突破専用。384 次元 AoSoA 内積・Top-K のみ。ビジネスロジック・ファイル形式の解釈・日本語処理を C++ に入れるな。バイナリ形式 `PKBVEC01` は Python `pipeline.py` と 1 バイト単位で同期していること（片方だけ変えたら即データ破損）。
   - **Python (`src/python/core/`)**: オーケストレーション・データ管理・プロンプト構築。UI コード（Textual / React への依存）を `core/` に 1 行でも入れるな。
   - **UI (React / Textual)**: 表示と入力だけ。ビジネスロジックは書くな。新機能は必ず `core/facade.py` に API を足す → `engine_stdio.py` の dispatch に載せる → `apps/desktop/src/lib/engine.ts` にラッパーを足す、の順で通す。

4. **記録と分析を分離せよ。**
   - RECORD 保存・カレンダー同期で profiler を走らせるな（`sync_diary_index()` のみ）。
   - profiler の自動実行は LINE 取込 (`import.line`) のみ。他で走らせたければユーザーに聞く前に HANDOFF を読み直せ。
   - 埋め込みモデル・llama.cpp子プロセスは初回 consult / profiler まで起動しない（遅延初期化）。起動を早めるな。

---

## 2. Tauri + Python Sidecar トラブルシューティング

### 2.1 stdio IPC の鉄則（破ると即座に UI が死ぬ）

このアプリの UI ↔ エンジン通信は「stdout に JSON を 1 行ずつ」だけで成立している。
**Python エンジン側の stdout は Rust 専用のプロトコル線である。1 バイトでも汚したら JSON パースエラーで全機能が止まる。**

- `engine_stdio.py` は import 直後に `sys.stdout = sys.stderr` で print を stderr へ退避し、JSON は `_JSON_OUT`（UTF-8 固定 TextIOWrapper）にだけ書く。**この構造を変更するな。**
- 新しいライブラリを core に足す時、そのライブラリが「import 時や実行時に stdout へ print するか」を必ず確認せよ（tqdm・警告・バナー出力が典型犯）。汚すなら stderr へリダイレクトしてから使え。
- プロトコル仕様:
  - 起動直後: `{"event": "ready", "offline": true}`
  - 応答: `{"id": N, "ok": true, "result": {...}}` / `{"id": N, "ok": false, "error": "..."}`
  - 中間イベント（consult のみ）: `{"id": N, "event": "status", "message": "..."}` / `{"id": N, "event": "chunk", "text": "..."}` — **`ok` キーを持たない行がイベント、持つ行が最終応答**。Rust 側 (`engine.rs::invoke_sync`) はこの規約でループしている。イベント行に `ok` を入れたら最終応答と誤認されるので絶対に入れるな。
- 中間イベントは Rust から Tauri イベント `pkb-engine-event` として React へ転送される。React 側は `listen<EngineEvent>("pkb-engine-event", ...)` で受ける（`ConsultTab.tsx` 参照）。

### 2.2 cp932 (Windows 日本語環境) の罠

**Windows の Python はデフォルトで stdout/stdin/ファイルが cp932 になる。日本語 JSON が即死する。**

- Python サブプロセスを spawn する側（Rust `engine.rs`）は必ず `-X utf8` + `PYTHONUTF8=1` + `PYTHONIOENCODING=utf-8` を付けている。新しい spawn 経路を作るなら同じ環境変数を必ず付けろ。
- `subprocess.run` で外部 exe の出力を受けるときは必ず `encoding="utf-8", errors="replace"` を明示せよ（`consultation_engine.search_index` 参照）。指定しないと cp932 デコードで落ちる。
- ファイル I/O は全箇所 `encoding="utf-8"` 明示。**`open(path)` と裸で書いたらレビューで落とせ。**
- ユーザー持ち込みファイル（`data/knowledge/` 等）はメモ帳の ANSI 保存 = cp932 の可能性がある。`consultation_engine._read_text_lenient()`（UTF-8 → cp932 → replace の順で試す）を使え。新たな取込機能にも同じフォールバックを入れろ。
- UI へ返す文字列は `core/text_utils.sanitize_obj` を通す（サロゲート・制御文字で JSON が壊れるのを防ぐ）。

### 2.3 ARM64 / x86 混在エラー (LNK4272 ほか)

この開発機は **Snapdragon X = Windows on ARM64** である。x86/x64 バイナリの混入がビルド失敗の最頻原因。

症状と対処:

| 症状 | 原因 | 対処 |
|---|---|---|
| `LNK4272: library machine type 'x64' conflicts with target machine type 'ARM64'` | x64 の .lib/.dll をリンクしている | 依存を ARM64 版に差し替える。無ければソースビルド |
| pip パッケージの import で `ImportError: DLL load failed` | x64 wheel が入った | `python -c "import platform; print(platform.machine())"` が `ARM64` の Python を使え。`pip debug --verbose` で対応 wheel タグ確認 |
| llama.cpp子が起動しない/異常に遅い | x64 エミュレーション実行 | `tools/llama-arm64/` の ARM64 native ビルドを使う |
| C++ ビルド失敗 | MSVC x64 ツールチェーン | `build.ps1` を使う（llvm-mingw clang++ + OpenMP、ARM64 NEON 前提） |

**デバッグコマンド（迷ったら上から順に実行）:**

```powershell
# 1. Python のアーキテクチャ確認（ARM64 と出なければ即アウト）
python -c "import platform,sys; print(platform.machine(), sys.version)"

# 2. exe/dll のマシンタイプ確認 (8664=x64, AA64=ARM64)
dumpbin /headers build\search_engine.exe | Select-String "machine"

# 3. C++ 再ビルド + Python/C++ バイナリ互換確認
.\build.ps1
python tests\benchmark.py --quick

# 4. エンジン単体の疎通確認（Tauri を介さず直接叩く）
#    ready 行 → health 応答が UTF-8 JSON で返れば IPC 層は健全
echo '{"id":1,"cmd":"health","params":{}}' | python -X utf8 src\python\engine_stdio.py

# 5. デスクトップのエンジンログ（V1 固定診断のみ・有限保持）
#    raw stderr / traceback は永続化されない。受理行は [PKB_DIAG_V1] REQUEST_FAILED のみ。
#    保持契約: 1 MiB × engine.log + .1 + .2（最新結果ではなく不変上限）
Get-Content data\logs\engine.log -Tail 50   # 開発時はリポジトリ data/、本番は %LOCALAPPDATA%\PKB\logs

# 6. 回帰テスト（変更後の最低ライン — 単体実行は pytest 経由）
python -m pytest tests/test_calendar_sync.py -q
python -m pytest tests/test_ui_smoke.py -q
```

### 2.4 その他の既知の罠

- `pkb-desktop.exe` 直接起動は黒画面になる。**必ず `apps/desktop/dev.cmd`** で起動せよ。
- JS の `new Date().toISOString()` は **UTC**。JST では 0:00〜8:59 に日付が 1 日ずれる。日付文字列は必ず `dateUtils.todayIso()` / `toIsoDate()`（ローカル時刻ベース）を使え。`toISOString().slice(0,10)` を書いたら即修正対象。
- Textual のカレンダー日ボタンに `id` を付けるな（DuplicateIds でクラッシュ）。
- 7B モデルの初回ロードは 2〜3 分かかる。`LlamaStdioBackend`の固定timeoutを根拠なく短くするな。
- **スキーマ移行時は grep で全参照を潰せ。** 過去に `fixed_attributes` の `age` → `birthday` 移行で TUI とテストに古い `#fixed-age` 参照が残り、SETTINGS タブが実行時クラッシュした。フィールド名・cmd 名・イベント名を変えたら `rg <旧名>` をリポジトリ全体に必ず実行し、ヒット 0 を確認してから完了と言え。
- llama.cppのライフサイクル: 推論ごとに新しい所有子をspawnし、そのPIDだけをstop対象にする。listener探索・外部プロセス再利用・三回目の再送を追加するな。`atexit`登録済み。ゾンビを残す変更をするな。

---

## 3. コード生成のトーン＆マナー (Code Generation Guidelines)

### 3.1 全言語共通

- **最小 diff。** 依頼されていないリファクタ・リネーム・整形をするな。
- コメントは非自明なビジネスロジックのみ。「何をしているか」を説明するコメントは書くな。書くのは「なぜそうでなければならないか」だけ。
- コミットはユーザーが明示的に指示した時のみ。

### 3.2 Python — 提出前チェックリスト（全項目 YES になるまで出すな）

1. **ループの計算量を言え。** ネストループを書いたら、N が何で最悪何回回るかをコメントか PR 説明で言語化せよ。日記が 10 年分（~3,650 日 × 複数ソース）でも耐えるか？ O(N²) は原則書き直し。
2. **ファイル I/O をループに入れるな。** `load_calendar()` / `load_finance()` のような JSON 全読みを for の中で呼んだら即修正。ループ外で 1 回読み、dict で引け。
   未変更ファイルの derived count（`import.stats` の diary/LINE 等）を毎回全文再計算するな。
   process-local の bounded metadata cache（file identity = `st_dev`/`st_ino` + `st_size` + `st_mtime_ns`）を使え。
   `st_ino == 0` で identity 不明なら cache せず再走査。scan 前後の fingerprint が一致したときだけ保存。
   既知の書込経路では書込試行の前に invalidate。scanner 例外結果を cache するな。
   TTL / timer / background thread / sidecar / content-hash 全読込による cache は禁止。
3. **メモリコピーを数えろ。** numpy では `copy()` / `tolist()` / 不要な `astype` を疑え。ベクトルは `float32` 統一。`np.frombuffer` + view で済むところに reshape コピーを重ねるな。
4. **遅延 import を守れ。** 重い依存（numpy, sentence-transformers, urllib）は使う関数の中で import する（起動時間 = UI の体感速度）。`engine_stdio.py` の ready 送出前に重い import を足すな。
5. **エンコーディング明示。** `open`/`read_text`/`write_text`/`subprocess` に `encoding="utf-8"` が付いているか。
6. **例外は握り潰すな、ただし UI は死なせるな。** core では具体的な例外を投げ、stdio 境界（`engine_stdio.main`）で捕まえて `{"ok": false, "error": ...}` に変換する。この二層構造を守れ。

### 3.3 C++ — 提出前チェックリスト

1. ホットループ（内積・Top-K）に分岐・仮想呼び出し・ヒープ確保を入れるな。データレイアウトは AoSoA 4-lane 前提。SoA/AoS に「わかりやすいから」で戻すな。
2. NEON intrinsics を触るなら、まずスカラー版で正しさを固めてからベクトル化し、`tests/benchmark.py` で前後の QPS を計測して数字を出せ。**計測なしの「速くなったはず」は禁止。**
3. mmap 読みのバイナリ (`PKBVEC01`) のヘッダ/ブロック構造を変えるなら、Python `pipeline.py` の struct と同時に変更し、magic のバージョンを上げろ。
4. OpenMP の並列化は Top-K のローカルマージ構造を壊さないこと。共有 heap への直書きはレース。

### 3.4 React UI — 「派手さ」ではなく「情報の密度」

このアプリの利用者は毎日の記録と意思決定のために使う。**装飾は 1 ピクセルも要らない。1 画面に載る情報量と入力の速さがすべて。**

1. アニメーション・グラデーション・カード影・ヒーローセクションを追加するな。既存の `App.css` のダークトーン（#0f1419 系）とコンポーネントクラスを再利用せよ。新しい色を発明するな。
2. 新 UI ライブラリ（MUI, Tailwind, framer-motion 等）を導入するな。現状は素の React + CSS で足りている。依存追加はバンドル・起動時間・オフライン保証の全部を悪化させる。
3. 情報を隠すな。モーダルやアコーディオンで畳む前に、一覧で見せられないか考えろ。クリック数を増やす変更はUX改悪である。
4. キーボード操作を守れ。保存は Ctrl+S、送信は Ctrl+Enter。新機能にもキーボード経路を必ず用意しろ。
   メインタブは WAI-ARIA Tabs の manual activation に従え: `tablist` / `tab` / `tabpanel`、
   `aria-selected`、決定論的 ID の相互参照（`aria-controls` / `aria-labelledby`）、roving `tabIndex`。
   ArrowLeft/Right/Home/End は focus 移動のみ（`setTab` 禁止）。Enter/Space（native button）と
   click で activation。マウント時に IPC を開始し得るため、focus 即 activation は禁止。
   ARIA ID 参照のための空 tabpanel shell のみ常設可。inactive の React component keep-alive は禁止。
   フォーカス可視化（`:focus-visible` outline）を必須とする。
5. 状態は最小に。サーバー状態（エンジンからのデータ）を複数コンポーネントに複製するな。取得はタブのマウント時、更新は保存成功時の再取得で足りる。
6. `useEffect` のイベントリスナー（Tauri `listen` 含む）は必ずクリーンアップを返せ。
7. 長時間処理（consult, profiler, import）は必ず「進捗の見える化」をセットで実装しろ。busy フラグでボタンを殺すだけの UI は不合格。status イベント（2.1 参照）を表示せよ。

### 3.5 完了の定義 (Definition of Done)

以下を全部通してから「完了」と報告せよ。エラーが出たら自分で直せ。
件数・module 数・所要時間などの固定スナップショットを DoD に書くな（陳腐化する）。

```powershell
# --- リポジトリルート ---
python -m py_compile <変更した .py 全部>
python -m pytest tests/ -q
python -m pytest tests/test_ui_smoke.py -q  # TUI スモーク (textual 不在なら SKIP)
git diff --check

# --- apps/desktop ---
cd apps/desktop
npm.cmd run test:boundary
# 境界 runtime。通常の `npx.cmd tsc --noEmit` は tests-runtime を対象にしないため代替不可。
# fixture 生成と `"type":"module"` 下の CommonJS 回避は runner 内。手動再現するな。入口はこれだけ。
npx.cmd tsc --noEmit
npm.cmd run build

# --- apps/desktop/src-tauri (Rust 変更時は必須。無変更でも回帰確認推奨) ---
cd src-tauri
cargo test
cargo check
```

**テスト運用メモ**: 単体実行は `python -m pytest tests/test_x.py`。
`python tests/test_x.py` の `__main__` 直接実行は廃止（conftest の Sandbox
隔離を迂回するため）。各 `test_*.py` 先頭の `setdefault("PKB_PROJECT_ROOT")`
は pytest 非経由 import 時の後方互換用で、pytest 下では conftest が先に env
を確定させ no-op に縮退する。

報告には「何を変えたか」「なぜか」「何で検証したか」を必ず含めろ。テストが通らないまま完了と言うことは、いかなる理由があっても禁止する。

---

## 4. macOS 配布・公証 (Code Signing & Notarization) Skills

**次世代 AI エージェントへの命令書。** macOS 配布は「ビルドが通る」と「ユーザーがダブルクリックで起動できる」の間に Gatekeeper という壁がある。以下を一言一句守れ。

### 4.0 このリポジトリの macOS ビルドの前提（まず暗記しろ）

- Sidecar は **`pkb-engine`（stdio JSON エンジン、エントリ `run_engine.py`）** である。FastAPI / HTTP サーバーは存在しないし、設計原則（完全オフライン）により**今後も導入禁止**。「pkb-api」という名前が指示に出てきたらそれは古い/誤った情報であり、`pkb-engine` に読み替えろ。
- `externalBin: ["binaries/pkb-engine"]` は**二重化不要**。Tauri が target triple を自動付与して解決する: Windows は `pkb-engine-aarch64-pc-windows-msvc.exe`、macOS は `pkb-engine-aarch64-apple-darwin` / `pkb-engine-x86_64-apple-darwin`。**やることは正しいファイル名でバイナリを置くことだけ**（Windows: `scripts/build-engine.ps1`、macOS: `scripts/build-sidecar.sh`）。ファイルが無いと `tauri build` はその場で失敗する。
- プラットフォーム差分は `tauri.conf.json` 本体ではなく **`tauri.macos.conf.json`**（自動マージされる platform-specific config）に書く。Windows の挙動を変えずに macOS を足すのが原則。
- データルート: Windows `%LOCALAPPDATA%\PKB` / macOS production `~/Library/Containers/com.ai-shizu.pkb/Data/Library/Application Support/PKB` / macOS非sandbox dev `~/Library/Application Support/PKB`（`src-tauri/src/paths.rs::user_data_root()` の cfg 分岐）。releaseは`PKB_PROJECT_ROOT`とrepo探索を無視する。パスを変えるならここ**だけ**を変えろ。
- ネイティブ実行ファイル名は Python 側で `core/paths.py` の `SEARCH_EXE` / `LLAMA_CLI_EXE` に集約済み（Windows のみ `.exe`）。**`"xxx.exe"` という文字列リテラルを新たに書いた時点で不合格。**

### 4.1 GitHub Actions — Mac 用 (aarch64 / x86_64) ビルド構成

実物は `.github/workflows/build-macos.yml`。構成の要点:

1. **PyInstaller はクロスコンパイル不可。** これが全構成を支配する制約である。
   - aarch64 (Apple Silicon) → `macos-14` 以降のランナー（arm64 ネイティブ）
   - x86_64 (Intel) → `macos-13` ランナー（x86_64 ネイティブ）
   - `macos-latest` は arm64 である。**「latest 1 本で両アーキ」は Python sidecar がある限り不可能。** matrix で分けろ:

```yaml
strategy:
  matrix:
    include:
      - { os: macos-14, target: aarch64-apple-darwin }
      - { os: macos-13, target: x86_64-apple-darwin }
steps:
  - run: bash build.sh --skip-py                      # C++ (NEON は aarch64 で自動有効)
  - run: bash apps/desktop/scripts/build-sidecar.sh   # Python sidecar (ネイティブビルド)
  - run: npx tauri build --target ${{ matrix.target }}
    working-directory: apps/desktop
```

2. **ユニバーサルバイナリ (`--target universal-apple-darwin`) は使うな。** Rust 側は lipo で結合できるが、PyInstaller 製 sidecar と C++ exe が単アーキのままなので不整合になる。per-arch の DMG を 2 つ配布する方が確実で、サイズも半分になる。どうしても 1 本にしたければ、両ランナーの成果物を `lipo -create -output pkb-engine pkb-engine-x86_64 pkb-engine-arm64` で自分で結合してから universal ビルドに載せる必要がある（工数に見合わない。やるな）。
3. **C++ の指定:** aarch64 は AArch64 の仕様として NEON 必須なので `__ARM_NEON` が自動定義され、既存の NEON パスがそのまま有効になる（macOS では `-march` 指定不要。Linux のみ `-march=armv8-a+simd`）。x86_64 に NEON パスは無い — スカラー + 自動ベクトル化で `-march=x86-64-v2`。OpenMP は Apple clang に同梱されないため `brew install libomp` + `-Xpreprocessor -fopenmp -lomp`、失敗時は直列フォールバック（`build.sh` 実装済み）。

### 4.2 Apple Developer ID 署名・公証の自動化テンプレート

Tauri v2 は**環境変数が設定されているだけ**で署名→公証→ステープルまで自動実行する。コードを書くな。env を渡せ。

```yaml
- name: Tauri build (署名 + 公証込み)
  working-directory: apps/desktop
  env:
    # ── コード署名 (3点セット) ──
    # APPLE_CERTIFICATE: Developer ID Application 証明書 (.p12) の base64
    #   作成: base64 -i certificate.p12 | pbcopy
    APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE }}
    APPLE_CERTIFICATE_PASSWORD: ${{ secrets.APPLE_CERTIFICATE_PASSWORD }}
    # 例: "Developer ID Application: Taro Yamada (TEAM123456)"
    APPLE_SIGNING_IDENTITY: ${{ secrets.APPLE_SIGNING_IDENTITY }}
    # ── 公証 (3点セット / App用パスワード方式) ──
    APPLE_ID: ${{ secrets.APPLE_ID }}                # Apple ID メールアドレス
    APPLE_PASSWORD: ${{ secrets.APPLE_PASSWORD }}    # https://account.apple.com で発行した App 用パスワード
    APPLE_TEAM_ID: ${{ secrets.APPLE_TEAM_ID }}      # 10桁の Team ID
  run: npx tauri build --target ${{ matrix.target }}
```

- 代替: `tauri-apps/tauri-action@v0` を使う場合も **env の渡し方は完全に同一**（action の `with:` ではなく `env:` に渡す）。API キー方式なら `APPLE_API_ISSUER` / `APPLE_API_KEY` / `APPLE_API_KEY_PATH` の 3 点に置き換え可。
- **絶対規則:**
  - 証明書は必ず **Developer ID Application**（App Store 用の "Apple Distribution" とは別物。間違えると公証は通るが Gatekeeper に弾かれる）。
  - `APPLE_PASSWORD` は Apple ID のログインパスワードではない。**App 用パスワード**を発行して使え。
  - 公証には **Hardened Runtime 必須** — `tauri.macos.conf.json` の `"hardenedRuntime": true` を消すな。
  - 親アプリは`com.apple.security.app-sandbox=true`を必須とし、`com.apple.security.network.client/server`を一切持たせない。PyInstaller製sidecarは`sidecar-entitlements.plist`の`app-sandbox + inherit`だけで別途署名し、CIで実体から再抽出して検証する。親のHardened Runtime例外（`allow-unsigned-executable-memory` / `disable-library-validation`）と子の継承entitlementを混同するな。
  - env 未設定なら Tauri は ad-hoc 署名でビルドを完走させる（CI の PR ビルドはこれで良い）。「secrets が無いから」とワークフローを分岐で複雑化するな。
- **手動での公証確認コマンド**（失敗調査時に上から順に）:

```bash
codesign -dv --verbose=4 PKB.app                 # 署名の確認 (Authority が Developer ID か)
codesign --verify --deep --strict PKB.app        # 全ネストバイナリの署名検証
xcrun notarytool history --apple-id $APPLE_ID --team-id $APPLE_TEAM_ID --password $APPLE_PASSWORD
xcrun notarytool log <submission-id> ...         # 公証 rejected の理由 (JSON) を取得
spctl -a -vv PKB.app                             # Gatekeeper の最終判定 ("accepted" が出れば勝ち)
xcrun stapler validate PKB.dmg                   # ステープル確認 (オフライン起動に必須)
```

### 4.3 軽量モデルへの厳命 — 自己チェックリスト

**A. App Sandboxとカレンダー取込:**

1. productionのApp Sandboxは**必須**であり、network client/server entitlementは追加禁止。`entitlements.plist`、`sidecar-entitlements.plist`、`build-sidecar.sh`、macOS CIの4点を同時に検証する。
2. App SandboxとCalendar.app SQLiteの任意パス直読みは非互換である。production UIではApple DB直読を利用可能と表示せず、ユーザーがCalendarから書き出したICSのローカル取込だけを正規経路とする。
3. `FULL_DISK_ACCESS_HINT`とSQLite parserは非sandbox開発・既存単体テスト用として残すが、release機能であると説明してはならない。App Sandboxを外す回避策は禁止。
4. 将来直接同期を復活させるなら、SQLite直読ではなくEventKit + usage description +最小entitlementを別Findingで設計・検収する。

**B. アーキテクチャ不一致 — ビルドのたびに機械的に検証しろ（目視判断禁止）:**

```bash
uname -m                                   # いま居る環境: arm64 か x86_64 か
rustc -vV | grep host                      # Rust のホスト triple
file build/search_engine                   # C++ exe のアーキ
file apps/desktop/src-tauri/binaries/pkb-engine-*   # sidecar のアーキ
lipo -info <binary>                        # universal か単アーキか
python3 -c "import platform; print(platform.machine())"  # Python 自体のアーキ
```

チェック項目（1 つでも NO なら出荷禁止）:
- [ ] `tauri build --target X` の X と、sidecar バイナリのファイル名 triple と、`file` が示す実アーキの **3 つが一致**しているか
- [ ] Rosetta 下の x86_64 Python で arm64 向けをビルドしていないか（`platform.machine()` で検出。PyInstaller は Python 自体のアーキでしか作れない）
- [ ] Homebrew の混在に注意: Apple Silicon は `/opt/homebrew`、Intel は `/usr/local`。x86_64 ランナーで `/opt/homebrew` を参照したら混在事故
- [ ] CI の matrix で `uname -m` アサーションを消していないか（`build-macos.yml` に実装済み。「冗長だから」と削除するな — それはお前のためのガードレールだ）

**C. macOS 移植で踏みがちな地雷（このリポジトリで実際に対処済みのもの）:**

- [ ] `.exe` ハードコード → `core/paths.py` の `SEARCH_EXE` / `LLAMA_CLI_EXE` を使ったか
- [ ] `beforeBuildCommand` が PowerShell のまま → macOS 差分は `tauri.macos.conf.json` で bash スクリプトに上書き済み。Windows 側 conf を書き換えるな
- [ ] シェルスクリプトの改行が CRLF になっていないか（Git for Windows で編集した .sh は要注意。`bash: /bin/bash^M` エラーの原因。`.gitattributes` か `git config core.autocrlf` で LF を保証しろ）
- [ ] データルートの分岐（`paths.rs`）を触った後、**Windows / macOS / Linux の 3 分岐全部**が `cargo check` を通るか（cfg ブロックは書いた環境でしかコンパイル検証されないことを忘れるな）

---

## 5. モデル戦略 (LLM Selection Policy)

**単一の真実は `config/model_params.json`。** モデル名・量子化・runtime引数・生成パラメータをコードにハードコードするな。読み込みは `core/llm_config.py` に集約されており、優先順位は **環境変数 > config/model_params.json > 組み込みデフォルト** である。

| 役割 | モデル | 理由 |
|---|---|---|
| `default`（profiler / 汎用） | **Qwen2.5-7B-Instruct (IQ4_XS 優先、次点 Q4_K_M)** | 日本語性能と 8GB 級メモリでの安定動作の均衡点 |
| `consult`（相談） | **DeepSeek-R1-Distill-Qwen-7B**（無ければ default へフォールバック） | 推論特化蒸留。意思決定の多段推論に強い |

**規則（違反したらレビューで落とせ）:**

1. **7B 未満のモデルは原則禁止。** `find_gguf()` はサイズ下限 `min_model_bytes`（既定 3.5GB）未満を候補から除外する。検証目的で小型モデルが要る時だけ `PKB_ALLOW_SMALL_LLM=1`（または config の `allow_small_models`）で解除しろ。恒久的に緩めるな — 7B 未満は日本語の深層プロファイル分析で使い物にならないという実測に基づく方針である。
2. **サイズ下限を 4GB に「戻す」な。** IQ4_XS 量子化の 7B は約 3.9GB であり、4GB 下限は正規モデルを不完全ダウンロード扱いで弾く（実際に踏んだバグ）。下限は 3.5GB。
3. **モデルの追加・変更は config の `roles.*.preferred` を編集するだけ**にしろ。glob パターン（`DeepSeek-R1-Distill-Qwen-7B*.gguf`）が使える。`llm_config.py` の `_DEFAULT_PARAMS` は config 不在時のフォールバックなので、config と同時に更新して同期を保て。
4. **DeepSeek-R1 系は `<think>…</think>` を出力する。** `consult()` が最終応答から除去済み（`consultation_engine.py`）。ストリーミング中は思考過程が見えるが、最終置換でクリーンになる仕様。この除去を消すと保存ログと DailyContext が思考過程で汚染される。
5. 生成パラメータ（temperature / max_tokens）は `generation_params()` 経由で取れ。`0.6` や `900` を直書きするな。
6. モデルは `models/` に手動配置（gitignore 済み）。**ダウンロードを自動化するコードを書くな** — オフライン原則違反である。
7. **LLM transportの唯一所有者は`core/llm_backend.py`、prompt channelの唯一所有者は`core/llm_transport.py`。** prompt本文をargv・環境・通常ファイルへ置くな。Windows Named Pipeは`PIPE_REJECT_REMOTE_CLIENTS` + first-instance + owner/SYSTEM/AppContainer SID DACL、POSIXは`/dev/stdin`以外を認めない。子はengineのOS sandboxを継承する。TCP/HTTP、listener、port、cloud fallbackを再導入するな。
8. **model / generation / stdio commandの正本は`llm_config.py`。** `llama_stdio_cmd()`は`completion` + `--offline` +固定local modelのみ。`--rpc` / remote model option / remote環境変数は禁止。クライアントへtemperature・max_tokens・ctxを複製するな。
9. **レガシーCLI（`app.py` → `cli.py`）を「古い」という理由だけで削除するな。** 固有retrieval / prompt / `--show-prompt` / `--top-k` / interactive loopは維持し、shared `LlamaStdioBackend`だけを使う。相談modelは`find_gguf(role="consult")`。通常契約テストはnetworkless fake、transport検収時だけ公開promptのローカルGGUFを使う。

---

## 6. 高度推論とプロファイリング戦略: 主観と客観の差分分析 (Gap Analysis)

**このセクションは PKB の「頭脳」の設計図である。実装は `core/gap_analysis.py`。ここに書かれた推論ステップを理解せずに同ファイルを編集することを禁止する。**

### 6.1 設計思想 — なぜ「差分」なのか

PKB のプロファイリングは 2 世代で構成される:

- 第1世代（`profiler.py` §1〜8）: 各データソースを**個別に**分析する（バイアス語彙、価値観頻度、対話セッション）。これは「本人が書いたこと」の要約であり、本人の自己申告バイアスをそのまま継承する限界がある。
- 第2世代（`gap_analysis.py`）: データを**主観と客観の 2 軸に分離して突合**する。人間の自己認識の歪みは「言っていること」単体からは見えず、「言っていること」と「やっていること」の**差分**にだけ現れる。これが本モジュールの存在理由である。

| 軸 | ソース | 意味 |
|---|---|---|
| 主観 (What you think) | 日記 + AI相談の Query | 私的な内省・感情・願望 — 自己申告 |
| 客観 (What you do) | 家計簿（金額）+ カレンダー（時間）+ **LINE の自分発話** | 資源の実際の投じ先 — 反証可能なハードデータ |

検出する差分は 2 方向のみ（増やす場合もこの二項対立を維持せよ）:
- **intention_gap（意図過剰）**: 主観で頻出、行動が伴わない。「キャリアに焦っている（日記）のに、学習に金も時間も使っていない（家計簿・予定）」— 認知的不協和。
- **blind_spot（盲点）**: 資源を大きく投じているのに内省ゼロ。「娯楽に月3.5万使っているのに日記で一度も触れない」— 無自覚の行動パターン。

### 6.2 絶対に削ってはいけない推論ステップ

軽量モデルへ厳命する。以下は 1 つでも削るとシステム全体が「心地よい自己申告の要約機」に退化する。

1. **【最重要】LINE 等の他者コミュニケーションログは客観軸。絶対に主観軸へ移すな。**
   - 根拠: 他者への発話は発した瞬間に社会的拘束（コミットメント）が生まれるため、私的な日記より行動に近い。「日記に書く転職願望」と「友人に語る転職活動」は証拠能力が違う。
   - `build_subjective_corpus()` が `line_self_text` を含まないのは**バグではなく設計**。これを「データを増やすため」に混入させると、自己申告の二重計上でギャップ検出の意味が消滅する（`test_gap_analysis.py::test_subjective_objective_separation` がこの原則を守る回帰テスト。落ちたら実装ではなくお前の変更が間違っている）。
   - 客観軸の合成では LINE 言及シェアを金・時間と**等重（1/3）**で扱う。「テキストだから」と軽くするな。
2. **決定論コアと LLM の役割分離を崩すな。** ギャップの**発見**は決定論アルゴリズム（スコア差分 ≥ 0.25）が行い、LLM は検出済みギャップの**言語化のみ**を担う（`llm_gap_analysis` のプロンプトが「新たなギャップを創作しない」と明示している）。LLM にギャップ発見自体を任せると、実行のたびに結果が変わり、幻覚のギャップで本人を誤誘導する。
3. **全ギャップに定量証拠（金額・件数・日記引用）を添付する構造を維持せよ。** 反証可能性が「残酷な事実」を伝える倫理的な最低条件である。証拠のない指摘は削れ。
4. **`data_sufficiency` による確度の自己申告を消すな。** 3 日分のデータで人格を断定するのは分析ではなく偏見である。閾値 0.5 未満で「断定を避けよ」の注記がプロンプトに載る仕組みを保て。
5. **収入 (income) を支出集計に混ぜるな。** 「金の投じ先」だけが意思の証拠であり、給与入金は本人の選択ではない。

### 6.3 CONSULT への強制注入と R1 メタ認知プロンプト

- `ConsultationEngine._gap_section()` が deep_profile の `gap_analysis` を**毎回の相談プロンプトに強制注入**する。相談文そのものがユーザーの主観の産物である以上、注入をオプション化してはならない。
- `SYSTEM_PROMPT` は DeepSeek-R1 系の思考フェーズ（`<think>`）を前提に、回答前の 3 点検証を命じている:
  1. この相談は本人の主観バイアスの産物ではないか（ギャップ表と照合）
  2. 行動データ（支出・予定・発話）が示す事実は何か — **自己申告と矛盾したら行動データを信頼**
  3. これから出す助言はギャップを埋めるか、思い込みを心地よく強化するだけか
- この指示は R1 以外（Qwen 等、think を持たないモデル）でも「回答前の内部検証」として機能するよう、`<think>` タグ名に依存しない書き方をしている。**タグ名をハードコードした指示に書き換えるな**（モデル交換で壊れる）。
- `<think>` ブロックは `consult()` が最終応答・保存ログから除去する（§5 参照）。思考は検証装置であり、成果物ではない。

### 6.4 非線形評価モデル (v2 拡張 — 就活・自己分析特化)

線形差分ベースライン (§6.1) の上に、3 つの非線形モデルが `gaps` リストへ追加合流する（stdlib のみ・決定論）。**フラグの型名 (`type`) は CONSULT の `format_gap_table` ラベル表と対応しており、片方だけ変えるな。**

1. **`task_avoidance`（双曲割引によるタスク逃避検知）** — `analyze_procrastination()`
   - 宣言（日記・相談でタスク語彙 `TASK_LEXICON` + 意思マーカー共起）→ 実行（カレンダー予定 / LINE+完了マーカー / 日記+完了マーカー）の遅延 D 日を、**双曲割引 `V = 1/(1 + 0.3·D)`** で評価。未実行は V=0。タスク別 `avoidance_index = 1 − mean(V)` ≥ 0.5 でフラグ。
   - **指数割引に「修正」するな。** 人間の先延ばしは双曲割引に従う（遅延初期の価値毀損が急峻）というのが行動経済学の実証であり、意図的な選択である。
   - 宣言検出は「意思マーカーあり・完了マーカーなし」の両条件必須。片方を外すと「面接を受けた」（過去の実行報告）を宣言と誤認する。
2. **`true_gakuchika`（乖離ペアによる真の熱量発掘）** — `analyze_gakuchika()`
   - 主観首位テーマ（シェア ≥ 0.3）に行動が伴わず（客観が実熱量テーマの 1/2 未満）、別テーマに客観資源が集中（≥ 0.35）している時、前者を**建前/サンクコスト**、後者を**真の熱量（ガクチカ候補）**とラベリング。
   - 目的は断罪ではなく武器の発掘 — 「金融志望と焦って語るが、実際は部活マネジメントに 7.5 万円と週次の時間を投じている」なら、語るべきエピソードは後者にある、という自己アピール資源の転換。insight には必ず両テーマ名と定量値を含めること。
3. **`intellectualization_gap`（防衛機制「知性化」の検知）** — `analyze_intellectualization()`
   - ISO 週単位で「抽象語インデックス（`ABSTRACT_LEXICON` ヒット数）」と「就活アクション数（`JOBHUNT_ACTION_KEYWORDS` にマッチする予定・発話）」を突合。**アクション 0 かつ 抽象語 ≥ 3 かつ 全週平均の 1.5 倍以上**の週のみフラグ。
   - **アクション条件を外すな。** 抽象的な内省それ自体は健全であり、病的なのは「行動が止まった週にだけ内省が難解化する」相関である。アクションのある週は同じ抽象度でもフラグしない（`test_intellectualization_detection` の対照群が守る）。
4. **`stabilizer_effect`（ライフバランス・スタビライザー評価）** — `analyze_life_balance()`
   - 「私的時間の予定（`PRIVATE_TIME_KEYWORDS`）→ 当日/翌日の日記に罪悪感（`GUILT_MARKERS`）→ 事後 2 日間の生産性シグナル（`PRODUCTIVITY_MARKERS`）が事前 2 日間より増加」の 3 点が揃った時のみ、その時間を「浪費ではなく必要な充電」と**肯定的に**フラグする。
   - これは本システムで唯一の**ポジティブ・フラグ**である。他のギャップと同じ「欠陥の指摘」に書き換えるな — 罪悪感（Productivity_Guilt_Trap）を行動データで**反証**することが目的であり、全人的メンタリングの中核。
   - **事後効率の向上条件を外すな。** 向上がないのに私的時間を無条件に肯定すると、単なる現状追認botになる（`test_life_balance_stabilizer` の対照群が守る）。

テーマ体系には「課外活動・組織運営」「技術開発・ものづくり」を追加済み（テーマ追加時の 4 点セット規則は §6.5 参照）。回帰テストは `tests/test_gap_analysis.py` の 7 ケース — 双曲減衰の単調性・対照群（翌日実行は非フラグ / アクション週は非フラグ）を含み、**対照群アサーションを「テストを通すため」に消すことを禁止する**。

### 6.5 メンテナンス指針

- テーマ追加は `THEME_TAXONOMY` に「主観語彙 / 支出カテゴリ / 予定語彙 / LINE語彙」の**4 点セット**で足せ。どれかを省略したテーマは片軸しか観測できず、必ず偽ギャップを生む。
- 閾値 (`GAP_THRESHOLD = 0.25`) を変える時は `tests/test_gap_analysis.py` の全ケースを通した上で、実データの deep_profile を目視し偽陽性を確認せよ。感度を上げたい誘惑に負けて 0.1 まで下げると、全テーマがギャップ扱いになり信号が死ぬ。
- 出力スキーマは `deep_profile.v6` の `gap_analysis` キー。スキーマを変えたら `_gap_section()`（CONSULT 読み手）と `update_user_profile()`（`gap_insights` 抽出）の両方を追従させろ。読み手を忘れると「profiler は動くのに相談に反映されない」というサイレント故障になる。

---

## 7. CONSULT 拡張: interview_sim と外部知識インジェクション

### 7.1 面接シミュレーター (`mode="interview_sim"`) — ES 駆動・敵対的構造

- 入口は `consult(query, mode="interview_sim")`（facade / stdio の `params.mode` から到達）。通常 consult と違い**ベクトル検索・インデックス同期を行わない** — 状態機械 (出題→議論→講評) とプロンプト構築のみ。重くするな。
- `data/es/` に ES があれば **ES 駆動の敵対的面接**（面接官ペルソナは `es_manager.build_interviewer_persona()` が ES のターゲットドメインから動的生成、ES の矛盾・選択を悪意を持って攻撃する指示付き）。無ければ `INTERVIEW_CASE_BANK` の決定論的巡回にフォールバック。乱数を入れるな。
- **【情報の非対称性 — 本機能の魂。変更禁止】** 出題・議論フェーズのプロンプトには **ES とトランスクリプトのみ**を与え、`_gap_section()`（gap_insights）は絶対に注入しない。面接官が候補者の日常プロファイルを知っている状況は本番に存在せず、漏らした瞬間にストレステストの価値が消える。`test_integration.py` の `_assert_no_gap_leak` が回帰ガード — **このアサーションを消す変更は情報漏洩バグの導入である。**
- 講評フェーズで初めて **ES + 会話録 + gap_insights を全統合**し、「面接で露呈した防御の甘さが日常のどの行動（摩擦回避・閉じこもり・タスク逃避）に起因するか」を突きつける。この非対称構造（隔離→統合）が「面接は日常の縮図」という洞察を生む装置であり、どちらか片方だけにする変更は機能の削除に等しい。
- セッション状態 (`_interview_state` / `_gd_state`) は**エンジンプロセスのメモリ内のみ**。永続化するな。講評だけが `[interview_sim]` / `[gd_sim]` / `[es_review]` プレフィックス付きで相談履歴に保存され、DailyContext に入る。

### 7.1.1 ES 管理 (`core/es_manager.py`) と ES 添削 (`mode="es_review"`)

- ES/企画書は `data/es/*.md|*.txt`。ターゲットドメインは**常に ES テキスト自身から導出**する: 明示フィールド（`志望職種:` 等）優先、なければ頻度ベースのキーワード抽出。**特定業界の if-elif をここに書くな** — テック・金融・クリエイティブ・アカデミアのどの ES を置いてもペルソナが自動追従するのが本設計の価値（`test_es_manager_dynamic_domain` の映像制作ケースが回帰ガード）。
- `mode="es_review"` は**ドキュメント単体の論理的強度のみ**をテストする。**gap_insights を絶対に注入するな** — 混ぜると「書類が弱いのか、人（日常行動）が弱いのか」の切り分けが不可能になる。interview_sim（講評で統合する）との役割分担であり、「せっかくあるデータだから」と注入する変更は両モードの存在意義を同時に壊す。

### 7.1.2 カオス GD シミュレーター (`mode="gd_sim"`)

- 1 回の LLM 応答内で **N 人（1〜9、`MAX_GD_PERSONAS`）のペルソナを同時演技**する多重人格プロンプト。ペルソナ配列はフロントエンドのロビー画面から `personas: [{name, trait}]` で渡され、`build_gd_system_prompt()` が動的展開する。trait はプリセット（`PRESET_PERSONA_TRAITS`）または自由記述（そのまま挙動指示になる）。**未指定時は既定 3 人構成（`GD_SYSTEM_PROMPT` 定数と完全一致）— この後方互換を壊すな。**
- **司会者・まとめ役を追加するな** — カオスを収束させる存在を入れた瞬間、ユーザーが摩擦に介入する必然性が消え、シミュレーションが接待になる。
- 議論フェーズは gap 隔離（interview_sim と同一原則）。講評フェーズで gap_insights と統合し、「フリーライダー放置・クラッシャーへの敗北 = 日常の人間関係の摩擦 (Friction) からの逃避と同型」という接続を必ず含める。GD テクニックではなく**日常の対人行動**の改善アクションを要求する構造を維持せよ。

### 7.1.3 応答速度 (Response Latency) の評価

- UI（`InterviewTab.tsx`）が「AI メッセージ表示 → ユーザー送信」の経過秒を計測し、`response_time_sec` として stdio ペイロードに載せる。バックエンドはターンプロンプトに注記し、セッション state の `latencies` に蓄積、講評プロンプトの Response Latency セクション（`_format_latency_section`）で「トップティア（HFT・戦略コンサル）基準での思考速度」を評価対象に含めさせる。
- **計測は UI 側の責務**（バックエンドは受け取った値を信じる）。ネットワーク遅延や LLM 生成時間を含めるな — 計測起点は「AI メッセージの表示完了時刻」である。

### 7.1.4 建前人格 (`is_simulated_persona`) の隔離 — 最重要アーキテクチャ規則

面接・GD・ES 添削でユーザーが発するテキストは「選考用に演じられた建前」であり、**日記と同じ主観データとして扱った瞬間、Gap 分析のベクトル空間が虚飾で汚染され、システム全体が自己欺瞞の増幅器になる。**

- シミュレーター由来の相談ログは `append_consultation(..., simulated=True)` で `is_simulated_persona: true` が付与される。**新しいシミュレーターモードを追加する時、このフラグを付け忘れることが最も起きやすい退化バグ**（`test_simulated_persona_isolation` が回帰ガード）。
- 重み付けの規約: `gap_analysis` の主観スコアでは `SIMULATED_PERSONA_WEIGHT = 0.1`（重み機構があるため 0.1）。`profiler` の自己テキスト収集（`extract_user_queries`）では**除外**（ルールベース分析に重み機構がないため 0/1 でしか制御できず、建前は 0 が正）。この非対称は意図的である。
- 建前 doc からは Gap の「引用証拠」も「先延ばしの宣言」も採らない（面接で語った「毎日 LeetCode を解いています」は日常の宣言ではない）。
- DailyContext のレンダリングとベクトル検索インデックスには建前ログも含まれる（**記録**としては本物）。除外するのは**自己分析チャネル**だけ。「記録と分析の分離」原則（§1）の適用例である。

### 7.2 外部知識キュー (`core/knowledge_fetcher.py`) — E0a 封鎖中

Phase 4-E **E0a EMERGENCY EGRESS LOCKDOWN** により、外向き knowledge fetch は無条件封鎖されている。

- `_http_get` / `default_online_fetcher` / `process_pending` / `facade.fetch_pending_knowledge` は
  `NotImplementedError("Egress blocked by E0a strict lockdown.")` を raise する一文 stub。
- `online_fetch_allowed()` は常に `False`。`PKB_ALLOW_ONLINE_FETCH` や mock fetcher では解除できない。
- legacy `_fetch_queue.json` と `extract_fetch_queries` / `queue_fetch_queries` / `ingest_results` は
  **ローカル専用**として残るが、キューから外へ送出する経路は無い。
- consult の SYSTEM_PROMPT に外向き検索タグ生成指示は無い。生成後のタグ抽出・キュー書込 hook も無い。
- ローカル knowledge の手動配置 → `sync_knowledge_index()` → `knowledge_hits` 検索注入は維持（オフライン充足）。

**テスト規律**: 実ネットワークを叩くテストを書くな。E0a 契約は `tests/test_e0a_egress_lockdown.py`。

---

## 8. Target Alpha: KV slot cache — RETIRED by FSA-2026-07-13-01/02

旧HTTP serverのslot save/restoreは、loopback listenerとprompt送信を必要とするため
2026-07-14のCommander裁定で廃止した。`core/kv_cache.py`、slot API、port設定、
`data/processed/kv_slots/`のproduction参照は削除済み。性能上の理由で復活させてはならない。

維持する契約はpromptの論理構造だけである。`build_static_prefix()`を先、
`build_dynamic_suffix()`（検索hit + Future Context +相談文）を後に連結し、動的情報を
静的側へ混入させない。これは`test_prompt_static_dynamic_split`が検証する。

推論は毎回、PKBがspawnした新しい`llama.cpp completion`子へ送る。Windowsでは
owner-only Named Pipe、macOS/Linuxでは`/dev/stdin`を`--file`へ指定し、prompt本文を
argv・環境・通常ファイルへ保存しない。旧KV最適化を再検討する場合も、TCP/HTTPや
共有listenerを使わない別FindingとしてRED契約と指揮官裁定を先に得ること。

---

## 9. Target Bravo: mmap ゼロコピー IPC (`search_engine --daemon` + `core/search_daemon.py`)

検索 1 回あたりの「プロセス生成 + 一時ファイル + stdout 正規表現パース」(実測 27.5 ms/query @10k vectors) を、常駐デーモンとの「stdio JSON 制御プレーン + mmap 共有 scratch データプレーン」(実測 109 µs/query、うち約 100 µs は検索カーネル自体 = IPC オーバーヘッド約 10 µs) に置き換える機構。**Step 1 (C++ デーモン + クライアント) / Step 2 (consultation_engine への配線) とも完遂済み。**

### 9.0 フォールバック連鎖とライフサイクル (Step 2 の不変条件)

- `search_index()` の検索経路は **デーモン → 1-shot exe → NumPy** の三段。**NumPy 分岐は exe 不在環境の生命線 — 削除禁止。** 「デーモンがあるから 1-shot は要らない」も禁止 (デーモン障害時の中速経路)。
- デーモンは **初回検索での遅延起動** (`_get_search_daemon()`)。LLM・埋め込みと同じ遅延初期化原則 (§1) — エンジン生成時に起動を早めるな。
- **障害時は drop して以後そのプロセスでは使わない** (`_search_daemon_failed`)。クラッシュするデーモンを検索のたびに respawn するとスポーンストームになる。ただし `shutdown()` は failed フラグを立てない (正常終了後の再利用は再起動してよい) — この非対称は意図的。
- **インデックス再構築の直前に必ず `_release_index_mapping(bin)` を呼ぶ** (`sync_diary_index` / `sync_knowledge_index` に配線済み)。remap 失敗時はデーモンごと close してプロセス死でマッピング解放を保証する — 「remap が失敗しても続行」と書いた瞬間、Windows で PermissionError の時限爆弾が復活する。新しいインデックス (新 .bin) を再構築するコードを書く時も同じ解放を忘れるな。再構築後の再マップは不要 (次の search が遅延リマップする)。
- `SearchDaemonClient` は **内部 Lock で search/remap を直列化**している (scratch と seq は 1 組しかない。TUI は同期ワーカーと consult が別スレッドで走り得る)。「速くするため」にロックを外すなら scratch を複数化してからにしろ。
- scratch の掃除は二重: 自プロセスは `close()` (atexit 登録) で unlink、他プロセスの残骸は `_init_scratch()` の sweep で削除 (生きているエンジンの scratch は OS がオープン中のため Windows では削除に失敗し自然にスキップされる)。

### 9.1 レイアウトの不変条件 (1 バイトのズレ = 静かな誤読 or SEGV)

1. **共有 scratch (`ScratchBuffer`, 2072 bytes, magic `"PKBSCR01"`) の対応物は 2 つだけ**: C++ `search_engine.cpp` (pack(1) + static_assert + offsetof で全オフセット釘付け) と Python `core/search_daemon.py` (`struct.calcsize` の import 時 assert)。**変えるなら両方同時 + magic バージョン更新** (PKBVEC01 と同規則)。`tests/test_search_daemon.py::test_layout_cross_validation` が数値を固定している。
2. レイアウトは**全フィールド自然整列** (`seq: u64` は offset 8、results 先頭は offset 1560 = 8 の倍数) になるよう設計してある。フィールドを足す時もこれを維持せよ — pack(1) は「暗黙パディング防止」であって「非整列アクセス許可」ではない。
3. results の未使用スロットは `chunk_id = -1`。PKBVEC01 のパディング/墓標規約と同一の意味論であり、Python 側は `cid >= 0` でフィルタする。この規約を別の意味に転用するな。
4. 同期は **seqlock 簡易版**: Python が query/top_k → seq の順に書いてから stdio でリクエスト、C++ は scratch の seq とリクエスト seq の一致を検証してから検索する。**stdio の 1 行往復がメモリバリアを兼ねる**ためロックは無い。この seq 検証を「冗長だから」と削ると、書き込み順序バグや別プロセスの割り込みが「古いクエリの検索結果が黙って返る」という最悪の形で現れる。

### 9.2 デーモンの規律

- **デーモンモードの stdout はプロトコル専用線** (Python engine_stdio と同一原則)。診断は全て stderr。1-shot モードの人間可読出力を daemon パスに漏らすな。
- **ゾンビ化防止は stdin の EOF 検知** (`std::getline` ループ終了 → return 0)。親 Python がどんな死に方をしてもパイプ切断で自己終了する。Python 側も `SearchDaemonClient.close()` (shutdown 送信 → Terminate → Kill、atexit 登録) の二重防御。**どちらか片方を「もう片方があるから」と削るな。**
- C++ 側の JSON パーサは**意図的に最小サブセット** (フラットなオブジェクト・既知キーのみ)。クライアントは `json.dumps(..., ensure_ascii=False)` + UTF-8 が規約 — ensure_ascii=True にすると Windows の日本語パスが \uXXXX になる (パーサは復号できるが、規約はあくまで生 UTF-8)。プロトコルを拡張する時にネスト構造を入れたくなったら、それは設計の間違い。
- index は path → mmap のキャッシュで**一度だけマップして使い回す**。エラー応答はデーモンを殺さない (応答して次のリクエストを待つ)。

### 9.3 実際に踏んだ罠 (Step 1)

- **mmap 中のファイルは Windows では truncate できない**: `path.write_bytes()` (= "wb" オープン) が errno 22 で死ぬ。scratch への書き込みは必ず in-place ("r+b" seek+write、または mmap 経由)。**同じ理由で、インデックス再構築 (`sync_diary_index`) の前には必ず `remap` コマンドをデーモンへ送ってマッピングを解放させること** — Step 2 の配線で忘れると Windows でのみ再構築が失敗する時限爆弾になる (remap は即時アンマップ + 次回 search で遅延リマップ)。
- scratch のデフォルトパスは `_ce_shared_scratch_{pid}.bin` と **pid サフィックス付き** (TUI とデスクトップの 2 エンジン同時起動で取り合わないため。`_ce_query_{pid}.bin` と同じ規約)。固定名に「短くて綺麗だから」と変えるな。
- `build.ps1` は PowerShell 7 構文 (`??`) を含むため **Windows PowerShell 5.1 では実行できない**。pwsh 不在の環境では build.ps1 と同一フラグ (`clang++ -O3 -std=c++17 -march=armv8-a+simd -fopenmp`) で直接ビルドする。
- f32 の score を Python の float (f64) と直接 `==` 比較するな (0.9 は f32 で 0.8999…)。テストは round(s, 4) で比較する。

### 9.4 テスト戦略 (確立パターンの継承)

- プロトコルの Python 側は **fake プロセス (stdin/stdout モック) の依存注入** (`SearchDaemonClient(spawn=...)`) で決定論的に検証。fake は scratch ファイルを実際に読み書きするため、データプレーンのバイトレイアウトも同時に検証される。
- 実デーモンの E2E は **exe 不在なら SKIP (FAIL ではない)** のゲート付き。one-hot ベクトル (dot = query の該当次元値) で期待値が厳密に既知の index を stdlib `struct` だけで生成する — numpy 禁止・乱数禁止・実データ非接触の三原則を守ったまま実カーネルを検証できる。
- C++ 変更時は `tests/benchmark.py --quick` で前後の QPS を計測して数字を出す (Step 1: 10k vectors で前 101.63 µs/q → 後 101.38 µs/q — カーネル非改変のパリティ確認)。

---

## 10. Target Charlie C1: PKBVEC01 の LSM 化 (`core/lsm_index.py`)

日記の同期を「O(全履歴) の再埋め込み」から「O(変更日数) の再埋め込み」へ縮める機構。セグメントファイル (`vectors.seg-NNNNNN.bin`) の中身は既存 `PKBVEC01` 形式のまま 1 バイトも変えない — バージョニングは `segments.json` マニフェスト側でのみ行う。**C++ (`search_engine.cpp`) / `pipeline.py` の `to_aosoa`/`write_binary` は無改造のまま流用する。**

### 10.1 アーキテクチャの不変条件

1. **`index_in_segment` は明示フィールドとして manifest に持つ。chunk_id からの逆算 (min 値推定など) はしない。** 墓標書きで生存エントリが間引かれると、セグメント内で最小の chunk_id がインデックス 0 に対応しなくなる — 実装中に一度この設計で書いて自己発見したバグ。`write_segment()` は `date -> index_in_segment` を返し、呼び出し側が `_tomb_offset(idx)` を都度算出する。
2. **content_hash は embed 入力そのもの (`title+text`) をハッシュする。** 個々のソース (日記/LINE/家計簿/予定/相談) を列挙して結合する方式は「ソース追加時に hash 側の更新を忘れる」事故 (SPEC 罠 T-3) を構造的に踏み得る。embed に渡す文字列と同じものをハッシュすれば、その文字列が変わらない限り再埋め込み不要という判定が自動的に正しくなる。
3. **手順順序 (§2.1.3 由来。変えるな)**: 新セグメント確定 (metadata → manifest の順で `os.replace`) → 墓標書き (in-place) → **デーモンへの remap** (`engine._release_index_mapping`) → コンパクション判定。墓標を書いた直後に remap を送らないと、デーモンが保持する読み取り専用 mmap の可視性が OS 依存になる (§9 と同じ罠)。
4. **コンパクションの unlink は必ず remap の後。** `maybe_compact(manifest, release_fn=...)` は `release_fn(path)` を呼んでから `unlink()` する。Windows でマップ中のファイルを削除しようとすると PermissionError — Bravo で踏んだ時限爆弾の変奏。`release_fn` を省略して直接 unlink するコードを書いたらレビューで落とせ。
5. **bootstrap は rename ではなく copy。** 既存 (非LSM) `vectors.bin` を `seg-000001.bin` として採用する際、`core/cli.py` が `VECTORS_BIN`(=`DIARY_BIN`) を直接参照する検証ツールのため、legacy ファイルは残す。1 回だけのコストなので許容する。
6. **bootstrap 結果は「変更なし」でも即座に永続化する。** `sync_diary_index_lsm` は `not LSM_MANIFEST.exists()` を検出したら、diff 判定の結果を待たずに manifest を書き出す。これを怠ると、初回同期後に「今日は差分なし」が続く限り毎回 legacy `metadata.json` を全走査して manifest を再構成する羽目になり、読み取りパスに O(全履歴) が舞い戻る (実装中に自己発見)。
7. **埋め込み空間の同一性 (I-6)。** `manifest["embedder_id"]` (`{name}#{DIM}`) が現在の embedder と不一致なら、diff 判定をバイパスして全日付を「変更」扱いにする。異空間ベクトルの混在検索は「エラーの出ない乱数」になる。
8. **検索はセグメントごとに既存 `search_index()` を無改造で呼び、Python 側で日付デデュープ**(同一日付は score 最大のみ採用)。C++ 側マージ (マイルストーン C2) は実測が遅い場合のみ検討 — 現状は 1 セグメントあたり ~10µs (Bravo 実測) なので着手不要。
9. **`metadata.json` は全セグメント共通の生存チャンク台帳。** セグメントごとに分けない。`chunk_id` はグローバル一意なので、どのセグメントのヒットでも同じ metadata から解決できる。

### 10.2 実際に踏んだ罠 (次の実装者への警告)

- **min(chunk_id) を index_in_segment の代用にすると、墓標書き後に壊れる。** §10.1-1 参照。テストで tombstone 後の compaction を必ず往復させろ (`test_compaction_byte_copy_no_reembed`)。
- **manifest/metadata の書き込みは親ディレクトリの存在を仮定するな。** `atomic_save_manifest`/`_save_meta` は `parent.mkdir(parents=True, exist_ok=True)` を自前で行う。フレッシュインストール (data/processed が未作成) での実機 E2E で実際に `FileNotFoundError` を踏んだ。`pipeline.write_binary` は既にこれをやっている — 新しい書き込み関数を足すたびに確認しろ。
- **他モジュールがフラットな再エクスポートに依存している。** `consultation_engine.py` から未使用に見える `paths` の re-import (`DIARY_MD`, `LINE_HISTORY` 等) を削除すると、`facade.py` が `from .consultation_engine import LINE_HISTORY` で壊れる (実際に `ui_smoke` が ImportError で落ちた)。未使用インポートを消す前に `grep -rn "from .consultation_engine import\|from core.consultation_engine import"` で依存元を全部確認しろ。
- **コンパクションは埋め込みモデルに一切触れない。** `maybe_compact()` はベクトルをバイトとして読み書きするだけ (NumPy の `frombuffer`/`view` のみ)。embedder を渡す引数がそもそも存在しない設計にした — 「うっかり再埋め込みする」の芽を型シグネチャで摘む。

### 10.3 検証の作法

`tests/test_lsm_index.py` (9 ケース、全て決定論・stdlib+numpy のみ・`HashedNgramEmbedder` 使用): レイアウト計算の単体テスト、差分同期での再埋め込み件数を `CountingEmbedder` (encode 呼び出しテキストを記録するラッパー) で検証、埋め込み空間不一致での全再構築、legacy bootstrap での再埋め込みゼロ確認、`search_lsm` の日付デデュープ (クラッシュ窓の再現)、コンパクションのバイトコピー往復 (NumPy デコードで生存ベクトルの一致を確認)。実機 E2E は一時 `PKB_PROJECT_ROOT` + 実 `search_engine.exe` daemon で、差分同期 2 回 + remap + consult() フルフローがデーモンを生かしたまま完走することを確認した。

---

## 11. Target Delta-LINE DL1: 対人プロトコル・テレメトリ (`core/line_telemetry.py`)

LINE 全ログ (本人+他者) を「人間関係の物理的衝突ログ」として解析する第3チャネル。docs/SPEC_CHARLIE_DELTA.md §3.9 の実装。**発見は決定論のみ。LLM による感情推定は一切使わない** (gap_analysis の統治原則を継承)。

### 11.1 アーキテクチャの不変条件

1. **既存 `extract_conversation_sessions()` は流用不可。理由を忘れるな。** この状態機械は「相手が発言 → 本人が返信」の一方向専用 (本人が会話を始めたバーストは構造的に捨てられる)。`core/line_telemetry.py` は独自の対称なバースト抽出 (`_bursts_by_contact`) を持つ。日付/時刻パースのみ `data_merger.normalize_date`/`_parse_dt` を再利用する。SPEC_CHARLIE_DELTA.md §3.9.1 の訂正コメント参照。
2. **摩擦検出は 2-of-3 多重シグナル必須。単一キーワード判定は禁止。** `FRICTION_MARKERS` の出現だけでフラグすると、仲の良い dyad ほど摩擦だらけに誤認される (親密なほど否定語彙の遊びが増える系統誤差 — SPEC 罠 T-11)。`_detect_friction_events` は (a) 相手の摩擦語彙 + (b) 本人の**自己ベースライン**からの返信遅延 (3倍超) + (c) スレッド死/謝罪、のうち (a) 確定を前提に (b)(c) から最低1つを要求する。
3. **(b) の比較基準は「本人自身の user_median」であって「相手の peer_median」ではない。** 実装時に一度このバグを書いた — 別人の返信速度と比較しても「いつもより遅い」の検出にならない。
4. **深夜到着 (23:00-08:00) は着信側の時刻で判定する。返信時刻ではない。** 判定対象を `b["start"]`(返信時刻) にすると深夜返信そのものを弾いてしまい、本来除きたい「深夜に届いたから気づけなかった」交絡因子を除けない。正しくは `prev["end"]` (着信側の終端時刻)。実装時に一度この向きを逆に書いたバグを踏んだ。
5. **第三者は salt 付き一方向 alias のみ。実名は一切永続化しない (I-15)。** `contact_alias()` は blake2b(実名, salt) — salt を知っていても alias→実名は戻せない。salt 自体は `data/processed/line_telemetry_salt.bin` に平文保存されるが、これは「alias の安定性」のためであって「実名の保護」の対象ではない (salt 単体からは実名は導出できない)。
6. **グループチャットは v1 で全指標から除外。** 多者間の発話帰属は曖昧で、2者間モデルの F/L/P 軸に混ぜると意味をなさない (`compute_dyad_stats(..., group_contacts=...)` で明示的に除外)。
7. **confidence ゲート: `exchanges < MIN_EXCHANGES(=20)` の dyad は3軸の計算対象から除外。** 標本不足で人間関係を断定しない (gap_analysis の `data_sufficiency` と同じ思想)。有効 dyad が無ければ `score=None, confidence=0.0` を返す — 0.0 や中央値で埋めるな。
8. **再計算トリガは `import.line` のみ。** RECORD 保存・consult では走らせない (AI_SKILLS §1 の profiler 自動実行規約に相乗り)。

### 11.2 テストで固定した対照群 (退化させるな)

- 摩擦語彙が出ても即レス・スレッド継続なら**フラグしない** (`test_friction_requires_multi_signal_not_keyword_alone` — 罠 T-11 の直接回帰ガード)。
- 深夜到着への返信は latency 中央値の計算から除外される (`test_night_arrival_excluded_from_latency`)。
- 自分起点/相手起点、両方の会話を対称に数えられる (`test_initiation_ratio_symmetric` — 旧 `extract_conversation_sessions` の非対称バグの回帰確認でもある)。

### 11.3 DL2: 一人称×二人称の衝突 (`analyze_social_positioning`) と Bounty

`profiler.py` の `gap_analysis.analyze_gaps()` 呼び出し直後で `line_telemetry.sync_line_telemetry()` → `analyze_social_positioning(daily, dyads)` → `gap_result["gaps"].extend(...)` の順に合流させる (`profiler.py` の該当箇所参照)。**`_gap_section()` (講評フェーズのみが呼ぶ既存の唯一のゲート) を経由するだけなので、interview_sim/gd_sim/es_review 側のコードは 1 行も変更していない。**

1. **`_assert_no_gap_leak` は本番コードの関数ではなく test_integration.py 内のテストヘルパーである。** SPEC I-14 の当初案は「本番の検査関数にキーを追加する」ように読めたが誤り — 実際の隔離は「`_gap_section()` を呼ぶメソッドと呼ばないメソッドが分かれている」という**コード構造そのもの**によって保証されている。ガードは `GAP_LEAK_MARKERS` タプルへのマーカー追加 + `_write_phase3_assets()` フィクスチャへの対応エントリ追加、という形で拡張した (`tests/test_integration.py`)。「ガードが先、機能が後」は本セッションでは「マーカーと講評フェーズでの存在アサーションを追加してから profiler.py を配線する」という順序で実践した。
2. **既存の `intention_gap`/`blind_spot` 型名をそのまま再利用し、新規 type `social_positioning_gap` は作らなかった (Architect's Override)。** `format_gap_table` のラベル表は未知の type 名をそのまま表示するフォールバックを持つが、既存2型に合流させれば表示品質もテストの型チェックも既存のものに完全に乗る。新規テーマ名 `"対人関係・役割認識"` は THEME_TAXONOMY に登録不要 (`format_gap_table` はテーマ名を検証しない)。
3. **Bounty はここでは「存在するだけ」。中身は theme/type/tension/status/bank_question_id のみで、insight/quotes 等の生テキストを持たせない。** これは意図的なデータ最小化であり、D3 (Puppeteer) が Bounty を扱う際に生テキストが誤って面接官コンテキストへ流れる経路をそもそも作らない設計。`register_bounties` の閾値 (`BOUNTY_TENSION_THRESHOLD=0.3`) 未満は登録しない。ID は内容ベースの安定ハッシュ (`_bounty_id`) — 同一内容の再登録は重複しない。
4. **`build_subjective_corpus` の weight を尊重する。** 建前人格 (simulated) の発話から「聞き役」自認の引用証拠を採らない (`GENUINE_DOC_MIN_WEIGHT` 未満は quotes に含めない) — gap_analysis 本体の規律 (§7.1.4) をそのまま踏襲。
5. **対照群テスト必須。** 自認と実測が一致する dyad (聞き役自認なし・会話量対等・関係維持言及あり) はフラグしない (`test_social_positioning_control_group_no_false_positive`)。有効 dyad が `min_dyads`(既定3) 未満なら断定を避けて空リストを返す。

### 11.4 D3: Puppeteer (`core/question_bank.py`) — 完遂済み

`select_question(bounties, k)` は Bounty の **id/type/tension/status/bank_question_id のみ**を読み、theme/insight は一切参照しない。tension 降順 (同値は id で安定化) に走査し、type 一致のバンク質問を辞書順の先頭から選ぶ (乱数不使用)。既出判定は `Bounty.bank_question_id` フィールドをそのまま「使用済み」の記録として使う専用の永続化を新設していない。type 一致が尽きたら未使用の質問へフォールバックする (面接を止めない)。

1. **注入は議論ターンのみ。opening ターンには一切触れない。** 既存の ES 駆動オープニング (`test_adversarial_interview_with_es` 等) の互換性を壊さないための意図的な制約。`_interview_state["priority_queue"]` はメモリ内のみ (永続化しない、既存のセッション状態規約と同一)。
2. **面接官プロンプトに追記するのは `QUESTION_BANK` の質問テキストそのものだけ。** Bounty の theme/tension/insight を組み立てる `_consult_interview_sim` の discussion-turn コードには一切現れない (`state["priority_queue"]` に積む時点で既にテキストへ変換済み)。
3. **ガードは機能より先に書いた。** `tests/test_integration.py::test_puppeteer_injects_whitelisted_text_only` は Puppeteer 配線前に一度 RED (未検出でAssertionError) を確認してからコードを書いた。Bounty の `theme` 文字列 (`"課外活動・組織運営"`) が議論プロンプトに含まれないことを明示的にアサートする。
4. **講評終了時に `mark_bounty_status(bid, "resolved")`。** キューに積まれたが未使用の質問が残っていても (`priority_queue` に残余があっても) 気にしない — 次回セッションでまた選ばれるだけ。

### 11.5 D3: Narrative Compiler (`core/narrative_compiler.py`) — 完遂済み (スコープ限定)

**Architect's Override**: SPEC 原案は HumanSourceCode (5軸・D1 未着手) と HistoricalNode (信頼済み事実グラフ・D1/D2 未着手) を入力に想定していたが、これらは存在しない。実装は `deep_profile.json["gap_analysis"]["gaps"]` (証拠付きの決定論的ギャップ。true_gakuchika を含む) のみを素材として使う — 存在しないデータへの参照より、実在するデータで確実に動く設計を選んだ。5軸/HistoricalNode が実装されたら `_select_material` の入力をそちらに差し替えるだけで昇格できる設計にしてある。

1. **証拠 (quotes または calendar/LINE evidence) を持たないギャップは素材にしない。** 反証可能性の原則 (AI_SKILLS §6.2-3) を Narrative Compiler にも適用する。
2. **`[ref:N]` タグの無い段落は幻覚として破棄する。** スタイルの問題ではなく正誤の問題として扱う — 「もっと自然な文章にして」ではなく「その段落は無かったことにする」。最大 `MAX_RETRIES(=2)` 回リトライし、それでも 0 件なら `ok=False, reason="no_valid_claims"` を返す。**この場合 draft ファイルは書かず、consultation_log にも記録しない** (中途半端な生成物を成果物として残さない)。
3. **Recruiter's Eye は本文生成と別の `backend.generate()` 呼び出しで作る。** 同じ呼び出しで「本文+メタ解説」を一度に出させると、パース処理が複雑化するだけでなく、メタ解説の文言が本文に混入するリスクが生まれる。
4. **`data/es/draft_*.md` に書き込むのは `es_text` のみ。** Recruiter's Eye を同じファイルに書くと、es_review モード (「ドキュメント単体評価」原則, §7.1.1) がこのドラフトを添削する際にメタ解説まで一緒に添削されてしまう。Recruiter's Eye は result dict 経由で UI にだけ渡す。
5. **narrative_compile のログは `simulated=True`。** AI が生成した ES 文面であり本人の自己申告ではない — gap_analysis の主観チャネルを汚染させない (es_review/interview_sim と同じ規約)。

D3 完遂によりTarget Charlie / Target Delta (DL1・DL2・D3) は全て実装済み。残るは Target Delta の D1/D2 サブトラック (HumanSourceCode 5軸・PROBE・HistoricalNode) のみ — これは今回未着手。

---

## 12. Target Echo — E0〜E4完遂 / E5は計測ゲート未達のため未着手

本節は Target Echo の設計規律と as-built の両方を所有する。現在挙動の最終正本は実コード。
凍結された数式・レイアウト・不変条件は `docs/SPEC_ECHO_GENESIS.md` を参照する。Echo 変更時は
SPEC の該当節と §5.9〜§5.10、および本節の対応 as-built を読むこと。完遂済み E0〜E4 を再実装しては
ならない。E5 は性能劣化の実測が 3000ms ゲートを超えない限り着手禁止。

| Phase | 状態 | 正本 |
|---|---|---|
| E0 | 完遂 | 憲法ガードと既存テスト |
| E1 | 完遂 | `tensor_store.py` / PKBTEN01 |
| E2 | 完遂 | `coupling.py` |
| E3 | 完遂 | `digital_twin.py` |
| E4 | 完遂 | `oracle.py`、facade/stdio/frontend配線 |
| E5 | 未着手・着手禁止 | 3000ms計測ゲート未達 |

コードより先に存在する凍結事項 (設計規律。完遂済みフェーズの再実装禁止と両立する):

1. **PKBTEN01 レイアウトは凍結済み** (header 64B "<8sIIIIiIQ24x" / row 136B "<iI32f"
   / 特徴量レーン番号表 §5.2)。実装時は C++ struct と tensor_store.py を同時に
   書き、PKBSCR01 と同じ相互 assert で釘付けにする。レーン番号の再利用禁止。
2. **欠測 = valid_mask のみ (I-18)。** NaN 格納禁止。「支出 0 円」と「未記録」の
   混同 (マスクを見ない集計) が Echo 最頻の静かな死と予測されている (罠 T-15)。
3. **乱数は Philox + 入力内容由来 seed のみ (I-17)。** unseeded np.random は 1 箇所
   でもバグ。モンテカルロは「同一入力 → ビット同一出力」が不変条件。
4. **ツインはスキルゲート付き (I-20)**: walk-forward BSS ≥ 0.05 ∧ 検証失策数 ≥ 10
   を満たさない限り forecast/介入は空。in-sample 適合をスキルと呼ぶな (罠 T-18)。
5. **介入の標的は本人側特徴量レーンのみ (I-19)。** 第三者の反応・感情を最適化
   目標にする介入・数式・プロンプトは憲法 7 の系として禁止。INTERVENTION_BANK は
   QUESTION_BANK と同じ「生成ではなく選択」+ target_lane の import 時機械検査。
6. **Echo 出力の聖域 (I-22)**: oracle_payload/twin/coupling/OII は面接官・GD 議論・
   es_review に不出。合法出口は consult 動的サフィックス (静的側に置くと KV 全滅
   — §8.1-2)・講評フェーズ・PROFILE UI の 3 つだけ。実装順序は E0 (憲法ガード
   RED 確認) が最初 — 「ガードが先、機能が後」。
7. **E5 (C++ カーネル) は計測ゲート封印**: E1〜E4 の numpy 実装が profiler 1 回
   あたり合計 3000ms を超えない限り着手禁止 (C2 と同じ規律)。scratch (2072B) は
   拡張しない。scipy 導入は却下済み (SPEC §2 Note 2) — 再提案するな。
8. **test_oracle.py はゲート付き SKIP (Rev.2 訂正)**: `core.oracle`/`core.tensor_store`
   の ImportError のみを SKIP 扱いにし、それ以外は FAIL とする (I-5 の確立パターンに
   合流)。「削除・スキップ化せず常設 RED を保持せよ」という当初指示は DoD
   (全スイート PASS まで完了と言わない) と矛盾するため、レビューで訂正済み。
   E4 のゲートは「本ファイルが SKIP なしで GREEN」。

### E0/E1 完遂 (2026-07-07) — as-built

- **E0**: `tests/test_integration.py` の `GAP_LEAK_MARKERS` に 5 マーカー追加 +
  `_write_phase3_assets()` へ対応エントリ追加 (DL2 と同一手順)。**踏んだ罠**:
  `format_gap_table(max_gaps=4)` の既定切り詰めにより 5 件目以降の gap が
  無言で無視される。他テストで内容参照のない `true_gakuchika` エントリを配列末尾
  (切り詰め対象) へ退避し、必須マーカーを持つ4件を先頭に揃えて解決した。
  `tests/test_oracle.py` を新規作成し I-19 (target_lane ホワイトリスト) /
  `_assert_sterile` のガードを先置き。
- **E1**: `src/python/core/tensor_store.py` (新規) + `src/cpp/search_engine.cpp` へ
  `TensorHeader`/`TensorRow` (PKBTEN01) を追加。`tests/test_tensor_store.py` が
  E1 ゲート (レイアウト相互検証/mask対照群/rebuild-under-handle/simulated除外/
  日付格子) を全て GREEN。

**新規に踏んだ罠 (T-14 の具体化 — 次の実装者への警告)**:
`np.frombuffer(mmap_obj, ...)` で作った ndarray (またはそのスライス/フィールド
ビュー) が 1 つでも生きている状態で `mmap.close()` を呼ぶと
`BufferError: cannot close exported pointers exist` になる。これは「長期保持」
に限らず、**同一関数内で `window()` の戻り値を使った直後に `close()` を呼ぶ
だけでも発生する** (戻り値がローカル変数としてまだ束縛されているため)。
対策は 2 段: (1) `TensorStore.close()` は自身の `self._rows` 参照を `None` に
してから `mmap.close()` する、(2) それでも呼び出し側が `window()` の戻り値を
束縛したままなら防げない — **呼び出し側が close() 前に del するか、長期保持
するなら `.copy()` する規律を必ず守ること**。「.copy() は長期保持の時だけ」
という当初の理解は誤りで、**同一スコープでの短命な使用でも close() の直前では
参照を手放す必要がある**。

### E1.1 是正 + E2 完遂 (2026-07-07) — as-built

**E1.1 (SPEC Rev.3 §5.10.2 是正指令)**: `_task_daily_counts()` が
`(declared, declared_observed, executed, executed_observed)` の4値を返すよう
拡張し、`build_tensor()` は `*_observed` 集合に無い日付を mask=0 のまま残す
ように変更 (lane 18/19)。`_line_daily_aggregates()` は LINE ログのカバレッジ窓
(最古日, 最新日) を第2戻り値として返すよう拡張し、窓内で当日データが無い日は
lane 11,12,13,15,16,17 を mask=1/value=0 (観測済み沈黙) にする第2パスを
`build_tensor()` に追加 (lane 14 は対象外)。対照群テスト2本
(`test_task_lanes_mask_only_on_observed_days` / `test_line_silence_within_coverage_is_observed_zero`)
を `tests/test_tensor_store.py` に追加、全 GREEN。

**E2**: `core/coupling.py` (新規)。§3.2 ランク変換 (argsort×2 の平均ランク、
乱数不使用) + §3.3 の6配列 FFT 相互相関 (n, S_xy, S_x, S_y, S_xx, S_yy) +
遠ラグ帰無 (NULL_LAG_RANGE=45〜365) による自給的有意性判定。
`tests/test_coupling.py` (6ケース): 既知ラグ注入検出 / 独立系列の帰無対照群
(素朴な固定閾値なら誤検出するケースを自給帰無が正しく棄却することを実測で
確認) / 決定論 (2回実行の完全一致) / **W-8 共有欠測対照群** (同一欠測パターンを
共有する無関係な2レーンが sig にならない) / **W-11 ラグ符号恒等式**
(rho_ij(tau)==rho_ji(-tau) を列入替え呼び出しで実測検証) / 系列長不足の
CouplingError。全 GREEN。

**踏まなかった罠 (設計時点で SPEC の W-1〜W-14 が事前に塞いだため実装中に
発現しなかった)**: W-9 (irfft の整数量) は `np.rint` を先に入れていたため
未発現、W-10 (分散項の微小負) も `max(·,0.0)` クランプを先に入れていたため
未発現。**これは「バグが起きなかった」のではなく「バグを起こす前に塞いだ」
ことが正確な表現である** — レビューが実装より先に警告を発行した効果の実例。

### E3 完遂 (2026-07-07) — as-built

`core/digital_twin.py` (新規): 認知リソース状態方程式 (§3.4) + IRLS ハザード
(§3.5, W-0 訂正込み) + walk-forward スキルゲート (I-20) + モンテカルロ (§3.5)。

- **trailing causal baseline (W-19 の実装形)**: 失策ラベルの分位点閾値
  (p90/p95/p50) と D(t) の正規化 (median/MAD) は `_rolling_quantile_causal`/
  `_rolling_median_mad_causal` により「時刻 t は [t-180, t) の有効値のみ参照」
  する trailing 方式で実装した。これにより fold 単位の再計算を待たず、
  ラベル/D(t) 自体が構成的に未来を見ない。**θ_dyn (状態方程式パラメータ) は
  全履歴 1 回の SSE グリッド探索で fit する設計上の割り切り** — SPEC §3.4 に
  walk-forward の指示が無く、walk-forward は §3.5 のハザードモデルにのみ適用
  されるため。この境界は `digital_twin.py` 冒頭の docstring に明記済み。
- **W-19 の実測効果 (test_digital_twin.py で実証)**: 無関係な時間トレンドを
  共有するだけの合成データ (R が緩やかに減衰・spend_hedonic が無関係な上昇
  トレンド) に対し、素朴なグローバル分位点閾値は BSS=+0.12 (ゲート閾値 0.05
  を超え「本物のスキルがある」と誤判定) を出したが、trailing causal baseline
  は BSS=-0.0007 (正しく棄却) だった。**これは机上の懸念ではなく実測できる
  失敗モードである。**
- **W-16 (分離対照群)**: 完全分離データ (下位20%だけ lapse=1) で IRLS が
  収束し `‖β‖<100` に収まることを確認 — Hessian のみのリッジ (Rev.1 の式) では
  満たせなかったはずの条件。
- **TwinParams.theta_r は `float | None` に変更 (Rev.1 からの必要な型修正)**:
  κ≤0 (R と失策が無関係) の場合 `beta0/kappa` は無意味かつ `inf` は JSON
  非互換になり得るため、`abs(kappa)>1e-9` を満たさない場合は `None` を返す。
  `gate_passed` は既にこのケースで False になるため実害はない。
- テストは `TensorStore` の duck-type フェイク (`FakeTensorStore`) で依存注入
  (`SearchDaemonClient(spawn=...)` と同じ確立済みパターン) — 実ファイルを
  書かず、`window()`/`dates`/`content_hash64`/`flags` のみ提供する。
  `tests/test_digital_twin.py` (10ケース) 全 GREEN: fit_twin 決定論 /
  walk-forward ゲート落ち (失策数不足・失策皆無の2種) / MC seed 再現性 /
  MC 出力サイズの paths 非依存 (O(P) メモリの構造確認) / W-16 分離対照群 /
  W-15 非有限値の入口拒否 (TwinParams 側・状態方程式入力側の2箇所) / W-19
  ラベル漏洩回帰 / compute_oii の dyad スコープ限定ガード。

### E4 完遂 (2026-07-07) — as-built

`core/oracle.py` (新規): `INTERVENTION_BANK` (6件、全 target_lane を import 時
assert — I-19) + `ORACLE_RULES` (6ルール、R-GATE-01/SWITCH-01/VOL-01/NIGHT-01/
RECOVERY-01/SPEND-01) + `_assert_sterile()` (本番コードの実行時ガード) +
`build_oracle_payload()` + `render_oracle_consult()`。

- **oracle.payload / oracle.report の cmd 分離**: `facade.oracle_payload()`
  (LLM なし・無菌 JSON のみ) と `facade.oracle_report()` (LLM 言語化込み) を
  別関数・別 stdio cmd (`"oracle.payload"`/`"oracle.report"`) に分離した。
  `twin.forecast`/`tensor.rebuild` も追加。`apps/desktop/src/lib/engine.ts` に
  型付きラッパー4本を配線 (UI コンポーネントは Foxtrot 側で実装)。
- **oracle_payload の合法出口の是正 (SPEC 本文の誤りを訂正)**: SPEC_ECHO_GENESIS
  §4 は「consult の動的サフィックス」としていたが、gap_analysis の既存配置
  (静的プレフィックス — profiler 再実行時のみ更新されるため KV キャッシュ効率が
  高い) を確認し、`_oracle_section()` を `_gap_section()` と並べて
  `build_static_prefix()` に置いた。interview_sim/gd_sim の講評フェーズにも
  `_gap_section()` の直後へ追加 (議論フェーズには一切触れない — 既存の
  非対称構造を維持)。**この配置判断は as-built が正であり、SPEC 本文の
  「動的サフィックス」表記は誤り (次回 SPEC 改訂時に訂正すること)。**
- **deep_profile への追加**: `profiler.build_profile()` の末尾に Echo パイプライン
  (`tensor_store.build_tensor()` → `oracle.build_oracle_payload("global")`) を
  try/except で追加 (失敗しても既存分析は維持 — LLM 分析と同じ耐性パターン)。
  `deep_profile["oracle_payload"]` は自己バージョン (`"schema": "oracle_payload.v1"`)
  を持つ新規トップレベルキーとして追加し、**外側の `"deep_profile.v6"` は
  据え置いた** (gap_analysis/interpersonal 追加時の前例を踏襲。「読み手側の
  追従」は新設の `_oracle_section()` そのものであり、既存の `_gap_section()`/
  `update_user_profile()` に変更は不要だった)。
- **dyad スコープは正直な unratable スタブ**: `scope="dyad"` は
  `sufficiency.gate_passed=False` の空 payload を返す (グローバルデータでの
  代用はしない — I-19/T-19 の温床)。
- **境界防衛**: `engine_stdio.dispatch()` は `params.get()` ベースの既知キー
  抽出のみ (未知パラメータは構造的に無視される) — `test_oracle.py` に
  monkey-patch による回帰テストを追加し固定した。
- **test_oracle.py が SKIP なしで全 GREEN** (E4 ゲート達成)。ケース: I-19
  ホワイトリスト / 無菌検査 / **`build_oracle_payload()` の実テンソルによる
  E2E** (下記の実バグ2件を検出した回帰ガード) / stdio 境界防衛。

**実装中に発見した実バグ2件 (単体テストが実経路を一度も通していなかったため
単体テストをすり抜けていた — N=3650 ベンチマークで初めて発覚)**:
1. `TensorStore.close()` と同型の T-14: `build_oracle_payload()` 自身が
   `window()` の戻り値 (`values`/`mask`) を保持したまま `store.close()` を
   呼んでおり `BufferError` になっていた。coupling 計算後、必要な値
   (`n_rows`/`dead_lanes`/`coverage`) を先に確定させてから `del values, mask`
   で明示的に手放す修正を入れた。
2. `tensor_store.py` が `paths.TENSOR_GLOBAL_BIN`/`tensor_dyad_bin` を
   再エクスポートしておらず、`oracle.py`/`facade.py` の
   `tensor_store.TENSOR_GLOBAL_BIN` 参照が `AttributeError` になっていた。
   `tensor_store.py` へ `from .paths import TENSOR_GLOBAL_BIN, tensor_dyad_bin`
   を追加して解決。
**教訓**: hand-crafted payload dict によるテスト (E0/E4 の隔離ガード検証) は
実配線の構造は検証するが、実コードパスの実行は検証しない。**「生成物の形」を
テストするテストと「生成する経路」を実行するテストは別物であり、両方が
無ければ E2E バグは踏めない。** `test_build_oracle_payload_end_to_end_real_tensor`
をこの教訓の回帰ガードとして残した。

**N=3650 (10年相当) 実測 — E5 (C++カーネル) 着手条件の判定**:
```
tensor build:                                   ~230-330 ms
build_oracle_payload (coupling+twin+MC+無菌検査): ~1,150-1,810 ms
TOTAL:                                          ~1.6-2.2 秒 (< 3000ms ゲート)
```
**E5 着手条件 (3000ms 超過) を満たさない — E5 は現時点で着手しない。**
2回目実行 (テンソル再構築 → payload 再計算のフルサイクル) でも `TensorStore`
の解放 (T-14) が正しく機能し、ハンドルリークなく完走することを確認した。

**アーキテクトへの確認待ち事項 (Sonnet5 の判断で最小拡張した箇所)**:
1. `cal_private_hours` (lane 9): calendar.json は開始時刻のみで終了時刻を
   持たない (`calendar_manager.py`)。「合計時間」の真値は測定不能なため、
   1 件あたり `PRIVATE_EVENT_NOMINAL_HOURS = 1.5` の名目値で近似した
   (`tensor_store.py` 内に理由を明記)。
2. `build_tensor()` の実引数に `line_messages`/`group_contacts`/`contact`
   (keyword-only) を追加した。SPEC の位置引数シグネチャ (`daily, dyads, out_path,
   scope, alias`) はそのまま維持しているが、dyads (履歴全体の集計値) には
   日付分解能が無く、lane 11-17 (LINE 由来) の日次集計には日付付き生メッセージ
   が別途必要なため。LINE のバースト抽出/摩擦検出は `line_telemetry.py` の
   既存実装 (DL1) を呼ぶのみで、状態機械の再実装はしていない。
3. dyad スコープ (`scope="dyad"`) の lane 5-7 (`spend_tagged` 読み替え) は
   タグ規則の config が現時点で存在しないため、構造 (flags bit0/ファイル名) は
   実装したが値は常に mask=0 のまま (未実装として明示。E2 以降で config が
   定義されたら差し替える)。

---

## 13. Target Foxtrot — UI/UX 設計 (`docs/SPEC_FOXTROT_UI.md`。F0/F7/F1/F2/F2-EXT/F3/F3.5/F4a/F4b/F4c/Rev.10相関ID復元/Rev.11 Phase A(Sandbox)・Phase B(単一ES/F-16)・Phase C(es_review無latency/F-17)・Phase D(面接スタンス/F-18) 完遂・§10.5〜§10.6/エンジン多重化 未着手)

フロントエンド (Tauri + React) の設計仕様。**§3.4 (React UI 規約) が上位法** —
SPEC はその適用解釈を確定させるもの。コードより先に存在する凍結事項:

1. **ライブラリ 0 依存を再確認**: Tailwind / Framer Motion / Recharts / D3 /
   Three.js は SPEC §0 で個別に検討の上**全て却下済み**。再提案するな。
   グラフは全てインライン SVG (座標は閉形式の三角関数 — 力学レイアウトの
   揺らぎは決定論の放棄)。
2. **デザイントークン (F-1)**: 色は App.css から抽出した 15 トークンで閉じる。
   新しい hex / リテラル px の新規記述はレビュー落ち。F0 (トークン移行) の
   ゲートは「視覚的差分ゼロ」。
3. **F-5 (最重要)**: Echo/twin/oracle のデータで**セッション中の面接 UI を
   駆動しない** (ツイン予測 → UI 妨害 → 成績低下 → 予測的中、の自己成就予言
   ループ = 罠 T-8 の UI 版)。接続点はセッション前ブリーフィングと講評後表示
   の 2 つだけ。TensionMeter はセッション内観測量のみ・表示専用・非永続。
4. **運動の法 (F-3)**: 状態フィードバックの transition (--t-fast 120ms,
   opacity/transform) のみ。自発的に動く UI (点滅・パルス・パーティクル) 禁止。
   prefers-reduced-motion 対応必須。
5. **PROBE タブは D2 + E4 完成が前提条件** — プレースホルダタブの追加も禁止。
6. UI は要約してよいが**捏造してはならない** (偽の数値・偽のランダム性・
   偽の緊急性の禁止 — 憲法 6 の UI 側対偶)。

### F0 完遂 (2026-07-08) — as-built

`apps/desktop/src/App.css` の全リテラル hex (90 箇所) のうち **84 箇所**を
§1.1 の 15 トークンへ機械置換した (`:root` に一括定義)。**残り 6 箇所の
`#fff` (+1 箇所の `rgba(255,255,255,0.65)`) は意図的に非置換のまま残した**
— 15 トークンのいずれにも white/ほぼ白の値が無く、最も近い `--text`
(`#e8eaed`) で代替すると "視覚的差分ゼロ" ゲートに反する僅かな色差が生じる
ため。新規トークンの追加は F-1「15 トークンで閉じる」への違反となるので
行わなかった。この非対称は次の実装者への申告事項として記録する — 将来
白系トークンが真に必要になったら、SPEC 改訂で明示的に 16 個目を追加すること
（勝手に追加するな）。

検証: `npx tsc --noEmit` エラーなし、`python tests/ui_smoke.py` ALL PASS、
vite 単体プレビューでの実描画確認 (`.app.loading` の computed `color` が
`rgb(232, 234, 237)` = `#e8eaed` と厳密一致 — CSS カスタムプロパティが
元のリテラル値と数学的に同一に解決されることを実測で確認)。

### F7 完遂 (2026-07-08) — as-built

`TitleBar.tsx` 新設 + `tauri.conf.json` の `decorations: false`。ドラッグ領域
とウィンドウボタンは兄弟要素 (W-27)。`"__TAURI_INTERNALS__" in window` で
ブラウザ/ネイティブを判定し、vite 単体プレビューではボタン非表示 (F0 の
検証パイプラインを壊さない)。

検証: `tsc`/`cargo check` エラーなし、`ui_smoke.py` ALL PASS (Textual TUI
側の回帰確認 — React 側の直接検証ではないことに注意、後述)、`cargo tauri dev`
実起動でビルド成功・エンジン ready まで到達 (ログ実測)。**ネイティブウィンドウ
のドラッグ・ボタンクリックという GUI 操作自体は、本セッションの検証ツール
(CDP ベースの vite プレビュー) では自動化不可能** — 起動確認はログで、
実際の操作感は指揮官の目視確認に委ねた。

### F1 完遂 (2026-07-08) — as-built (RECORD タブ)

`RecordTab.tsx` にファイルローカル・シングルトン `recordDraft` を新設し、
タブアンマウント時の draft 退避・マウント時の決定論的復元 (ディスク内容が
`baseline` と一致する場合のみ) を実装。`lib/keyUtils.ts::isCommitEnter()`
を QuickAdd 系の全単一行 input (event/expense/income) の `onKeyDown` に配線
(diary の textarea には適用しない)。`Ctrl+1/2/3` はサブタブ切替として既存
Ctrl+S リスナーへ相乗り、`Alt+1..5` は `App.tsx` にメインタブ切替として
新設 (`ready` 後のみ登録)。両者は修飾キーで直交するため `stopPropagation`
は新設分に付けていない。diary autofocus は `useRef` + `useEffect([subTab])`
(subTab 切替・draft 復元による remount 後の両方で発火)。`saveNotice` は
条件レンダリングを廃し常駐要素 + `visible` クラスの opacity transition に
変更 (`--t-fast`)、タイマーは `useRef` 保持で cleanup 必須。予定の時刻・
家計簿の金額は `.record-item-time`/`.record-item-amount` (`--font-mono` +
右揃え) に分離。`CalendarPicker` の `.cal-day.selected.has-events::after`
の背景 `#fff` を `var(--text)` に置換 (RECORD の子要素ツリー内で唯一の
背景/枠線 white — 他タブ共通の `color:#fff` (ボタンテキスト等) は F0 で
既に「トークン不在のため意図的保持」と申告済みのため今回は対象外)。

**仕様との差異 (申告)**: SPEC §2.1.1 裁定1 は draft の unmount 保存を
「アンマウント時」とだけ規定していたが、実装では stale closure を避けるため
`liveRef`/`baselineRef` の 2 段 ref ミラーを追加した (SPEC は明示していない
実装詳細だが W-26 の思想と矛盾しない拡張)。

検証: `tsc`/`cargo check` エラーなし、`ui_smoke.py` ALL PASS (Textual TUI
の回帰確認のみ — RecordTab.tsx は対象外)。**IME 誤爆防止・draft 復元の
実機での対話的確認は、ネイティブ GUI 操作を自動化する手段がなくコード
トレースによる論理検証に留まる** — `cargo tauri dev` を再起動しウィンドウは
起動済みだが、実際のキー入力・タブ往復操作は指揮官の実施を要する。

### F2 完遂 (2026-07-08) — as-built (IMPORT タブ・初のバックエンド配線)

**実測が裁定を変えた点**: `invoke_sync` (engine.rs) のイベント転送は
コマンド非依存の汎用機構であり、`engine_stdio` には既に `emit` コール
バックが存在した。これにより F-6 (status 逐次表示) はロジック変更なしの
配線のみで実現できた。

- **バックエンド (ロジック変更なし・配線のみ)**: `facade.import_line_text`/
  `import_line_batch`/`sync_calendar`/`sync_calendar_ics_batch`/
  `sync_calendar_ics_content` に optional `status: StatusCallback | None`
  を追加 (キーワード専用引数、既存の位置引数呼び出しは非破壊)。
  `engine_stdio.dispatch` の `import.line`/`calendar.sync` から consult と
  同型の `emit` ラムダを配線。新規 `facade.data_source_stats()` (stdlib の
  み・LLM/埋め込み不使用) + stdio `import.stats` + `engine.ts::importStats()`
  を正規 3 層経路で新設 (diary 行数・LINE エクスポート数・calendar/finance
  の JSON エントリ数・es/knowledge のファイル数を `{exists, count, mtime}`
  で返す)。
- **フロント**: `ImportTab.tsx` にファイルローカル・シングルトン
  `importLog` (上限 50 行、アプリ終了で揮発) を新設し、`RecordTab` の
  `recordDraft` と同族の「タブ往復してもログが消えない」設計を適用。
  `pkb-engine-event` の購読は `importingRef` で「自分の import が
  in-flight の間だけ」処理するようガード (W-28)。`mountedRef` で unmount
  後の setState を封じつつ (W-29)、`pushImportLog()` 自体は unmount 有無に
  関わらず常に実行し、完了通知は失われない (§2.2.1 裁定2)。取込処理自体は
  中断しない (アンマウントしても継続、finally のフロント反映のみガード)。
  `resetInput` パターンは維持 (W-30)。
- **term- CSS レイヤ (§1.5) を建設**: `.term-panel`/`.term-header`/
  `.term-row`/`.term-value`/`.term-glyph-*`/`.term-log-line` を新設。
  SourceTable (●○ グリフ + mono 右揃えの件数・mtime) と IMPORT_LOG の
  両方に適用。INTERVIEW の SessionHUD・将来の PROBE がこの語彙を流用する。
- **W-31 (D&D 非実装) を遵守**: ファイルドロップは実装していない (ファイル
  ピッカーのみ)。**W-32 (ファイル名プライバシー) を遵守**: ファイル名は
  UI の一時ログ (importLog) にのみ現れ、stderr/永続化には一切書いていない
  (既存 `_run_profiler` の traceback もファイル名を含まない)。

**仕様との差異 (申告)**: なし — §2.2.1 の 4 裁定・W-28〜W-32 を過不足なく
実装した。ただし `sync_calendar` 系の status は 2 段階 ("同期中"→"完了")
のみで、SPEC が例示した LINE import の 3 段階 ("受信"→"追記完了・profiler
再分析中"→"完了") より粗い — カレンダー同期は profiler を起動しない軽量
処理 (AI_SKILLS §1-4) であり、実際の処理段がそれだけしか存在しないため
(偽の中間段階を作らない = F-14 の原則)。

検証: `tsc`/`cargo check` エラーなし、**13 スイート (Python) + 新規
`tests/test_import_stats.py` (4 ケース: 欠損ソースの exists=False・実データ
での件数一致・ディレクトリ型ソース・status コールバックの発火順序) 全て
ALL PASS** — バックエンドに触れた F2 で初めて Python 回帰が必須になった。
`ui_smoke.py` ALL PASS (Textual TUI 側)。`cargo tauri dev` 実起動でビルド・
エンジン ready まで到達。**SourceTable の数値表示・IMPORT_LOG の逐次追記・
term- レイヤの見た目確認は、F1 と同じ理由でネイティブ GUI の対話的検証が
自動化できず、指揮官の実施を要する。**

### F2-EXT 完遂 (2026-07-08) — as-built (汎用インポート。指揮官要求への対応)

指揮官要求 (「その他」入力欄 + 自動判別) を「判別は提案、書き込みは明示」の
権限分離設計で満たした。

- **バックエンド**: `facade.classify_document(content, filename)` — 拒絶
  ゲート (拡張子ホワイトリスト `.txt/.md/.csv/.json/.ics`・先頭8KBのNUL
  バイト検出・10MB上限) → `[LINE]`/`BEGIN:VCALENDAR`/ES語彙 (志望動機・
  自己PR・ガクチカ等)の順で先勝ち判定 → 既定は knowledge。読み取り専用の
  純関数で一切書き込まない。`facade.import_document(content, filename,
  dest, *, status=None)` — `dest` は `"es"|"knowledge"` の2値ホワイトリスト
  のみ (`ValueError` で拒否)。UI の分類結果を信用せず拒絶ゲートを内部で
  再実行し、line/ics 判定分は例外で弾く (専用パイプラインへ回送させる)。
  冪等性は blake2b ハッシュ比較 (同一内容は skip)、ファイル名は sanitize
  し衝突時はハッシュ接尾辞で別名保存 (上書きは構造的に不可能)。knowledge
  書き込み時のみ `sync_knowledge_index(force=True)` を実行。stdio
  `import.classify`/`import.document` を consult と同型の emit 配線で新設。
- **実装中に発見した罠 (T-21 系の新しい亜種として記録)**: 冪等性チェックが
  最初 `Path.write_text()` で失敗した。**Windows の text モード書き込みは
  `"\n"` を `"\r\n"` へ変換するが、`read_bytes()` は変換しない** ため、
  「書いた内容」と「読み直した内容」のハッシュが一致しない。
  `target_path.write_bytes(content.encode("utf-8"))` に変更して解決した —
  **バイト単位の同一性を扱うコード (ハッシュ比較・冪等性判定) は
  write_text ではなく write_bytes を使うこと。次の実装者はこれを踏むな。**
- **フロント**: `ImportTab.tsx` に「その他 (自動判別)」ブロックを追加。
  ファイル選択 → `classifyDocument()` (読み取りのみ) → `pending` state
  (揮発でよい・recordDraft対象外) に判定結果+根拠(reasons)を表示 →
  ユーザーが dest (es/knowledge/スキップ) を確認・変更 → 「取込を確定」で
  一括実行。line/ics 判定分は `import.line`/`calendar.sync` へ直接回送。
  **W-33**: 全ての `File.text()` 呼び出し (LINE/ICS 既存経路も含む) を
  `lib/textDecode.ts::readTextLenient()` に置換 — `TextDecoder("utf-8",
  {fatal:true})` を試し失敗したら `shift_jis` へフォールバックする
  (`core/es_manager.py::_read_text_lenient` と同じ配慮をフロントにも導入。
  既存の LINE/ICS 経路にも同じ潜在バグがあったため、新機能に留めず横断的に
  修正した)。W-31 (D&D非実装) 遵守 — ファイルピッカーのみ。

**仕様との差異 (申告)**: なし。テスト6本 (分類優先順位・拒絶ゲート3種・
冪等スキップ・名前衝突での別名保存・dest ホワイトリスト・knowledge の
index同期呼び出し) を `test_import_stats.py` に追加し全て ALL PASS。

検証: `tsc`/`cargo check` エラーなし、Python 15スイート (新規6ケース含む)
ALL PASS、`ui_smoke.py` ALL PASS、`cargo tauri dev` 実起動でエンジン ready
まで到達。分類結果パネルの表示・dest選択・確定ボタンの実操作確認は
指揮官の実施を要する。

### F3 完遂 (2026-07-08) — as-built (CONSULT。実測で未報告バグを1件発見)

F3 は「作り直し」ではなく既存 `ConsultTab.tsx` の骨格 (リスナー・スクロール・
確定置換) を維持したままの規律締め上げ。着工前の実測で **W-34 (未報告バグ)**
を発見: F2 で import がタブ離脱後もバックエンドで継続する設計になったため、
IMPORT で取込開始 → CONSULT へ移動すると import の status イベントが
consult の status 行に混線していた (chunk 側は `last.streaming` ガードで
守られていたが status 側は無条件だった)。

- **W-22/W-23 の disposed フラグ標準形**: `listen()` の `.then()` 内で
  `disposed` フラグを確認し、cleanup が resolve より先に走っていれば
  即座に unlisten する。StrictMode の二重マウントでも購読が漏れない。
- **W-34 の是正**: `busyRef` (自分の consult が in-flight の間のみ true)
  で status ハンドラをゲート。chunk 側の `last.streaming` ガードと対に
  なる第二の防衛線。
- **stick-to-bottom (`stickRef`)**: state ではなく ref。`onScroll` で
  `scrollHeight - scrollTop - clientHeight < 24` を判定し、ストリーミング
  追記時はこの ref が true の時のみ `behavior:"auto"` でスクロール。
  ユーザーが読み返し中に上へスクロールした瞬間に自動解除され、最下端へ
  戻せば自然に再開する (専用ボタン・解除フラグ UI は追加していない)。
- **トークン再割当 (term- 不使用のまま)**: chat-log の枠線を `--accent`→
  `--border`、ユーザーバブルを `--bg-hover`→`--bg-selected`、AIバブルに
  `--bg-raised`+`max-width:68ch` を付与、ロールラベルを `--font-mono`
  0.72rem 化。ストリーミング中の本文は `--text-muted` (`.chat-text.streaming`
  クラス) にし確定置換で通常色へ復帰。CONSULT の status 行のみ
  `.consult-panel .status-line` のスコープ付きセレクタで mono 化 (RECORD
  等、他タブ共通の `.status-line` には触れていない)。
- **履歴クリアの F-7 化**: インライン2段クリック (「履歴クリア」→
  「本当にクリア」、`onBlur` で確認状態を解除)。モーダル・ダイアログなし。

**仕様との差異 (申告)**: なし。バックエンド不可触 (facade/engine_stdio に
1行も触れていない) につき Python 全スイートは対象外 — `ui_smoke.py`
(Textual TUI 側の回帰確認) のみ実施。

**運用上の教訓 (次の実装者への申告)**: `cargo tauri dev` の再起動を素早く
繰り返すと、前セッションの `pkb-desktop.exe`/vite (node.exe) の子プロセスが
終了しきらずポート1420を握ったまま残ることがある (バックグラウンドタスクの
「completed」通知は必ずしも子プロセス全滅を保証しない)。起動失敗時は
`Get-Process pkb-desktop` / `Get-NetTCPConnection -LocalPort 1420` で残存
プロセスを確認し、`Stop-Process -Force` で掃除してから再起動すること。

検証: `tsc`/`cargo check` エラーなし、`ui_smoke.py` ALL PASS、`cargo tauri
dev` 実起動でエンジン ready まで到達 (1回目はポート衝突で失敗、残存
プロセスを掃除して2回目で成功)。ストリーミング挙動・スクロール追従・
履歴クリアの2段確認は指揮官の実施を要する。

### F3.5 完遂 (2026-07-08) — as-built (CONSULT ストリーミングの一定速スロットリング)

**アーキテクチャの不変条件**: `lib/useThrottledStream.ts` はコンポーネントごとに
「文字キュー (`queueRef`) + interval タイマー1個 (`timerRef`)」のみを持つ。
chunk 受信は `push()` でキューへ追記するだけで、放出は `setInterval` の
tick (30ms × 2文字。**乱数ジッタ禁止 — F-14**) が単独で担う。この分離により
chunk ハンドラ自体は「キュー投入」に縮退し、放出側 (`stickRef`/`disposed`
規律) は F3 の実装に一切手を触れていない。

- **W-35 (キューと確定置換のレース) の実装形**: 最終応答が到着した瞬間に
  `flushAndStop()` を **`setMessages` による確定置換の直前** に呼ぶ。順序が
  逆 (置換→flush) だと、置換後のメッセージにタイマーの残り tick が数文字
  追記される競合が発生する。`ConsultTab.tsx` の `handleSubmit` は初回送信時・
  成功パス・エラーパスの **3箇所すべて**で `flushChunkQueue()` を呼ぶ
  (新しい相談の開始時に前回の残留キューを持ち越さないため初回送信時にも
  必要 — 見落としやすい)。
- **W-36 (タイマー多重化) の実装形**: `ensureTimer()` は `timerRef.current
  !== null` なら即 return するガードのみで多重起動を防ぐ。React
  StrictMode の二重マウントでも `useEffect(() => stopTimer, [stopTimer])`
  の cleanup が確実に走るため、2個目の interval が生き残ることはない
  (disposed フラグと同型の「フラグ1つで多重防止」パターン)。
- **W-39 (計測の錨)**: `response_time_sec` の起点は確定置換の `setMessages`
  呼び出し (`aiShownAtRef.current = Date.now()`) のまま — スロットル導入
  前後で1行も変更していない。スロットルは chunk の見せ方のみを変え、
  測定コードには一切触れない設計にすることで「1msも歪めない」を構造的に
  保証した。
- **共用の教訓**: `useThrottledStream` は CONSULT で先行検証したのみで、
  INTERVIEW (`InterviewTab.tsx`) へはまだ未適用 (F4c 以降の対象)。将来
  INTERVIEW にも適用する際は、chunk ハンドラを `pushChunk` に置き換え、
  確定置換の直前に `flushChunkQueue()` を呼ぶ、という同一の2手順を踏むこと。

**仕様との差異 (申告)**: なし。バックエンド不可触につき Python スイートは
対象外。検証: `tsc --noEmit` エラーなし、`python tests/ui_smoke.py` ALL PASS。

### F4a/F4b 完遂 (2026-07-08) — as-built (面接コンフィギュレータ + 成績表評価エンジン)

**F4a (コンフィギュレータ)**: `InterviewConfig` ({industry, genre,
difficulty}) はフロントの `SESSION_CONFIG` term-panel (`InterviewTab.tsx`)
で組み立て、`consult(q, {mode:"interview_sim", config})` として「開始」
ターンのみに送る (persona と同じ「開始時にのみ送る」規約 — 継続ターンでは
バックエンドが `self._interview_state["config"]` を保持しているため不要)。
バックエンドは `INTERVIEW_INDUSTRY_BANK`/`INTERVIEW_GENRE_BANK`/
`INTERVIEW_DIFFICULTY_LABELS` (`core/consultation_engine.py`) で ID→表示
ラベルを解決し、プリセット外の自由記述はそのままラベルとして使う
(es_manager のドメイン非依存原則と同居)。**優先順位は固定**: ES が
`data/es/` に存在すれば ES 駆動が常に勝ち、config は無視される
(記録用に `state["config"]` へは保持されるが出題内容には影響しない)。
未知フィールドは `.get()` で個別に読むだけなので自動的に無視される
(専用のバリデーション層は追加していない — 既存の境界防衛パターンを踏襲)。

**F4b (成績表): `core/interview_report.py` を新設**。narrative_compiler と
同型の「スキーマ検証→リトライ→上限で summary のみ返す」パターンを流用した。

- **軸ホワイトリスト + evidence 必須の実装**: `_parse_metrics()` は
  `AXIS_WHITELIST` (論理性/技術力/構成力/具体性) に無い軸・evidence が
  空文字の軸・**重複軸** (同じ axis が2回来たら2個目を無視) を無条件で
  削る。score は `int(round(float(...)))` 後に `max(0, min(100, ...))` で
  clamp — LLM が小数や範囲外を返しても構造体は必ず健全になる。
- **latency はコードのみが書く (憲法2)**: `synthesize_latency()` は
  `state["latencies"]` (UI が計測し `response_time_sec` として送ってきた
  実測値の履歴) から中央値・最大値・件数を算出する。この関数は LLM の
  `generate()` を一切呼ばない — `interview_report.v1` の `latency` ブロックは
  常にこの関数の戻り値そのもの。
- **リトライループの停止条件**: `len(metrics) < len(AXIS_WHITELIST)` の間
  最大 `MAX_RETRIES+1` 回 (既定3回) 生成をやり直す。**リトライを使い切っても
  `summary`/`latency`/`config` は必ず埋まった report を返す** (`metrics`
  だけが空配列になり得る) — narrative_compiler の「no_valid_claims で
  `es_text` を空にする」設計と同じ「講評本文は決して失われない」思想。
  UI (`InterviewTab.tsx` MISSION_RESULT) は `metrics.length === 0` を
  表示分岐で吸収する。
- **永続化 (壁A)**: `persist_report()` が `data/records/interviews/
  interview_{ISO風タイムスタンプ}_{genreスラグ}.json` へ書く。genre は
  `state["config"].get("genre")` を最優先し、config が無い場合は
  `case["format"]` (ケースバンク/config駆動時) または `"es_interview"`
  (ES駆動時) にフォールバックする。**このディレクトリの読み書きは
  `core/interview_report.py` のみに限定** — `profiler.py`/`gap_analysis.py`/
  `tensor_store.py`/`oracle.py`/`digital_twin.py` がこのパスへ結合したら
  壁A違反であり、`tests/test_integration.py::
  test_interview_records_isolated_from_profiler` (静的ソーススキャンで
  `INTERVIEW_RECORDS_DIR`/`records/interviews` 文字列の混入を検出する
  `_assert_no_gap_leak` の鏡像) がこれを検出する。
- **W-37 (UI は JSON.parse を書かない) の配線**: `ConsultationEngine
  .consult()` は呼び出しごとに `self._last_interview_report = None` へ
  リセットしてから各モードへディスパッチする (per-call スナップショット
  — 古いターンの成績表が別ターンの応答に紛れ込まない)。`interview_sim`
  の講評ターンのみがこれを実体化する。`facade.last_interview_report()`
  → `engine_stdio.py` の `consult` コマンドが `report` キーとして応答へ
  同梱し、UI (`InterviewTab.tsx`) は `res.report` をそのまま `setReport()`
  するだけ — JSON.parse は一度も書いていない。
- **UI**: `ScoreBar` (`<rect>`×10、TensionMeter と同型) を新設。点灯数は
  `Math.round(score/10)`、色は `score>=70 → --ok / >=40 → --accent / それ
  未満 → --err` (TensionMeter のセグメント色分けを踏襲した独自の閾値 —
  「スコアは高いほど良い」なので TensionMeter の危険増加方向とは逆順)。
  MISSION_RESULT はアニメーションなし (`transition` 未使用)。

**次の実装者への申告 (テストのハマりどころ)**: `FakeBackend`/
`ScriptedBackend` を使うテストで interview_sim の「講評」ターンの**直後**に
同じ `fake.calls` リストへ別の呼び出しを追加する場合、**インデックスを
固定値で書くな**。F4b の report 生成が講評テキスト生成の直後に
`engine.backend.generate()` を最大 `MAX_RETRIES+1` 回追加で呼ぶため、
講評より後の呼び出しの位置が呼び出し履歴内でずれる (`test_interview_sim_flow`
の再開始ターン検証がこれで実際に壊れ、`fake.calls[3]` → `fake.calls[-1]`
に修正した)。新しいテストを書くときは相対インデックス (`fake.calls[-1]`)
かフィルタ (`fake.calls[len_before:]`) を使うこと。

**仕様との差異 (申告)**: F4c (成長コンテキスト注入・過去2件の差分要約) は
本ミッションのスコープ外のため未着手 (次回ミッション)。

検証: `npx tsc --noEmit` / `cargo check` エラーなし。`tests/test_integration.py`
に4ケース追加 (config駆動出題・スキーマ+latency合成・軸ホワイトリスト拒否・
壁A静的分離ガード) して19ケース全て ALL PASS。`test_calendar_sync.py`/
`test_apple_calendar_sync.py`/`test_gap_analysis.py`/`ui_smoke.py` (実データ)
全て ALL PASS。SESSION_CONFIG/MISSION_RESULT の実描画・ES優先の実操作確認は
指揮官の実施を要する。

### F4c 完遂 (2026-07-08) — as-built (継続学習ループ。fable5 最終裁定の実装)

`core/interview_report.py` に `_genre_slug()`(persist_report と共有導出)・
`load_recent_reports(genre, limit=2)`・`compute_growth_context(genre)` を
追加。`consultation_engine.py` に `_interview_genre(cfg, case)` (セッション
開始時・講評時で同一の genre 導出。旧来の講評フェーズのインライン導出を
これに統一) と `_GROWTH_CONTEXT_TEMPLATE` を新設し、セッション開始の3経路
(ES駆動/config駆動/bankフォールバック) 全てが共通の1注入点 (ES駆動は専用
分岐、config駆動とbank駆動は `case` 確定後の共有分岐) を通るよう配線した。
成長コンテキストの読み込みは `_interview_state` 初期化時に1回だけ行われ
(W-44)、以後のターンではセッション開始時に焼き込まれた `system` 文字列を
再利用するだけなのでホットパスI/Oは発生しない。

**壁Bのコード側ガード**: `compute_growth_context` の戻り値は
`AXIS_WHITELIST` の固定ラベルと整数スコアのみから文字列結合され、
`evidence`/`summary` (LLM生成の自由テキスト) を一切参照しない — 型として
混入経路が存在しない。

**W-40〜W-44 の実装**: ソートは `Path.stat().st_mtime` を一切使わず
`sorted(glob(...))` のファイル名 (ISO basic タイムスタンプ) のみに依存
(W-40)。0件は空文字・1件はデルタなし焦点軸のみ (W-41)。`_genre_slug` を
persist/load 両方から呼ぶ一元化 (W-42)。欠測軸は「(前回データ無)」注記で
0と区別 (W-43、`newest`/`older` 双方に軸が存在する場合のみデルタを出す)。

**仕様との差異 (申告)**: なし。fable5 の§8裁定を実装レベルの差異なく実装
した。唯一の実装判断: `compute_growth_context` は「最新レポートが全軸欠測
(退化レポート)」の場合も空文字を返すよう追加した (SPEC には明記なし。
`focus = min(newest, ...)` が空dictに対して`ValueError`を投げるのを防ぐ
ための必須の防御的分岐であり、W-43の精神と矛盾しない)。

検証: `npx tsc --noEmit`/`cargo check` エラーなし (フロント不可触)。
`tests/test_integration.py` に4ケース追加 (ファイル名ソート決定性・
0/1/2件フォールバック・欠測軸マスク意味論・壁B注入後gap-leakガード) +
壁A鏡像テストへ`compute_growth_context`/`load_recent_reports`の静的スキャン
を追加。Python 15スイート全て ALL PASS、`ui_smoke.py` ALL PASS。実機での
面接複数回セッション (成長コンテキストが実際に出題へ反映される様子) の
対話的確認は指揮官の実施を要する。

### Rev.10 完遂 (2026-07-08) — as-built (相関ID復元。fable5 遺言への後継 Opus 訂正の実装)

**3層の実変更点**:
- `apps/desktop/src-tauri/src/commands.rs`: `pkb_invoke` に `cid: Option<u64>`
  引数を追加し `manager.invoke(&cmd, params, cid)` へ透過。
- `apps/desktop/src-tauri/src/engine.rs`: `invoke(cmd, params, cid)` /
  `invoke_sync(cmd, params, cid, _allow_restart)` へ拡張し、リクエストJSON
  最上位へ `"cid": cid` を追加 (`REQ_COUNTER`/`id`/`forward_event` は完全に
  無改造 — ロジック変更ゼロの配線のみを厳守)。呼び出し2箇所 (`shutdown` の
  `None` 直渡し、`invoke` の正常/再起動後リトライ経路) 両方を更新。
- `src/python/engine_stdio.py`: `main()` で `cid = req.get("cid")` を読み、
  `emit_event` クロージャのデフォルト引数 `_cid=cid` へ束縛。刻印は
  この1箇所のみ (`dispatch` 内の各コマンドは `emit(...)` を呼ぶだけで
  cid の存在を意識しない — W-48)。**最終応答 (`{"id","ok","result"}`) は
  cid を運ばない** — `event` キーを持たないため `engine.rs::invoke_sync` の
  `forward_event` 対象外であり (同期RPCの戻り値そのもの)、cid 照合が必要な
  のは中間イベント行のみという設計 (§9.1 の3層図の「全イベント」は
  intermediate な status/chunk 行を指す)。
- `apps/desktop/src/lib/useCorrelationId.ts` (新設): SPEC §9.1 のフックを
  そのまま実装。`_cidSeq` は単一モジュール変数、`begin`/`end`/`accepts`/
  `disposedRef` の構成。
- `ConsultTab.tsx`: `busyRef` を完全撤廃し `cid.accepts(payload)` ガードへ。
  `handleSubmit` は `cid.begin()` → `consult(q, {}, myCid)` → `finally` で
  `cid.end(myCid)`。`disposed`/`stickRef`/`flushChunkQueue` (F3/F3.5 の資産)
  は無改造で温存。
- `ImportTab.tsx`: `importingRef` を完全撤廃。`handleLine`/`handleIcs`/
  `handleApple`/`handleConfirmOther` の各ハンドラが `cid.begin()`/
  `cid.end()` を持つ。`handleConfirmOther` はループ内の複数IPC呼出しを
  **1つの `myCid`** で束ねた (「取込確定」1クリック = 1論理リクエストという
  設計判断。SPEC に明記はないが §9.3 の「1コンポーネント=1論理リクエスト」
  の精神と整合)。`mountedRef` は W-46 と役割が重なるが、unmount後も
  `importLog` への push を続ける F2 の設計 (W-29) を壊さないため意図的に
  温存 (SPEC の指示通り)。
- `InterviewTab.tsx`: **これは撤廃ではなく新設**。実装調査の結果、
  `pkb-engine-event` ハンドラに元々 busy 系ガードが一切無く (W-28/W-34 系の
  潜在バグ — F4a/F4bの実装時に見落とされていた)、`cid.accepts(payload)` を
  新規追加することで初めて他タブとの混線防御が入った。`send()` の
  `consult()` 呼び出しへ `cid.begin()`/`cid.end()` を配線。F4a/F4b の
  config/report ロジックは無改造。

**fable5 の遺言への訂正2点 (後継者が同じ誤解をしないための記録)**:
1. 「React層のみに限定せよ」という遺言の処方は**物理的に不可能**だった。
   相関ID (`id`) は `REQ_COUNTER` により Rust の `invoke_sync` 内部で採番
   され (`engine.rs:150` 相当)、`pkb_invoke` コマンドは `result` のみを
   返すため id はフロントへ一切 surface していなかった。フロントは
   「自分がどの id を割り当てられたか」を知る手段が構造的に無く、
   「`payload.id` と自分の in-flight id を照合する」処方は実装不能だった。
2. 直列性は「規約 (busyフラグを置く習慣)」ではなく**構造 (プロセスロック)**
   だった。`invoke_sync` は `self.process.lock()` をリクエスト全体の期間
   ロックし続け、イベント転送はこのロック下の読み取りループ内からのみ
   発生する。したがって2つの `pkb_invoke` はロックで構造的に直列化され、
   相関IDの導入だけでは並行実行は一切解禁されない。

**並行実行は本Revでは未達 (意図的なスコープ限定)**: 今回実装したのは
「イベントを正しい宛先へ配る」ための cid 基盤のみ。「2つのリクエストを
同時に飛ばす」ための構造 (`invoke_sync` のプロセスロック撤廃 + 単一
リーダースレッドによる id→チャネル demux) は §9.4 に **Target Golf**
として青写真のみ残し、本Revでは着手していない (YAGNI — 現行の直列
エンジンは正しく動作しており、まだ要求されていない並行実行のために
多重化を今建てるのはスコープクリープ)。cid 基盤は多重化の前提を無償で
用意するが、多重化そのものは独立ターゲットである。

**検証手段の限界の申告 (省略ではなく申告)**: `apps/desktop/package.json`
を確認した結果、フロントに vitest 等のテストランナーは存在しない。
そのため (b) 異cidイベントの相互非干渉 と (c) 超過リクエストの旧cidイベント
破棄 (W-45) は、フロント側の `accepts()` を直接ユニットテストする代わりに
**Python側のプロトコルテスト** (`tests/test_oracle.py` に3ケース追加:
`test_cid_stamped_on_all_events_single_path` (W-48)・
`test_cid_distinguishes_sequential_requests` (W-45 の前提となる
バックエンド側の cid 分離)・`test_cid_absent_request_emits_null_cid`
(cid 未指定リクエストの null 伝播)) に寄せ、`useCorrelationId.ts` の
コメントで `accepts()`/`begin()` の不変条件を明記する形で妥協した。
フロント `accepts()` 自体の純関数テストは持たない。

**検証コマンド (全て ALL PASS)**: `cargo check` / `python -m py_compile
src/python/engine_stdio.py` / `npx tsc --noEmit` / `rg
"busyRef|importingRef" apps/desktop/src` (ヒット0。コメント文言も含めて
リテラル一致を排除済み) / `tests/test_oracle.py` (新規3ケース含む) /
`tests/test_integration.py` (F4a/F4b/F4c の既存23ケース無退行) / Python
バックエンド15スイート全て ALL PASS / `ui_smoke.py` ALL PASS /
`npm run tauri:dev` 実起動で `[PKB] エンジン ready (stdio IPC,
完全オフライン)` をログ確認 (起動時に旧セッションの残留
`pkb-desktop.exe` がポート1420とビルドディレクトリを占有していたため
停止してから再起動 — 実機起動時の既知の運用上の注意点として記録)。

**実機での対話確認は指揮官の実施を要する**: 複数タブ (CONSULT/IMPORT/
INTERVIEW) を同時に操作した際のイベント混線ゼロの体感確認は、自動テストの
範囲外 (プロセスロックにより実際には直列実行されるため、真の同時発火では
なく「タブAの処理中にタブBへ切り替えて別処理を投げる」形の手動シナリオに
なる)。`tauri:dev` は起動済みのまま残してあるので、指揮官はそのウィンドウで
直接検証できる。

**仕様との差異 (申告)**: なし。SPEC Rev.10 §9〜§9.4 を実装レベルの差異なく
実装した。

---

### Rev.11 Phase A 完遂 (2026-07-09) — as-built (Target Sandbox: テスト隔離防壁 / F-15)

**急所**: `core/paths.py` の `PROJECT_ROOT`/`ES_DIR` 等は **import 時に確定する
束縛済み Path**。したがって `PKB_PROJECT_ROOT` は「いかなる `from core...`
import よりも前」に立てねば無効。`tests/conftest.py` はテスト収集
(= 各 `test_*.py` の import) より前に pytest が読むため、ここが唯一の
確実な差し込み点になる。

**実変更点**:
- `tests/conftest.py` (新設): モジュール最上部で `tempfile.mkdtemp(prefix=
  "pkb_test_")` → `os.environ["PKB_PROJECT_ROOT"]` を設定し、
  `data/{raw,es,knowledge,processed,records/interviews}`・`build`・`models`
  のスケルトンを作成。`atexit.register` で全体を `rmtree(ignore_errors=True)`。
  autouse・function-scoped の `_isolate_data` フィクスチャが各テストの前後で
  `data/{es,knowledge,records/interviews,processed}` を rmtree→再作成する
  (`data/raw` は対象外 — 同一ファイル内の複数テストが `data/raw` を介して
  状態を共有する既存パターンを壊さないための意図的な除外。SPEC §10.1 の
  施工構造どおり)。conftest 自身は `core` を import しない。
- `tests/test_sandbox.py` (新設): `test_sandbox_active` (PROJECT_ROOT/ES_DIR
  がリポジトリ実ルートを指さないことの二重証明) と
  `test_no_literal_data_writes` (`src/python/core` 配下の `open("data/...")`
  / `Path("data/...")` 直書きが `paths.py` 以外に無いことを静的検査する
  トリップワイヤ)。
- **既存8ファイルの自前 Sandbox 実装を `conftest.py` に一元化**
  (`test_oracle.py`/`test_integration.py`/`test_lsm_index.py`/
  `test_import_stats.py`/`test_narrative_compiler.py`/
  `test_line_telemetry.py`/`test_tensor_store.py`/`test_line_dedup.py`):
  各ファイルが個別に持っていた `os.environ["PKB_PROJECT_ROOT"] = _TMP`
  (無条件上書き) を `os.environ.setdefault("PKB_PROJECT_ROOT", _TMP)` へ変更。
  **これが本 Phase の実質的な根本修正** — 無条件上書きだと、pytest の
  テスト収集がファイルを import する順序 (アルファベット順) によって
  「最後に import されたファイルの `_TMP` が全テストに適用される」という
  実行順依存のレースが発生していた (これが「HEADで11失敗」の実体)。
  `setdefault` により conftest の Sandbox を尊重する。`_TMP` 変数は
  `test_tensor_store.py` のスクラッチパス生成等で再利用されているため維持。
- **汚染依存だった既存テストの自己完結化** (W-50): Sandbox 導入で
  「前のテストが書いた状態にあとのテストが暗黙に依存する」設計が可視化
  された。
  - `test_integration.py`: `test_offline_default_never_fetches` /
    `test_mock_fetch_ingestion_pipeline` が `test_fetch_tag_hook_and_queue`
    の副作用 (fetch queue への enqueue) に依存していたため、各テスト内で
    `kf.queue_fetch_queries(...)` を自前 seed。`test_es_review_isolation` /
    `test_adversarial_interview_with_es` / `test_gd_sim_chaos` /
    `test_dynamic_gd_personas` / `test_kv_prefix_cache` が
    `test_es_manager_dynamic_domain` の副作用 (`_write_phase3_assets()` に
    よる ES_DIR / DEEP_PROFILE の永続化) に依存していたため、各テストの
    先頭で `_write_phase3_assets()` を明示呼び出しに変更 (既存の
    `test_es_manager_dynamic_domain`/`test_oracle_payload_isolated_to_review_
    phase`/`test_puppeteer_injects_whitelisted_text_only` が既に確立していた
    パターンへの合流)。
  - `test_lsm_index.py`: `_reset_project()` が `data/raw` 全体を事前に
    `rmtree` するよう変更。`data/raw` は `_isolate_data` の対象外 (意図的)
    のため、アルファベット順で先に collect される他ファイル
    (`test_import_stats.py`/`test_integration.py` 等) が残す
    `calendar.json`/`finance.json`/`ai_consultations.json` の残骸が
    DailyContext チャンク数を狂わせていた (`assert 4 == 3` 等の失敗の実体)。
  - `test_data_source_stats_dir_sources` / `test_interview_sim_flow` /
    `test_interview_configurator_no_es` は Sandbox 導入だけで自動的に
    GREEN になった (元々自己完結していたか、`_isolate_data` の対象範囲と
    合致していたため追加の是正が不要だった)。
- `core/*` (本番ロジック) は無変更 (`git diff --stat src/python/core/` が
  空であることを確認済み)。

**検証結果 (DoD)**:
- `python -m pytest tests/ -q` (通常順): **144 passed, 0 failed**
  (Sandbox導入直後の一時的な17失敗 — 汚染依存7件 + `_TMP` 未定義の
  自己回帰7件 + LSM残骸3件 — を含め、最終的に全て解消)。
- 同じコマンドをファイル逆順 (`test_tensor_store.py` → ... →
  `test_apple_calendar_sync.py`) で実行: **144 passed, 0 failed** (実行順
  非依存の証明。`pytest-randomly` は未インストールのため逆順収集で代替)。
- 個別ファイル実行 (`test_lsm_index.py`/`test_integration.py`/
  `test_tensor_store.py`/`test_import_stats.py` の4ファイル同時指定):
  **52 passed**。単独スクリプト実行 (`python tests/test_oracle.py`・
  `python tests/test_lsm_index.py`) も ALL PASS (conftest 非経由でも
  `setdefault` 経路が機能することの確認)。
- `git status --short data/` を pytest 実行の前後で比較し **差分ゼロ**
  (本番 `data/es`・`data/knowledge` への新規書き込みなし)。
- `git diff --stat src/python/core/` が空 (本番ロジック無変更の証明)。

**仕様との差異 (申告)**: `test_no_literal_data_writes` は SPEC の想定どおり
「現状ゼロ検出」で実装 (簡素化は不要だった)。`_isolate_data` の対象を
SPEC 施工構造の4ディレクトリ (`data/{es,knowledge,records/interviews,
processed}`) に厳密に一致させ、`data/raw` は意図通り対象外とした
(その代替として `test_lsm_index.py` 側で自前リセットを実装 — SPEC の
「入力不足になったテストはテスト側で seed して直す」方針の解釈)。

**Phase A スコープ外で発見した既知問題 (このRevでは未修正・報告のみ)**:
`python tests/ui_smoke.py` を実行すると、実リポジトリの
`data/processed/metadata.json` (gitignore 対象・個人データ) の chunk
レコードが `chunk_id` キーを持つ一方、`core/lsm_index.py:329` の
`sync_diary_index_lsm()` は `c["id"]` を読もうとして `KeyError` になる。
これは Sandbox とは無関係な**実データの状態不整合** (おそらく LSM 化以前の
フォーマットの残骸、または最近の実機操作で生成された metadata.json が
現行 `lsm_index.py` のスキーマ前提とズレている) であり、Phase A の
スコープ (「core/\* を一切変更しない」「本番データに触れない」) 上、
本Revでは修正しない。指揮官の実データ (`data/processed/metadata.json`)
の検死、または `core/lsm_index.py` の別途是正指令を要する。

---

### Rev.11 Phase B 完遂 (2026-07-09) — as-built (単一ES保持と可視化 / F-16)

**方針の急所**: 指揮官裁定 (§10.2改定) により**レガシー ES は削除しない**。
`core/paths.py::ACTIVE_ES` (`data/es/active_es.md`) を唯一の真実の源とし、
読み手側 (`es_manager`) だけがそこへ収束することで、破壊操作ゼロで単一化を
実現する — レガシーファイルは物理的に残るが、`load_es_documents()`/
`select_es()`/`facade.active_es()`/`data_source_stats()["es"]` の**4経路
すべて**が構造的にそれを見ない。

**実変更点 (バックエンド)**:
- `core/paths.py`: `ACTIVE_ES = ES_DIR / "active_es.md"` を `ES_DIR` 直後に追加。
- `core/es_manager.py`:
  - `load_es_documents()`: `ES_DIR.iterdir()` の全走査を撤廃し、
    `ACTIVE_ES.exists()` の1点判定 + `[_parse_es(ACTIVE_ES)]` へ縮退。
  - `select_es(name)`: `name` を完全に無視 (引数は呼び出し側の互換のため
    残す)。常に `load_es_documents()[0]` (= active_es.md) または `None`。
  - 新設 `get_active_es() -> dict | None`: `_parse_es` の全フィールド +
    `char_count` (`len(body)`) を返す View 専用アクセサ。
- `core/facade.py`:
  - `import_document` の `dest=="es"` 分岐を `_import_es_document()`
    (新設ヘルパー) へ切替。ロジック: `ACTIVE_ES` 存在時は blake2b で
    内容一致を判定 (一致 → skip)、不一致/不在なら `ACTIVE_ES.write_bytes(...)`
    で**上書き** (Phase A で確立済みの `write_bytes` 罠 — `write_text` の
    `\n`→`\r\n` 変換がハッシュ比較を壊す既知の罠 — を踏襲)。**ES_DIR 内の
    他ファイルには一切触れない** (削除もリネームもしない)。`knowledge` 分岐
    (衝突時ハッシュ接尾辞で別名保存する既存ロジック) は無変更。
  - 新設 `active_es()`: `es_manager.get_active_es()` を UI 形
    (`{"exists": bool, title, target_domain, keywords, body, char_count,
    mtime}`) へ変換。未登録時は `{"exists": False}` のみ (他キー無し)。
    `filename` は含めない (W-32)。`__all__` に追加。
  - `data_source_stats()` の `"es"` エントリを `ES_DIR` の
    `_dir_file_count` 集計から `ACTIVE_ES.exists()` 基準
    (`count = 1 if exists else 0`) へ差し替え。`"knowledge"` 側は無変更。
- `engine_stdio.py`: `dispatch` に `if cmd == "es.view": return
  facade.active_es()` を追加 (`import.stats` と同型の純関数呼び出し。
  emit 不要)。

**実変更点 (フロントエンド)**:
- `lib/types.ts`: `EsView` interface 新設。
- `lib/engine.ts`: `esView(): Promise<EsView>` 新設
  (実装当時は汎用IPC経由。FSA-2026-07-13-03で明示command + runtime parserへ移行済み。
  状態取得の純クエリのため cid 引数なし)。
- `components/ImportTab.tsx`:
  - `esActive` state 新設。初期ロード (`refreshStats` と並行) で
    `refreshEsActive()` を呼ぶ。`mountedRef` ガード踏襲。
  - `DATA_SOURCES` term-panel の直後に新設 `term-panel "ES_ACTIVE"`:
    未登録時は `<p className="hint">登録済み ES なし</p>`。登録時は
    `term-row` で title/target_domain/char_count を表示し、本文は
    `<details><summary>本文を表示</summary><pre className="term-es-body">`
    のスクロール View (`max-height: 240px; overflow-y: auto`)。
  - `handleConfirmOther` のループ内、`dest === "es"` の import 成功直後に
    `refreshEsActive()` を呼び ES_ACTIVE を即時更新 (`knowledge` import 時は
    呼ばない — 差分のみ反映)。
  - `App.css`: `.term-es-details`/`.term-es-body` を新設。両方に
    `font-family: var(--font-mono)` を明示 (F-10)。絵文字は使用していない
    (F-9)。

**回帰テスト (`tests/test_import_stats.py` に追加)**:
`test_es_import_overwrites_single` / `test_es_import_idempotent` /
`test_legacy_es_invisible` / `test_active_es_view_shape` /
`test_data_source_stats_es_single` の5件を新設。いずれも Sandbox 上で
実行され、`test_active_es_view_shape`/`test_data_source_stats_es_single`
は単独スクリプト実行 (`python tests/test_import_stats.py`) 時にも
「未登録から始まる」前提を自前で保証するため `shutil.rmtree(ES_DIR,
ignore_errors=True)` を明示的に行う (W-50: pytest の `_isolate_data` に
頼らず自己完結)。既存 `test_data_source_stats_dir_sources` は
`ES_DIR/es1.md` への直接書き込みから `ACTIVE_ES` への seed に変更して
是正 (旧ディレクトリ走査モデルの前提が崩れたため)。

**既存 ES 依存テストの是正 (`tests/test_integration.py`)**:
- `_write_phase3_assets()` が `ES_DIR/opengl_engine.md` +
  `ES_DIR/film_planning.md` (2ファイル・mtime操作で新旧を作る旧モデルの
  フィクスチャ) を書いていたのを、`ACTIVE_ES` への単一書き込みへ縮退。
- `test_es_manager_dynamic_domain`: 旧モデル (2件保持・名前部分一致選択・
  mtime降順) の検証だったため、単一 `active_es.md` + `name` 無視の検証へ
  書き換え。「明示フィールドなし ES は本文語彙からドメインを導出する」
  (業界ハードコードなしの原則の証明) というテスト意図は失わず、
  `test_es_manager_implicit_domain_from_text` として独立させた
  (F-16 単一化により同時に2件を保持できないため、同一テスト内では
  もはや両方を検証できない)。
- `test_es_review_isolation` / `test_adversarial_interview_with_es`:
  `consultation_engine.py` (無変更・不可触) がログへ書く ES 記録名
  (`f"[es_review] {es['name']}"` 等) が、旧来の `"opengl_engine"` から
  常に `"active_es"` (= `active_es.md` の stem) へ変わる自然な帰結を
  アサーションへ反映 (機能的な後退ではなく、単一化の直接的な副産物)。

**検証結果 (DoD)**:
- `python -m pytest tests/ -q` (通常順): **150 passed, 0 failed**
  (Phase A の144 + Phase B新設6 = 150。退行ゼロ)。
- 同コマンドをファイル逆順で実行: **150 passed, 0 failed** (実行順非依存)。
- `npx tsc --noEmit` (apps/desktop): エラーなし。Rust 変更なしのため
  `cargo check` は対象外 (DoD (2) の通り)。
- `git status --short data/`: 差分ゼロ (テスト実行前後で比較)。
- `git diff --stat src/python/core/`: `es_manager.py`/`facade.py`/
  `paths.py` の3ファイルのみ (`engine_stdio.py` は `core/` 外)。
  `twin`/`oracle`/`profiler`/`consultation_engine` 等の聖域は無変更
  (`narrative_compiler.py` の `_persist_draft()` が `ES_DIR/draft_*.md`
  へ書く既存経路も無変更 — これは W-53 が禁じる「新設の**読み**経路」
  ではなく既存の書き込み経路であり、かつ `active_es.md` 以外の
  ファイル名であるため単一化と衝突しない)。

**仕様との差異 (申告)**:
1. ミッション文中の `<details>` 許可根拠の引用が「F-9」表記だったが、
   `SPEC_FOXTROT_UI.md` の実際の該当規則は **F-8**
   (「`<details>` の使用は SETTINGS の Advanced 1 箇所のみ」・F-9 は
   絵文字禁止規則)。指揮官裁定として本 ES_ACTIVE パネルへの `<details>`
   使用を明示的に許可された前提でそのまま実装したが、これにより
   `<details>` の使用箇所が SETTINGS Advanced + IMPORT ES_ACTIVE の
   **2箇所**になった。F-8 の規則文 (「1箇所のみ」) は現状のまま未更新
   なので、指揮官の裁定で「2箇所」への更新、または引用の是正
   (F-9→F-8) をご確認いただきたい。
2. Phase A で報告した `ui_smoke.py` の既知問題
   (`data/processed/metadata.json` の `chunk_id`/`id` 不整合) は本 Phase
   でも解消していない (スコープ外・無変更)。加えて本検証環境には
   実データ (`data/raw/diary.md` 等) が存在しないため
   `python tests/ui_smoke.py` は `FileNotFoundError` で早期終了する
   (これは Phase B の変更起因ではなく、本検証環境に実データが無いという
   環境条件そのもの — 指揮官の実機での確認を要する)。
3. `tests/test_integration.py::test_offline_default_never_fetches`
   (および `test_fetch_tag_hook_and_queue` との組み合わせ) は、**pytest
   経由では GREEN** だが、`python tests/test_integration.py` の単独実行
   (conftest の `_isolate_data` 非経由) では実行順依存で失敗することを
   確認した。この問題は Phase A のベースライン (コミット `4e8bc7b`)
   から既に存在する pre-existing の問題であり、Phase B のいかなる変更にも
   起因しない (`git stash` で Phase B の変更を除いた状態でも同じ失敗を
   再現し確認済み)。DoD (1) が要求する「pytest GREEN・正順/逆順」は
   完全に満たしているため Phase B の完了条件には影響しないが、
   スコープ外の既知問題として申告する (修正には `test_integration.py`
   の fetch queue テスト群への手を Phase B の許可範囲外で入れる必要が
   あるため、本Revでは未修正)。
4. 手動確認 (実機での ImportTab の ES_ACTIVE パネル表示・体感) は
   DoD (5) の通り不要と判断し実施していない。指揮官の実施を要する。

**誓約の充足確認**: 保持ESは `active_es.md` ただ1件へ収束し (レガシーは
削除せず不可視化)、UIで現ESをView可能、フル pytest は実行順非依存で
GREEN、`data/` 汚染ゼロ — 以上を全て満たしている。

---

### Rev.11 Phase C 完遂 (2026-07-09) — as-built (es_review 無latency性の固定 / F-17)

**前提の確認**: 着工前に `_consult_es_review()` (計算経路) と
`consult()` のモード分岐を実測した結果、**除去すべき latency 注入の実体は
元々存在しなかった** — `_consult_es_review` のシグネチャに
`response_time_sec` が無く、`consult()` の `es_review` 分岐もそれを
`_consult_es_review` へ渡していない。敵は「UIの偽装 hint (全モード共通で
『回答時間は計測され…』を表示していた)」と「将来この構造が壊れて latency
が紛れ込むこと」の2つだけ。本Phaseは**封印 (現状の構造的不可能性を
コメント・テスト・UI表示の3点で固定)**であり、ロジック変更はゼロ。

**実変更点 (フロントエンド)**:
- `InterviewTab.tsx`: `MODES` 配列の `hint` フィールドをモード別の完全な
  文言へ拡張 (`interview_sim`/`gd_sim` → 「回答時間を計測し、思考速度も
  講評対象になります。」、`es_review` → 「書類単体の論理的強度のみを
  評価します (思考速度は評価しません)。」)。表示側 (`{currentMode.hint}`)
  は単一箇所のみで、以前あった「全モード共通の latency 文言をハード
  コードで連結」する二重定義を撤廃 (`currentMode.hint` を単一の真実の源
  にする — MODES 配列がモードごとの文言を完全に持つ)。
- `handleEsReview()`: `send()` への呼び出しが `opts.responseTime` を渡さない
  ことに「意図的な設計であり事故ではない」旨のコメントを追加
  (`send()` は `opts.responseTime === undefined` の時
  `response_time_sec` を組み立てない既存ロジックは無変更)。

**実変更点 (バックエンド — コメントのみ・ロジック変更ゼロ)**:
- `consultation_engine.py::consult()` の `mode == "es_review"` 分岐、
  および `_consult_es_review()` のdocstringに、F-17 を根拠として
  「このシグネチャに `response_time_sec` を追加してはならない」という
  明文化コメントを追加。`git diff --stat` は本ファイルのみ・
  変更は「コメント追加+docstring追記」の11行 (ロジック行の変更ゼロ)。

**回帰テスト (`tests/test_integration.py` に追加)**:
- `test_es_review_ignores_latency`: `ScriptedBackend` で
  `consult(mode="es_review", response_time_sec=42.0)` を呼び、(a) 例外なく
  完走し正常応答を返すこと、(b) reviewer へ渡る system/user プロンプトの
  いずれにも「秒」「応答時間」「latency」「response_time」のいずれの
  文字列も含まれないことを assert。
- `test_interview_latency_preserved` (退行検出の鏡): `interview_sim` で
  `response_time_sec=15.5` 付きターン→「講評」を回し、ターンプロンプトに
  「15.5 秒」が注入され、講評プロンプトに「応答時間 (Response Latency)」
  セクションと「思考速度」が依然含まれることを assert。F4b の latency 評価
  (`synthesize_latency`・成績表 median/max/n) が Phase C で巻き添えに
  壊れていないことの直接証明。

**検証結果 (DoD)**:
- `python -m pytest tests/ -q` (通常順): **152 passed, 0 failed**
  (Phase B の150 + Phase C新設2 = 152。退行ゼロ)。
- 同コマンドをファイル逆順で実行: **152 passed, 0 failed** (実行順非依存)。
- `npx tsc --noEmit` (apps/desktop): エラーなし。Rust 変更なしのため
  `cargo check` は対象外。
- `git status --short data/`: 差分ゼロ。
- `git diff --stat src/python/core/consultation_engine.py`:
  11行 (コメント/docstring追記のみ・ロジック変更ゼロ)。
  `synthesize_latency`/`_format_latency_section`/`_consult_interview_sim`/
  `_consult_gd_sim` の実装本体には**1行も触れていない**
  (`git diff` で証明済み — 変更箇所は `_consult_es_review` の docstring と
  `consult()` の `es_review` 分岐のコメントのみ)。

**仕様との差異 (申告)**: なし。ミッション文の前提通り、除去すべき latency
注入の実体は最初から存在せず、本Phaseは「封印 (コメント + hint是正 + 回帰
テスト)」のみで完結した。

**誓約の充足確認**: es_review は思考速度を計測も評価もしない (UI hint も
モード別に正確化)。interview_sim/gd_sim の latency (F4b) は無傷
(`test_interview_latency_preserved` で直接証明)。フル pytest は実行順
非依存で GREEN (152 passed)。

---

### Rev.11 Phase D 完遂 (2026-07-09) — as-built (面接スタンス選択式化 / F-18)

**方針**: 面接スタンス (敵対的/標準) を UI で選択可能にし、全 `interview_sim`
経路 (ES駆動/config駆動/bank駆動) のプロンプトへ反映。既定は `"adversarial"`
(指揮官裁定: 既存のストレステスト契約を無断で弱めない)。`stance` は
`_interview_genre` に混ぜない (W-42: genre slug 分裂 = 成長ループ分断)。
`gd_sim`/`es_review`/`build_reviewer_persona` (ES添削)/latency 系は無変更。

**実変更点 (バックエンド)**:
- `es_manager.build_interviewer_persona(es, stance="adversarial")`:
  共通ヘッダ (ドメイン専門家設定) + stance 別スタイル節。
  `adversarial` = 現行の圧迫スタイル維持、`standard` = 穏和・建設的スタイル。
  両 stance 共通で「人格攻撃はしない。攻撃対象は常に論理と事実」を維持。
  未知 stance は adversarial にフォールバック。
- `consultation_engine.py`:
  - モジュール定数 `STANCE_CLAUSES` (非ES経路用) と `_stance_clause(cfg)` 新設。
  - `_consult_interview_sim`: ES経路 → `build_interviewer_persona(es, stance=cfg...)`
    、非ES経路 (config/bank) → `INTERVIEWER_SYSTEM_PROMPT + _stance_clause(cfg)`。
    成長コンテキスト (`_GROWTH_CONTEXT_TEMPLATE`) の付加順序は現行維持
    (stance 節は growth より前 = persona 定義の一部)。`stance` は既存の
    `state["config"]=cfg` に自然継承 (追加保存配線不要)。

**実変更点 (フロントエンド)**:
- `types.ts`: `InterviewConfig` に `stance: "adversarial" | "standard"` 追加。
- `InterviewTab.tsx`: `STANCE_OPTIONS` 定義、`DEFAULT_CONFIG.stance="adversarial"`、
  SESSION_CONFIG term-panel に「面接スタンス」select 追加 (難易度 select と同型)。
  F-13 (明示フィールドのみ送信) 遵守 — 既存 `withConfig: true` 経路で
  `config` 全体が送られるため追加配線不要。

**回帰テスト (`tests/test_integration.py` に追加/更新)**:
- `test_stance_switches_persona`: ES seed 後、`build_interviewer_persona` の
  adversarial/standard で圧迫語/建設語が切り替わることを assert。
- `test_stance_does_not_split_genre`: 同一 industry/genre で stance を変えても
  `_interview_genre` が同一 slug を返すことを assert (W-42)。
- `test_nonES_stance_clause_applied`: ES 不在 config 駆動で system に
  standard stance 節が付くことを assert。
- 既存 `test_interview_sim_flow` / `test_interview_configurator_no_es` を
  `INTERVIEWER_SYSTEM_PROMPT + _stance_clause(...)` 期待値へ更新 (bank/config
  駆動の既定 adversarial stance 節付加に追随)。

**検証結果 (DoD)**:
- `python -m pytest tests/ -q` (通常順): **155 passed, 0 failed**
  (Phase C の152 + Phase D新設3 = 155。退行ゼロ)。
- 同コマンドをファイル逆順で実行: **155 passed, 0 failed** (実行順非依存)。
- `npx tsc --noEmit`: エラーなし。Rust 変更なし。
- `git status --short data/`: 差分ゼロ。
- 変更ファイル: `es_manager.py` / `consultation_engine.py` / `types.ts` /
  `InterviewTab.tsx` / `test_integration.py` の5ファイルのみ。
  `build_reviewer_persona` (ES添削)/`_interview_genre`/latency 系/gd_sim/
  es_review の実装本体は無変更 (`git diff --name-only` で証明)。

**仕様との差異 (申告)**: なし。

**誓約の充足確認**: stance は UI で選択でき既定は adversarial、全 interview_sim
経路に反映され、genre slug は stance で分裂しない (成長ループ不変)。フル
pytest は実行順非依存で GREEN (155 passed)。

---

### Rev.11 Phase E 完遂 (2026-07-09) — as-built (GD学習ループ配線 / F-19)

**監査結果 (着工前提)**: `interview_sim` は「開始で成長注入 / 講評で成績表
永続化」の両輪を持つが、`_consult_gd_sim` は `append_consultation` のみで
`generate_report`/`persist_report`/`_last_interview_report`/成長注入の
**全てを欠いていた**。GD は学習ループから完全に脱落していた。本Phaseは
`_consult_gd_sim` を `_consult_interview_sim` と対称化してこれを塞ぐ。
`es_review` は対象外 (書類レビューであり面接ではない — 設計上の除外、穴ではない)。

**実変更点 (`src/python/core/consultation_engine.py` — 純追加、既存行の書き換えゼロ)**:
- モジュール定数 `GD_GENRE = "group_discussion"` を新設 (`_interview_genre`
  相当。GD には config 由来の可変 genre が無いため全セッション共通の固定
  slug とする — W-42 の鏡像)。
- `_consult_gd_sim` の START ブロック: `build_gd_system_prompt` 直後に
  `compute_growth_context(GD_GENRE)` を呼び、在れば
  `_GROWTH_CONTEXT_TEMPLATE` で system へ1回だけ注入 (W-44)。`_gd_state` に
  `"config": {"genre": GD_GENRE}` を追加保持。
- `_consult_gd_sim` の 講評END ブロック: `append_consultation` の直後に
  `generate_report` → `persist_report(report, GD_GENRE)` (OSError は握り
  つぶし講評提示をブロックしない) → `self._last_interview_report = report`
  を追加。`self._gd_state = None` (Phase F で phase 遷移に置き換わる箇所)
  は現状のまま — Phase E ではスコープ外として触っていない。
- UI 変更なし: `engine_stdio` の `last_interview_report → result.report`
  配線と `InterviewTab` の `if (res.report) setReport()` が既存のまま GD の
  MISSION_RESULT パネルを自動表示する (確認のみ、コード変更不要)。

**回帰テスト (`tests/test_integration.py` に追加)**:
- `test_all_interview_modes_persist`: `interview_sim`/`gd_sim` を
  `ScriptedBackend` で START→継続→講評まで回し、両モードで
  `_last_interview_report` が設定され `INTERVIEW_RECORDS_DIR` へ新規 `.json`
  が増えることを対称に assert (pytest.mark.parametrize は本ファイルの
  規約 — 単独実行可能な平関数群 — に合わせずループで代替)。
- `test_gd_growth_injected`: `group_discussion` genre で過去成績2件を
  `_write_report_at` で seed → GD 開始の system に「訓練継続コンテキスト」
  「最重点課題軸」が含まれ、`GAP_LEAK_MARKERS` (壁B) が一切混入しない
  ことを assert。

**検証結果 (DoD)**:
- `python -m pytest tests/ -q` (通常順): **157 passed, 0 failed**
  (Phase D の155 + Phase E新設2 = 157。退行ゼロ)。
- 同コマンドをファイル逆順で実行: **157 passed, 0 failed** (実行順非依存)。
- `npx tsc --noEmit`: エラーなし。Rust 変更なし。
- `git status --short data/`: 差分ゼロ。
- `git diff --stat src/python/core/`: `consultation_engine.py` のみ
  **+28/-0 (純追加、削除ゼロ)** — 既存の `_consult_interview_sim`/
  `_interview_genre`/`interview_report.py` (diff空)/latency 系の実装本体は
  一切書き換えていないことを構造的に証明。

**申告 (Phase Eのスコープ外・既存事象)**: `python tests/test_integration.py`
(standalone 直接実行) が `test_nonES_stance_clause_applied` で
`AssertionError` を出す。`git stash` で Phase D 時点 (コミット `cf70c46`)
まで遡って再現することを確認済み — **Phase D 由来・Phase E とは無関係の
既存不良**。pytest 経由 (本プロジェクトの正式 DoD 基準) は正順・逆順とも
157/157 GREEN であり影響なし。standalone runner の `conftest.py` 非経由
実行順依存の問題 (Phase A 完了報告で申告された `test_offline_default_
never_fetches` と同種) として別途追跡が必要。

**誓約の充足確認**: GD は開始で成長を読み、講評で成績表を永続化する。全
模擬面接 (interview_sim/gd_sim) が等しく学習ループに乗る。壁B (生gap非注入)
は GD でも不変。フル pytest は実行順非依存で GREEN (157 passed)。

---

### Rev.11 Phase F 完遂 (2026-07-09) — as-built (感想戦 / Debrief 対話フェーズ / F-20)

**Rev.11 最終Phase。** 講評出力後もセッションを継続でき、AIが「面接の文脈と
評価をすべて記憶した建設的メンター」として改善を議論できる感想戦フェーズを
導入。状態を「面接中(active)」→「感想戦(debrief)」へ遷移させる。
interview_sim/gd_sim の両方に対称に実装した。

**実変更点 (`src/python/core/consultation_engine.py` のみ、+73/-4)**:
- `build_mentor_persona()` 新設: 引数不要の定数ペルソナ。面接官/選考官と
  違い講評・スコアの開示を許す唯一の人格 (「面接官の仮面はもう外してよい」)。
- `_debrief_turn(state, q, status, on_token)` 新設 (interview_sim/gd_sim
  共通メソッド)。材料は【既に公開された成果物のみ】= transcript / summary
  (講評本文) / report metrics。生の `_gap_section()`/`_oracle_section()`
  (聖域) は一切参照しない (壁B・W-52 の構造的遵守 — このメソッドはそもそも
  gap/oracle にアクセスするコードパスを持たない)。`append_consultation` は
  呼ばない (成績表は確定済み。感想戦はセッション内のみ・永続化しない)。
- 講評END ブロック (interview_sim/gd_sim 両方): `self._interview_state =
  None` / `self._gd_state = None` の null 化を削除し、`state["phase"] =
  "debrief"` + `state["summary"]`/`state["report"]`/`state["mentor_system"]
  = build_mentor_persona()` への遷移へ差し替え (削除4行のうち2行はこの
  null化行、残り2行は say() 文言の更新)。
- 冒頭ディスパッチ (両モード): `state = self._..._state` 取得直後・
  `INTERVIEW_END_COMMANDS` 判定より前に `if state.get("phase") ==
  "debrief":` 分岐を追加。debrief 中の END コマンドは state を None へ戻し
  別れの文言を返す。それ以外は `_debrief_turn` へ routing。START コマンドは
  冒頭の `if self._..._state is None or q in START:` が先に捕捉するため、
  debrief 中の「開始」は新セッションとして正しくリセットされる (優先順位は
  無改変で健全)。

**実変更点 (フロントエンド `apps/desktop/src/components/InterviewTab.tsx`)**:
- `sessionActive: boolean` を `phase: "idle" | "active" | "debrief"` へ
  完全置換 (残存参照ゼロを `grep` で確認済み)。
- `send()` の speaker 決定に `phase === "debrief"` 分岐を追加: mode に
  依らず speaker="メンター" (GD の `splitSpeakers` 複数話者分解は適用しない
  — メンターは単一の統合された声)。
- `handleStart` → `setPhase("active")`。`handleFeedback` → 成功後
  `setPhase("debrief")` (report はクリアしない — 感想戦中も MISSION_RESULT
  を見せ続ける)。新設 `handleDebriefEnd()`: `send("終了")` 後
  `setPhase("idle")` + メッセージ/report リセット (switchMode 相当の後始末)。
- `handleSend`: `phase === "debrief"` のとき `responseTime` を組み立てない
  (感想戦に思考速度評価は無い)。
- `showLobby`/`showConfig`: `!sessionActive` → `phase === "idle"`。
- ヘッダー: 「講評 (Feedback)」ボタンは `phase === "active"` のみ表示、
  `phase === "debrief"` で「感想戦を終了」ボタンに切り替え。
- フォーム表示条件: `phase === "idle" && mode !== "es_review"` で Start
  ボタン、それ以外 (active/debrief/es_review) で textarea
  (debrief 中は placeholder が「感想戦: 質問を入力…」)。

**回帰テスト (`tests/test_integration.py` に追加)**:
- `test_debrief_transition_and_turn`: interview_sim/gd_sim 両方で講評後に
  state が null化されず `phase=="debrief"` になること、続く非END入力が
  `build_mentor_persona` 由来の system で応答し `transcript` に
  `("メンター", ...)` が積まれることを対称に assert。
- `test_debrief_no_sanctuary_leak` (壁B構造証明): `_write_phase3_assets()`
  で gap/oracle を seed した状態で debrief ターンを回し、メンターへ渡る
  system/user プロンプトに `GAP_LEAK_MARKERS` が一切含まれないことを assert
  (講評フェーズ自体は仕様通り gap/oracle を統合するが、その後の感想戦
  ターンには漏れないことを検証)。
- `test_debrief_end_closes`: debrief 中に「終了」を送ると state が None に
  戻り、別れの文言が返ることを assert。
- **既存4テストの終端条件を是正** (講評 answer 自体の内容 assert は不変、
  state の終端条件のみ調整): `test_interview_sim_flow` /
  `test_adversarial_interview_with_es` / `test_gd_sim_chaos` /
  `test_dynamic_gd_personas` が「講評後に `_interview_state`/`_gd_state`
  が `None`」を前提にしていたため、「`None` ではなく
  `phase=="debrief"`」への期待値更新 (Phase F 着工直後の pytest 実行で
  4件が想定通り RED になり、この是正で GREEN に復帰したことを確認済み)。

**検証結果 (DoD)**:
- `python -m pytest tests/ -q` (通常順): **160 passed, 0 failed**
  (Phase E の157 + Phase F新設3 = 160。既存4テストの終端条件是正を含めて
  退行ゼロ)。
- 同コマンドをファイル逆順で実行: **160 passed, 0 failed** (実行順非依存)。
- `npx tsc --noEmit`: エラーなし。Rust 変更なし。
- `git status --short data/`: 差分ゼロ。
- `git diff --stat src/python/core/`: `consultation_engine.py` のみ
  **+73/-4**。削除4行は `git diff` で確認済み — `_interview_state = None`/
  `_gd_state = None` の null化行2行と、say() 文言更新2行のみ。
  `_interview_genre`/`_consult_es_review`/`synthesize_latency`/
  `_format_latency_section` への変更なし (grep で確認)。
  `interview_report.py` は diff 空 (完全無変更)。

**仕様との差異**: なし。

**誓約の充足確認**: 講評後に state は debrief へ遷移し感想戦を継続できる。
メンターは講評・スコアを開示してよいが、生の gap/oracle (聖域) は感想戦へ
一切注入しない (壁B不変・構造的に注入経路自体が存在しない)。フル pytest は
実行順非依存で GREEN (160 passed)。

---

### Rev.11 Phase P2 完遂 (2026-07-09) — as-built (テスト基盤の完全無菌化)

**方針**: pytest 収集 + conftest Sandbox を全テストの唯一の実行経路に統一。
TUI スモークも Sandbox 上で自己完結させ、実 `data/` への書込ゼロを DoD に含める。

**実変更点 (`tests/` のみ — `core/*` 無変更)**:
- `tests/ui_smoke.py` → `tests/test_ui_smoke.py`: pytest 収集対象化。
  `pytest.importorskip("textual")` で textual 不在環境は SKIP。
  `asyncio.run(_smoke())` で同期テスト関数から完結 (pytest-asyncio 不要)。
  `_seed_sandbox_tui_fixtures()` が `core.paths` 経由で `DIARY_MD` /
  `CALENDAR_JSON` / `FINANCE_JSON` を seed。`facade.get_engine` を
  `_FakeEngine` (sync_diary_index no-op) に差し替え、RECORD 保存時の
  embedder/pipeline 起動を遮断。実データの backup/restore ロジックは撤去。
- **16 ファイルの `if __name__ == "__main__":` ブロックを削除**
  (`test_apple_calendar_sync` / `test_calendar_sync` / `test_coupling` /
  `test_digital_twin` / `test_gap_analysis` / `test_import_stats` /
  `test_integration` / `test_line_dedup` / `test_line_telemetry` /
  `test_lsm_index` / `test_narrative_compiler` / `test_oracle` /
  `test_question_bank` / `test_sandbox` / `test_search_daemon` /
  `test_tensor_store`)。各ファイル先頭の `_TMP` + `setdefault(
  "PKB_PROJECT_ROOT", _TMP)` は維持 (pytest 下では no-op)。

**検証結果 (DoD)**:
- `python -m pytest tests/ -q` (通常順): **161 passed, 0 failed**
  (160 + `test_ui_smoke` 1件)。
- 同コマンドをファイル逆順で実行: **161 passed, 0 failed** (実行順非依存)。
- `git status --short data/`: 差分ゼロ (pytest 実行前後)。
- `test_*.py` 内の `if __name__ == "__main__"`: **0 件** (stdlib grep 確認)。

**仕様との差異**: なし。

---

### Rev.11 Foxtrot F5 完遂 (2026-07-09) — as-built (SETTINGS iOS 化 / Option 3)

**方針 (指揮官裁定 Option 3)**: 再利用可能な iOS Toggle 部品 + Advanced
`<details>` 骨格 + SETTINGS 視覚 refresh のみ。新規 boolean 設定・バックエンド
settings 拡張・localStorage/Tauri store は**一切しない** (settings-backend フェーズへ
繰り延べ)。

**実変更点 (フロントのみ — `src/python/**` 無変更)**:
- `apps/desktop/src/components/Toggle.tsx` (新設): 制御コンポーネント。
  `<input type="checkbox">` + `<label>` の CSS-only iOS スイッチ。
  `onChange` 未指定 or `disabled` 時は読取専用。データ値ラベルは `--font-mono`
  (F-10)。
- `apps/desktop/src/App.css`: `.toggle*` トークン (--accent / --border / --t-fast
  のみ。F-1)。`.settings-list` / `.settings-row` (iOS 角丸グループ +
  SettingRow)。`.settings-advanced` (`<details>` 1 箇所のみ — F-8)。
- `apps/desktop/src/components/SettingsTab.tsx`: 基本情報を SettingRow レイアウトへ
  refresh (保存ロジック無変更)。Advanced に Apple カレンダー連携
  (`apple_calendar_available` を disabled Toggle で表示 — F-14) と
  「再分析 (profiler)」ボタンを移設。`<details>` は SETTINGS Advanced のみ。
- `docs/SPEC_FOXTROT_UI.md` §2.6: F5 Option 3 as-built を 1 段落追記。

**検証結果 (DoD)**:
- `npx tsc --noEmit`: エラーなし。
- `python -m pytest tests/ -q`: **161 passed** (退行なし)。
- `git diff --stat src/python/`: 空 (バックエンド無変更)。

**仕様との差異**: Advanced 内の LLM params / port / KV 露出は意図的に未実装
(将来 settings-backend 配線待ち)。F-8/F-14 は本施工で充足。

---

### Phase F6 完遂 (2026-07-10) — as-built (PROBE タブ UI)

**方針**: D1 (`core.source_code` / `core.probe_engine`) と D2
(`core.probe_funnel`) の GREEN 実装を、Tauri + React UI へ薄く接続した。
UI 層は外部 API・localStorage・乱数・LLM 呼び出しを持たず、PROBE の質問選択
と保存は backend の決定論的ファネルに委譲する。表示は `message_code` と
`contact_alias` 済みデータのみを扱い、Evidence quote / `fact_text` / 実名は UI
に出さない。

**実変更点**:
- `src/python/core/facade.py`: `get_source_code` / `probe_status` /
  `probe_next` / `probe_answer` を追加。`load_daily_contexts` /
  `load_line_telemetry` / `load_probe_store` / `compute_source_code` /
  `probe_funnel` を組み合わせる薄い orchestration のみ。`today` は UI から
  ISO date として受け取り、PROBE store は `save_probe_store` で保存。
- `src/python/engine_stdio.py`: `profile.source_code` / `probe.status` /
  `probe.next` / `probe.answer` の dispatch を追加。
- `apps/desktop/src/lib/types.ts`: PROBE 用型 (`ProbeAxis` /
  `ProbeStage` / `ProbeStatus` / `ProbeQuestionView` /
  `ProbeAnswerResult` / `SourceCodeView`) と `MainTab = ... | "probe"` を追加。
- `apps/desktop/src/lib/engine.ts`: `sourceCode` / `probeStatus` /
  `probeNext` / `probeAnswer` wrapper を追加。`probe.answer` は snake_case
  (`session_id`, `question_id`, `answer`, `today`) で IPC へ渡す。
- `apps/desktop/src/App.tsx`: PROBE タブを INTERVIEW と SETTINGS の間に登録。
  Alt ショートカットは `Alt+1..6` へ拡張。
- `apps/desktop/src/components/ProbeTab.tsx` (新設): 5軸レール、FACT →
  CONTEXT → EMOTION → MEANING stepper、静的質問パネル、120字 `maxLength`
  入力、文字数カウンタ、Ctrl/Meta+Enter 送信、決定論的 insight 表示を実装。
- `apps/desktop/src/App.css`: `.probe-*` class を追加。既存 design token
  (`--accent` / `--border` / `--bg-*` / `--text-*`) のみを使用し、新規 hex 色は
  追加しない。
- `tests/test_probe_ui_ipc.py` (新設): stdio IPC 契約と LLM 不使用を検証。
- `tests/test_probe_ui_contract.py` (新設): React 側のタブ登録、engine wrapper、
  120字制限、禁止 API (`fetch` / `localStorage` / `Math.random` 等) 不在を静的検証。
- `docs/SPEC_PHASE_F6_PROBE_UI.md` (新設): F6 PROBE UI の実装仕様書。

**不変条件**:
- F6 から `src/python/core/probe_funnel.py` / `source_code.py` /
  `probe_engine.py` / `tests/test_source_code.py` / `tests/test_probe_funnel.py`
  へは触れない。検収時 diff は空。
- PROBE UI は `backend.generate` を呼ばない。感情推定・曖昧スコアリング・
  実行時刻依存の優先度計算を持ち込まない。
- 表示される第三者情報は D1/D2 側で alias 化された値のみ。PROBE v1 UI は
  answer history の `text_quote` や HistoricalNode の `fact_text` を表示しない。
- 回答入力は UI `maxLength={120}` と D2 `sanitize_probe_text` の二重防壁。

**検証結果 (DoD)**:
- `python -m py_compile src\python\core\facade.py src\python\engine_stdio.py`: OK。
- `python -m pytest tests/test_probe_ui_ipc.py -q`: **2 passed**。
- `python -m pytest tests/test_probe_ui_contract.py -q`: **3 passed**。
- `python -m pytest tests/test_source_code.py tests/test_probe_funnel.py -q`:
  **12 passed** (D1/D2 回帰)。
- `python -m pytest tests/test_ui_smoke.py -q`: **1 passed**。
- `cd apps\desktop; npx.cmd tsc --noEmit`: OK。
- `cd apps\desktop; npm.cmd run build`: Vite production build OK
  (`56 modules transformed`, built in 870ms)。
- `git diff --check`: OK。
- `git status --short data/`: 差分ゼロ。
- `package.json` / lockfile 差分なし。npm 依存追加なし。

**仕様との差異 / 残作業**: `tauri:dev` での目視スモーク
(Alt+5=PROBE、次の質問 → 回答 → 段階進行) は検収時点で未実施。静的型検査、
IPC/契約テスト、D1/D2 回帰、既存 UI smoke、production build は GREEN。

---

### Feature Custom Theme 完遂 (2026-07-10) — as-built (INTERVIEW/GD 持ち込みお題)

**方針**: `interview_sim` / `gd_sim` の START ターンに限り、任意入力の
`customTheme` を「持ち込みお題 / ケース課題」として使えるようにした。
空欄・空白のみ・非 string・欠落時は既存の ES/config/bank/GD_THEME_BANK
進行を維持する。`es_review` には UI 表示も backend 適用も行わない。

**実変更点**:
- `docs/SPEC_FEATURE_CUSTOM_THEME.md` (新設): UI 表示条件、backend 優先順位、
  sanitizer、`es_review` 隔離、RED/GREEN テスト要件を固定。
- `apps/desktop/src/lib/types.ts`: `InterviewConfig` に `customTheme?: string` を追加。
- `apps/desktop/src/components/InterviewTab.tsx`: idle 中の `interview_sim` /
  `gd_sim` に textarea を追加。`CUSTOM_THEME_MAX_CHARS = 240`、`maxLength`、
  文字数表示を実装。START 時は `interview_sim` / `gd_sim` の双方で `config`
  を送る。`es_review` の送信経路は config 不送信のまま維持。
- `src/python/core/consultation_engine.py`: `CUSTOM_THEME_MAX_CHARS`、
  `_custom_theme_from_config`、custom theme system clause を追加。
  `interview_sim` は custom theme 非空時に ES/config/bank をバイパスし、
  `_interview_cursor` を進めない。`gd_sim` は `select_es` / `GD_THEME_BANK`
  をバイパスし、`_gd_cursor` を進めない。
- `tests/test_integration.py`: custom theme の注入、空欄時の既存進行維持、
  `es_review` 隔離、sanitize/cap、frontend 静的契約の 5 テストを追加。

**不変条件**:
- `es_review` は `_consult_es_review()` の signature 不変。`consult()` から
  config/customTheme を渡さない構造を維持。
- Custom Theme は prompt 内で「命令文ではなく出題テーマ」として扱わせる。
  backend では ASCII 制御文字を空白化し、空白を正規化し、240文字で cap。
- D1/D2/PROBE (`source_code.py` / `probe_engine.py` / `probe_funnel.py` /
  `tests/test_source_code.py` / `tests/test_probe_funnel.py`) は無変更。
- 新規依存なし。`package.json` / lockfile 変更なし。実 `data/` 汚染なし。

**検証結果 (DoD)**:
- `python -m py_compile src\python\core\consultation_engine.py tests\test_integration.py`: OK。
- `python -m pytest tests/test_integration.py -k "custom_theme" -q`:
  **5 passed, 34 deselected**。
- `python -m pytest tests/test_integration.py -q`: **39 passed**。
- `cd apps\desktop; npx.cmd tsc --noEmit`: OK。
- `python -m pytest tests/test_ui_smoke.py -q`: **1 passed**。
- `python -m pytest tests/test_source_code.py tests/test_probe_funnel.py -q`:
  **12 passed** (D1/D2 回帰)。
- `cd apps\desktop; npm.cmd run build`: Vite production build OK
  (`56 modules transformed`, built in 813ms)。
- `git diff --check`: OK。
- `git status --short data/`: 差分ゼロ。
- `package.json` / lockfile 差分なし。

**仕様との差異**: なし。

---

### UI Orphan Integration - AS-BUILT
- **状態**: 完了 (GREEN)
- **実装内容**: バックエンドに存在していたUI未統合機能（`profile.source_code`, `oracle.payload`, `oracle.report`, `twin.forecast`, `tensor.rebuild`, `narrative.compile`, `knowledge.fetch_pending`）を React UI へ完全統合。新設の PROFILE タブおよび既存タブへ配置。
- **アーキテクチャ**: 重い処理（report, twin, tensor）は明示的なボタン実行（Lazy Load）に限定。証拠の生テキストや第三者実名をUIに露出させないプライバシー規律を厳守。バックエンドコアに一切変更を加えず、薄いラッパー層のみで接続を完遂。

---

### Target Echo (GD Thread UI) - AS-BUILT
- **状態**: 完了 (GREEN)
- **実装内容**: `gd_sim` モードにおいて、単一テキストだったLLMの応答を参加者別（スレッド形式/チャットバブル）にパースして表示する専用UIを実装。ストリーミング中もリアルタイムにスレッドレンダリングを適用。
- **アーキテクチャ**: バックエンド(`consultation_engine.py`)で `GD_FORMAT_V1` を強制し出力フォーマットを安定化。フロントエンド(`InterviewTab.tsx`)で正規表現を用いた専用パーサーとレンダラーを組み込み。既存の `interview_sim` や `es_review`、講評（debrief）フェーズへの影響は完全に隔離・保護。

---

### Project Calculus Phase 1 - AS-BUILT
- **状態**: 完了 (GREEN)
- **実装内容**: ストリーミング応答に対する `<think>` タグ（Hidden CoT）の O(n) 非表示化パーサー実装によるフロントエンド二重防衛線の構築。および、外部依存ゼロ（純粋なSVGと三角関数）による6次元テンソルプロファイリング用六角形レーダーチャートUIの基盤構築。
- **アーキテクチャ**: `InterviewTab.tsx` 内で `redactHiddenReasoning` を適用し、`<think>` 出力がストリーミングされた瞬間に失敗閉鎖でUIから完全除去。`ProfileTab.tsx` に `TensorRadarChart.tsx` を新設しプレビューデータを配置。バックエンドには一切影響を与えずにUI層を保護・拡張している。

### Project Calculus Phase 2 - AS-BUILT
- **状態**: 完了 (GREEN)
- **実装内容**: MBB評価基準を正規化した6次元テンソルプロファイリングを実装。各軸を観測可能な候補者発言のEvidenceと厳密に結び付け、スコアとconfidenceを決定論的に算出する。長時間セッション向けに、固定文字予算とTurn/Atom単位の採否による決定論的Semantic Compressionを導入。
- **アーキテクチャ**: `session_memory.py` が境界付きWorking Memoryと最新発言優先の証拠コンテキストを構築し、`tensor_profile.py` が6Dスキーマ、厳格validator、集約式を所有する。`interview_report.py` は構造化JSON生成、参照整合性検証、再試行、退化profileを提供する。`consultation_engine.py` にはnestedタグとchunk境界に対応した真のO(n) Hidden Reasoning除去ステートマシンを配線し、IPC前とUI側の二重防衛を完成させた。不正・未知Evidenceを拒否してハルシネーション由来の値を採用せず、既存`oracle.py`の無菌性と`interview_report.v1`の後方互換を維持。
- **検証結果**: Python関連全回帰114件、TypeScript型検査、Vite本番ビルド、`git diff --check`がすべてPASS。frontend、package files、`data/`、既存D1/D2/PROBEコアへの無関係な変更なし。

### Project Calculus Phase 3-A - AS-BUILT
- **状態**: 完了 (GREEN) — 歴史記録。Finding 9 (2026-07-12) で PROFILE 恒久モックを退役。
- **実装内容**: INTERVIEWの持ち込みお題textareaを拡大して縦方向のリサイズに対応し、NARRATIVE_DRAFTの説明を初学者向けに平易化。PROFILEへ未測定であることを明示した4軸MBTIグラデーションバーを追加し、6次元テンソル評価の英語軸名と日本語ヘルプツールチップを実装。
- **アーキテクチャ**: Phase 3-Aはフロントエンド表示層のみに限定し、新規IPC、永続化、推定処理を追加していない。MBTIは固定モックとして測定値・推定値から隔離。6D tooltipは外部ライブラリを使わず、ReactとCSSのみでhoverおよびkeyboard focusに対応した。新規hex色、リテラルpx、letter-spacing、外部npm依存を追加せず、既存CSS変数と`thin solid`によるスタイリング規律を維持。
- **検証結果**: 強化UI契約、Phase 3-A契約、既存UI回帰、TypeScript型検査、Vite本番ビルド、`git diff --check`がすべてPASS。backend、package files、`data/`への変更なし。
- **Finding 9 追補 (2026-07-12)**: PROFILE の `MbtiGradientBars` と固定 `TENSOR_RADAR_PREVIEW` を撤去。`TensorRadarChart` の `preview` prop / preview CSS / `.mbti-preview-*` を削除。実測 6D は Interview/GD の `MISSION_RESULT`→`TensorProfilePanel` のみ。MBTI は測定契約ができるまで非表示・推定禁止。PROFILE 用 latest 契約・新規 IPC は追加しない。再導入禁止。

### Project Calculus Phase 3-B - AS-BUILT
- **状態**: 完了 (GREEN)
- **実装内容**: CONSULTへ`romance_analysis`モードを追加し、会話履歴から観測可能な発話数、ターン切り替え、往復バランス、返信遷移率を集計する交流パルス解析を実装。検証済み`romance_analysis.v1`構造体をstdio経由でReactへ渡し、PROFILEと共通するサイバーUI規律のメーター、傾向、次の一手として表示する。
- **アーキテクチャ**: APIフィールド`affinity_score`は恋愛感情や脈あり度の推定ではなく、決定論的な交流往復指数として定義。生LINE本文・実名をLLM、ログ、派生UIへ渡さず、`contact_alias`形式と集計済み物理量だけを扱う。空本文を観測件数から除外し、未観測の文字数・時刻差を傾向へ使用しない。通常時とデータ不足時の文言集合を分離し、JSON Schema、strict validator、決定論fallbackの往復契約を保証。再解析開始・通信失敗・構造体欠落時には旧UI結果を確実に破棄する。
- **検証結果**: Phase 3-B backend/UI契約、Python関連全回帰150件、TypeScript型検査、Vite本番ビルド、`git diff --check`がすべてPASS。Phase 1〜3-Aコア、package files、`data/`への無関係な変更なし。

---

## 14. インシデント 2026-07-07: metadata.json 4.2GB 肥大 (IMP-1 是正指令)

### 検死結果 (読み取り専用フォレンジックで確定した事実)

- 台帳エントリは **208 件・chunk_id 重複ゼロ・単一世代** — LSM (Charlie) の
  追記/墓標/コンパクション機構は**無罪**。
- 肥大は約 195 件の「鯨チャンク」(各 ~21.6MB) の text/conversation_sessions
  フィールド内部にあり、中身は**同一メッセージ列の多重反復** (本文・実名を
  含むためログ・ドキュメントへの引用禁止。検死サンプルは確認後に削除済み)。
- 真犯人は **import 層の冪等性欠如**: `facade._append_line_text` は無条件
  append であり、`data/raw/line_history.txt` に **同一エクスポートが 12 回**
  取り込まれていた ([LINE] ヘッダ 12 個 / 135,225 行中ユニーク 53,058 行 /
  最頻行の重複度 48 = 12 インポート × 同一分内の実反復 ~4)。
- 増幅機構: 重複メッセージが `extract_conversation_sessions` のギャップ検出を
  破壊しセッションが橋渡しされて巨大化・多数日にまたがり、`data_merger` は
  「その日を含む全セッションの全文」を**各日に**添付するため乗算複製、さらに
  チャンクが text と conversation_sessions の両方に同内容を持つため倍加。
  7.3MB (raw) → 4.2GB (台帳) の ~600 倍増幅はこの合成である。

### IMP-1 是正指令 (実装は Sonnet5。ui_smoke が赤い間、E4 は未完了扱い)

1. **隔離 (証拠保全 + 解除)**: `data/processed/_quarantine_20260707/` を作り
   metadata.json / segments.json / vectors.bin / vectors.seg-*.bin を **move**
   (rename。同一ボリュームで即時)。tensor_*.bin・deep_profile・salt・
   telemetry・bounty には触れない。実行前にエンジンプロセス不在を確認
   (mmap 保持者ゼロは検死時に確認済み)。**raw (line_history.txt) は改変しない**
   — 記録は聖域。修復は分析・ロード層で行う。
2. **根治 = ロード層の多重集合デデュープ**: `profiler.load_line_messages()` に
   エクスポートブロック単位 ([LINE] ヘッダ区切り) の**多重集合和**を実装する。
   キー (contact, date, time, sender, text) の出現数を各ブロック内で数え、
   ブロック間では **max を採る (sum ではない)**。同一分内の本物の連投
   (同文を 2 回送る) は 1 ブロック内の多重度 2 として保存され、12 回の再取込は
   max=1 に潰れる — 「実データの反復」と「取込の重複」を区別できる唯一の
   決定論的意味論。対照群テスト必須: (a) 同一エクスポート 2 回取込 → 件数不変、
   (b) 部分重複エクスポート (旧 ⊂ 新) → 和集合、(c) **1 ブロック内の本物の
   連投は失われない**。
3. **トリップワイヤ**: `_save_meta` に台帳シリアライズサイズの上限
   (200MB) を置き、超過時は黙って書かず診断メッセージ (import 重複を疑え) 付きで
   即エラー。静かな破損を騒がしい失敗に変換する。
4. **再構築と検収**: 修復後にフレッシュ再構築 (隔離により legacy 不在 →
   pipeline がゼロから構築)。ゲート: 全 13 スイート + **ui_smoke GREEN**
   (これが E4 ゲートの残項目)。完走後、隔離ディレクトリは指揮官の承認を得て削除。

**教訓 (T-20 として凍結)**: 追記式取込 (append) は冪等ではない。取込 API を
書くときは「同じものを 2 回入れたら何が起きるか」を最初に問え。増幅は
単層では起きない — 「重複 (import) × 橋渡し (session) × 日数複製 (merger) ×
二重保持 (chunk)」のような**無害に見える設計の積が爆発する**。

### IMP-2 是正指令 (2026-07-07 同日再発。IMP-1 完了後、実データ検証で発覚)

**再発の経緯**: IMP-1 (T-20 デデュープ) 適用後、隔離済みディレクトリからの
フレッシュ再構築で `ui_smoke.py` を実データに対して実行したところ、
`_save_meta` の 200MB トリップワイヤが **8,515.5MB** で発火 (IMP-1 前の
4.2GB より悪化)。デデュープ自体は正常動作していたが、デデュープの
**下流**で新たな増幅源が発覚した。

**検死確定事実**:
- **T-21 (is_self 全滅)**: `"is_self": sender == "自分"` のハードコード判定が
  実 LINE エクスポート (本人も実名で記録される) で 129,635 件中 **9 件**しか
  一致せず、`extract_conversation_sessions` の「返信待ちセッション
  (`awaiting_user`) は最大 24h 超でもクローズしない」ルールが恒久的に
  解除されず、**253 日・121,818 ターンの巨大セッション**が形成された。
- **T-22 (日次フル添付の増幅)**: `data_merger.py` がセッション全文を
  「セッションが触れる全日」に複製添付する設計だったため、鯨セッション
  1 個 (32 万文字) × 253 日 = **8.2 億文字**の添付総量になった。
- **T-23 (ヘッダ無し追記によるブロック融合)**: `[LINE]` ヘッダを伴わない
  追記がブロック境界を消し、T-20 の「ブロック間 max」が「ブロック内 sum」
  に退化 (多重度 6/12/18/24/30/36 の系列として検出)。
- **T-25 (グループチャットの dyad 前提破綻。T-21〜24 是正後に新規発覚)**:
  is_self を正しく直した後も同一コンタクトで再度トリップワイヤが発火。
  検死の結果、そのコンタクトは **sender が 11 人のグループチャット**で
  あり、本人はほぼ発言していなかった (46,318 件中 3 件のみ)。
  `extract_conversation_sessions` の状態機械は「本人の返信を待つ 1 対 1
  dyad」を前提としており、本人が寡黙な大人数グループに適用すると
  「返信待ち」のまま無限に蓄積し続ける。T-24 トリップワイヤが実際に
  この実例を正しく検知・阻止した。

**是正内容 (実装は Sonnet5、全て `tests/test_line_dedup.py` に回帰ガード
11 ケースあり)**:
1. **T-21**: `profiler._resolve_self_by_contact()` — is_self をコンタクト
   単位の 3 段階決定論で解決 (① `user_profile.fixed_attributes
   .line_self_name` 明示設定 → ② `"自分"/"self"` リテラル → ③ フォール
   バック: **真の 1 対 1 (sender がちょうど 2 種) コンタクト全体**に共通する
   sender の積集合)。感情推定・ハードコード禁止、集合演算のみ。
2. **T-22**: `_session_from_buffer()` に `turns_by_date`/`responses_by_date`
   を追加し、`_render_text`/`load_daily_contexts` は当日分のターンのみを
   添付する (全ターンは必ずどこか 1 日にのみ属し情報ロスなし。セッション
   自体の `text`/`turns`/`stimulus`/`response` は dyad 分析用の完全版として
   従来通り保持)。
3. **T-23**: `load_line_messages` が日付の後退 (`cur_date` が過去に戻る)
   もブロック境界とみなす。さらに `facade.format_line_import()` を新設し、
   全 3 つの append 経路 (`_append_line_text` / `import_line_history` /
   `ui_tui/app.py::_import_line_file`) がヘッダ無しテキストに自前ヘッダを
   付与してから追記するよう統一 (DRY 化も兼ねる)。
4. **T-24**: `extract_conversation_sessions` に span>30日 or turns>5000 の
   トリップワイヤ。200MB ワイヤより上流で、より具体的な診断とともに
   騒がしく死ぬ。
5. **T-25 Rev.1 (棄却)**: 初版は「sender が 3 人以上のコンタクトを session
   抽出から完全除外」だったが、これは「情報ロスゼロ」原則違反としてアーキ
   テクト自身が自己監査で訂正した (Architect's Note 参照)。グループの会話は
   「本人の対話」ではないが「本人の認知への入力」であり、丸ごと破棄すると
   デジタルツインの解像度 (周囲環境の認識) を損なう。
6. **T-25 Rev.2 (確定)**: グループチャットを「状態レスの受動観測ログ」
   として扱う。`data_merger.extract_group_daily_logs()` — 状態機械
   (`awaiting_user`) を一切通さず `(contact, date)` で単純に群化・時刻順
   整列するだけの決定論的処理。増幅率は恒等的に 1 (各メッセージが自分の
   date キーにちょうど1回だけ属する) であり、鯨が構造的に発生し得ない。
   DailyContext に新フィールド `group_line_text` / `has_line_group` /
   sources の `"line_group"` を追加、`_render_text` に
   `## LINE_GroupActivity (受動観測)` セクションを新設。**隔離ガード
   (最重要)**: `line_self_text`/`self_text`/`has_line` (dyad 意味論) には
   一切合流させない — simulated persona (§7.1.4) と同型の非対称
   (記録としては本物、自己分析チャネルからは除外)。
   `profiler._resolve_self_by_contact` の tier3 積集合計算は
   `len(senders) == 2` (グループを含まない真の dyad のみ) に限定し、
   グループの sender 集合が積集合を空へ潰す汚染を防止 (Rev.1 から継続)。

**効果 (実データ実測)**: metadata.json **8,515.5MB → 80,041 バイト
(T-21〜24) → 5,953,139 バイト (T-25 Rev.2 適用、グループ観測ログ復元後)**。
約 6MB は raw テキスト量に対する線形成長であり、200MB トリップワイヤに
対して安全マージンを持つ。セッション最大 span **253 日 → 2 日**。日次
添付総量 **8.2 億文字 → 1,184 文字** (dyad 分)。`ui_smoke.py` 実データ実行で
ALL PASS 確認済み — **E4 正式クローズ**。回帰ガードは `tests/test_line_dedup.py`
に 15 ケース (T-20〜T-25 Rev.2 の増幅ゼロ証明・鯨阻止・隔離ガード・dyad
不変を含む)。

**教訓 (T-21〜T-25 として凍結)**: デデュープ (T-20) は「取込の重複」を
消したが、「取込の下流にあるドメインロジック側の前提」(is_self・dyad・
日次添付) が実データの多様性 (実名記録・グループチャット) を想定して
いなければ、別の増幅源が新たに顕在化する。**1 つの層を直しても、
「疑ったら計測しろ」を隣接する全層に対して再度実行せよ** — 本インシデント
は IMP-1 → IMP-2 → (T-21〜24 是正後の再計測で) T-25 Rev.1 → (情報ロスゼロ
原則との衝突で自己監査) T-25 Rev.2、と多段階の実測と自己訂正を経て
初めて根治した。**「増幅を止める」だけでなく「受け皿を設計する」こと**
— ガードは破棄ではなく隔離であるべき、という訂正も含めて記録する。

## 15. The Final 5 Legacies — 青写真のみ (`docs/MASTER_PLAN_LEGACIES.md`)

将来ターゲットの青写真 (詳細 SPEC は各着手時に錬成): L1 LARYNX (発話物理
テレメトリ — 音調感情推定は永久禁止) / L2 SCAVENGER (デジタル排気 importer —
coverage() 必須の型強制) / L3 BLACKBOX (実戦結果台帳 — 実選考の合否による
システム校正。面接官プロファイル構築禁止) / L4 CHRONOSCOPE (介入効果の
会計監査 — n=1 の効果推定を「証明」と呼ぶな) / L5 PHANTOM (合成ペルソナ
known-answer 校正 — fixture-blindness 規律)。**着手順序は PHANTOM が最初**
(校正装置なしの計測器増築は倒錯)。着手時は個別 SPEC → 憲法ガード RED → 実装。

---

## 16. SKILL-PKB-BOUNDARY-V3: Strict Runtime & Persistence Integrity (Phase 4-A As-Built)

**次世代エージェントへの命令書。** Phase 4-A (Context Observatory / RetrievalManifestV1) の
厳格監査ループで確立した「境界不変則」である。観測・シリアライズ・IPC・フロントランタイムの
どの境界でも、**修復するな・握り潰すな・キャストで済ませるな**。違反はレビューで即座に落とせ。
実装リファレンス: `src/python/core/retrieval_manifest.py`、`apps/desktop/src/lib/parseManifest.ts`、
`apps/desktop/src/lib/manifestFetchState.ts`。事故の一次資料は
`docs/architecture/INCIDENT_LEDGER.md` (INC-PHASE4A-01〜05)。

### 16.1 Hash-Before-Construction Principle (INC-PHASE4A-02)

- 構造体の ID / ハッシュを**ダミー値で一度インスタンス化してから再計算・上書きする two-phase
  construction を永久禁止**する。平文状態から canonical hash を計算し、確定インスタンスを
  **一度だけ**生成せよ。
- `__post_init__` でも ID を独立再計算し、`self.manifest_id == expected_id` を強制する
  (生成経路が hash-before-construction を守った証明を、モデル自身に持たせる)。
- `x or ""` / `x or default` による欠損値の握り潰しを禁止。`None` は `None` として明示検証・拒否。
- 型検証は `type(x) is T` と `bool` 明示除外で行え。`isinstance` の緩さ (bool⊂int、サブクラス
  通過) に依存するな。float / bool / list を strict int として通した時点で不合格。

### 16.2 Zero-Trust Deserialization & Hard Failure (INC-PHASE4A-03)

- 永続化層 (JSON 読込等) での **Silent Sanitization を全面禁止**する。不正値を黙って安全値や
  `None` へ変換して受理する処理を書くな (改ざん alias を `None` 化して「正常な無名 Manifest」
  として蘇生させる、が典型犯)。
- deserializer は**修復役を兼ねてはならない**。生入力をそのまま dataclass へ渡し、境界
  (`__post_init__`) で検知したら即座に `ValueError` (Hard Fail) を強制せよ。
- validator を恒真 (pass-through) にするな。serializer / writer / reader の**全境界**で
  同一 validator を通し、書込時の健全性を根拠に読込時の検証を省略するな。
- corrupt / 改ざん Manifest を `NO_MANIFEST` や無名 Manifest へ**退化させるな**。不存在
  (`latest.json` が無い) のみが `NO_MANIFEST`。破損は hard failure。

### 16.3 Pointer-Payload Cryptographic Binding (INC-PHASE4A-04)

- 永続化の読込・保存時、**ポインタが主張する ID とペイロード内部の ID の完全一致 (`==`) を
  強制**せよ。各ファイルの自己整合性だけでは「自己整合した別の有効 payload」によるすり替えを
  防げない — 参照結合 (referential binding) を明示検証する。
- 不一致時は**修復・上書き・自動整合を一切せず**、ファイルと pointer を不変のまま即座に
  `ValueError` (`latest pointer manifest_id mismatch`)。既存 immutable ファイルの ID と保存
  対象 ID も同様に突き合わせ、同一 ID の内容差し替えを拒否せよ。
- 検証失敗時は副作用ゼロ (ファイル・pointer 不変、`.tmp` 残骸なし) を保証せよ。

### 16.4 Runtime Boundary Validation for IPC (STEP 5 As-Built)

- Tauri IPC 経由の外部 JSON に対する **TypeScript の型アサーション (`as` キャスト /
  `invoke<T>()` の generic) を runtime 検証の代用にするな**。renderer から呼べるのは
  `engine.ts` が所有する有限個の明示 command だけとし、`invoke<unknown>(explicitCommand, ...)`
  → command 固有の `parse...(raw)` の順で通せ。任意の backend command 名を受ける
  `pkbInvoke` wrapper、generic cast、parser を通らない返却は禁止する。
- パーサーは backend validator の**鏡像**とせよ: exact key 集合、Enum allowlist、
  `Number.isSafeInteger` + 非負、strict 文字列、hex 形式、固定 schema / lane 順 / budget、
  会計 (要素和・レーン別再計算)。extra key / `undefined` / NaN / Infinity / bool / float を
  拒否し、clamp・null 化・削除・既定値化・catch-and-default をするな。
- パーサーの例外 message に**違反値そのものを埋め込むな** (個人情報漏洩経路の遮断)。path と
  期待形のみを載せよ。frontend で BLAKE2b を再実装するな (暗号学的 ID 結合は検収済み Python
  境界が所有)。ただし hash 形式は検証する。
- 検証失敗・IPC 失敗時は**安全な error ステートへハード遷移**させよ。状態は純 reducer が
  `loading/ready/empty/error` を所有し、request 開始時に旧成功表示を消去、stale response は
  seq 不一致で破棄、error イベントに payload / 例外文言を運ばせるな。取得は明示ボタンのみ
  (polling・自動再試行・時刻依存を足すな)。
- **seq / stale guard は応答整合性であり、多重発行防止ではない** (Finding 10)。明示取得 UI は
  即時更新される `useRef` 再入拒否と `phase === "loading"` の button `disabled` を併用せよ。
  ref は IPC / dispatch より前に立て、`finally` で必ず解除する。第2操作は queue / retry /
  debounce / throttle せず即座に無視する。polling・`useEffect` 自動取得の禁止は維持。

### 16.4.1 UI 例外表示の無菌化 (INC-UI-ERROR-01 / Finding 13)

- React catch は例外値を表示するな。`String(err)` / `err.message` / `err.stack` /
  template 展開 / `console.log(err)` 禁止。state・log・DOM・aria へ渡すな。
- 操作種別は catch 値ではなく、呼び出し前から確定した有限キー (`UiErrorCode`) で選べ。
  `apps/desktop/src/lib/uiErrorMessages.ts` の固定文言のみを表示せよ。error 引数を取る helper を作るな。
- 例外内容の解析・分類（substring / regex / class名 / Rust固定文言比較 / code抽出）禁止。
  開発モードだけの生表示分岐も禁止。
- Finding 2 `replay_policy` と回復案内を一致させよ。retry-safe（読取再試行可）だけ
  「もう一度お試しください」。NoReplay 相当は verify-first（状態確認後、必要な場合だけ再実行）。
  error 文字列を見て retry-safe へ昇格するな。自動再試行・polling・hidden resend 禁止。
- progress 表示は `role="status"`、error は `role="alert"` + `error-text`。kind を
  メッセージ内容から推測するな（操作開始/status event/成功 → info、catch 固定文言 → error）。
- error state へ payload / path / query / filename を運ぶな。backend raw error を消すために
  IPC schema や `engine_stdio` を勝手に変えるな（Finding 12 との責務分離）。

### 16.4.2 WebView / Tauri IPC Isolation (INC-WEBVIEW-IPC-01 / FSA-2026-07-13-03)

- production CSP は `default-src 'self'` を基礎とし、外部 `connect-src`、remote image、
  media、object、frame、child、worker、form、base URL を許可しない。development の例外は
  `localhost:1420` の HTTP/WebSocket に限定し、production CSP へ混ぜるな。
- main WebView は Rust が構築し、exact origin allowlist を `on_navigation` で検証する。
  credential、非既定port、prefix/suffix一致を拒否し、new-window と download は常に deny する。
- capability は必要な event/window 操作だけを列挙する。`core:default`、window/webview 作成権限、
  remote capability を付与するな。設定ファイルだけに依存せず、Rust runtime policy と契約テストを
  二重化する。
- Rust command は機能単位の明示関数とし、request は `#[serde(deny_unknown_fields)]` の閉じた型、
  literal enum、有限値、長さ・件数・one-of・相互依存を command dispatch 前に検証する。
  renderer から `cmd: String` や任意 `serde_json::Value` を受け取る汎用commandは禁止する。
- frontend の応答と event は必ず `unknown` で受け、exact-key runtime parser を通してから state へ
  入れる。command応答のparse失敗は呼出側の固定UIエラーへ遷移する。非同期eventのparse失敗は
  stateを一切変更せず破棄する。どちらも違反値を例外文言へ含めない。
- 本節は renderer/command/schema のblast radiusを所有する。Rust stdio transport の最大byte、
  deadline、JSON depth、response `id`/`cid`照合は FSA-2026-07-13-12 の責務であり、未解決のまま
  本節のGREENへ混同してはならない。

### 16.5 Persistence Failure Containment (INC-PHASE4A-05)

観測ストアの障害で面接・GD・debrief を落とすな。ただし hard-fail 契約を緩めたり
corrupt store を修復したりするな。

| 事象 | 動作 |
|---|---|
| 生成Manifestのvalidation失敗 | 即時hard-fail |
| typed persistence failure (`RetrievalManifestPersistenceError`) | 固定警告＋検証済みcontextで継続 |
| latest読込時の破損 (`context.manifest.latest` 等) | 即時hard-fail |
| 未知例外 | 伝播 |
| corrupt store | 修復・削除・上書き・`NO_MANIFEST`化禁止 |

- `save_retrieval_manifest()` は入力 `validate_manifest` を storage `try` の**外**で行い、
  書込前に既存 latest pointer+payload を厳格検証する。storage 中の `OSError`/`ValueError`
  のみを固定文言 `"retrieval manifest persistence failed"` の typed error へ変換
  (`raise ... from exc`)。message に path / JSON / 元例外文字列を連結するな。
- `_bounded_context(..., status=None)` は validate → working_memory 更新 → save の順。
  typed persistence error だけを捕捉し、固定 stderr と status 警告を各1回。
- 通常 `mode="consult"` は `_bounded_context()` を使わない。影響範囲は interview_sim /
  gd_sim / 講評 / debrief。

### 16.5.1 Bounded Retrieval Manifest Retention (INC-PHASE4A-06)

immutable は保持中の非改変性であり、無期限保存ではない。`save_retrieval_manifest()` は
latest commit **後**にだけ `_prune_retrieval_manifests` を実行する。

| 事象 | 動作 |
|---|---|
| owned valid が上限以内 | 削除なし |
| owned valid が上限超過 | latest 保護 + `(mtime_ns, filename)` 降順で残りを保持し、超過分のみ unlink |
| corrupt / ID mismatch / symlink (owned) | preflight hard-fail、削除ゼロ、修復禁止 |
| non-hex / `.tmp` / 未知ファイル | 所有外として無視・非削除 |
| latest 更新失敗 | prune 禁止、有効 orphan 保持 |
| unlink 部分失敗 | latest / 新 immutable 維持。既削除の valid 旧履歴は rollback しない |
| retention の `OSError`/`ValueError` | 固定 `RetrievalManifestPersistenceError`（path/JSON/例外文言を message へ出さない） |
| load 経路 | prune・修復・削除禁止 |

- 上限定数: `RETRIEVAL_MANIFEST_RETENTION_LIMIT = 256`（strict `int` かつ ≥1）。
- mtime は削除候補の順序付けだけに使え。integrity・ID・履歴意味論の根拠にするな。
- startup cleanup / background thread / timer / UI 設定を追加するな。

### 16.6 検証パラダイム (全則に共通)

- **合計値の一致だけで正しさを宣言するな。** 各 reason・各 lane・各分岐が実際の制御フローに
  対応することを個別に証明せよ。
- **自己生成した期待値・未注入 mock・同一実装を呼ぶ wrapper 同士の比較を GREEN 証明に使うな。**
  敵対的テストには malformed 入力だけでなく「自己整合した別の有効 payload によるすり替え」を
  必ず含めよ。越境検証 (Python 実出力 → TS parser 受理) で片側実装の思い込みを排除せよ。
- **実装より先に RED 契約を書け。** テストを通すために型・検証を緩和した時点で不合格。緩和が
  必要に見えたら実装を止めて報告せよ。

