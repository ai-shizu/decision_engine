# PKB — 完全オフライン・ローカル完結型 意思決定支援エンジン

Snapdragon X (Windows on ARM64) 向けに最適化された個人ナレッジベース (PKB) です。  
日記・LINE・**予定**・家計簿・AI 相談履歴を **DailyContext** として統合し、C++ NEON ベクトル検索 + ローカル LLM (llama.cpp) で意思決定を支援します。

**外部 API と TCP/IP は使用しません。** Python はIP socketを実行時拒否し、
ローカル推論はPKBがspawnした`llama.cpp`子プロセスとのprivate prompt channelだけで行います。

---

## ディレクトリ構造

```
decision_engine/
├── apps/desktop/                  # デスクトップアプリ (主軸) — Tauri v2 + React
│   ├── src/                       # React UI (RECORD / IMPORT / CONSULT / SETTINGS)
│   ├── src-tauri/                 # Rust: EngineManager, pkb_invoke コマンド
│   │   └── binaries/              # リリース用 PyInstaller エンジン (開発時は未使用)
│   ├── dev.cmd                    # 開発起動
│   └── build.cmd                  # NSIS インストーラー生成
│
├── src/python/
│   ├── core/                      # ビジネスロジック (UI 非依存)
│   │   ├── facade.py              # TUI / デスクトップ共通 API
│   │   ├── consultation_engine.py # 相談パイプライン (検索 + LLM)
│   │   ├── pipeline.py            # 384 次元ベクトル化 → AoSoA バイナリ
│   │   ├── data_merger.py         # 日付結合 → DailyContext
│   │   ├── profiler.py            # 深層プロファイル (deep_profile.v5)
│   │   ├── calendar_manager.py    # 予定・日記の読み書き
│   │   ├── finance_manager.py     # 家計簿
│   │   ├── calendar_sync.py       # Google カレンダー (ICS) 取込
│   │   ├── apple_calendar_sync.py # Apple カレンダー (macOS ローカル DB)
│   │   ├── consultation_log.py    # AI 相談履歴
│   │   ├── llm_config.py          # ローカル LLM 設定
│   │   ├── settings_api.py        # 基本情報 (fixed_attributes)
│   │   └── paths.py               # データパス解決
│   ├── ui_tui/                    # Textual TUI (開発・デバッグ用)
│   │   ├── app.py                 # 4 タブダッシュボード
│   │   ├── calendar_widget.py
│   │   └── time_picker.py
│   ├── engine_stdio.py            # デスクトップ IPC — stdin/stdout JSON 1 行
│   ├── run_engine.py              # エンジン起動エントリ (Tauri / 手動テスト)
│   └── app.py                     # CLI 相談 (旧来)
│
├── src/ui/
│   └── app.py                     # TUI 互換ラッパー (python src/ui/app.py)
│
├── src/cpp/
│   └── search_engine.cpp          # NEON SIMD 内積 + OpenMP Top-K
│
├── data/
│   ├── raw/                       # 日記・予定・家計簿・LINE 等 (gitignore)
│   ├── processed/                 # vectors.bin, プロファイル (gitignore)
│   └── knowledge/                 # 外部知識 Markdown
│
├── build/search_engine.exe        # C++ 検索バイナリ (gitignore)
├── models/*.gguf                  # ローカル LLM (gitignore)
└── tools/llama-arm64/             # llama.cpp runtime (gitignore)
```

### アーキテクチャ概要

| レイヤ | 役割 |
|--------|------|
| **React UI** | `apps/desktop/src/` — 4 タブ、Tauri コマンド経由でエンジン呼び出し |
| **Rust IPC** | `src-tauri/src/engine.rs` / `os_sandbox.rs` — bundled Python子をOS sandbox内で管理、stdio JSON |
| **Stdio API** | `engine_stdio.py` — コマンド dispatch (`record.*`, `consult`, `import.*` 等) |
| **共通 Facade** | `core/facade.py` — RECORD / IMPORT / CONSULT / SETTINGS のビジネス操作 |
| **C++ 検索** | `search_engine.exe` — PKBVEC01 AoSoA、384 次元 Top-K |
| **Textual TUI** | `ui_tui/app.py` — facade を直接呼び出す開発用 UI |

---

## 起動方法

### デスクトップアプリ (推奨)

PKB は **Web アプリではなく、Chrome のようにインストーラーをダウンロードして使う**デスクトップアプリとして配布します。

```powershell
# 開発
cd apps\desktop
.\dev.cmd

# インストーラー生成
.\build.cmd
# → src-tauri\target\release\bundle\nsis\PKB_*-setup.exe
```

**注意:** `target\debug\pkb-desktop.exe` を直接起動しないでください（Vite 未接続で黒画面になります）。

詳細: [`apps/desktop/README.md`](apps/desktop/README.md)

### TUI (開発・フォールバック)

```powershell
# Textual 4 タブ (推奨パス)
python src\python\ui_tui\app.py

# 後方互換ラッパー
python src\ui\app.py
```

### CLI / パイプライン

```powershell
# 一括: パイプライン → C++ コンパイル → 検索デモ
.\build.ps1

# 個別
python src\python\core\pipeline.py
python src\python\core\profiler.py              # --no-llm 可

# CLI 相談
python src\python\app.py "転職すべきか悩んでいる"
python src\python\core\consultation_engine.py "相談内容"   # 知識検索統合版

# エンジン単体 (stdio IPC テスト)
python src\python\run_engine.py
```

### 前提

- Python 3.12 (ARM64 推奨)
- `clang++` + OpenMP または MSVC
- デスクトップ開発: Node.js 20+, Rust (stable)
- (任意) `models/Qwen2.5-7B-Instruct-Q4_K_M.gguf`

---

