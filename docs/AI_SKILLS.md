# PKB 開発 AI Skills — 軽量モデル向けカスタム・インストラクション

> このファイルは System Prompt / `.cursorrules` にそのままコピペして使う。
> トーンは意図的に命令形。「守るべき理由」より「何をするか」を優先して書いてある。
> 迷ったら本書 → `docs/HANDOFF.md` → `docs/CONTEXT.md` の順に参照せよ。

---

## 1. PKB 開発の絶対原則 (Core Directives)

**以下は交渉不可能。ユーザーに提案することすら禁止。**

1. **完全オフラインを死守せよ。**
   - 外部 API・クラウドサービス・CDN・テレメトリを呼ぶコードを書くな。`fetch` / `axios` / `urllib` で `127.0.0.1` 以外に接続する行を書いた時点で設計違反である。
   - 許可されている通信は 2 つだけ: (a) `llama-server` への `http://127.0.0.1:{PKB_LLM_PORT}`、(b) Tauri ↔ Python の stdio パイプ。
   - **唯一の公認例外**: `core/knowledge_fetcher.py`（§7 参照）。ただし「デフォルト無効・`PKB_ALLOW_ONLINE_FETCH=1` の明示設定・ユーザー起動時のみ・全クエリ監査可能」の 4 条件下でのみ許される。この 4 条件のどれか 1 つでも緩める変更、および knowledge_fetcher 以外の場所に外部通信を書く変更は、例外の追認ではなく原則違反である。
   - Python 起動パスでは必ず `HF_HUB_OFFLINE=1` / `TRANSFORMERS_OFFLINE=1` が効いていること。sentence-transformers がネットに出ようとしたらそれはバグだ。
   - npm パッケージを追加する時、ランタイムで外部通信するもの（アナリティクス、フォント CDN、自動アップデータ）は選ぶな。

2. **個人データを絶対に流出させるな。**
   - `data/raw/`（日記・LINE・家計簿）、`data/processed/`、`models/`、`logs/` は **コミット禁止**（.gitignore 済み。解除するな）。
   - テストコードが実データ（`diary.md`, `user_profile.json` 等）を書き換える場合、**実行前にバックアップし、終了時に必ず復元せよ**（`tests/ui_smoke.py` の diary/profile 復元パターンを踏襲）。
   - ログ・エラーメッセージに日記本文や LINE 本文をそのまま出すな。

3. **責務分離を守れ。**
   - **C++ (`src/cpp/search_engine.cpp`)**: パフォーマンス限界突破専用。384 次元 AoSoA 内積・Top-K のみ。ビジネスロジック・ファイル形式の解釈・日本語処理を C++ に入れるな。バイナリ形式 `PKBVEC01` は Python `pipeline.py` と 1 バイト単位で同期していること（片方だけ変えたら即データ破損）。
   - **Python (`src/python/core/`)**: オーケストレーション・データ管理・プロンプト構築。UI コード（Textual / React への依存）を `core/` に 1 行でも入れるな。
   - **UI (React / Textual)**: 表示と入力だけ。ビジネスロジックは書くな。新機能は必ず `core/facade.py` に API を足す → `engine_stdio.py` の dispatch に載せる → `apps/desktop/src/lib/engine.ts` にラッパーを足す、の順で通す。

4. **記録と分析を分離せよ。**
   - RECORD 保存・カレンダー同期で profiler を走らせるな（`sync_diary_index()` のみ）。
   - profiler の自動実行は LINE 取込 (`import.line`) のみ。他で走らせたければユーザーに聞く前に HANDOFF を読み直せ。
   - 埋め込みモデル・llama-server は初回 consult / profiler まで起動しない（遅延初期化）。起動を早めるな。

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
| llama-server が起動しない/異常に遅い | x64 エミュレーション実行 | `tools/llama-arm64/` の ARM64 native ビルドを使う |
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

# 5. デスクトップのエンジンログ（stderr の退避先）
Get-Content data\logs\engine.log -Tail 50   # 開発時はリポジトリ data/、本番は %LOCALAPPDATA%\PKB\logs