## TUI ダッシュボード (`src/python/ui_tui/app.py`)

| タブ | 内容 |
|---|---|
| **RECORD** | 月/週カレンダー + **[予定 \| 家計簿 \| 日記]** サブタブ。`Ctrl+S` で保存 |
| **IMPORT** | LINE 履歴 / **ICS 同期** / **Apple カレンダー同期** (macOS) |
| **CONSULT** | チャット相談 (送信時のみ検索 + LLM) |
| **SETTINGS** | 基本情報 6 項目 + 自動プロフィール (読取専用) + profiler 再分析 |

デスクトップ版 UI も同じ 4 タブ構成です (`apps/desktop/src/components/`)。

### RECORD

- **予定** — 時刻ピッカー + タイトル → `data/raw/calendar.json`
- **家計簿** — 支出/収入 → `data/raw/finance.json`
- **日記** — Markdown → `data/raw/diary.md` (1 日 1 枠)

保存時は `sync_diary_index(force=True)` のみ実行。**profiler は走らない。**

### IMPORT

| 取込 | 方式 | ネットワーク |
|---|---|---|
| LINE | 公式エクスポート `.txt` をドロップ | 不使用 |
| Google カレンダー | ICS を手動エクスポート → ファイル選択 | 不使用 |
| Apple カレンダー | macOS ローカル SQLite DB を読取 (Windows ではボタン無効) | 不使用 |

- マージモード: **追記** / **上書き** (ICS・Apple 共通)
- ICS 取込時: 選択ファイルを `data/raw/calendar_import.ics` にもアーカイブ
- 取込後: `calendar.json` 更新 → **`sync_diary_index(force=True)`** → DailyContext / `vectors.bin` へ即反映
- LINE 取込時のみ **profiler が自動実行** (価値観抽出)

---

## カレンダー同期 (オフライン)

```
手動エクスポート / ローカル DB 読取  →  calendar_sync / apple_calendar_sync
                                      →  calendar.json
                                      →  sync_diary_index(force=True)
                                      →  vectors.bin
```

- **ICS:** `src/python/core/calendar_sync.py` — 標準ライブラリのみでパース
- **Apple:** `src/python/core/apple_calendar_sync.py` — `~/Library/Calendars/` 等の SQLite
- クラウド API・バックグラウンド同期は**一切なし**

---

## DailyContext

1 日 = 1 ベクトルチャンク。以下を日付 (YYYY-MM-DD) で結合:

| ソース | ファイル |
|---|---|
| 予定 | `calendar.json` |
| 家計簿 | `finance.json` |
| 日記 | `diary.md` |
| AI 相談 | `ai_consultations.json` |
| LINE | `line_history.txt` (ConversationSession) |

CONSULT 時、日記 + LINE が同日にあるチャンクは **ranking_score × 1.15** で優先。  
相談プロンプトには **Future Context** (向こう 30 日の予定) も注入。

---

## 相談パイプライン

```
consult(query):
  embed → sync_diary_index / sync_knowledge_index
       → C++ NEON 検索 (DailyContext + 外部知識)
       → プロファイル + ヒットコンテキスト + Future Context
       → llama.cpp owned child (Windows Named Pipe / POSIX `/dev/stdin`)
       → 4 セクション Markdown 回答
```

---

## ローカル LLM

```powershell
curl.exe -L -o models\Qwen2.5-7B-Instruct-Q4_K_M.gguf `
  https://huggingface.co/bartowski/Qwen2.5-7B-Instruct-GGUF/resolve/main/Qwen2.5-7B-Instruct-Q4_K_M.gguf
```

- 優先: `Qwen2.5-7B-Instruct-Q4_K_M.gguf` (約 4.7GB)
- 環境変数: `PKB_LLAMA_THREADS`, `PKB_LLAMA_CTX`, `PKB_LLAMA_BATCH`, `PKB_MODELS_DIR`

---

## 設計原則

| 原則 | 内容 |
|---|---|
| 記録と相談の分離 | RECORD / カレンダー同期 → インデックス更新のみ |
| 記録と分析の分離 | RECORD 保存は profiler 非実行 (LINE 取込のみ例外) |
| 手動インポート | カレンダーは人間がトリガー。自動クラウド同期なし |
| 完全遅延初期化 | 埋め込み・LLM は初回相談 / profiler まで起動しない |
| プロファイル二層 | `fixed_attributes` (手入力) / `inferred_profile` (自動・読取専用) |

---

## テスト

```powershell
python tests\ui_smoke.py
python tests\test_calendar_sync.py
python tests\test_apple_calendar_sync.py
python tests\benchmark.py --quick
```

---

## C++ 検索コア (`PKBVEC01`)

384 次元 L2 正規化ベクトル、AoSoA 4-lane、OpenMP 並列 Top-K、Windows mmap。

```powershell
.\build\search_engine.exe data\processed\vectors.bin data\processed\query.bin 5
```

---

## 詳細ドキュメント

- 開発規律: [`docs/AI_SKILLS.md`](docs/AI_SKILLS.md)
- 現在地・引継ぎ: [`docs/HANDOFF.md`](docs/HANDOFF.md)
- 安定アーキテクチャ索引: [`docs/CONTEXT.md`](docs/CONTEXT.md)
- 事故裁定: [`docs/architecture/INCIDENT_LEDGER.md`](docs/architecture/INCIDENT_LEDGER.md)

---

## 注意

- `data/raw/`・`data/processed/`・`models/`・`build/` は `.gitignore` 対象 (個人データ・大容量モデル)
- インストール版のデータは `%LOCALAPPDATA%\PKB\` (`PKB_PROJECT_ROOT` で上書き可)
- Apple カレンダー DB スキーマは非公式。ICS フォールバックを推奨