# 6. 回帰テスト（変更後の最低ライン）
python tests\test_calendar_sync.py
python tests\ui_smoke.py
```

### 2.4 その他の既知の罠

- `pkb-desktop.exe` 直接起動は黒画面になる。**必ず `apps/desktop/dev.cmd`** で起動せよ。
- JS の `new Date().toISOString()` は **UTC**。JST では 0:00〜8:59 に日付が 1 日ずれる。日付文字列は必ず `dateUtils.todayIso()` / `toIsoDate()`（ローカル時刻ベース）を使え。`toISOString().slice(0,10)` を書いたら即修正対象。
- Textual のカレンダー日ボタンに `id` を付けるな（DuplicateIds でクラッシュ）。
- 7B モデルの初回ロードは 2〜3 分かかる。タイムアウトを短くするな（`llm_config.model_startup_timeout()` を使え）。
- **スキーマ移行時は grep で全参照を潰せ。** 過去に `fixed_attributes` の `age` → `birthday` 移行で TUI とテストに古い `#fixed-age` 参照が残り、SETTINGS タブが実行時クラッシュした。フィールド名・cmd 名・イベント名を変えたら `rg <旧名>` をリポジトリ全体に必ず実行し、ヒット 0 を確認してから完了と言え。
- llama-server のライフサイクル: 自分が spawn したプロセスだけを stop 対象にする（既存サーバー再利用時は殺さない）。`atexit` 登録済み。ゾンビを残す変更をするな。

---

## 3. コード生成のトーン＆マナー (Code Generation Guidelines)

### 3.1 全言語共通

- **最小 diff。** 依頼されていないリファクタ・リネーム・整形をするな。
- コメントは非自明なビジネスロジックのみ。「何をしているか」を説明するコメントは書くな。書くのは「なぜそうでなければならないか」だけ。
- コミットはユーザーが明示的に指示した時のみ。

### 3.2 Python — 提出前チェックリスト（全項目 YES になるまで出すな）

1. **ループの計算量を言え。** ネストループを書いたら、N が何で最悪何回回るかをコメントか PR 説明で言語化せよ。日記が 10 年分（~3,650 日 × 複数ソース）でも耐えるか？ O(N²) は原則書き直し。
2. **ファイル I/O をループに入れるな。** `load_calendar()` / `load_finance()` のような JSON 全読みを for の中で呼んだら即修正。ループ外で 1 回読み、dict で引け。
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
5. 状態は最小に。サーバー状態（エンジンからのデータ）を複数コンポーネントに複製するな。取得はタブのマウント時、更新は保存成功時の再取得で足りる。
6. `useEffect` のイベントリスナー（Tauri `listen` 含む）は必ずクリーンアップを返せ。
7. 長時間処理（consult, profiler, import）は必ず「進捗の見える化」をセットで実装しろ。busy フラグでボタンを殺すだけの UI は不合格。status イベント（2.1 参照）を表示せよ。

### 3.5 完了の定義 (Definition of Done)

以下を全部通してから「完了」と報告せよ。エラーが出たら自分で直せ。

```powershell
python -m py_compile <変更した .py 全部>
python tests\test_calendar_sync.py     # ALL PASS
python tests\test_apple_calendar_sync.py  # ALL PASS
python tests\test_gap_analysis.py      # ALL PASS
python tests\test_integration.py       # ALL PASS
python tests\ui_smoke.py               # ALL PASS
npx tsc --noEmit                       # apps/desktop で (フロント変更時)
cargo check                            # apps/desktop/src-tauri で (Rust 変更時)
```

報告には「何を変えたか」「なぜか」「何で検証したか」を必ず含めろ。テストが通らないまま完了と言うことは、いかなる理由があっても禁止する。

---

## 4. macOS 配布・公証 (Code Signing & Notarization) Skills

**次世代 AI エージェントへの命令書。** macOS 配布は「ビルドが通る」と「ユーザーがダブルクリックで起動できる」の間に Gatekeeper という壁がある。以下を一言一句守れ。

### 4.0 このリポジトリの macOS ビルドの前提（まず暗記しろ）

- Sidecar は **`pkb-engine`（stdio JSON エンジン、エントリ `run_engine.py`）** である。FastAPI / HTTP サーバーは存在しないし、設計原則（完全オフライン）により**今後も導入禁止**。「pkb-api」という名前が指示に出てきたらそれは古い/誤った情報であり、`pkb-engine` に読み替えろ。
- `externalBin: ["binaries/pkb-engine"]` は**二重化不要**。Tauri が target triple を自動付与して解決する: Windows は `pkb-engine-aarch64-pc-windows-msvc.exe`、macOS は `pkb-engine-aarch64-apple-darwin` / `pkb-engine-x86_64-apple-darwin`。**やることは正しいファイル名でバイナリを置くことだけ**（Windows: `scripts/build-engine.ps1`、macOS: `scripts/build-sidecar.sh`）。ファイルが無いと `tauri build` はその場で失敗する。
- プラットフォーム差分は `tauri.conf.json` 本体ではなく **`tauri.macos.conf.json`**（自動マージされる platform-specific config）に書く。Windows の挙動を変えずに macOS を足すのが原則。
- データルート: Windows `%LOCALAPPDATA%\PKB` / macOS `~/Library/Application Support/PKB`（`src-tauri/src/paths.rs::user_data_root()` の cfg 分岐）。パスを変えるならここ**だけ**を変えろ。
- ネイティブ実行ファイル名は Python 側で `core/paths.py` の `SEARCH_EXE` / `LLAMA_SERVER_EXE` に集約済み（Windows のみ `.exe`）。**`"xxx.exe"` という文字列リテラルを新たに書いた時点で不合格。**

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
  - PyInstaller 製 sidecar は自己解凍で dylib を展開するため、entitlements の `com.apple.security.cs.allow-unsigned-executable-memory` と `com.apple.security.cs.disable-library-validation`（`src-tauri/entitlements.plist`）を消すな。消すと**署名済みビルドだけがクラッシュ**し、未署名の dev ビルドでは再現しない地獄のバグになる。
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

**A. プライバシー権限（カレンダー DB）— 「権限リクエストの入れ忘れ」の正体を理解しろ:**

1. PKB は EventKit API ではなく **Calendar.app の SQLite を直接読む**（`core/apple_calendar_sync.py`）。この方式に entitlement や Info.plist の usage description は**存在しない** — 必要なのはユーザーが手動で与える **フルディスクアクセス**（システム設定 > プライバシーとセキュリティ）である。プログラムから要求する API は無い。
2. したがってお前の仕事は「権限を要求するコード」を書くことではなく、**拒否された時のエラーメッセージにフルディスクアクセスへの誘導を必ず含める**こと（実装済み: `apple_calendar_sync.FULL_DISK_ACCESS_HINT`。この文言を消したら不合格）。
3. TCC に拒否されると DB ファイルは「存在しない」ように見える（`is_file()` が False）。「ファイルが無い」と「権限が無い」をユーザー向けに区別できない前提でメッセージを書け。
4. もし将来 EventKit へ移行するなら、その時初めて `NSCalendarsFullAccessUsageDescription`（Info.plist）+ `com.apple.security.personal-information.calendars` が必要になる。**SQLite 直読みのまま entitlement だけ足すな**（無意味な権限は審査・信頼の両面で有害）。
5. App Sandbox は**有効化するな**。ローカルファイル直読み（データルート・モデル・カレンダー DB）の設計と根本的に非互換である。

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

- [ ] `.exe` ハードコード → `core/paths.py` の `SEARCH_EXE` / `LLAMA_SERVER_EXE` を使ったか
- [ ] `beforeBuildCommand` が PowerShell のまま → macOS 差分は `tauri.macos.conf.json` で bash スクリプトに上書き済み。Windows 側 conf を書き換えるな
- [ ] シェルスクリプトの改行が CRLF になっていないか（Git for Windows で編集した .sh は要注意。`bash: /bin/bash^M` エラーの原因。`.gitattributes` か `git config core.autocrlf` で LF を保証しろ）
- [ ] データルートの分岐（`paths.rs`）を触った後、**Windows / macOS / Linux の 3 分岐全部**が `cargo check` を通るか（cfg ブロックは書いた環境でしかコンパイル検証されないことを忘れるな）

---

## 5. モデル戦略 (LLM Selection Policy)

**単一の真実は `config/model_params.json`。** モデル名・量子化・サーバー引数・生成パラメータをコードにハードコードするな。読み込みは `core/llm_config.py` に集約されており、優先順位は **環境変数 > config/model_params.json > 組み込みデフォルト** である。

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

### 7.2 外部知識のオンデマンド・インジェクション (`core/knowledge_fetcher.py`)

オフライン原則（§1）の唯一の公認例外。動作 4 段階と絶対条件:

1. **タグフック**: LLM が回答末尾に `<fetch_query>クエリ</fetch_query>` を出す（SYSTEM_PROMPT が「一般名詞のみ・個人情報禁止・最大2件」を指示）。consult() は本文からタグを除去し**キューに永続化するだけ** — consult 中にネットワークへ出ることは絶対にない。
2. **キュー**: `data/knowledge/_fetch_queue.json` に平文保存。ユーザーが取得前に監査・削除できる。
3. **取得（明示起動のみ）**: `facade.fetch_pending_knowledge()` / stdio `knowledge.fetch_pending` をユーザーが呼んだ時だけ実行。さらに `PKB_ALLOW_ONLINE_FETCH=1` が無ければ**一切通信せず** pending 件数を返すだけ。`default_online_fetcher` は未許可時に RuntimeError を投げる — **これを try/except で握って続行するコードを書いた瞬間、それは情報漏洩バグである。**
4. **統合**: 取得結果は `data/knowledge/fetched_*.md` に永続化 → 既存の `sync_knowledge_index()` が次回 consult で自動ベクトル化。**専用のインデックス機構を新設するな** — 手動配置の知識ファイルと同じ経路に乗せることで、オフライン充足（ユーザーが資料を手で置く）と等価になる。

**テスト規律**: ネットワークを叩くテストを書くな。`process_pending(fetcher=mock)` の依存注入がそのためにある。`test_integration.py` の `test_offline_default_never_fetches` は「未設定なら絶対に外へ出ない」ことの回帰ガードであり、削除禁止。

---

## 8. Target Alpha: KV キャッシュのプレフィックス・ピニング (`core/kv_cache.py`)

consult プロンプトの静的部分 (プロファイル群) の KV テンソルを llama-server のスロット save/restore でディスク永続化し、TTFT から静的 prefill を消す機構。**「プロファイルを LLM の KV テンソルにコンパイルして成果物として持つ」**という設計であり、以下の不変条件を破ると静かに (エラーなく・ただ遅く) 死ぬ。

### 8.1 アーキテクチャの不変条件 (絶対に壊すな)

1. **プロンプトの静的→動的の順序は絶対に変えるな。** KV 再利用は「共通トークン接頭辞」でのみ機能する。`build_static_prefix()`（プロファイル4セクション）が必ず先頭、`build_dynamic_suffix()`（検索ヒット + Future Context + 相談文）が必ず後ろ。セクションを 1 つでも入れ替えたり動的情報を静的側に挿すと、**キャッシュは毎回先頭からミスするがエラーは出ない** — 発見が最も遅れる種類の退化である。`test_kv_prefix_cache` が「連結のバイト同一性」「相談文が静的側に無いこと」「Future Context が動的側に有ること」をガードしている。
2. **Future Context（カレンダー30日）は動的側。** 「プロファイルっぽいから」と静的側に移すと日付が変わるたびハッシュが変わり、キャッシュが毎日全滅する。静的側に置いてよいのは「プロファイルファイル由来のテキストのみ」。
3. **ハッシュは SYSTEM_PROMPT + 静的プレフィックスの両方を含む**（`kv_cache.prefix_hash`）。チャットテンプレートは system と user 先頭を連結してトークン化するため、SYSTEM_PROMPT の 1 文字変更でも KV は無効 — 片方だけハッシュする「最適化」をするな。
4. **キャッシュは best-effort。** `ensure_prefix` / `commit_prefix` のいかなる失敗（接続不能・500・ファイル欠損）も consult を失敗させてはならない。キャッシュ機構の例外を上に投げるコードはバグである。
5. **保存は「ハッシュ変化時の初回のみ」**（`commit_prefix` の state 照合）。毎生成後に保存すると、数百 MB になり得るスロットファイルを毎回書き直すファイルチャーンになる。サーバー稼働中の 2 回目以降は `cache_prompt` のメモリ内再利用で足り、restore はサーバー再起動後にだけ必要。
6. **パージは決定論的に。** 新ハッシュの保存成功時に旧 `pkb-prefix-*.bin` を全削除（`purge_stale`）。KV ファイルは巨大なので「念のため残す」は許されない。`data/processed/kv_slots/` は gitignore 圏内。

### 8.2 今回踏んだ罠 (次の実装者への警告)

- **`--slot-save-path` を旧 llama-server に盲目的に渡すと、サーバーの起動自体が失敗し consult が全滅する。** 対策として `llm_config.server_supports_slot_save()` が `--help` 出力をプローブして対応時のみフラグを付与する（結果は exe パス毎にキャッシュ、`PKB_KV_CACHE=0` で強制無効）。**このプローブを「たぶん対応してるから」と削った瞬間、ユーザーの llama.cpp 更新忘れが致命傷になる。**
- **スロット API 非対応 (404/501) は 1 回で恒久無効化**（`_disabled`）。毎 consult で 404 を叩き続けるな。一方 **500 や接続失敗は一時障害なので無効化しない** — この非対称は意図的。
- **部分的な deep_profile で `_profile_section()` が KeyError で死ぬ潜在バグ**を本実装のテストが暴いた（`p["value_hierarchy"]` 直接参照）。プロンプト構築セクションは**全キーを `.get()` で防御的に読む**こと。プロンプト構築の失敗 = consult 全体の失敗である。
- llama-server の生成リクエストには `id_slot`（復元したスロット番号）と `cache_prompt: true` の**両方**を付けること。restore してもリクエストが別スロットに載れば無意味。

### 8.3 検証の作法

スロット API はテスト環境に無いため、`SlotCacheClient(http_post=...)` の**依存注入**でフェイクトランスポートを差す（knowledge_fetcher と同じ確立済みパターン）。実サーバーでの効果測定は: 同一相談を 2 回投げ、llama-server ログの `prompt eval` トークン数が 2 回目に動的サフィックス分まで縮むことを確認する。

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

